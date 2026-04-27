#pragma once

#include <cstdint>
#include <vector>
#include <array>
#include <string>
#include <memory>
#include <optional>

#include "pow_engine.hpp"
#include "pos_validator.hpp"
#include "chain_state.hpp"

namespace qv::consensus {

// Forward declarations
struct Transaction;
struct Block;

/**
 * Validation error codes
 */
enum class ValidationError {
    OK = 0,
    INVALID_POW = 1,
    INVALID_HEADER = 2,
    INVALID_MERKLE_ROOT = 3,
    INVALID_TRANSACTION = 4,
    DUPLICATE_TRANSACTION = 5,
    INVALID_SIGNATURE = 6,
    DOUBLE_SPEND = 7,
    INSUFFICIENT_FINALITY = 8,
    INVALID_TIMESTAMP = 9,
    INVALID_DIFFICULTY = 10,
    ORPHAN_BLOCK = 11,
    INVALID_HEIGHT = 12,
};

/**
 * Result type for validation operations
 */
template<typename T = void>
class Result {
public:
    Result() : error_(ValidationError::OK), value_() {}
    Result(ValidationError err) : error_(err) {}
    Result(const T& val) : error_(ValidationError::OK), value_(val) {}

    bool is_ok() const { return error_ == ValidationError::OK; }
    bool is_err() const { return error_ != ValidationError::OK; }

    ValidationError error() const { return error_; }
    const T& value() const { return value_; }

    static Result<T> ok(const T& val) { return Result<T>(val); }
    static Result<T> err(ValidationError err) { return Result<T>(err); }

private:
    ValidationError error_;
    T value_;
};

// Specialization for void
template<>
class Result<void> {
public:
    Result() : error_(ValidationError::OK) {}
    Result(ValidationError err) : error_(err) {}

    bool is_ok() const { return error_ == ValidationError::OK; }
    bool is_err() const { return error_ != ValidationError::OK; }

    ValidationError error() const { return error_; }

    static Result<void> ok() { return Result<void>(); }
    static Result<void> err(ValidationError err) { return Result<void>(err); }

private:
    ValidationError error_;
};

/**
 * UTXO Set for transaction validation
 *
 * Represents unspent transaction outputs available for spending.
 * Used to detect double spends and validate transaction inputs.
 */
struct UTXOSet {
    // Maps (txid, output_index) -> (pubkey, amount)
    std::map<std::pair<std::array<uint8_t, 32>, uint32_t>,
             std::pair<PublicKey, Amount>> utxos;

    bool has_utxo(const std::array<uint8_t, 32>& txid, uint32_t output_idx) const;
    std::optional<std::pair<PublicKey, Amount>> get_utxo(
        const std::array<uint8_t, 32>& txid, uint32_t output_idx) const;
    void spend_utxo(const std::array<uint8_t, 32>& txid, uint32_t output_idx);
    void add_utxo(const std::array<uint8_t, 32>& txid, uint32_t output_idx,
                  const PublicKey& pubkey, Amount amount);
};

/**
 * Full Block Validator
 *
 * Validates all aspects of a block:
 * 1. Header validity: PoW puzzle solved, timestamps valid, height correct
 * 2. Transaction validity: signatures valid, no double spends, inputs exist
 * 3. Finality: PoS committee votes confirming block
 * 4. Structural: merkle tree correct, no missing fields
 *
 * Implements the consensus rules for the hybrid PoW+PoS system.
 */
class BlockValidator {
public:
    BlockValidator() = default;
    ~BlockValidator() = default;

    // ========== Main Validation Entry Points ==========

    /**
     * Fully validate a block against chain state
     *
     * Performs all validation checks:
     * - Header validation (PoW, timestamps, height, difficulty)
     * - Transaction validation (signatures, double spends, UTXOs)
     * - Finality validation (PoS votes if finality is claimed)
     * - Structural validation (merkle root, parent link)
     *
     * @param block The block to validate
     * @param chain_state Current chain state
     * @param utxo_set Current UTXO set for double-spend detection
     * @param pow_params PoW parameters (difficulty settings)
     * @return ok() if all validations pass, err() with specific failure code
     */
    Result<void> validate_block(const Block& block,
                               const ChainState& chain_state,
                               const UTXOSet& utxo_set,
                               const PowParams& pow_params);

    /**
     * Validate a block header only
     *
     * Checks:
     * - PoW puzzle is solved (nonce is valid)
     * - Difficulty matches chain state
     * - Timestamp is reasonable (not too far in future, monotonic with parent)
     * - Height correctly increments from parent
     * - Parent block exists on canonical chain
     *
     * @param header Block header to validate
     * @param chain_state Current chain state
     * @param pow_params PoW parameters
     * @return ok() if header is valid, err() with specific code
     */
    Result<void> validate_header(const BlockHeader& header,
                                const ChainState& chain_state,
                                const PowParams& pow_params);

    /**
     * Validate all transactions in a block
     *
     * Checks:
     * - All transaction signatures are valid
     * - No duplicate transactions within block
     * - No double spends (all inputs exist in UTXO set)
     * - All input UTXOs are confirmed (past finality point)
     * - Transaction count limits not exceeded
     *
     * @param block The block containing transactions
     * @param utxo_set Current UTXO set for validation
     * @return ok() if all transactions valid, err() with code
     */
    Result<void> validate_transactions(const Block& block,
                                      const UTXOSet& utxo_set);

    /**
     * Validate finality votes for a block
     *
     * Checks:
     * - Votes are from current committee members
     * - Vote signatures are valid
     * - 2/3+ of committee stake votes for this block
     * - No double voting (equivocation)
     *
     * @param block_hash Hash of the block
     * @param votes Vector of votes for this block
     * @param committee The stake committee
     * @return ok() if finality achieved, err() if not enough votes
     */
    Result<void> validate_finality(const BlockHash& block_hash,
                                  const std::vector<Vote>& votes,
                                  const StakeCommittee& committee);

    // ========== Component Validation ==========

    /**
     * Verify a single transaction signature
     *
     * @param tx Transaction with signature field
     * @param pubkey Public key to verify against
     * @return true if signature is valid, false otherwise
     */
    bool verify_transaction_signature(const Transaction& tx,
                                     const PublicKey& pubkey) const;

    /**
     * Verify merkle root of transactions
     *
     * Computes merkle tree from transactions and checks against header's merkle_root.
     *
     * @param block Block with transactions
     * @return true if merkle root matches, false otherwise
     */
    bool verify_merkle_root(const Block& block) const;

    /**
     * Check if block timestamp is valid relative to parent
     *
     * @param block_timestamp Timestamp of current block
     * @param parent_timestamp Timestamp of parent block
     * @param max_future_time_ms Max milliseconds block can be in future
     * @return true if timestamp is valid
     */
    bool verify_timestamp(uint64_t block_timestamp,
                         uint64_t parent_timestamp,
                         uint64_t max_future_time_ms = 15000) const;

    /**
     * Verify height is correct relative to parent
     *
     * @param block_height Height of current block
     * @param parent_height Height of parent block
     * @return true if height is parent_height + 1
     */
    bool verify_height(Height block_height, Height parent_height) const {
        return block_height == parent_height + 1;
    }

    /**
     * Check if difficulty matches chain state expectations
     *
     * @param block_difficulty Difficulty in block
     * @param chain_difficulty Current chain difficulty
     * @return true if matches or within tolerance
     */
    bool verify_difficulty(uint64_t block_difficulty,
                          uint64_t chain_difficulty) const;

    // ========== Utility Functions ==========

    /**
     * Convert validation error to human-readable string
     */
    static std::string error_to_string(ValidationError err);

    /**
     * Compute merkle root from transaction list
     */
    std::array<uint8_t, 32> compute_merkle_root(
        const std::vector<Transaction>& transactions) const;

private:
    PowEngine pow_engine_;
    PosValidator pos_validator_;

    /**
     * Hash a single transaction for merkle tree construction
     */
    std::array<uint8_t, 32> hash_transaction(const Transaction& tx) const;

    /**
     * Compute merkle tree parent node
     */
    std::array<uint8_t, 32> merkle_parent(
        const std::array<uint8_t, 32>& left,
        const std::array<uint8_t, 32>& right) const;

    /**
     * Check for duplicate transactions in block
     */
    bool has_duplicate_transactions(const std::vector<Transaction>& txs) const;

    /**
     * Serialize transaction for signature verification
     */
    std::vector<uint8_t> serialize_transaction(const Transaction& tx) const;
};

}  // namespace qv::consensus
