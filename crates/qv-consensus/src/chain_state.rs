//! Fork choice, chain selection, and finality for Ouroboros Praos.
//!
//! # Fork choice rule
//!
//! The current rule is **longest chain by height** with a deterministic
//! tie-break:
//!
//! 1. Among candidate tips, prefer the one with the greatest height.
//! 2. Ties are broken by block hash (lowest hash wins — deterministic).
//! 3. A block is only accepted if it descends from the finalized block
//!    (see below). Blocks on a branch that conflicts with finalized
//!    history are rejected outright, in every fork-choice path.
//!
//! Note: density-weighting (Ouroboros Genesis maxvalid-bg) is not yet wired
//! into the fork choice — [`ChainState::chain_density`] is a building block
//! for it but is currently unused. Planned in ADR-008. See also
//! `docs/security/qv-consensus-fork-finality-audit.md`.
//!
//! # k-deep finality
//!
//! The chain tracks an explicit **finalized point** (`final_hash` /
//! `final_height`) that sits `k` blocks below the tip. It is **sticky and
//! monotonic**: it only ever moves forward, and once a block is final no
//! fork-choice activity can revert it. Any incoming block that does not
//! descend from the finalized block is rejected. The parameter `k` comes
//! from [`ConsensusParams::k_finality`] (default: 50 blocks ≈ 100 seconds).
//!
//! This guarantees *local* finality safety (one node never finalizes two
//! conflicting blocks) and, combined with the deterministic fork choice,
//! cross-node agreement when nodes share the same blocks. It is **not** a
//! BFT absolute-finality guarantee under network partition — that needs a
//! finality gadget (tracked for a future ADR).
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
// EquivocationProof — evidence of a producer breaking one-block-per-slot
// ============================================================================

/// Evidence that a block producer equivocated: it produced two distinct
/// blocks for the same slot.
///
/// In Ouroboros Praos a slot leader must produce at most one block for its
/// slot. Two distinct blocks at the same `(slot, producer)` is provable
/// misbehaviour. The chain records this as evidence for the slashing /
/// accountability layer; the blocks themselves are *not* rejected — fork
/// choice resolves them deterministically and equivocation can never add
/// chain length (the two blocks are siblings, not a longer chain).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquivocationProof {
    /// Slot in which the equivocation occurred.
    pub slot: Slot,
    /// Hash of the offending producer's key.
    pub producer_key_hash: Hash256,
    /// First block seen for this `(slot, producer)` pair.
    pub first: BlockHash,
    /// Second, conflicting block seen for the same `(slot, producer)` pair.
    pub second: BlockHash,
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
    /// The block is on a branch that conflicts with finalized history.
    ///
    /// It does not descend from the current finalized block, so adopting it
    /// would revert finality — which is forbidden.
    #[error(
        "block at height {block_height} conflicts with finalized history \
         (finalized at height {final_height})"
    )]
    ConflictsWithFinalized {
        final_height: u64,
        block_height: u64,
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
    /// Consensus parameter `k` (depth at which a block becomes final).
    k_finality: u64,
    /// Hash of the finalized block. Sticky: only ever moves forward.
    final_hash: BlockHash,
    /// Height of the finalized block. Sticky: only ever moves forward.
    final_height: Height,
    /// First block accepted for each `(slot, producer)` pair — the index
    /// used to detect equivocation. Key is `(slot, producer_key_hash)`.
    slot_producers: BTreeMap<(u64, [u8; 32]), BlockHash>,
    /// Recorded equivocation evidence, keyed by `(slot, producer)`.
    equivocations: BTreeMap<(u64, [u8; 32]), EquivocationProof>,
}

impl ChainState {
    /// Create a new chain state rooted at a genesis entry.
    #[must_use]
    pub fn new(genesis: ChainEntry, params: &ConsensusParams) -> Self {
        let tip = genesis.hash;
        // The genesis block is final from the start.
        let final_hash = genesis.hash;
        let final_height = genesis.height;
        let mut entries = BTreeMap::new();
        entries.insert(genesis.hash, genesis);
        Self {
            entries,
            tip,
            k_finality: params.k_finality,
            final_hash,
            final_height,
            slot_producers: BTreeMap::new(),
            equivocations: BTreeMap::new(),
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

    /// Hash of the current finalized block.
    ///
    /// Finality is sticky: this only ever advances, never rewinds.
    #[must_use]
    pub fn final_hash(&self) -> BlockHash {
        self.final_hash
    }

    /// Whether `hash` is final — i.e. it is the finalized block itself or
    /// one of its ancestors.
    ///
    /// Once this returns `true` for a block it will *always* return `true`
    /// for that block: finality can never be revoked by later fork choice.
    /// Note this is keyed on the block *hash*, not on height — two
    /// different blocks at the same height never both count as final.
    #[must_use]
    pub fn is_final(&self, hash: &BlockHash) -> bool {
        self.is_ancestor(*hash, self.final_hash)
    }

    /// The height of the finalized block. All blocks at or below this
    /// height on the canonical chain are final.
    #[must_use]
    pub fn finality_height(&self) -> Height {
        self.final_height
    }

    /// Recorded equivocation evidence — producers that signed two distinct
    /// blocks for the same slot. Returned in deterministic `(slot, producer)`
    /// order; consumed by the slashing / accountability layer.
    #[must_use]
    pub fn equivocation_proofs(&self) -> Vec<&EquivocationProof> {
        self.equivocations.values().collect()
    }

    /// Number of distinct equivocation events recorded.
    #[must_use]
    pub fn equivocation_count(&self) -> usize {
        self.equivocations.len()
    }

    /// Add a new block entry and potentially update the tip.
    ///
    /// The entry's parent must already be known, and the block must descend
    /// from the finalized block — otherwise it sits on a branch that
    /// conflicts with finalized history and is rejected. If the new block
    /// extends a chain better than the current tip (per the fork-choice
    /// rule), the tip is switched and the finalized point is advanced.
    ///
    /// Returns `Ok(true)` if the tip changed, `Ok(false)` otherwise.
    pub fn add_block(&mut self, entry: ChainEntry) -> Result<bool, ChainError> {
        // 1. Parent must be known (genesis is the sole exception).
        if !self.entries.contains_key(&entry.parent_hash) && entry.hash != BlockHash::ZERO {
            return Err(ChainError::UnknownParent(entry.parent_hash));
        }

        // 2. Reject duplicates.
        if self.entries.contains_key(&entry.hash) {
            return Err(ChainError::DuplicateBlock(entry.hash));
        }

        // 3. SAFETY: the block must descend from the finalized block.
        //    This rejects every block on a branch that conflicts with
        //    finalized history — in *all* fork-choice paths, closing the
        //    horizon-crossing reorg holes. A block whose parent chain
        //    cannot be resolved fails closed (rejected).
        if entry.hash != BlockHash::ZERO
            && !self.is_ancestor(self.final_hash, entry.parent_hash)
        {
            return Err(ChainError::ConflictsWithFinalized {
                final_height: self.final_height.as_u64(),
                block_height: entry.height.as_u64(),
            });
        }

        let new_hash = entry.hash;
        let new_height = entry.height;
        let new_slot = entry.slot;
        let new_producer = entry.producer_key_hash;

        // 4. Insert only after every validity check has passed, so a
        //    rejected block never pollutes chain state.
        self.entries.insert(entry.hash, entry);

        // 4b. Equivocation check: if this producer already has a different
        //     block for this slot, record the evidence. The block is kept —
        //     fork choice resolves siblings; equivocation cannot add length.
        self.record_equivocation(new_slot, new_producer, new_hash);

        // 5. Fork choice: switch the tip only if the new chain is strictly
        //    better. Both arms are now safe — step 3 already guaranteed the
        //    new block descends from the finalized block.
        let tip_height = self.tip_height();
        let becomes_tip = match new_height.cmp(&tip_height) {
            core::cmp::Ordering::Greater => true,
            // Tie-break by lower block hash (deterministic across nodes).
            core::cmp::Ordering::Equal => new_hash.0 .0 < self.tip.0 .0,
            core::cmp::Ordering::Less => false,
        };

        if becomes_tip {
            self.tip = new_hash;
            // 6. Advance the sticky, monotonic finalized point.
            self.advance_finality();
            Ok(true)
        } else {
            Ok(false)
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

    /// The local canonical chain, ordered genesis→tip, as owned entries.
    ///
    /// Convenience for building the `local` argument to [`maxvalid_bg`].
    #[must_use]
    pub fn canonical_chain(&self) -> Vec<ChainEntry> {
        let mut chain: Vec<ChainEntry> = self
            .ancestors(self.tip, self.entries.len())
            .into_iter()
            .cloned()
            .collect();
        chain.reverse(); // ancestors() yields tip→genesis; we want genesis→tip
        chain
    }

    /// Whether `ancestor` is `descendant` itself or appears on the chain
    /// reached by following parent links back from `descendant`.
    ///
    /// Reflexive: `is_ancestor(x, x)` is `true`. The walk stops as soon as
    /// it drops to or below the ancestor's height, so it is `O(distance)`.
    /// An unknown `ancestor` or a broken parent link yields `false`
    /// (fail-closed — safe for the reorg guard).
    fn is_ancestor(&self, ancestor: BlockHash, descendant: BlockHash) -> bool {
        let target_height = match self.entries.get(&ancestor) {
            Some(e) => e.height.as_u64(),
            None => return false,
        };
        let mut current = descendant;
        loop {
            if current == ancestor {
                return true;
            }
            let (height, parent) = match self.entries.get(&current) {
                Some(e) => (e.height.as_u64(), e.parent_hash),
                None => return false,
            };
            // Reached the ancestor's height (or below) on a different
            // block — `current` is on a separate branch.
            if height <= target_height {
                return false;
            }
            // Reached genesis (its parent link points to itself).
            if current == parent {
                return false;
            }
            current = parent;
        }
    }

    /// Advance the finalized point so it sits `k` blocks below the tip.
    ///
    /// Finality is **sticky and monotonic**: the point only ever moves
    /// forward, walking the current tip's chain. Called after every tip
    /// change. A no-op while the chain is shorter than `k` blocks.
    fn advance_finality(&mut self) {
        let tip_height = self.tip_height().as_u64();
        let target = match tip_height.checked_sub(self.k_finality) {
            Some(t) => t,
            None => return, // chain not yet `k` blocks long
        };
        if target <= self.final_height.as_u64() {
            return; // nothing new to finalize
        }
        // Walk back from the tip to the block at `target` height.
        let mut current = self.tip;
        let new_final = loop {
            let (height, parent) = match self.entries.get(&current) {
                Some(e) => (e.height, e.parent_hash),
                None => return,
            };
            if height.as_u64() == target {
                break (current, height);
            }
            // Defensive: never walk past the target or off the chain.
            if height.as_u64() < target || current == parent {
                return;
            }
            current = parent;
        };
        self.final_hash = new_final.0;
        self.final_height = new_final.1;
    }

    /// Index a block by `(slot, producer)` and record equivocation evidence
    /// if the same producer already accepted a *different* block for this
    /// slot. Idempotent: at most one proof is recorded per `(slot, producer)`.
    fn record_equivocation(&mut self, slot: Slot, producer: Hash256, hash: BlockHash) {
        let key = (slot.as_u64(), producer.0);
        if let Some(first) = self.slot_producers.get(&key).copied() {
            if first != hash {
                self.equivocations
                    .entry(key)
                    .or_insert_with(|| EquivocationProof {
                        slot,
                        producer_key_hash: producer,
                        first,
                        second: hash,
                    });
            }
        } else {
            self.slot_producers.insert(key, hash);
        }
    }

    /// Compute chain density: number of blocks in the last `window` slots
    /// for a given chain tip.
    ///
    /// Building block for the planned Genesis maxvalid-bg chain-selection
    /// rule (ADR-008). Not yet wired into the fork choice — the current rule
    /// is longest-chain by height (see the module-level docs).
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
// Genesis maxvalid-bg chain selection (ADR-008)
// ============================================================================

/// Outcome of comparing a candidate chain against the local chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPreference {
    /// Keep the local chain.
    KeepLocal,
    /// Switch to the candidate chain.
    AdoptCandidate,
}

/// Default `s` window, in slots, for [`maxvalid_bg`] — see ADR-008.
///
/// Validated baseline: with a 70/30 honest/adversary stake split this window
/// adopts the honest chain in 100% of simulated deep forks
/// (`docs/security/maxvalid_bg_reference.py`). Final calibration for the
/// 51/49 worst case is open work, tracked in ADR-008.
pub const DEFAULT_MAXVALID_WINDOW_SLOTS: u64 = 2160;

/// Genesis maxvalid-bg chain selection (ADR-008).
///
/// Decides whether a node should adopt `candidate` over `local`. Both chains
/// must be ordered genesis→tip, share the genesis block, and already be
/// validated (block validity is the caller's responsibility).
///
/// - **Shallow fork** — `local` diverges from the fork point by at most `k`
///   blocks: the longer chain wins, with a deterministic lowest-tip-hash
///   tie-break on equal length.
/// - **Deep fork** — `local` diverges by more than `k` blocks: the chain
///   with more blocks in the `s`-slot window right after the fork point
///   wins; on a tie `local` is kept (no thrashing).
///
/// `k` is the finality depth; `s` is the density window in slots (see
/// [`DEFAULT_MAXVALID_WINDOW_SLOTS`]). The whole computation is integer and
/// hash comparison — deterministic across nodes.
#[must_use]
pub fn maxvalid_bg(
    local: &[ChainEntry],
    candidate: &[ChainEntry],
    k: u64,
    s: u64,
) -> ChainPreference {
    let common = common_prefix_len(local, candidate);
    // The chains must share at least the genesis block.
    if common == 0 {
        return ChainPreference::KeepLocal;
    }
    let fork_slot = match local.get(common.saturating_sub(1)) {
        Some(e) => e.slot.as_u64(),
        None => return ChainPreference::KeepLocal,
    };
    let local_after = u64::try_from(local.len().saturating_sub(common)).unwrap_or(u64::MAX);

    if local_after <= k {
        // Shallow fork — standard longest-chain rule.
        return longer_chain(local, candidate);
    }

    // Deep fork — compare density in the s-slot window after the fork point.
    let window_end = fork_slot.saturating_add(s);
    let local_density = density_in_window(local, common, fork_slot, window_end);
    let cand_density = density_in_window(candidate, common, fork_slot, window_end);
    if cand_density > local_density {
        ChainPreference::AdoptCandidate
    } else {
        ChainPreference::KeepLocal
    }
}

/// Length of the shared prefix of two chains (equal leading entries by hash).
fn common_prefix_len(local: &[ChainEntry], candidate: &[ChainEntry]) -> usize {
    local
        .iter()
        .zip(candidate.iter())
        .take_while(|(a, b)| a.hash == b.hash)
        .count()
}

/// Number of `chain`'s post-fork blocks whose slot is in
/// `(fork_slot, window_end]`.
fn density_in_window(
    chain: &[ChainEntry],
    common: usize,
    fork_slot: u64,
    window_end: u64,
) -> u64 {
    let count = chain
        .iter()
        .skip(common)
        .filter(|e| {
            let sl = e.slot.as_u64();
            sl > fork_slot && sl <= window_end
        })
        .count();
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// Longest-chain rule with a deterministic lowest-tip-hash tie-break.
fn longer_chain(local: &[ChainEntry], candidate: &[ChainEntry]) -> ChainPreference {
    match candidate.len().cmp(&local.len()) {
        core::cmp::Ordering::Greater => ChainPreference::AdoptCandidate,
        core::cmp::Ordering::Less => ChainPreference::KeepLocal,
        core::cmp::Ordering::Equal => match (candidate.last(), local.last()) {
            (Some(c), Some(l)) if c.hash.0 .0 < l.hash.0 .0 => {
                ChainPreference::AdoptCandidate
            }
            _ => ChainPreference::KeepLocal,
        },
    }
}

// ============================================================================
// Header sync helpers (ADR-010)
// ============================================================================

/// Build a block locator from the canonical chain (ordered genesis→tip).
///
/// The locator lists block hashes from the tip toward genesis: the most
/// recent entries densely, then at exponentially growing gaps, always
/// ending at genesis. A peer matches the highest hash it recognises to
/// find the fork point. Suitable for a `GetHeaders` request.
#[must_use]
pub fn build_locator(chain: &[ChainEntry]) -> Vec<BlockHash> {
    let mut locator = Vec::new();
    if chain.is_empty() {
        return locator;
    }
    let mut idx = chain.len().saturating_sub(1); // tip
    let mut step: usize = 1;
    loop {
        if let Some(e) = chain.get(idx) {
            locator.push(e.hash);
        }
        if idx == 0 {
            break;
        }
        // First ~10 entries dense, then double the gap each step.
        let stride = if locator.len() < 10 { 1 } else { step };
        if locator.len() >= 10 {
            step = step.saturating_mul(2);
        }
        idx = idx.saturating_sub(stride);
    }
    locator
}

/// Server side of `GetHeaders`: given the canonical chain and a peer's
/// `locator`, return the hashes of the blocks the peer is missing.
///
/// The result is the chain entries *after* the highest locator hash that is
/// present in `chain`, in genesis→tip order, capped at `max`. If `stop` is
/// non-zero, the result ends once `stop` has been included. A peer whose
/// locator shares nothing with `chain` is sent everything from genesis. The
/// caller maps the returned hashes to full `BlockHeader`s.
#[must_use]
pub fn select_headers_for_locator(
    chain: &[ChainEntry],
    locator: &[BlockHash],
    stop: BlockHash,
    max: usize,
) -> Vec<BlockHash> {
    // The locator is ordered tip→genesis, so the first locator hash found
    // in `chain` is the highest block the peer and we share.
    let start_idx = locator
        .iter()
        .find_map(|h| chain.iter().position(|e| &e.hash == h))
        .map_or(0, |pos| pos.saturating_add(1));

    let mut out = Vec::new();
    for e in chain.iter().skip(start_idx) {
        if out.len() >= max {
            break;
        }
        out.push(e.hash);
        if stop != BlockHash::ZERO && e.hash == stop {
            break;
        }
    }
    out
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

        // Tip at height 3, k=2 → the block at height 1 is finalized.
        assert!(cs.is_final(&BlockHash::ZERO)); // genesis
        assert!(cs.is_final(&BlockHash::from_bytes([0x01; 32])));
        assert!(!cs.is_final(&BlockHash::from_bytes([0x02; 32])));
        assert!(!cs.is_final(&BlockHash::from_bytes([0x03; 32])));
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

    // ------------------------------------------------------------------
    // Regression tests for the fork-finality audit (2026-05-22).
    // ------------------------------------------------------------------

    #[test]
    fn finality_advances_monotonically() {
        let p = ConsensusParams {
            k_finality: 2,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        assert_eq!(cs.finality_height(), Height::GENESIS);
        cs.add_block(make_entry(0x01, 0x00, 1, 5)).unwrap();
        assert_eq!(cs.finality_height(), Height::GENESIS); // not k-deep yet
        cs.add_block(make_entry(0x02, 0x01, 2, 10)).unwrap();
        assert_eq!(cs.finality_height(), Height::GENESIS); // tip 2, k 2 → 0
        cs.add_block(make_entry(0x03, 0x02, 3, 15)).unwrap();
        assert_eq!(cs.finality_height(), Height::from(1));
        cs.add_block(make_entry(0x04, 0x03, 4, 20)).unwrap();
        assert_eq!(cs.finality_height(), Height::from(2));
    }

    #[test]
    fn block_conflicting_with_finalized_history_is_rejected() {
        let p = ConsensusParams {
            k_finality: 2,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        // Linear chain genesis → 1 → 2 → 3 → 4. Tip height 4, k=2 →
        // the finalized point is the block at height 2.
        cs.add_block(make_entry(0x01, 0x00, 1, 5)).unwrap();
        cs.add_block(make_entry(0x02, 0x01, 2, 10)).unwrap();
        cs.add_block(make_entry(0x03, 0x02, 3, 15)).unwrap();
        cs.add_block(make_entry(0x04, 0x03, 4, 20)).unwrap();
        assert_eq!(cs.finality_height(), Height::from(2));

        // A block forking at height 1 (below finalized height 2) must be
        // rejected — it conflicts with finalized history.
        let conflicting = make_entry(0x99, 0x01, 2, 11);
        assert!(matches!(
            cs.add_block(conflicting),
            Err(ChainError::ConflictsWithFinalized { .. })
        ));
        // The rejected block must not pollute chain state.
        assert!(cs.get(&BlockHash::from_bytes([0x99; 32])).is_none());
    }

    #[test]
    fn equal_height_reorg_cannot_revert_finalized_block() {
        // Regression: an equal-height competing chain must NOT be able to
        // reorg past the finalized point via the hash tie-break.
        let p = ConsensusParams {
            k_finality: 2,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        cs.add_block(make_entry(0x10, 0x00, 1, 5)).unwrap();
        cs.add_block(make_entry(0x11, 0x10, 2, 10)).unwrap();
        cs.add_block(make_entry(0x12, 0x11, 3, 15)).unwrap();
        cs.add_block(make_entry(0x13, 0x12, 4, 20)).unwrap();
        let final_before = cs.final_hash();
        assert_eq!(cs.finality_height(), Height::from(2));

        // A rival chain forking at genesis, even with a numerically lower
        // hash, cannot take over: it conflicts with finalized history.
        let rival = make_entry(0x01, 0x00, 1, 6);
        assert!(matches!(
            cs.add_block(rival),
            Err(ChainError::ConflictsWithFinalized { .. })
        ));

        // The finalized block is unchanged and still final.
        assert_eq!(cs.final_hash(), final_before);
        assert!(cs.is_final(&BlockHash::from_bytes([0x11; 32])));
    }

    #[test]
    fn legal_reorg_above_finality_is_allowed() {
        // A fork that diverges *above* the finalized point is a normal,
        // legal reorg and must still switch the tip.
        let p = ConsensusParams {
            k_finality: 3,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        // Chain A: genesis → A1 → A2 → A3 → A4. Tip height 4, k=3 →
        // the finalized point is the block at height 1.
        cs.add_block(make_entry(0xA1, 0x00, 1, 5)).unwrap();
        cs.add_block(make_entry(0xA2, 0xA1, 2, 10)).unwrap();
        cs.add_block(make_entry(0xA3, 0xA2, 3, 15)).unwrap();
        cs.add_block(make_entry(0xA4, 0xA3, 4, 20)).unwrap();
        assert_eq!(cs.finality_height(), Height::from(1));

        // Chain B forks at A2 (height 2, above finality) and grows longer.
        cs.add_block(make_entry(0xB3, 0xA2, 3, 16)).unwrap();
        cs.add_block(make_entry(0xB4, 0xB3, 4, 21)).unwrap();
        let switched = cs.add_block(make_entry(0xB5, 0xB4, 5, 25)).unwrap();
        assert!(switched);
        assert_eq!(cs.tip_hash(), BlockHash::from_bytes([0xB5; 32]));

        // A1 was finalized before the reorg — still final afterwards.
        assert!(cs.is_final(&BlockHash::from_bytes([0xA1; 32])));
    }

    #[test]
    fn equivocation_is_detected_and_recorded() {
        let p = ConsensusParams {
            k_finality: 10,
            ..ConsensusParams::mainnet()
        };
        let mut cs = ChainState::genesis(&p);

        // The same producer signs two different blocks for the same slot.
        let producer = Hash256::from_bytes([0xAA; 32]);
        let block_a = ChainEntry {
            hash: BlockHash::from_bytes([0x01; 32]),
            parent_hash: BlockHash::ZERO,
            height: Height::from(1),
            slot: Slot::from(5),
            producer_key_hash: producer,
        };
        let block_b = ChainEntry {
            hash: BlockHash::from_bytes([0x02; 32]),
            parent_hash: BlockHash::ZERO,
            height: Height::from(1),
            slot: Slot::from(5),
            producer_key_hash: producer,
        };

        // Both blocks are accepted (fork choice resolves them); the
        // equivocation is recorded as evidence.
        cs.add_block(block_a).unwrap();
        cs.add_block(block_b).unwrap();

        assert_eq!(cs.equivocation_count(), 1);
        let proofs = cs.equivocation_proofs();
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].slot, Slot::from(5));
        assert_eq!(proofs[0].producer_key_hash, producer);
        assert_eq!(proofs[0].first, BlockHash::from_bytes([0x01; 32]));
        assert_eq!(proofs[0].second, BlockHash::from_bytes([0x02; 32]));
    }

    #[test]
    fn honest_chain_has_no_equivocations() {
        let p = params();
        let mut cs = ChainState::genesis(&p);
        cs.add_block(make_entry(0x01, 0x00, 1, 5)).unwrap();
        cs.add_block(make_entry(0x02, 0x01, 2, 10)).unwrap();
        cs.add_block(make_entry(0x03, 0x02, 3, 15)).unwrap();
        assert_eq!(cs.equivocation_count(), 0);
        assert!(cs.equivocation_proofs().is_empty());
    }

    // ------------------------------------------------------------------
    // Genesis maxvalid-bg chain selection (ADR-008).
    // ------------------------------------------------------------------

    #[test]
    fn maxvalid_shallow_fork_prefers_longer() {
        let g = make_entry(0x00, 0x00, 0, 0);
        let a1 = make_entry(0x01, 0x00, 1, 10);
        let local = vec![g.clone(), a1.clone(), make_entry(0x02, 0x01, 2, 20)];
        let candidate = vec![
            g.clone(),
            a1.clone(),
            make_entry(0x03, 0x01, 2, 21),
            make_entry(0x04, 0x03, 3, 31),
        ];
        // Fork at a1, local diverges 1 block (≤ k) → shallow → longest wins.
        assert_eq!(
            maxvalid_bg(&local, &candidate, 3, 100),
            ChainPreference::AdoptCandidate
        );
        assert_eq!(
            maxvalid_bg(&candidate, &local, 3, 100),
            ChainPreference::KeepLocal
        );
    }

    #[test]
    fn maxvalid_deep_fork_prefers_denser() {
        let g = make_entry(0x00, 0x00, 0, 0);
        // local: sparse — 6 blocks every 20 slots (slots 20..120).
        let mut local = vec![g.clone()];
        for i in 1..=6u64 {
            local.push(make_entry(0x10 + i as u8, 0x00, i, i * 20));
        }
        // candidate: dense — 6 blocks every 5 slots (slots 5..30).
        let mut candidate = vec![g.clone()];
        for i in 1..=6u64 {
            candidate.push(make_entry(0xC0 + i as u8, 0x00, i, i * 5));
        }
        // Fork at genesis, local diverges 6 blocks (> k=3) → deep fork.
        // Window (0,100]: local has 5, candidate has 6 → adopt candidate.
        assert_eq!(
            maxvalid_bg(&local, &candidate, 3, 100),
            ChainPreference::AdoptCandidate
        );
        // Sparse candidate vs dense local → keep local.
        assert_eq!(
            maxvalid_bg(&candidate, &local, 3, 100),
            ChainPreference::KeepLocal
        );
    }

    #[test]
    fn maxvalid_rejects_chain_without_shared_genesis() {
        let local = vec![make_entry(0x00, 0x00, 0, 0), make_entry(0x01, 0x00, 1, 10)];
        let alien = vec![make_entry(0xFF, 0x00, 0, 0), make_entry(0xFE, 0x00, 1, 10)];
        assert_eq!(
            maxvalid_bg(&local, &alien, 3, 100),
            ChainPreference::KeepLocal
        );
    }

    #[test]
    fn maxvalid_keeps_local_on_identical_chains() {
        let chain = vec![make_entry(0x00, 0x00, 0, 0), make_entry(0x01, 0x00, 1, 10)];
        assert_eq!(
            maxvalid_bg(&chain, &chain.clone(), 3, 100),
            ChainPreference::KeepLocal
        );
    }

    #[test]
    fn canonical_chain_is_genesis_to_tip() {
        let p = params();
        let mut cs = ChainState::genesis(&p);
        cs.add_block(make_entry(0x01, 0x00, 1, 5)).unwrap();
        cs.add_block(make_entry(0x02, 0x01, 2, 10)).unwrap();
        let chain = cs.canonical_chain();
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].hash, BlockHash::ZERO); // genesis first
        assert_eq!(chain[2].hash, BlockHash::from_bytes([0x02; 32])); // tip last
    }

    // ------------------------------------------------------------------
    // Header sync helpers (ADR-010).
    // ------------------------------------------------------------------

    #[test]
    fn build_locator_dense_then_sparse() {
        // 30-entry chain → locator: tip + dense head + exponential gaps + genesis.
        let mut chain = vec![make_entry(0x00, 0x00, 0, 0)];
        for i in 1..=29u64 {
            chain.push(make_entry(i as u8, (i - 1) as u8, i, i * 2));
        }
        let loc = build_locator(&chain);
        assert_eq!(loc[0], BlockHash::from_bytes([29u8; 32])); // tip first
        assert_eq!(loc[loc.len() - 1], BlockHash::ZERO); // genesis last
        assert_eq!(loc[9], BlockHash::from_bytes([20u8; 32])); // dense head
        assert!(loc.len() < chain.len()); // far shorter than the chain
    }

    #[test]
    fn select_headers_returns_blocks_after_common_point() {
        let mut chain = vec![make_entry(0x00, 0x00, 0, 0)];
        for i in 1..=10u64 {
            chain.push(make_entry(i as u8, (i - 1) as u8, i, i * 2));
        }
        // Peer knows up to height 4 → expect heights 5..=10.
        let locator = vec![BlockHash::from_bytes([4u8; 32]), BlockHash::ZERO];
        let got = select_headers_for_locator(&chain, &locator, BlockHash::ZERO, 100);
        assert_eq!(got.len(), 6);
        assert_eq!(got[0], BlockHash::from_bytes([5u8; 32]));
        assert_eq!(got[5], BlockHash::from_bytes([10u8; 32]));
    }

    #[test]
    fn select_headers_respects_max_and_unknown_locator() {
        let mut chain = vec![make_entry(0x00, 0x00, 0, 0)];
        for i in 1..=10u64 {
            chain.push(make_entry(i as u8, (i - 1) as u8, i, i * 2));
        }
        // `max` caps the result.
        let locator = vec![BlockHash::from_bytes([2u8; 32])];
        let capped = select_headers_for_locator(&chain, &locator, BlockHash::ZERO, 3);
        assert_eq!(capped.len(), 3);
        assert_eq!(capped[0], BlockHash::from_bytes([3u8; 32]));
        // A locator that shares nothing → everything from genesis.
        let alien = vec![BlockHash::from_bytes([0xFF; 32])];
        let all = select_headers_for_locator(&chain, &alien, BlockHash::ZERO, 100);
        assert_eq!(all.len(), chain.len());
        assert_eq!(all[0], BlockHash::ZERO);
    }
}
