#pragma once

#include "types.hpp"
#include <span>
#include <array>
#include <cstdint>

namespace qv::crypto {

/// 32-byte cryptographic hash digest
using HashDigest = std::array<uint8_t, 32>;

/// Enumeration of supported hash algorithms
enum class HashAlgorithm {
    SHA3_256,
    BLAKE3,
};

/// Compute a single-pass cryptographic hash of the input data
///
/// @param algorithm The hash algorithm to use (SHA3-256 or BLAKE3)
/// @param data Input bytes to hash
/// @return 32-byte hash digest, or error if computation fails
Result<HashDigest> hash(HashAlgorithm algorithm, std::span<const uint8_t> data);

/// Convenience overload for SHA3-256
/// @param data Input bytes to hash
/// @return 32-byte hash digest
Result<HashDigest> sha3_256(std::span<const uint8_t> data);

/// Convenience overload for BLAKE3
/// @param data Input bytes to hash
/// @return 32-byte hash digest
Result<HashDigest> blake3(std::span<const uint8_t> data);

/// Compute the double hash of input data (hash of hash)
/// Commonly used in blockchain Merkle tree constructions
///
/// @param algorithm The hash algorithm to use
/// @param data Input bytes to hash
/// @return Double-hashed digest (hash(hash(data)))
Result<HashDigest> double_hash(HashAlgorithm algorithm, std::span<const uint8_t> data);

/// Compute a double SHA3-256 hash
/// @param data Input bytes to hash
/// @return Double-hashed digest using SHA3-256
Result<HashDigest> double_sha3_256(std::span<const uint8_t> data);

/// Compute a double BLAKE3 hash
/// @param data Input bytes to hash
/// @return Double-hashed digest using BLAKE3
Result<HashDigest> double_blake3(std::span<const uint8_t> data);

/// Compute incremental hash for streaming data (stateful hasher)
/// Useful for large files or streaming sources
class Hasher {
public:
    /// Create a new stateful hasher with the specified algorithm
    explicit Hasher(HashAlgorithm algorithm);

    Hasher(const Hasher&) = delete;
    Hasher& operator=(const Hasher&) = delete;

    Hasher(Hasher&& other) noexcept;
    Hasher& operator=(Hasher&& other) noexcept;

    ~Hasher();

    /// Update the hash state with additional data
    /// @param data Bytes to include in the hash computation
    /// @return Error code if update fails
    Result<void> update(std::span<const uint8_t> data);

    /// Finalize the hash computation and return the digest
    /// @return Final 32-byte hash digest
    Result<HashDigest> finalize();

private:
    struct Impl;
    std::unique_ptr<Impl> m_impl;
};

} // namespace qv::crypto
