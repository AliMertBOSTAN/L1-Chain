/// Hash function implementations.
///
/// SHA3-256: OpenSSL EVP interface (native support since OpenSSL 1.1.1).
/// BLAKE3:   Official BLAKE3 C reference library (<blake3.h>).
///
/// Both produce 32-byte digests and are collision-resistant under
/// the quantum adversary model (Grover gives only 2^128 preimage work).

#include "qv/crypto/hash.hpp"

#include <openssl/evp.h>

#if __has_include(<blake3.h>)
#include <blake3.h>
#define QV_HAS_BLAKE3 1
#else
#define QV_HAS_BLAKE3 0
#endif

#include <cstring>
#include <memory>

namespace qv::crypto {

namespace {

/// RAII wrapper for OpenSSL EVP_MD_CTX.
struct EvpMdCtx {
    EVP_MD_CTX* ctx = nullptr;

    EvpMdCtx() : ctx(EVP_MD_CTX_new()) {}
    ~EvpMdCtx() {
        if (ctx != nullptr) {
            EVP_MD_CTX_free(ctx);
        }
    }
    EvpMdCtx(const EvpMdCtx&) = delete;
    EvpMdCtx& operator=(const EvpMdCtx&) = delete;
};

/// One-shot SHA3-256 using OpenSSL EVP.
Result<HashDigest> sha3_256_oneshot(std::span<const uint8_t> data) {
    HashDigest out{};
    EvpMdCtx mdctx;
    if (mdctx.ctx == nullptr) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }

    const EVP_MD* md = EVP_sha3_256();
    if (md == nullptr) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }

    if (EVP_DigestInit_ex(mdctx.ctx, md, nullptr) != 1) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }
    if (!data.empty()) {
        if (EVP_DigestUpdate(mdctx.ctx, data.data(), data.size()) != 1) {
            return std::unexpected(CryptoError::HashComputationFailed);
        }
    }
    unsigned int len = 0;
    if (EVP_DigestFinal_ex(mdctx.ctx, out.data(), &len) != 1 || len != out.size()) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }
    return out;
}

/// One-shot BLAKE3-256.
Result<HashDigest> blake3_oneshot(std::span<const uint8_t> data) {
#if QV_HAS_BLAKE3
    HashDigest out{};
    blake3_hasher hasher;
    blake3_hasher_init(&hasher);
    if (!data.empty()) {
        blake3_hasher_update(&hasher, data.data(), data.size());
    }
    blake3_hasher_finalize(&hasher, out.data(), out.size());
    return out;
#else
    (void)data;
    // BLAKE3 library is missing from the build environment.
    // Add `blake3` to flake.nix and link it in src/crypto/CMakeLists.txt.
    return std::unexpected(CryptoError::HashComputationFailed);
#endif
}

}  // namespace

Result<HashDigest> hash(HashAlgorithm algorithm, std::span<const uint8_t> data) {
    switch (algorithm) {
        case HashAlgorithm::SHA3_256:
            return sha3_256_oneshot(data);
        case HashAlgorithm::BLAKE3:
            return blake3_oneshot(data);
    }
    return std::unexpected(CryptoError::InvalidInput);
}

Result<HashDigest> sha3_256(std::span<const uint8_t> data) {
    return sha3_256_oneshot(data);
}

Result<HashDigest> blake3(std::span<const uint8_t> data) {
    return blake3_oneshot(data);
}

Result<HashDigest> double_hash(HashAlgorithm algorithm, std::span<const uint8_t> data) {
    auto first = hash(algorithm, data);
    if (!first.has_value()) {
        return std::unexpected(first.error());
    }
    return hash(algorithm, std::span<const uint8_t>(first->data(), first->size()));
}

Result<HashDigest> double_sha3_256(std::span<const uint8_t> data) {
    return double_hash(HashAlgorithm::SHA3_256, data);
}

Result<HashDigest> double_blake3(std::span<const uint8_t> data) {
    return double_hash(HashAlgorithm::BLAKE3, data);
}

// ----------------------------------------------------------------------------
// Stateful Hasher (streaming API)
// ----------------------------------------------------------------------------

struct Hasher::Impl {
    HashAlgorithm algo;
    EVP_MD_CTX* evp_ctx = nullptr;
#if QV_HAS_BLAKE3
    blake3_hasher b3;
    bool b3_initialized = false;
#endif

    explicit Impl(HashAlgorithm a) : algo(a) {
        if (algo == HashAlgorithm::SHA3_256) {
            evp_ctx = EVP_MD_CTX_new();
            if (evp_ctx != nullptr) {
                EVP_DigestInit_ex(evp_ctx, EVP_sha3_256(), nullptr);
            }
        } else if (algo == HashAlgorithm::BLAKE3) {
#if QV_HAS_BLAKE3
            blake3_hasher_init(&b3);
            b3_initialized = true;
#endif
        }
    }

    ~Impl() {
        if (evp_ctx != nullptr) {
            EVP_MD_CTX_free(evp_ctx);
            evp_ctx = nullptr;
        }
    }

    Impl(const Impl&) = delete;
    Impl& operator=(const Impl&) = delete;
};

Hasher::Hasher(HashAlgorithm algorithm) : m_impl(std::make_unique<Impl>(algorithm)) {}

Hasher::Hasher(Hasher&& other) noexcept = default;

Hasher& Hasher::operator=(Hasher&& other) noexcept = default;

Hasher::~Hasher() = default;

Result<void> Hasher::update(std::span<const uint8_t> data) {
    if (!m_impl) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }
    if (data.empty()) {
        return {};
    }

    switch (m_impl->algo) {
        case HashAlgorithm::SHA3_256:
            if (m_impl->evp_ctx == nullptr ||
                EVP_DigestUpdate(m_impl->evp_ctx, data.data(), data.size()) != 1) {
                return std::unexpected(CryptoError::HashComputationFailed);
            }
            return {};

        case HashAlgorithm::BLAKE3:
#if QV_HAS_BLAKE3
            blake3_hasher_update(&m_impl->b3, data.data(), data.size());
            return {};
#else
            return std::unexpected(CryptoError::HashComputationFailed);
#endif
    }
    return std::unexpected(CryptoError::InvalidInput);
}

Result<HashDigest> Hasher::finalize() {
    if (!m_impl) {
        return std::unexpected(CryptoError::HashComputationFailed);
    }
    HashDigest out{};

    switch (m_impl->algo) {
        case HashAlgorithm::SHA3_256: {
            if (m_impl->evp_ctx == nullptr) {
                return std::unexpected(CryptoError::HashComputationFailed);
            }
            unsigned int len = 0;
            if (EVP_DigestFinal_ex(m_impl->evp_ctx, out.data(), &len) != 1 || len != out.size()) {
                return std::unexpected(CryptoError::HashComputationFailed);
            }
            return out;
        }
        case HashAlgorithm::BLAKE3:
#if QV_HAS_BLAKE3
            blake3_hasher_finalize(&m_impl->b3, out.data(), out.size());
            return out;
#else
            return std::unexpected(CryptoError::HashComputationFailed);
#endif
    }
    return std::unexpected(CryptoError::InvalidInput);
}

}  // namespace qv::crypto
