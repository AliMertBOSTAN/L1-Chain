/// Hybrid KEM: X25519 (OpenSSL) + Kyber/ML-KEM (liboqs) + SHA3-256 KDF.
///
/// Security argument: an adversary must break BOTH the classical discrete log
/// problem on Curve25519 AND the module-LWE assumption underlying Kyber to
/// recover the shared secret. The KDF binds the entire transcript (both
/// ciphertexts and the ephemeral X25519 public key) to prevent key reuse
/// and certain chosen-ciphertext attacks.
///
/// Wire format of the hybrid ciphertext:
///   [  0 .. 32 ) = ephemeral X25519 public key (32 bytes)
///   [ 32 .. N  ) = Kyber/ML-KEM ciphertext
///
/// Final shared secret derivation (32 bytes):
///   SS = SHA3-256( "QuantumVault-Hybrid-KEM-v1" ||
///                  X25519(eph_sk, peer_x25519_pk) ||
///                  KyberSharedSecret ||
///                  eph_x25519_pk ||
///                  peer_x25519_pk ||
///                  kyber_ciphertext )

#include "qv/crypto/hybrid_kem.hpp"

#include <openssl/evp.h>
#include <openssl/crypto.h>
#include <oqs/oqs.h>

#include <algorithm>
#include <cstring>

namespace qv::crypto {

namespace {

constexpr size_t kX25519KeySize = 32;
constexpr std::string_view kKdfContext = "QuantumVault-Hybrid-KEM-v1";

const char* kyber_algorithm_name(KyberParameterSet set) {
    switch (set) {
        case KyberParameterSet::Level1:
            return OQS_KEM_alg_is_enabled("ML-KEM-512") ? "ML-KEM-512" : "Kyber512";
        case KyberParameterSet::Level3:
            return OQS_KEM_alg_is_enabled("ML-KEM-768") ? "ML-KEM-768" : "Kyber768";
        case KyberParameterSet::Level5:
            return OQS_KEM_alg_is_enabled("ML-KEM-1024") ? "ML-KEM-1024" : "Kyber1024";
    }
    return "";
}

/// RAII wrappers ------------------------------------------------------------

struct OqsKem {
    OQS_KEM* kem = nullptr;
    explicit OqsKem(KyberParameterSet ps) {
        const char* name = kyber_algorithm_name(ps);
        if (name != nullptr && name[0] != '\0') {
            kem = OQS_KEM_new(name);
        }
    }
    ~OqsKem() {
        if (kem != nullptr) {
            OQS_KEM_free(kem);
        }
    }
    OqsKem(const OqsKem&) = delete;
    OqsKem& operator=(const OqsKem&) = delete;
    bool valid() const { return kem != nullptr; }
};

struct EvpPkey {
    EVP_PKEY* pkey = nullptr;
    EvpPkey() = default;
    explicit EvpPkey(EVP_PKEY* p) : pkey(p) {}
    ~EvpPkey() { if (pkey != nullptr) EVP_PKEY_free(pkey); }
    EvpPkey(const EvpPkey&) = delete;
    EvpPkey& operator=(const EvpPkey&) = delete;
    EvpPkey(EvpPkey&& o) noexcept : pkey(o.pkey) { o.pkey = nullptr; }
    EvpPkey& operator=(EvpPkey&& o) noexcept {
        if (this != &o) { if (pkey) EVP_PKEY_free(pkey); pkey = o.pkey; o.pkey = nullptr; }
        return *this;
    }
};

struct EvpPkeyCtx {
    EVP_PKEY_CTX* ctx = nullptr;
    explicit EvpPkeyCtx(EVP_PKEY_CTX* c) : ctx(c) {}
    ~EvpPkeyCtx() { if (ctx != nullptr) EVP_PKEY_CTX_free(ctx); }
    EvpPkeyCtx(const EvpPkeyCtx&) = delete;
    EvpPkeyCtx& operator=(const EvpPkeyCtx&) = delete;
};

/// X25519 keypair generation (OpenSSL EVP).
/// Returns a pair of 32-byte buffers: (public_key, secret_key).
bool generate_x25519_keypair(PublicKey& out_pk, SecretKey& out_sk) {
    EvpPkeyCtx ctx(EVP_PKEY_CTX_new_id(EVP_PKEY_X25519, nullptr));
    if (ctx.ctx == nullptr) return false;
    if (EVP_PKEY_keygen_init(ctx.ctx) != 1) return false;

    EVP_PKEY* raw = nullptr;
    if (EVP_PKEY_keygen(ctx.ctx, &raw) != 1) return false;
    EvpPkey pkey(raw);

    out_pk.resize(kX25519KeySize);
    out_sk.resize(kX25519KeySize);
    size_t pk_len = kX25519KeySize;
    size_t sk_len = kX25519KeySize;

    if (EVP_PKEY_get_raw_public_key(pkey.pkey, out_pk.data(), &pk_len) != 1) return false;
    if (EVP_PKEY_get_raw_private_key(pkey.pkey, out_sk.data(), &sk_len) != 1) return false;
    return pk_len == kX25519KeySize && sk_len == kX25519KeySize;
}

/// X25519 ECDH: derive 32-byte shared secret from our sk + peer pk.
bool x25519_derive(std::span<const uint8_t> our_sk,
                   std::span<const uint8_t> peer_pk,
                   std::array<uint8_t, 32>& out) {
    if (our_sk.size() != kX25519KeySize || peer_pk.size() != kX25519KeySize) return false;

    EvpPkey sk(EVP_PKEY_new_raw_private_key(EVP_PKEY_X25519, nullptr,
                                            our_sk.data(), our_sk.size()));
    if (sk.pkey == nullptr) return false;

    EvpPkey pk(EVP_PKEY_new_raw_public_key(EVP_PKEY_X25519, nullptr,
                                           peer_pk.data(), peer_pk.size()));
    if (pk.pkey == nullptr) return false;

    EvpPkeyCtx dctx(EVP_PKEY_CTX_new(sk.pkey, nullptr));
    if (dctx.ctx == nullptr) return false;

    if (EVP_PKEY_derive_init(dctx.ctx) != 1) return false;
    if (EVP_PKEY_derive_set_peer(dctx.ctx, pk.pkey) != 1) return false;

    size_t secret_len = out.size();
    if (EVP_PKEY_derive(dctx.ctx, out.data(), &secret_len) != 1) return false;
    return secret_len == out.size();
}

/// Derive final 32-byte hybrid shared secret by hashing the transcript.
Result<SharedSecret> kdf_combine(std::span<const uint8_t> ecdh_secret,
                                 std::span<const uint8_t> kyber_secret,
                                 std::span<const uint8_t> ephemeral_x25519_pk,
                                 std::span<const uint8_t> peer_x25519_pk,
                                 std::span<const uint8_t> kyber_ciphertext) {
    Hasher h(HashAlgorithm::SHA3_256);

    auto feed = [&](std::span<const uint8_t> s) -> bool {
        auto r = h.update(s);
        return r.has_value();
    };

    auto ctx_bytes = std::span<const uint8_t>(
        reinterpret_cast<const uint8_t*>(kKdfContext.data()), kKdfContext.size());

    if (!feed(ctx_bytes) || !feed(ecdh_secret) || !feed(kyber_secret) ||
        !feed(ephemeral_x25519_pk) || !feed(peer_x25519_pk) || !feed(kyber_ciphertext)) {
        return std::unexpected(CryptoError::EncapsulationFailed);
    }

    auto digest = h.finalize();
    if (!digest.has_value()) {
        return std::unexpected(digest.error());
    }
    SharedSecret out{};
    std::memcpy(out.data(), digest->data(), out.size());
    return out;
}

}  // namespace

// ============================================================================
// HybridKEMConfig — size queries
// ============================================================================

size_t HybridKEMConfig::kyber_public_key_size() const {
    OqsKem k(kyber_param_set);
    return k.valid() ? k.kem->length_public_key : 0;
}

size_t HybridKEMConfig::kyber_secret_key_size() const {
    OqsKem k(kyber_param_set);
    return k.valid() ? k.kem->length_secret_key : 0;
}

size_t HybridKEMConfig::kyber_ciphertext_size() const {
    OqsKem k(kyber_param_set);
    return k.valid() ? k.kem->length_ciphertext : 0;
}

// ============================================================================
// Keypair generation
// ============================================================================

Result<HybridKEMKeypair> generate_hybrid_kem_keypair(const HybridKEMConfig& config) {
    HybridKEMKeypair kp;

    if (!generate_x25519_keypair(kp.x25519_public_key, kp.x25519_secret_key)) {
        return std::unexpected(CryptoError::KeyGenerationFailed);
    }

    OqsKem kem(config.kyber_param_set);
    if (!kem.valid()) {
        return std::unexpected(CryptoError::KeyGenerationFailed);
    }

    kp.kyber_public_key.resize(kem.kem->length_public_key);
    kp.kyber_secret_key.resize(kem.kem->length_secret_key);

    if (OQS_KEM_keypair(kem.kem,
                        kp.kyber_public_key.data(),
                        kp.kyber_secret_key.data()) != OQS_SUCCESS) {
        return std::unexpected(CryptoError::KeyGenerationFailed);
    }
    return kp;
}

// ============================================================================
// HybridKEM — stateful encap/decap
// ============================================================================

struct HybridKEM::Impl {
    HybridKEMConfig config;
    OQS_KEM* kem = nullptr;

    explicit Impl(const HybridKEMConfig& cfg) : config(cfg) {
        const char* name = kyber_algorithm_name(cfg.kyber_param_set);
        if (name != nullptr && name[0] != '\0') {
            kem = OQS_KEM_new(name);
        }
    }
    ~Impl() {
        if (kem != nullptr) OQS_KEM_free(kem);
    }
    Impl(const Impl&) = delete;
    Impl& operator=(const Impl&) = delete;
};

HybridKEM::HybridKEM(const HybridKEMConfig& config)
    : m_impl(std::make_unique<Impl>(config)) {}

HybridKEM::~HybridKEM() = default;

Result<HybridEncapsulation> HybridKEM::encapsulate(
    const HybridKEMKeypair& peer_hybrid_public_key) {

    if (!m_impl || m_impl->kem == nullptr) {
        return std::unexpected(CryptoError::EncapsulationFailed);
    }
    if (peer_hybrid_public_key.x25519_public_key.size() != kX25519KeySize ||
        peer_hybrid_public_key.kyber_public_key.size() != m_impl->kem->length_public_key) {
        return std::unexpected(CryptoError::InvalidKeySize);
    }

    // --- 1. Generate ephemeral X25519 keypair and do ECDH with peer.
    PublicKey eph_pk;
    SecretKey eph_sk;
    if (!generate_x25519_keypair(eph_pk, eph_sk)) {
        return std::unexpected(CryptoError::EncapsulationFailed);
    }

    std::array<uint8_t, 32> ecdh_secret{};
    if (!x25519_derive(eph_sk.const_span(),
                       peer_hybrid_public_key.x25519_public_key.const_span(),
                       ecdh_secret)) {
        OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
        return std::unexpected(CryptoError::EncapsulationFailed);
    }

    // --- 2. Kyber encapsulation.
    std::vector<uint8_t> kyber_ct(m_impl->kem->length_ciphertext);
    std::array<uint8_t, 64> kyber_ss_buf{};  // max shared-secret length in liboqs is 64
    if (m_impl->kem->length_shared_secret > kyber_ss_buf.size()) {
        return std::unexpected(CryptoError::EncapsulationFailed);
    }

    if (OQS_KEM_encaps(m_impl->kem,
                       kyber_ct.data(),
                       kyber_ss_buf.data(),
                       peer_hybrid_public_key.kyber_public_key.data()) != OQS_SUCCESS) {
        OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
        OPENSSL_cleanse(kyber_ss_buf.data(), kyber_ss_buf.size());
        return std::unexpected(CryptoError::EncapsulationFailed);
    }

    std::span<const uint8_t> kyber_ss(kyber_ss_buf.data(), m_impl->kem->length_shared_secret);

    // --- 3. Combine both shared secrets via transcript-bound KDF.
    auto final_ss = kdf_combine(
        ecdh_secret,
        kyber_ss,
        eph_pk.const_span(),
        peer_hybrid_public_key.x25519_public_key.const_span(),
        kyber_ct);

    OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
    OPENSSL_cleanse(kyber_ss_buf.data(), kyber_ss_buf.size());

    if (!final_ss.has_value()) {
        return std::unexpected(final_ss.error());
    }

    // --- 4. Build wire-format ciphertext: eph_pk || kyber_ct.
    HybridEncapsulation out;
    out.ciphertext.reserve(kX25519KeySize + kyber_ct.size());
    out.ciphertext.insert(out.ciphertext.end(), eph_pk.data(), eph_pk.data() + kX25519KeySize);
    out.ciphertext.insert(out.ciphertext.end(), kyber_ct.begin(), kyber_ct.end());
    out.shared_secret = *final_ss;
    return out;
}

Result<SharedSecret> HybridKEM::decapsulate(const HybridKEMKeypair& local_keypair,
                                            std::span<const uint8_t> ciphertext,
                                            std::span<const uint8_t> peer_x25519_public_key) {
    if (!m_impl || m_impl->kem == nullptr) {
        return std::unexpected(CryptoError::DecapsulationFailed);
    }
    const size_t kyber_ct_len = m_impl->kem->length_ciphertext;
    const size_t expected_len = kX25519KeySize + kyber_ct_len;

    if (ciphertext.size() != expected_len) {
        return std::unexpected(CryptoError::InvalidCiphertext);
    }
    if (local_keypair.x25519_secret_key.size() != kX25519KeySize) {
        return std::unexpected(CryptoError::InvalidKeySize);
    }
    if (local_keypair.kyber_secret_key.size() != m_impl->kem->length_secret_key) {
        return std::unexpected(CryptoError::InvalidKeySize);
    }

    // --- 1. Parse ephemeral X25519 pubkey and Kyber ciphertext.
    std::span<const uint8_t> eph_pk = ciphertext.subspan(0, kX25519KeySize);
    std::span<const uint8_t> kyber_ct = ciphertext.subspan(kX25519KeySize, kyber_ct_len);

    // Optional safety check: if caller supplied a peer pubkey, it must match.
    if (!peer_x25519_public_key.empty() &&
        peer_x25519_public_key.size() == kX25519KeySize &&
        std::memcmp(peer_x25519_public_key.data(), eph_pk.data(), kX25519KeySize) != 0) {
        return std::unexpected(CryptoError::InvalidCiphertext);
    }

    // --- 2. X25519 ECDH with local secret key.
    std::array<uint8_t, 32> ecdh_secret{};
    if (!x25519_derive(local_keypair.x25519_secret_key.const_span(), eph_pk, ecdh_secret)) {
        OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
        return std::unexpected(CryptoError::DecapsulationFailed);
    }

    // --- 3. Kyber decapsulation.
    std::array<uint8_t, 64> kyber_ss_buf{};
    if (m_impl->kem->length_shared_secret > kyber_ss_buf.size()) {
        return std::unexpected(CryptoError::DecapsulationFailed);
    }
    if (OQS_KEM_decaps(m_impl->kem,
                       kyber_ss_buf.data(),
                       kyber_ct.data(),
                       local_keypair.kyber_secret_key.data()) != OQS_SUCCESS) {
        OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
        OPENSSL_cleanse(kyber_ss_buf.data(), kyber_ss_buf.size());
        return std::unexpected(CryptoError::DecapsulationFailed);
    }
    std::span<const uint8_t> kyber_ss(kyber_ss_buf.data(), m_impl->kem->length_shared_secret);

    // --- 4. Reconstruct the same transcript the encapsulator hashed.
    //        For decap: peer (=local) X25519 pubkey is the recipient's static pk.
    auto final_ss = kdf_combine(
        ecdh_secret,
        kyber_ss,
        eph_pk,
        local_keypair.x25519_public_key.const_span(),
        kyber_ct);

    OPENSSL_cleanse(ecdh_secret.data(), ecdh_secret.size());
    OPENSSL_cleanse(kyber_ss_buf.data(), kyber_ss_buf.size());

    if (!final_ss.has_value()) {
        return std::unexpected(final_ss.error());
    }
    return *final_ss;
}

size_t HybridKEM::kyber_public_key_size() const {
    return (m_impl && m_impl->kem) ? m_impl->kem->length_public_key : 0;
}
size_t HybridKEM::kyber_secret_key_size() const {
    return (m_impl && m_impl->kem) ? m_impl->kem->length_secret_key : 0;
}
size_t HybridKEM::kyber_ciphertext_size() const {
    return (m_impl && m_impl->kem) ? m_impl->kem->length_ciphertext : 0;
}
size_t HybridKEM::expected_hybrid_ciphertext_size() const {
    return kX25519KeySize + kyber_ciphertext_size();
}

// ============================================================================
// One-shot convenience wrappers
// ============================================================================

Result<HybridEncapsulation> hybrid_encapsulate(
    const HybridKEMKeypair& peer_public_key,
    const HybridKEMConfig& config) {
    HybridKEM kem(config);
    return kem.encapsulate(peer_public_key);
}

Result<SharedSecret> hybrid_decapsulate(
    const HybridKEMKeypair& local_keypair,
    std::span<const uint8_t> ciphertext,
    std::span<const uint8_t> peer_x25519_public_key,
    const HybridKEMConfig& config) {
    HybridKEM kem(config);
    return kem.decapsulate(local_keypair, ciphertext, peer_x25519_public_key);
}

}  // namespace qv::crypto
