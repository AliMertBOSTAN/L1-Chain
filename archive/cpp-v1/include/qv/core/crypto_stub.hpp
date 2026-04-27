#pragma once

/// Placeholder for cryptographic types
/// TODO: Replace with actual OpenSSL, libsodium, or liboqs integration

namespace qv::crypto {

/// Forward declaration for signature types
/// In a real implementation, this would be a proper cryptographic signature
/// supporting ECDSA, Ed25519, or post-quantum algorithms
struct Signature {
    std::vector<std::uint8_t> data;
};

/// TODO: Add the following crypto functions:
///
/// - sha256(const bytes& data) -> HashDigest
/// - sha256_double(const bytes& data) -> HashDigest  (Bitcoin-style)
/// - sign_ecdsa(const bytes& message, const PrivateKey& key) -> Signature
/// - verify_ecdsa(const bytes& message, const Signature& sig, const PublicKey& key) -> bool
/// - sign_ed25519(const bytes& message, const PrivateKey& key) -> Signature
/// - verify_ed25519(const bytes& message, const Signature& sig, const PublicKey& key) -> bool
/// - ripemd160(const bytes& data) -> HashDigest  (For address generation)
/// - hmac_sha256(const bytes& key, const bytes& data) -> HashDigest

} // namespace qv::crypto
