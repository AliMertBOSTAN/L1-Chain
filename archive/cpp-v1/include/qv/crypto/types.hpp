#pragma once

#include <cstdint>
#include <vector>
#include <array>
#include <memory>
#include <span>
#include <expected>
#include <cstddef>

namespace qv::crypto {

/// Secure byte vector with zeroing allocator for sensitive data
class SecureBytes {
public:
    SecureBytes() = default;
    explicit SecureBytes(size_t size);
    SecureBytes(const uint8_t* data, size_t size);

    ~SecureBytes();

    SecureBytes(const SecureBytes& other);
    SecureBytes& operator=(const SecureBytes& other);

    SecureBytes(SecureBytes&& other) noexcept;
    SecureBytes& operator=(SecureBytes&& other) noexcept;

    uint8_t* data();
    const uint8_t* data() const;

    size_t size() const;
    bool empty() const;

    void resize(size_t new_size);
    void clear();
    void zero();

    uint8_t& operator[](size_t idx);
    const uint8_t& operator[](size_t idx) const;

    std::span<uint8_t> span();
    std::span<const uint8_t> const_span() const;

private:
    std::vector<uint8_t> m_data;
};

/// Cryptographic error types
enum class CryptoError {
    Success = 0,
    InvalidInput,
    InvalidKeySize,
    InvalidSignature,
    InvalidCiphertext,
    EncapsulationFailed,
    DecapsulationFailed,
    HashComputationFailed,
    KeyGenerationFailed,
    VerificationFailed,
    UnknownError,
};

/// Result type for cryptographic operations using std::expected pattern
template<typename T>
using Result = std::expected<T, CryptoError>;

// Type aliases for clarity
using PublicKey = SecureBytes;
using SecretKey = SecureBytes;
using Signature = std::vector<uint8_t>;

} // namespace qv::crypto
