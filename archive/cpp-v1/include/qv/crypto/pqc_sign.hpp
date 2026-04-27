#pragma once

#include "types.hpp"
#include <span>
#include <memory>

namespace qv::crypto {

/// Dilithium/ML-DSA parameter sets (NIST standardized)
enum class DilithiumParameterSet {
    /// NIST Level 2 (128-bit post-quantum security)
    Level2,

    /// NIST Level 3 (192-bit post-quantum security)
    Level3,

    /// NIST Level 5 (256-bit post-quantum security)
    Level5,
};

/// Configuration for Dilithium PQC signing
struct DilithiumConfig {
    DilithiumParameterSet parameter_set = DilithiumParameterSet::Level3;

    /// Get the public key size in bytes for this parameter set
    size_t public_key_size() const;

    /// Get the secret key size in bytes for this parameter set
    size_t secret_key_size() const;

    /// Get the signature size in bytes for this parameter set
    size_t signature_size() const;
};

/// Post-quantum cryptography keypair for Dilithium/ML-DSA signatures
struct PQCKeyPair {
    /// Public key (can be shared openly)
    PublicKey public_key;

    /// Secret key (must be kept confidential with secure allocator)
    SecretKey secret_key;
};

/// Generate a new Dilithium keypair
///
/// @param config Configuration specifying parameter set and constraints
/// @return Keypair with public and secret keys, or error if generation fails
Result<PQCKeyPair> generate_pqc_keypair(const DilithiumConfig& config = DilithiumConfig{});

/// Post-quantum signature scheme
class PQCSignature {
public:
    /// Create a new PQC signature scheme with specified Dilithium parameters
    explicit PQCSignature(DilithiumParameterSet param_set = DilithiumParameterSet::Level3);

    PQCSignature(const PQCSignature&) = delete;
    PQCSignature& operator=(const PQCSignature&) = delete;

    ~PQCSignature();

    /// Generate a post-quantum signature over the given message
    ///
    /// Uses Dilithium/ML-DSA with the configured parameter set.
    /// The signature is deterministic and cryptographically secure against
    /// quantum adversaries.
    ///
    /// @param secret_key The signer's secret key
    /// @param message The message bytes to sign
    /// @return Digital signature bytes, or error if signing fails
    Result<Signature> sign(std::span<const uint8_t> secret_key,
                          std::span<const uint8_t> message);

    /// Verify a post-quantum signature
    ///
    /// @param public_key The signer's public key
    /// @param message The original message bytes
    /// @param signature The signature to verify
    /// @return true if signature is valid, false if invalid or error occurred
    Result<bool> verify(std::span<const uint8_t> public_key,
                        std::span<const uint8_t> message,
                        std::span<const uint8_t> signature);

    /// Get the public key size in bytes
    size_t public_key_size() const;

    /// Get the secret key size in bytes
    size_t secret_key_size() const;

    /// Get the signature size in bytes
    size_t signature_size() const;

private:
    struct Impl;
    std::unique_ptr<Impl> m_impl;
};

/// Convenience function for one-off signature generation
///
/// Creates a temporary PQCSignature instance and signs the message.
/// For multiple signatures, create a PQCSignature instance directly.
///
/// @param secret_key The signer's secret key
/// @param message The message to sign
/// @param param_set Dilithium parameter set to use
/// @return Digital signature bytes, or error if signing fails
Result<Signature> pqc_sign(std::span<const uint8_t> secret_key,
                           std::span<const uint8_t> message,
                           DilithiumParameterSet param_set = DilithiumParameterSet::Level3);

/// Convenience function for one-off signature verification
///
/// @param public_key The signer's public key
/// @param message The original message
/// @param signature The signature to verify
/// @param param_set Dilithium parameter set used for signing
/// @return true if signature is valid, false if invalid
Result<bool> pqc_verify(std::span<const uint8_t> public_key,
                        std::span<const uint8_t> message,
                        std::span<const uint8_t> signature,
                        DilithiumParameterSet param_set = DilithiumParameterSet::Level3);

} // namespace qv::crypto
