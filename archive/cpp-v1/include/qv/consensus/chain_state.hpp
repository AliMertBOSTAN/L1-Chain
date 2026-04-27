#pragma once

#include <cstdint>
#include <vector>
#include <array>
#include <map>
#include <memory>
#include <optional>

#include "pow_engine.hpp"
#include "pos_validator.hpp"

namespace qv::consensus {

/**
 * Represents a block in the consensus view
 * (Minimal structure; full Block in application layer)
 */
struct BlockInfo {
    BlockHash hash;
    Height height;
    uint64_t timestamp;
    uint64_t difficulty;
    bool is_finalized = false;

    BlockInfo() = default;
    BlockInfo(const BlockHash& h, Height ht, uint64_t ts, uint64_t diff)
        : hash(h), height(ht), timestamp(ts), difficulty(diff) {}
};

/**
 * Chain State Tracker
 *
 * Maintains the current state of the blockchain from the consensus layer perspective.
 * Tracks:
 * - Current canonical chain tip and height
 * - Difficulty adjustment state
 * - Validator set and current epoch
 * - Fork choice rule (heaviest finalized chain with most accumulated work)
 *
 * The chain state is deterministic and synchronized across honest nodes,
 * ensuring consensus on the canonical chain.
 */
class ChainState {
public:
    ChainState() = default;
    ~ChainState() = default;

    // ========== Block Chain Tracking ==========

    /**
     * Get current height of the canonical chain
     */
    Height current_height() const { return chain_height_; }

    /**
     * Get the hash of the current best block
     */
    const BlockHash& best_block_hash() const { return best_block_hash_; }

    /**
     * Get the hash of the last finalized block
     *
     * Finalized blocks cannot be reorged and represent true consensus.
     */
    const BlockHash& last_finalized_hash() const { return last_finalized_hash_; }

    /**
     * Get the height of the last finalized block
     */
    Height last_finalized_height() const { return last_finalized_height_; }

    /**
     * Update the canonical chain with a new block
     *
     * Applies fork choice rule: selects chain with most cumulative work
     * that contains the most recent finalized block as ancestor.
     *
     * @param block_info Information about the new block
     * @param is_finalized Whether this block has achieved finality
     * @return true if chain was updated, false if block rejected
     */
    bool add_block(const BlockInfo& block_info, bool is_finalized = false);

    /**
     * Retrieve information about a specific block
     */
    std::optional<BlockInfo> get_block_info(const BlockHash& hash) const;

    /**
     * Check if a block is an ancestor of the current canonical chain
     */
    bool is_on_canonical_chain(const BlockHash& hash) const;

    // ========== Difficulty Management ==========

    /**
     * Get the current difficulty target
     */
    uint64_t current_difficulty() const { return current_difficulty_; }

    /**
     * Update difficulty based on recent blocks
     *
     * Called when a difficulty adjustment period is reached.
     * Adjusts difficulty to maintain target block time.
     *
     * @param recent_timestamps Timestamps of recent blocks
     * @param target_block_time_ms Target milliseconds per block
     */
    void update_difficulty(const std::vector<uint64_t>& recent_timestamps,
                          uint64_t target_block_time_ms = 12000);  // 12s target

    /**
     * Set difficulty directly (for genesis or testing)
     */
    void set_difficulty(uint64_t difficulty) { current_difficulty_ = difficulty; }

    // ========== Epoch and Validator Management ==========

    /**
     * Get the current epoch number
     *
     * Epochs are periods during which the same committee validates blocks.
     * Typically 32 blocks per epoch.
     */
    Epoch current_epoch() const { return current_epoch_; }

    /**
     * Get the blocks per epoch (constant for network)
     */
    static constexpr Height blocks_per_epoch() { return 32; }

    /**
     * Check if a new epoch has started at this height
     */
    bool is_epoch_boundary(Height height) const {
        return height > 0 && height % blocks_per_epoch() == 0;
    }

    /**
     * Add a validator to the active validator set
     *
     * Validator becomes active at activation_height.
     * Stakes are locked and at risk of slashing.
     *
     * @param validator Validator info including stake and activation height
     * @return true if added, false if duplicate key
     */
    bool add_validator(const ValidatorInfo& validator);

    /**
     * Remove a validator from the active set
     *
     * Validator's stake is unlocked after withdrawal delay.
     * Cannot be called on finalized blocks (immutability).
     *
     * @param key Validator's public key
     * @return true if removed, false if not found
     */
    bool remove_validator(const PublicKey& key);

    /**
     * Get current set of active validators
     */
    const std::vector<ValidatorInfo>& get_validator_set() const { return validators_; }

    /**
     * Get validator by public key
     */
    std::optional<ValidatorInfo> get_validator(const PublicKey& key) const;

    /**
     * Get total stake in the system
     */
    Amount total_validator_stake() const { return total_validator_stake_; }

    // ========== State Transitions ==========

    /**
     * Advance epoch and update validator committee
     *
     * Called at epoch boundaries. Selects new committee for next epoch
     * based on randomness beacon.
     *
     * @param beacon_randomness Randomness for committee selection
     * @return New committee for upcoming epoch
     */
    StakeCommittee advance_epoch(const std::array<uint8_t, 32>& beacon_randomness);

    /**
     * Get the current committee for the active epoch
     */
    const StakeCommittee& current_committee() const { return current_committee_; }

    /**
     * Finalize a block
     *
     * Once a block is finalized, it becomes immutable and cannot be forked away.
     * Finalization implies all ancestors are also finalized.
     *
     * @param hash Hash of the block to finalize
     * @return true if finalized, false if not on canonical chain
     */
    bool finalize_block(const BlockHash& hash);

    // ========== Fork Choice Rule ==========

    /**
     * Calculate cumulative work (sum of difficulties) for a chain
     *
     * Used in fork choice: heaviest chain with valid finality wins.
     *
     * @param hash Hash of the block to calculate work up to
     * @return Cumulative work (u128 needed; represented as two u64s)
     */
    std::pair<uint64_t, uint64_t> get_cumulative_work(const BlockHash& hash) const;

    /**
     * Compare two chain tips and determine which wins fork choice rule
     *
     * Rules (in order):
     * 1. Block with higher last_finalized_height wins
     * 2. If same finalized height, block with more cumulative work wins
     * 3. If same work, block with lower hash (tiebreaker) wins
     */
    int compare_tips(const BlockHash& tip1, const BlockHash& tip2) const;

    // ========== Debugging and State Inspection ==========

    /**
     * Get total number of blocks stored
     */
    size_t block_count() const { return block_store_.size(); }

    /**
     * Print chain state for debugging
     */
    void debug_print() const;

private:
    Height chain_height_ = 0;
    BlockHash best_block_hash_;
    BlockHash last_finalized_hash_;
    Height last_finalized_height_ = 0;
    uint64_t current_difficulty_ = 24;  // Default difficulty
    Epoch current_epoch_ = 0;

    // Block storage: hash -> BlockInfo
    std::map<BlockHash, BlockInfo> block_store_;

    // Cumulative work: hash -> work
    std::map<BlockHash, std::pair<uint64_t, uint64_t>> cumulative_work_;

    // Validator management
    std::vector<ValidatorInfo> validators_;
    Amount total_validator_stake_ = 0;

    // Current committee for active epoch
    StakeCommittee current_committee_;

    // Helper: update best block based on fork choice rule
    void update_best_block(const BlockInfo& block_info);

    // Helper: add cumulative work
    void add_cumulative_work(const BlockHash& hash, uint64_t difficulty);

    // Helper: get parent block hash from block info
    std::optional<BlockHash> get_parent_hash(const BlockHash& hash) const;
};

}  // namespace qv::consensus
