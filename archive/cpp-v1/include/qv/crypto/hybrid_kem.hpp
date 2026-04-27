#pragma once

#include "types.hpp"
#include "hash.hpp"
#include <span>
#include <memory>
#include <array>
#include <vector>

namespace qv::crypto {

/// Kyber/ML-KEM parameter sets (NIST standardized)
enum class KyberParameterSet {
    /// NIST Level 1 (128-bit post-quantum security)
    Level1,

    /// NIST Level 3 (192-bit post-quantum security)
    Level3,

    /// NIST Level 5 (256-bit post-quantum security)
    Level5,
};

/// Shared secret from key encapsulation (typically 32 bytes)
using SharedSecret = std::array<uint8_t, 32>;

/// Ciphertext produced by encapsulation (variable size depending on Kyber parameter set)
using HybridCiphertext = std::vector<uint8_t>;

/// Hybrid KEM keypair combining classical X25519 and post-quantum Kyber/ML-KEM
struct HybridKEMKeypair {
    /// X25519 classical public key (32 bytes)
    PublicKey x25519_public_key;

    /// X25519 classical secret key (32 bytes, secured)
    SecretKey x25519_secret_key;

    /// Kyber/ML-KEM public key (variable size)
    PublicKey kyber_public_key;

    /// Kyber/ML-KEM secret key (variable size, secured)
    SecretKey kyber_secret_key;
};

/// Result of hybrid key encapsulation
struct HybridEncapsulation {
    /// Combined ciphertext from X25519 DH and Kyber encapsulation
    HybridCiphertext ciphertext;

    /// Shared secret derived from both X25519 and Kyber
    SharedSecret shared_secret;
};

/// Configuration for hybrid key encapsulation
struct HybridKEMConfig {
    /// Kyber parameter set to use in combination with X25519
    KyberParameterSet kyber_param_set = KyberParameterSet::Level3;

    /// Hash algorithm for combining classical and PQC components
    HashAlgorithm kdf_hash = HashAlgorithm::SHA3_256;

    /// Get the Kyber public key size for the configured parameter set
    size_t kyber_public_key_size() const;

    /// Get the Kyber secret key size for the configured parameter set
    size_t kyber_secret_key_size() const;

    /// Get the Kyber ciphertext size for the configured parameter set
    size_t kyber_ciphertext_size() const;
};

/// Generate a hybrid KEM keypair (X25519 + Kyber)
///
/// Creates both classical (X25519) and post-quantum (Kyber/ML-KEM) keypairs.
/// This hybrid approach ensures forward secrecy against quantum adversaries while
/// maintaining compatibility with existing classical infrastructure.
///
/// @param config Configuration specifying Kyber parameter set
/// @return Hybrid keypair with both classical and PQC components, or error if generation fails
Result<HybridKEMKeypair> generate_hybrid_kem_keypair(const HybridKEMConfig& config = HybridKEMConfig{});

/// Hybrid key encapsulation scheme combining X25519 and Kyber/ML-KEM
class HybridKEM {
public:
    /// Create a new hybrid KEM instance with specified parameters
    explicit HybridKEM(const HybridKEMConfig& config = HybridKEMConfig{});

    HybridKEM(const HybridKEM&) = delete;
    HybridKEM& operator=(const HybridKEM&) = delete;

    ~HybridKEM();

    /// Encapsulate a shared secret using the peer's hybrid public key
    ///
    /// Performs:
    /// 1. X25519 elliptic curve Diffie-Hellman with the peer's X25519 public key
    /// 2. Kyber/ML-KEM key encapsulation with the peer's Kyber public key
    /// 3. Derives final shared secret via KDF combining both secrets
    ///
    /// The resulting ciphertext can be sent to the peer for decapsulation.
    /// Each call produces a fresh shared secret and ciphertext.
    ///
    /// @param peer_hybrid_public_key The recipient's hybrid public key
    /// @return Encapsulated ciphertext and derived shared secret, or error if encapsulation fails
    Result<HybridEncapsulation> encapsulate(const HybridKEMKeypair& peer_hybrid_public_key);

    /// Decapsulate a hybrid ciphertext to recover the shared secret
    ///
    /// Performs:
    /// 1. X25519 ECDH using local secret key and peer's X25519 public key from ciphertext
    /// 2. Kyber/ML-KEM decapsulation of the PQC component
    /// 3. Derives final shared secret via KDF combining both secrets
    ///
    /// The recovered shared secret will match the one generated during encapsulation.
    ///
    /// @param local_keypair The recipient's hybrid keypair (secret key used here)
    /// @param ciphertext The encapsulated ciphertext from peer
    /// @param peer_x25519_public_key Peer's X25519 public key for ECDH
    /// @return Shared secret, or error if decapsulation fails
    Result<SharedSecret> decapsulate(const HybridKEMKeypair& local_keypair,
                                     std::span<const uint8_t> ciphertext,
                                     std::span<const uint8_t> peer_x25519_public_key);

    /// Get the size of the Kyber public key component
    size_t kyber_public_key_size() const;

    /// Get the size of the Kyber secret key component
    size_t kyber_secret_key_size() const;

    /// Get the size of the Kyber ciphertext component
    size_t kyber_ciphertext_size() const;

    /// Get the total expected hybrid ciphertext size
    /// (X25519 public key + Kyber ciphertext)
    size_t expected_hybrid_ciphertext_size() const;

private:
    struct Impl;
    std::unique_ptr<Impl> m_impl;
};

/// Convenience function for one-off encapsulation
///
/// Encapsulates a shared secret for the peer's public key.
/// For multiple operations, create a HybridKEM instance directly.
///
/// @param peer_public_key The recipient's hybrid public key
/// @param config Configuration for the hybrid KEM
/// @return Encapsulated ciphertext and shared secret, or error if encapsulation fails
Result<HybridEncapsulation> hybrid_encapsulate(
    const HybridKEMKeypair& peer_public_key,
    const HybridKEMConfig& config = HybridKEMConfig{});

/// Convenience function for one-off decapsulation
///
/// @param local_keypair The recipient's hybrid keypair
/// @param ciphertext The encapsulated ciphertext from peer
/// @param peer_x25519_public_key Peer's X25519 public key
/// @param config Configuration matching the encapsulation setup
/// @return Shared secret, or error if decapsulation fails
Result<SharedSecret> hybrid_decapsulate(
    const HybridKEMKeypair& local_keypair,
    std::span<const uint8_t> ciphertext,
    std::span<const uint8_t> peer_x25519_public_key,
    const HybridKEMConfig& config = HybridKEMConfig{});

} // namespace qv::crypto
