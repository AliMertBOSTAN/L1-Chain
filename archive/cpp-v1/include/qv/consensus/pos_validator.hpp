#pragma once

#include <cstdint>
#include <vector>
#include <array>
#include <memory>
#include <optional>

namespace qv::consensus {

// Type aliases
using PublicKey = std::array<uint8_t, 32>;  // Ed25519 or similar
using BlockHash = std::array<uint8_t, 32>;
using Amount = uint64_t;
using Height = uint64_t;
using Epoch = uint64_t;

/**
 * Information about a validator in the stake-weighted committee
 */
struct ValidatorInfo {
    /// Validator's public key for signature verification
    PublicKey key;

    /// Amount of stake locked in the validator (microunits)
    Amount stake;

    /// Block height at which this validator becomes active
    Height activation_height;

    /// Whether the validator is currently slashable
    bool active = true;

    ValidatorInfo() = default;
    ValidatorInfo(const PublicKey& k, Amount s, Height h)
        : key(k), stake(s), activation_height(h), active(true) {}
};

/**
 * Vote cast by a validator for block finalization
 */
struct Vote {
    /// Hash of the block being voted on
    BlockHash block_hash;

    /// Height of the block being voted on
    Height block_height;

    /// Public key of the voting validator
    PublicKey validator_key;

    /// Epoch in which this vote was cast
    Epoch epoch;

    /// Ed25519 signature over (block_hash, block_height, epoch)
    std::vector<uint8_t> signature;

    Vote() = default;
    Vote(const BlockHash& bh, Height h, const PublicKey& vk, Epoch e)
        : block_hash(bh), block_height(h), validator_key(vk), epoch(e) {}
};

/**
 * Committee of validators selected for an epoch
 *
 * Contains the set of validators who may vote for finality in this epoch,
 * along with their stake weights. Committee is deterministically selected
 * based on validator set and beacon randomness.
 */
struct StakeCommittee {
    /// Epoch number this committee is responsible for
    Epoch epoch;

    /// Selected validators with their stake information
    std::vector<ValidatorInfo> members;

    /// Total stake in committee (sum of all members' stakes)
    Amount total_stake;

    /// Required votes to achieve 2/3+ supermajority
    Amount supermajority_threshold;

    StakeCommittee() : epoch(0), total_stake(0), supermajority_threshold(0) {}

    /**
     * Calculate the supermajority threshold after committee is populated
     * Returns true if threshold was updated
     */
    bool update_threshold();

    /**
     * Check if a given stake amount meets the supermajority threshold
     */
    bool meets_supermajority(Amount stake) const {
        return stake * 3 > total_stake * 2;  // > 2/3
    }
};

/**
 * Proof of Stake Validator
 *
 * Handles stake-weighted finality gadget. Validators with locked stake
 * participate in a committee that votes on block finalization. Requires
 * 2/3+ of total stake to vote for a block to finalize it.
 *
 * This provides economic security: validators must lock stake and risk
 * slashing if they vote incorrectly or equivocate.
 */
class PosValidator {
public:
    PosValidator() = default;
    ~PosValidator() = default;

    /**
     * Select committee members for an epoch
     *
     * Deterministically selects a subset of validators from the full validator set
     * using stake-weighted randomness. Larger stake = higher probability of selection.
     *
     * Selection is deterministic based on validator set and randomness beacon,
     * ensuring all nodes select the same committee without communication.
     *
     * @param epoch Epoch number for this committee
     * @param validator_set All active validators in the network
     * @param randomness Beacon randomness source for this epoch (e.g., hash of previous block)
     * @return Selected committee for the epoch
     */
    StakeCommittee select_committee(Epoch epoch,
                                    const std::vector<ValidatorInfo>& validator_set,
                                    const std::array<uint8_t, 32>& randomness);

    /**
     * Create a vote for a block
     *
     * Signs the block hash, height, and epoch with the validator's key.
     * This vote can be included in blocks or gossiped independently.
     *
     * @param block_hash Hash of the block to vote on
     * @param block_height Height of the block
     * @param validator_key Validator's public key (must match private key used for signing)
     * @param epoch Current epoch
     * @return Vote with cryptographic signature
     */
    Vote create_vote(const BlockHash& block_hash,
                    Height block_height,
                    const PublicKey& validator_key,
                    Epoch epoch);

    /**
     * Verify a vote's cryptographic signature
     *
     * Checks that the signature is valid for the given block hash, height, and epoch.
     * Does NOT check if the validator is in the committee or if the vote is current.
     *
     * @param vote Vote to verify
     * @return true if signature is valid, false otherwise
     */
    bool verify_vote(const Vote& vote) const;

    /**
     * Check if a block has achieved finality
     *
     * A block is finalized when 2/3+ of the committee stake has voted for it.
     * Finalized blocks cannot be reorged and are permanently added to canonical history.
     *
     * @param block_hash Hash of the block to check
     * @param votes All votes received for this block
     * @param committee The committee responsible for finality
     * @return true if block is finalized, false otherwise
     */
    bool check_finality(const BlockHash& block_hash,
                       const std::vector<Vote>& votes,
                       const StakeCommittee& committee) const;

    /**
     * Calculate the total stake weight of valid votes
     *
     * Aggregates stake weight of all valid votes in the vote set.
     * Filters out invalid votes, duplicate votes, and votes from non-committee members.
     *
     * @param votes Vector of votes to aggregate
     * @param committee Committee to verify voter membership
     * @return Total stake weight of valid votes
     */
    Amount calculate_vote_weight(const std::vector<Vote>& votes,
                                const StakeCommittee& committee) const;

private:
    /**
     * Perform deterministic stake-weighted selection
     *
     * Uses randomness to select validators with probability proportional to stake.
     * Ensures all nodes select identical committees.
     */
    std::vector<ValidatorInfo> weighted_random_selection(
        const std::vector<ValidatorInfo>& validator_set,
        const std::array<uint8_t, 32>& randomness,
        size_t committee_size);

    /**
     * Find a validator in a committee by public key
     */
    std::optional<ValidatorInfo> find_validator(
        const PublicKey& key,
        const StakeCommittee& committee) const;

    /**
     * Serialize vote data for signature verification
     */
    std::vector<uint8_t> serialize_vote_data(
        const BlockHash& block_hash,
        Height block_height,
        Epoch epoch) const;
};

}  // namespace qv::consensus
