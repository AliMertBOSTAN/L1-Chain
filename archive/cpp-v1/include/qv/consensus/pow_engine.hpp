#pragma once

#include <cstdint>
#include <vector>
#include <array>
#include <string>
#include <memory>

namespace qv::consensus {

// Type aliases for hash output
using HashDigest = std::array<uint8_t, 32>;  // 256-bit hash

// Block header structure (forward declaration)
struct BlockHeader {
    uint32_t version;
    std::array<uint8_t, 32> prev_block_hash;
    std::array<uint8_t, 32> merkle_root;
    uint64_t timestamp;
    uint64_t nonce;
    uint64_t difficulty_target;
};

// Proof of Work parameters
struct PowParams {
    /// Target difficulty for block production (bits in leading zeros)
    uint64_t difficulty_target;

    /// Argon2id time cost parameter (iterations)
    uint32_t argon2_time_cost;

    /// Argon2id memory cost parameter (in KiB)
    uint32_t argon2_memory_cost;

    /// Argon2id parallelism parameter (lanes)
    uint32_t argon2_parallelism;

    // Default parameters for testnet
    static PowParams testnet_params() {
        return {
            .difficulty_target = 24,  // 24 leading zero bits
            .argon2_time_cost = 2,
            .argon2_memory_cost = 65536,  // 64 MiB
            .argon2_parallelism = 4
        };
    }

    // Default parameters for mainnet
    static PowParams mainnet_params() {
        return {
            .difficulty_target = 28,  // 28 leading zero bits
            .argon2_time_cost = 3,
            .argon2_memory_cost = 131072,  // 128 MiB
            .argon2_parallelism = 8
        };
    }
};

/**
 * Proof of Work Engine
 *
 * Implements quantum-resistant proof of work using Argon2id hash function.
 * This provides the computational work component of the hybrid consensus.
 * Argon2id is memory-hard and resistant to GPU/ASIC attacks while being
 * quantum-resistant, making it suitable for long-term blockchain security.
 */
class PowEngine {
public:
    PowEngine() = default;
    ~PowEngine() = default;

    /**
     * Mine a block by finding a valid nonce
     *
     * Iteratively adjusts the block nonce until the computed Argon2id hash
     * meets the difficulty target (required number of leading zero bits).
     *
     * @param header Block header to mine (nonce field will be modified)
     * @param params Proof of Work parameters including difficulty target
     * @return true if a valid nonce was found, false if mining stopped early
     */
    bool mine(BlockHeader& header, const PowParams& params);

    /**
     * Verify that a block header meets the Proof of Work requirement
     *
     * Computes the Argon2id hash of the block header and checks if it
     * has the required number of leading zero bits specified by difficulty_target.
     *
     * @param header Block header to verify
     * @param params Proof of Work parameters
     * @return true if the PoW is valid and meets difficulty, false otherwise
     */
    bool verify_pow(const BlockHeader& header, const PowParams& params) const;

    /**
     * Compute the quantum-resistant PoW hash for a block header
     *
     * Uses Argon2id with the parameters specified in PowParams.
     * This is the core computation that makes mining time-consuming.
     *
     * @param header Block header to hash
     * @param params PoW parameters (specifying Argon2id config)
     * @return 256-bit hash digest
     */
    HashDigest compute_pow_hash(const BlockHeader& header, const PowParams& params) const;

    /**
     * Adjust difficulty target based on recent chain state
     *
     * Implements difficulty adjustment algorithm to maintain ~constant block time.
     * Examines recent blocks' timestamps and adjusts difficulty_target up or down
     * to ensure target block production rate is maintained.
     *
     * @param recent_block_times Recent block timestamps (e.g., last 2016 blocks)
     * @param target_block_time_ms Target milliseconds between blocks
     * @param current_difficulty Current difficulty target
     * @return Adjusted difficulty target
     */
    uint64_t adjust_difficulty(const std::vector<uint64_t>& recent_block_times,
                               uint64_t target_block_time_ms,
                               uint64_t current_difficulty) const;

private:
    /**
     * Count the number of leading zero bits in a hash digest
     *
     * @param hash Hash digest to examine
     * @return Number of leading zero bits
     */
    uint32_t count_leading_zero_bits(const HashDigest& hash) const;

    /**
     * Serialize block header for hashing
     *
     * Converts block header to byte buffer suitable for Argon2id input.
     *
     * @param header Header to serialize
     * @return Serialized header bytes
     */
    std::vector<uint8_t> serialize_header(const BlockHeader& header) const;
};

}  // namespace qv::consensus
