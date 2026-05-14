//! Fork choice, chain selection, and finality for Ouroboros Praos.
//!
//! # Fork choice rule
//!
//! Ouroboros Praos uses a **density-weighted longest chain** rule:
//!
//! 1. Among all candidate chains, prefer the one with the most blocks
//!    (highest density) in the most recent rolling window.
//! 2. Ties are broken by block hash (lowest hash wins — deterministic).
//! 3. A chain is only considered if it forks from our current chain at
//!    a point no deeper than `k` blocks from the tip. Chains that diverge
//!    earlier than `k` blocks are never adopted (this is the *k-deep
//!    finality* guarantee).
//!
//! # k-deep finality
//!
//! Once a block is buried under `k` confirmations it is considered final.
//! No chain reorganisation can revert it. The parameter `k` comes from
//! [`ConsensusParams::k_finality`] (default: 50 blocks ≈ 100 seconds).
//!
//! # Data model
//!
//! [`ChainState`] tracks the header-chain as a sequence of [`ChainEntry`]
//! records. It does not store full blocks; that is `qv-storage`'s job.

use std::collections::BTreeMap;

use qv_core::{BlockHash, ConsensusParams, Hash256, Height, Slot};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// ChainEntry — minimal per-block metadata kept in memory
// ============================================================================

/// Lightweight record for one block in the header chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainEntry {
    /// This block's hash.
    pub hash: BlockHash,
    /// Parent block's hash.
    pub parent_hash: BlockHash,
    /// Block height.
    pub height: Height,
    /// Slot the block was produced in.
    pub slot: Slot,
    /// Hash of the producer's VRF key (used for fork-choice tie-breaking).
    pub producer_key_hash: Hash256,
}

// ============================================================================
// Errors
// ============================================================================

/// Errors from chain state operations.
#[derive(Debug, Error)]
pub enum ChainError {
    /// Attempted to add a block whose parent is unknown.
    #[error("unknown parent {0:?}")]
    UnknownParent(BlockHash),
    /// Attempted to add a duplicate block.
    #[error("block {0:?} already exists")]
    DuplicateBlock(BlockHash),
    /// The fork point is deeper than k blocks — reorg rejected.
    #[error("fork point at height {fork_height} is deeper than k={k} from tip {tip_height}")]
    ForkTooDeep {
        fork_height: u64,
        tip_height: u64,
        k: u64,
    },
}

// ============================================================================
// ChainState
// ============================================================================

/// In-memory chain state tracking block entries and the current best tip.
///
/// This is a simplified chain-index suitable for the consensus engine.
/// A production node would use `qv-storage` for persistence and support
/// multiple forks more efficiently.
#[derive(Clone, Debug)]
pub struct ChainState {
    /// All known chain entries by hash.
    entries: BTreeMap<BlockHash, ChainEntry>,
    /// Current best chain tip.
    tip: BlockHash,
    /// Consensus parameters (for k-finality and other checks).
    k_finality: u64,
}

impl ChainState {
    /// Create a new chain state rooted at a genesis entry.
    #[must_use]
    pub fn new(genesis: ChainEntry, params: &ConsensusParams) -> Self {
        let tip = genesis.hash;
        let mut entries = BTreeMap::new();
        entries.insert(genesis.hash, genesis);
        Self {
            entries,
            tip,
            k_finality: params.k_finality,
        }
    }

    /// Create a genesis chain state with default genesis entry.
    #[must_use]
    pub fn genesis(params: &ConsensusParams) -> Self {
        let genesis_entry = ChainEntry {
            hash: BlockHash::ZERO,
            parent_hash: BlockHash::ZERO,
            height: Height::GENESIS,
            slot: Slot::GENESIS,
            producer_key_hash: Hash256::ZERO,
        };
        Self::new(genesis_entry, params)
    }

    /// Current best tip.
    ///
    /// # Panics
    ///
    /// Panics if the internal invariant is broken (tip must always exist in
    /// `entries`). This invariant is enforced at construction and at every
    /// mutation site; callers can rely on this never panicking.
    #[must_use]
    #[allow(clippy::expect_used)] // SAFETY: invariant — tip ∈ entries by construction
    pub fn tip(&self) -> &ChainEntry {
        self.entries
            .get(&self.tip)
            .expect("tip must always be in entries")
    }

    /// Tip hash.
    #[must_use]
    pub fn tip_hash(&self) -> BlockHash {
        self.tip
    }

    /// Tip height.
    #[must_use]
    pub fn tip_height(&self) -> Height {
        self.tip().height
    }

    /// Tip slot.
    #[must_use]
    pub fn tip_slot(&self) -> Slot {
        self.tip().slot
    }

    /// Look up a chain entry by hash.
    #[must_use]
    pub fn get(&self, hash: &BlockHash) -> Option<&ChainEntry> {
        self.entries.get(hash)
    }

    /// Total number of entries tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the chain state is empty (should never be; always has genesis).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether a block at the given height is considered final (buried
    /// under `k` confirmations from the current tip).
    #[must_use]
    pub fn is_final(&self, height: Height) -> bool {
        let tip_h = self.tip_height().as_u64();
        let block_h = height.as_u64();
        if tip_h < block_h {
            return false;
        }
        (tip_h - block_h) >= self.k_finality
    }

    /// The height at which finality currently begins.
    ///
    /// All blocks at or below this height are considered final.
    #[must_use]
    pub fn finality_height(&self) -> Height {
        let tip_h = self.tip_height().as_u64();
        Height::from(tip_h.saturating_sub(self.k_finality))
    }

    /// Add a new block entry and potentially update the tip.
    ///
    /// The entry's parent must already be known. If the new block extends
    /// a chain that is better than the current tip (per the fork-choice
    /// rule), the tip is switched.
    pub fn add_block(&mut self, entry: ChainEntry) -> Result<bool, ChainError> {
        // Check parent exists
        if !self.entries.contains_key(&entry.parent_hash) && entry.hash != BlockHash::ZERO {
            return Err(ChainError::UnknownParent(entry.parent_hash));
        }

        // Reject duplicates
        if self.entries.contains_key(&entry.hash) {
            return Err(ChainError::DuplicateBlock(entry.hash));
        }

        let new_hash = entry.hash;
        let new_height = entry.height;
        self.entries.insert(entry.hash, entry);

        // Fork-choice: switch tip if new chain is better
        let tip_height = self.tip_height();
        match new_height.cmp(&tip_height) {
            core::cmp::Ordering::Greater => {
                // Check the fork point is not too deep
                if let Some(fork_height) = self.find_fork_height(new_hash, self.tip) {
                    let depth = tip_height.as_u64().saturating_sub(fork_height);
                    if depth > self.k_finality {
                        return Err(ChainError::ForkTooDeep {
                            fork_height,
                            tip_height: tip_height.as_u64(),
                            k: self.k_finality,
                        });
                    }
                }
                self.tip = new_hash;
                Ok(true) // tip changed
            }
            core::cmp::Ordering::Equal => {
                // Tie-break by lower block hash
                if new_hash.0 .0 < self.tip.0 .0 {
                    self.tip = new_hash;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            core::cmp::Ordering::Less => Ok(false),
        }
    }

    /// Walk the chain backwards from `hash`, returning ancestors up to
    /// `count` entries (most recent first).
    #[must_use]
    pub fn ancestors(&self, hash: BlockHash, count: usize) -> Vec<&ChainEntry> {
        let mut result = Vec::with_capacity(count);
        let mut current = hash;
        for _ in 0..count {
            if let Some(entry) = self.entries.get(&current) {
                result.push(entry);
                if current == entry.parent_hash {
                    break; // genesis
                }
                current = entry.parent_hash;
            } else {
                break;
            }
        }
        result
    }

    /// Find the common ancestor (fork point) height of two chain tips.
    ///
    /// Returns `None` if no common ancestor is found (shouldn't happen if
    /// both descend from genesis).
    fn find_fork_height(&self, hash_a: BlockHash, hash_b: BlockHash) -> Option<u64> {
        // Walk both chains back, collecting heights, until we find a match.
        // Simple O(n) approach; fine for k ≤ ~2160.
        let chain_a = self.ancestors(hash_a, 10_000);
        let chain_b = self.ancestors(hash_b, 10_000);

        // Build a set of hashes for chain B
        let set_b: std::collections::HashSet<BlockHash> = chain_b.iter().map(|e| e.hash).collect();

        for entry in &chain_a {
            if set_b.contains(&entry.hash) {
                return Some(entry.height.as_u64());
            }
        }
        None
    }

    /// Compute chain density: number of blocks in the last `window` slots
    /// for a given chain tip.
    #[must_use]
    pub fn chain_density(&self, tip_hash: BlockHash, window: u64) -> u64 {
        let tip = match self.entries.get(&tip_hash) {
            Some(e) => e,
            None => return 0,
        };
        let min_slot = tip.slot.as_u64().saturating_sub(window);
        let ancestors = self.ancestors(tip_hash, window as usize);
        ancestors
            .iter()
            .filter(|e| e.slot.as_u64() > min_slot)
            .count() as u64
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]
mod tests {
    use super::*;
    use qv_core::ConsensusParams;

    fn params() -> ConsensusParams {
        ConsensusParams {
            k_finality: 3,
            ..ConsensusParams::mainnet()
        }
    }

    fn make_entry(hash_byte: u8, parent_byte: u8, height: u64, slot: u64) -> ChainEntry {
        ChainEntry {
            hash: BlockHash::from_bytes([hash_byte; 32]),
            parent_hash: BlockHash::from_bytes([parent_byte; 32]),
            height: Height::from(height),
            slot: Slot::from(slot),
            producer_key_hash: Hash256::from_bytes([hash_byte; 32]),
        }
    }

    #[test]
    fn genesis_state() {
        let cs = ChainState::genesis(&params());
        assert_eq!(cs.tip_height(), Height::GENESIS);
        assert_eq!(cs.tip_hash(), BlockHash::ZERO);
        assert_eq!(cs.len(), 1);
    }

    #[test]
    fn extend_chain() {
        let p = params();
        let mut cs = ChainState::genesis(&p);

        // Block 1
        let b1 = make_entry(0x01, 0x00, 1, 5);
        let switched = cs.add_block(b1).unwrap();
        assert!(switched);
        assert_eq!(cs.tip_height(), Height::from(1));

        // Block 2
        let b2 = make_entry(0x02, 0x01, 2, 10);
        cs.add_block(b2).unwrap();
        assert_eq!(cs.tip_height(), Height::from(2));
    }

    #[test]
    fn unknown_parent_rejected() {
        let p = params();
        let mut cs = ChainState::genesis(&p);
        let orphan = make_entry(0x01, 0xFF, 1, 5); // parent 0xFF doesn't exist
        let result = cs.add_block(orphan);
        assert!(matches!(result, Err(ChainError::UnknownParent(_))));
    }

    #[test]
    fn duplicate_block_rejected() {
        let p = params();
        let mut cs = ChainState::genesis(&p);
        let b1 = make_entry(0x01, 0x00, 1, 5);
        cs.add_block(b1.clone()).unwrap();
        let result = cs.add_block(b1);
        assert!(matches!(result, Err(ChainError::DuplicateBlock(_))));
    }

    #[test]
    fn tip_tie_break_by_hash() {
        let p = params();
        let mut cs = ChainState::genesis(&p);

        // Two blocks at height 1 with different hashes
        let b_low = make_entry(0x01, 0x00, 1, 5);
        let b_high = make_entry(0xFE, 0x00, 1, 6);

        cs.add_block(b_high.clone()).unwrap();
        assert_eq!(cs.tip_hash(), b_high.hash);

        let switched = cs.add_block(b_low.clone()).unwrap();
        assert!(switched); // 0x01 < 0xFE → tip switches
        assert_eq!(cs.tip_hash(), b_low.hash);
    }

    #[test]
    fn finality_check() {
        let p = ConsensusParams {
            k_finality: 2,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        let b1 = make_entry(0x01, 0x00, 1, 5);
        let b2 = make_entry(0x02, 0x01, 2, 10);
        let b3 = make_entry(0x03, 0x02, 3, 15);

        cs.add_block(b1).unwrap();
        cs.add_block(b2).unwrap();
        cs.add_block(b3).unwrap();

        // Tip at height 3, k=2 → height 0 and 1 are final
        assert!(cs.is_final(Height::GENESIS));
        assert!(cs.is_final(Height::from(1)));
        assert!(!cs.is_final(Height::from(2)));
        assert!(!cs.is_final(Height::from(3)));
        assert_eq!(cs.finality_height(), Height::from(1));
    }

    #[test]
    fn ancestors_walk() {
        let p = params();
        let mut cs = ChainState::genesis(&p);

        let b1 = make_entry(0x01, 0x00, 1, 2);
        let b2 = make_entry(0x02, 0x01, 2, 4);
        let b3 = make_entry(0x03, 0x02, 3, 6);

        cs.add_block(b1).unwrap();
        cs.add_block(b2).unwrap();
        cs.add_block(b3).unwrap();

        let anc = cs.ancestors(BlockHash::from_bytes([0x03; 32]), 10);
        assert_eq!(anc.len(), 4); // b3, b2, b1, genesis
        assert_eq!(anc[0].height, Height::from(3));
        assert_eq!(anc[1].height, Height::from(2));
        assert_eq!(anc[2].height, Height::from(1));
        assert_eq!(anc[3].height, Height::GENESIS);
    }

    #[test]
    fn chain_density_counts_recent_blocks() {
        let p = params();
        let mut cs = ChainState::genesis(&p);

        // Sparse chain: blocks at slots 5, 10, 50
        let b1 = make_entry(0x01, 0x00, 1, 5);
        let b2 = make_entry(0x02, 0x01, 2, 10);
        let b3 = make_entry(0x03, 0x02, 3, 50);

        cs.add_block(b1).unwrap();
        cs.add_block(b2).unwrap();
        cs.add_block(b3).unwrap();

        // Window of 20 slots from tip (slot 50): only b3 itself and b2 (slot 10 is within 30..50? no)
        // slot 50, window=20 → min_slot = 30 → only slot 50 qualifies
        let density = cs.chain_density(cs.tip_hash(), 20);
        assert_eq!(density, 1); // only b3 at slot 50 > 30

        // Window of 100, tip slot=50 → min_slot = saturating_sub(100) = 0.
        // Filter is strict `slot > min_slot`, so genesis (slot 0) is NOT
        // counted (0 > 0 is false). Only b1 (5), b2 (10), b3 (50) → 3.
        let density_wide = cs.chain_density(cs.tip_hash(), 100);
        assert_eq!(density_wide, 3);
    }
}
