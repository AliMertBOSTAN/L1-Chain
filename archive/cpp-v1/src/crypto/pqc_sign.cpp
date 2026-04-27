/// Dilithium / ML-DSA signature implementation using liboqs.
///
/// Maps QuantumVault's parameter-set enum to liboqs algorithm names:
///   Level2 -> OQS_SIG_alg_ml_dsa_44   (fka Dilithium2)
///   Level3 -> OQS_SIG_alg_ml_dsa_65   (fka Dilithium3)
///   Level5 -> OQS_SIG_alg_ml_dsa_87   (fka Dilithium5)
///
/// Older liboqs versions use OQS_SIG_alg_dilithium_{2,3,5} — we try the
/// standardized ML-DSA names first and fall back to legacy Dilithium names
/// at runtime.

#include "qv/crypto/pqc_sign.hpp"

#include <oqs/oqs.h>

#include <array>
#include <cstring>
#include <string_view>

namespace qv::crypto {

namespace {

/// Ordered list (primary, legacy) of liboqs algorithm identifiers for each
/// parameter set. liboqs renamed these during NIST standardization.
const char* algorithm_name(DilithiumParameterSet set) {
    switch (set) {
        case DilithiumParameterSet::Level2:
            if (OQS_SIG_alg_is_enabled("ML-DSA-44")) {
                return "ML-DSA-44";
            }
            return "Dilithium2";
        case DilithiumParameterSet::Level3:
            if (OQS_SIG_alg_is_enabled("ML-DSA-65")) {
                return "ML-DSA-65";
            }
            return "Dilithium3";
        case DilithiumParameterSet::Level5:
            if (OQS_SIG_alg_is_enabled("ML-DSA-87")) {
                return "ML-DSA-87";
            }
            return "Dilithium5";
    }
    return "";
}

/// RAII wrapper around OQS_SIG handle.
struct SigHandle {
    OQS_SIG* sig = nullptr;

    explicit SigHandle(DilithiumParameterSet param_set) {
        const char* name = algorithm_name(param_set);
        if (name != nullptr && name[0] != '\0') {
            sig = OQS_SIG_new(name);
        }
    }
    ~SigHandle() {
        if (sig != nullptr) {
            OQS_SIG_free(sig);
        }
    }
    SigHandle(const SigHandle&) = delete;
    SigHandle& operator=(const SigHandle&) = delete;

    bool valid() const { return sig != nullptr; }
};

}  // namespace

// ============================================================================
// DilithiumConfig — size queries
// ============================================================================

size_t DilithiumConfig::public_key_size() const {
    SigHandle h(parameter_set);
    return h.valid() ? h.sig->length_public_key : 0;
}

size_t DilithiumConfig::secret_key_size() const {
    SigHandle h(parameter_set);
    return h.valid() ? h.sig->length_secret_key : 0;
}

size_t DilithiumConfig::signature_size() const {
    SigHandle h(parameter_set);
    return h.valid() ? h.sig->length_signature : 0;
}

// ============================================================================
// Keypair generation
// ============================================================================

Result<PQCKeyPair> generate_pqc_keypair(const DilithiumConfig& config) {
    SigHandle h(config.parameter_set);
    if (!h.valid()) {
        return std::unexpected(CryptoError::KeyGenerationFailed);
    }

    PQCKeyPair kp;
    kp.public_key.resize(h.sig->length_public_key);
    kp.secret_key.resize(h.sig->length_secret_key);

    if (OQS_SIG_keypair(h.sig, kp.public_key.data(), kp.secret_key.data()) != OQS_SUCCESS) {
        return std::unexpected(CryptoError::KeyGenerationFailed);
    }
    return kp;
}

// ============================================================================
// PQCSignature — stateful signer/verifier (holds an OQS_SIG handle)
// ============================================================================

struct PQCSignature::Impl {
    DilithiumParameterSet param_set;
    OQS_SIG* sig = nullptr;

    explicit Impl(DilithiumParameterSet ps) : param_set(ps) {
        const char* name = algorithm_name(ps);
        if (name != nullptr && name[0] != '\0') {
            sig = OQS_SIG_new(name);
        }
    }
    ~Impl() {
        if (sig != nullptr) {
            OQS_SIG_free(sig);
            sig = nullptr;
        }
    }
    Impl(const Impl&) = delete;
    Impl& operator=(const Impl&) = delete;
};

PQCSignature::PQCSignature(DilithiumParameterSet param_set)
    : m_impl(std::make_unique<Impl>(param_set)) {}

PQCSignature::~PQCSignature() = default;

Result<Signature> PQCSignature::sign(std::span<const uint8_t> secret_key,
                                     std::span<const uint8_t> message) {
    if (!m_impl || m_impl->sig == nullptr) {
        return std::unexpected(CryptoError::UnknownError);
    }
    if (secret_key.size() != m_impl->sig->length_secret_key) {
        return std::unexpected(CryptoError::InvalidKeySize);
    }

    Signature out(m_impl->sig->length_signature);
    size_t sig_len = out.size();

    OQS_STATUS rc = OQS_SIG_sign(
        m_impl->sig,
        out.data(), &sig_len,
        message.data(), message.size(),
        secret_key.data());

    if (rc != OQS_SUCCESS) {
        return std::unexpected(CryptoError::UnknownError);
    }
    out.resize(sig_len);
    return out;
}

Result<bool> PQCSignature::verify(std::span<const uint8_t> public_key,
                                  std::span<const uint8_t> message,
                                  std::span<const uint8_t> signature) {
    if (!m_impl || m_impl->sig == nullptr) {
        return std::unexpected(CryptoError::UnknownError);
    }
    if (public_key.size() != m_impl->sig->length_public_key) {
        return std::unexpected(CryptoError::InvalidKeySize);
    }
    if (signature.size() > m_impl->sig->length_signature || signature.empty()) {
        return std::unexpected(CryptoError::InvalidSignature);
    }

    OQS_STATUS rc = OQS_SIG_verify(
        m_impl->sig,
        message.data(), message.size(),
        signature.data(), signature.size(),
        public_key.data());

    // liboqs returns OQS_SUCCESS for valid, OQS_ERROR for invalid.
    // Distinguish "invalid signature" (ok, return false) from "library error".
    return rc == OQS_SUCCESS;
}

size_t PQCSignature::public_key_size() const {
    return (m_impl && m_impl->sig) ? m_impl->sig->length_public_key : 0;
}

size_t PQCSignature::secret_key_size() const {
    return (m_impl && m_impl->sig) ? m_impl->sig->length_secret_key : 0;
}

size_t PQCSignature::signature_size() const {
    return (m_impl && m_impl->sig) ? m_impl->sig->length_signature : 0;
}

// ============================================================================
// One-shot convenience wrappers
// ============================================================================

Result<Signature> pqc_sign(std::span<const uint8_t> secret_key,
                           std::span<const uint8_t> message,
                           DilithiumParameterSet param_set) {
    PQCSignature signer(param_set);
    return signer.sign(secret_key, message);
}

Result<bool> pqc_verify(std::span<const uint8_t> public_key,
                        std::span<const uint8_t> message,
                        std::span<const uint8_t> signature,
                        DilithiumParameterSet param_set) {
    PQCSignature verifier(param_set);
    return verifier.verify(public_key, message, signature);
}

}  // namespace qv::crypto
