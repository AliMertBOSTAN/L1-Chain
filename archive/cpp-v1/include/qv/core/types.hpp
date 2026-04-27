#pragma once

#include <array>
#include <cstdint>
#include <vector>
#include <chrono>

namespace qv::core {

/// TxId: 32-byte transaction hash (SHA256)
using TxId = std::array<std::uint8_t, 32>;

/// BlockHash: 32-byte block hash (SHA256)
using BlockHash = std::array<std::uint8_t, 32>;

/// HashDigest: Generic 32-byte hash digest for merkle roots, commitments, etc.
using HashDigest = std::array<std::uint8_t, 32>;

/// Height: Block height (chain position)
using Height = std::uint64_t;

/// Amount: Transaction amount in satoshi-like smallest units
using Amount = std::uint64_t;

/// Timestamp: Unix timestamp (seconds since epoch)
using Timestamp = std::uint64_t;

/// OutputIndex: Index of an output within a transaction
using OutputIndex = std::uint32_t;

/// Bytes: Generic byte vector for flexible binary data
using bytes = std::vector<std::uint8_t>;

/// OutPoint: Reference to a specific unspent output (txid + output_index)
/// Represents the location of a UTXO being spent
struct OutPoint {
    TxId tx_id;
    OutputIndex index;

    /// Default constructor
    OutPoint() = default;

    /// Constructor with values
    OutPoint(const TxId& id, OutputIndex idx) noexcept
        : tx_id(id), index(idx) {}

    /// Equality comparison
    bool operator==(const OutPoint& other) const noexcept {
        return tx_id == other.tx_id && index == other.index;
    }

    /// Inequality comparison
    bool operator!=(const OutPoint& other) const noexcept {
        return !(*this == other);
    }

    /// Less-than comparison (for use in ordered containers)
    bool operator<(const OutPoint& other) const noexcept {
        if (tx_id != other.tx_id) {
            return tx_id < other.tx_id;
        }
        return index < other.index;
    }
};

/// Numeric constants for blockchain parameters
namespace constants {
    /// Maximum transaction size in bytes
    constexpr std::uint32_t MAX_TX_SIZE = 4'000'000;

    /// Maximum number of inputs in a transaction
    constexpr std::uint32_t MAX_TX_INPUTS = 10'000;

    /// Maximum number of outputs in a transaction
    constexpr std::uint32_t MAX_TX_OUTPUTS = 10'000;

    /// Satoshi precision (smallest unit)
    constexpr Amount SATOSHI = 1;

    /// One unit in satoshis
    constexpr Amount ONE_UNIT = 100'000'000;

    /// Maximum total supply in satoshi units
    constexpr Amount MAX_SUPPLY = 21'000'000 * ONE_UNIT;
} // namespace constants

} // namespace qv::core
