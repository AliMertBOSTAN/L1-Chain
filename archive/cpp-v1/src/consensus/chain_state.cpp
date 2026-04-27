#include "qv/consensus/chain_state.hpp"

#include <algorithm>
#include <numeric>
#include <iostream>

namespace qv::consensus {

bool ChainState::add_block(const BlockInfo& block_info, bool is_finalized) {
    // TODO: Implement block addition with fork choice rule
    // 1. Validate block not already in store
    // 2. Validate parent block exists (except for genesis)
    // 3. Check block height is correct (parent.height + 1)
    // 4. Add block to block_store_
    // 5. Calculate and store cumulative work
    // 6. Apply fork choice rule to update best_block_hash_
    // 7. If finalized, update last_finalized_hash_ and height
    // 8. Update chain_height_ if this block extends canonical chain
    // 9. Return true if added, false if rejected

    if (block_store_.find(block_info.hash) != block_store_.end()) {
        return false;  // Duplicate block
    }

    block_store_[block_info.hash] = block_info;

    // TODO: Implement fork choice and finality tracking
    if (is_finalized) {
        last_finalized_hash_ = block_info.hash;
        last_finalized_height_ = block_info.height;
    }

    update_best_block(block_info);
    return true;
}

std::optional<BlockInfo> ChainState::get_block_info(const BlockHash& hash) const {
    // TODO: Implement block lookup
    auto it = block_store_.find(hash);
    if (it != block_store_.end()) {
        return it->second;
    }
    return std::nullopt;
}

bool ChainState::is_on_canonical_chain(const BlockHash& hash) const {
    // TODO: Implement canonical chain membership test
    // 1. Start from best_block_hash_
    // 2. Trace back through parents until reaching genesis
    // 3. Check if hash appears in the chain
    // 4. Return true if found, false otherwise
    // NOTE: This is O(height) - could be optimized with caching

    // Stub: return false
    return false;
}

void ChainState::update_difficulty(const std::vector<uint64_t>& recent_timestamps,
                                  uint64_t target_block_time_ms) {
    // TODO: Implement difficulty adjustment
    // 1. Validate timestamps vector has sufficient length
    // 2. Calculate actual average block time
    // 3. Adjust difficulty proportionally
    // 4. Update current_difficulty_

    if (recent_timestamps.size() < 2) {
        return;
    }

    // TODO: Implement actual adjustment logic
    // Stub: no adjustment
}

bool ChainState::add_validator(const ValidatorInfo& validator) {
    // TODO: Implement validator addition
    // 1. Check for duplicate key
    // 2. Add to validators_ vector
    // 3. Update total_validator_stake_
    // 4. Return true if added, false if duplicate

    for (const auto& existing : validators_) {
        if (existing.key == validator.key) {
            return false;  // Duplicate validator key
        }
    }

    validators_.push_back(validator);
    total_validator_stake_ += validator.stake;
    return true;
}

bool ChainState::remove_validator(const PublicKey& key) {
    // TODO: Implement validator removal
    // 1. Find validator by key
    // 2. If found, remove from validators_
    // 3. Update total_validator_stake_
    // 4. Return true if removed, false if not found

    auto it = std::find_if(validators_.begin(), validators_.end(),
                          [&key](const ValidatorInfo& v) { return v.key == key; });

    if (it != validators_.end()) {
        total_validator_stake_ -= it->stake;
        validators_.erase(it);
        return true;
    }

    return false;
}

std::optional<ValidatorInfo> ChainState::get_validator(const PublicKey& key) const {
    // TODO: Implement validator lookup
    for (const auto& validator : validators_) {
        if (validator.key == key) {
            return validator;
        }
    }
    return std::nullopt;
}

StakeCommittee ChainState::advance_epoch(
    const std::array<uint8_t, 32>& beacon_randomness) {
    // TODO: Implement epoch advancement
    // 1. Increment current_epoch_
    // 2. Use PosValidator to select new committee
    // 3. Update current_committee_
    // 4. Return new committee

    current_epoch_++;

    PosValidator pos_validator;
    current_committee_ = pos_validator.select_committee(
        current_epoch_,
        validators_,
        beacon_randomness
    );

    return current_committee_;
}

bool ChainState::finalize_block(const BlockHash& hash) {
    // TODO: Implement block finalization
    // 1. Check block exists in store
    // 2. Check block is on canonical chain
    // 3. Set is_finalized flag in BlockInfo
    // 4. Update last_finalized_hash_ and last_finalized_height_
    // 5. Return true if finalized, false if not on canonical chain

    auto block = get_block_info(hash);
    if (!block.has_value()) {
        return false;
    }

    if (!is_on_canonical_chain(hash)) {
        return false;
    }

    // TODO: Implement finalization
    last_finalized_hash_ = hash;
    last_finalized_height_ = block->height;

    return true;
}

std::pair<uint64_t, uint64_t> ChainState::get_cumulative_work(
    const BlockHash& hash) const {
    // TODO: Implement cumulative work calculation
    // 1. Look up hash in cumulative_work_ map
    // 2. Return stored work, or (0, 0) if not found
    // NOTE: Work is represented as two uint64s since total work can exceed uint64_t

    auto it = cumulative_work_.find(hash);
    if (it != cumulative_work_.end()) {
        return it->second;
    }

    return {0, 0};
}

int ChainState::compare_tips(const BlockHash& tip1, const BlockHash& tip2) const {
    // TODO: Implement fork choice comparison
    // Rules (in order):
    // 1. Block with higher last_finalized_height wins (+1 for tip1, -1 for tip2)
    // 2. If same finalized height, block with more cumulative work wins
    // 3. If same work, block with lower hash (lexicographic) wins
    // 4. If identical, return 0

    auto block1 = get_block_info(tip1);
    auto block2 = get_block_info(tip2);

    if (!block1.has_value() || !block2.has_value()) {
        return 0;
    }

    // TODO: Implement comparison logic
    // Stub: return 0 (equal)
    return 0;
}

void ChainState::debug_print() const {
    // TODO: Implement debug output
    // Print:
    // - Current chain height
    // - Best block hash (hex)
    // - Last finalized height and hash
    // - Current difficulty
    // - Current epoch
    // - Number of validators
    // - Number of blocks stored

    std::cout << "=== ChainState Debug Info ===" << std::endl;
    std::cout << "Height: " << chain_height_ << std::endl;
    std::cout << "Last finalized height: " << last_finalized_height_ << std::endl;
    std::cout << "Current difficulty: " << current_difficulty_ << std::endl;
    std::cout << "Current epoch: " << current_epoch_ << std::endl;
    std::cout << "Active validators: " << validators_.size() << std::endl;
    std::cout << "Total stake: " << total_validator_stake_ << std::endl;
    std::cout << "Blocks stored: " << block_store_.size() << std::endl;
}

void ChainState::update_best_block(const BlockInfo& block_info) {
    // TODO: Implement best block update
    // 1. Compare new block against current best_block_hash_
    // 2. Apply fork choice rule
    // 3. If new block wins, update best_block_hash_ and chain_height_

    best_block_hash_ = block_info.hash;
    chain_height_ = block_info.height;
}

void ChainState::add_cumulative_work(const BlockHash& hash, uint64_t difficulty) {
    // TODO: Implement cumulative work tracking
    // 1. Get parent block's cumulative work
    // 2. Add difficulty to parent's work (with overflow handling for uint128)
    // 3. Store result in cumulative_work_ map

    // Stub: store zero work
    cumulative_work_[hash] = {0, 0};
}

std::optional<BlockHash> ChainState::get_parent_hash(const BlockHash& hash) const {
    // TODO: Implement parent block lookup
    // NOTE: BlockInfo currently doesn't store parent hash
    // Must either:
    // - Add parent_hash field to BlockInfo, or
    // - Trace back from hash using height and canonical chain
    // This is a future implementation detail

    return std::nullopt;
}

}  // namespace qv::consensus
