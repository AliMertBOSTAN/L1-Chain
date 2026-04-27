//! Epoch-level bookkeeping for Ouroboros Praos.
//!
//! An *epoch* is a fixed-length window of slots during which the stake
//! distribution is frozen and a single *epoch nonce* seeds VRF leader
//! election. At every epoch boundary the protocol snapshots the ledger
//! state and derives a fresh nonce for the upcoming epoch.
//!
//! # Epoch nonce derivation
//!
//! The nonce for epoch `e` is:
//!
//! ```text
//! η_e = SHA3-256( η_{e-1} || extra_entropy || epoch_boundary_block_hash )
//! ```
//!
//! This mirrors Cardano's *evolving nonce* approach: VRF outputs from the
//! first 2/3 of the epoch are mixed in so that no single leader controls
//! the nonce. For the genesis epoch we use a fixed seed from the genesis
//! configuration.
//!
//! # Stake snapshot timing
//!
//! The stake distribution used for leader election in epoch `e` is taken
//! at the **start of epoch `e − 1`** (one-epoch look-back). This gives
//! delegators a full epoch of settlement before their delegation affects
//! leader selection, eliminating grinding attacks on the snapshot boundary.

use qv_core::{Epoch, Hash256, Slot};
use qv_crypto::sha3_256;
use serde::{Deserialize, Serialize};

use crate::slot::SlotClock;

// ============================================================================
// Epoch nonce
// ============================================================================

/// A 32-byte random seed that parameterises VRF leader election for one epoch.
///
/// The nonce is publicly derivable from on-chain data; it is *not* secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EpochNonce(pub Hash256);

impl EpochNonce {
    /// Hard-coded genesis nonce. In production this would come from the
    /// genesis ceremony; here we use a readable sentinel.
    pub const GENESIS: Self = Self(Hash256::from_bytes([
        0x51, 0x56, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // "QV" prefix
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    ]));

    /// Evolve the nonce for the next epoch.
    ///
    /// `extra_entropy` contains accumulated VRF outputs from the first 2/3
    /// of the current epoch. `boundary_hash` is the hash of the last block
    /// in the current epoch (or zero-hash if the epoch had no blocks).
    #[must_use]
    pub fn evolve(&self, extra_entropy: &[u8], boundary_hash: &Hash256) -> Self {
        let mut preimage = Vec::with_capacity(32 + extra_entropy.len() + 32);
        preimage.extend_from_slice(self.0.as_bytes());
        preimage.extend_from_slice(extra_entropy);
        preimage.extend_from_slice(boundary_hash.as_bytes());
        Self(Hash256::from_bytes(sha3_256(&preimage)))
    }

    /// Underlying bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

// ============================================================================
// EpochInfo — everything a consumer needs to know about one epoch
// ============================================================================

/// Aggregate snapshot of a single epoch's parameters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochInfo {
    /// The epoch number.
    pub epoch: Epoch,
    /// First slot of this epoch (inclusive).
    pub first_slot: Slot,
    /// Last slot of this epoch (inclusive).
    pub last_slot: Slot,
    /// Nonce that seeds VRF leader election for this epoch.
    pub nonce: EpochNonce,
}

impl EpochInfo {
    /// Build an `EpochInfo` from a clock and a nonce.
    #[must_use]
    pub fn new(clock: &SlotClock, epoch: Epoch, nonce: EpochNonce) -> Self {
        Self {
            epoch,
            first_slot: clock.epoch_first_slot(epoch),
            last_slot: clock.epoch_last_slot(epoch),
            nonce,
        }
    }

    /// Number of slots in this epoch.
    #[must_use]
    pub fn length(&self) -> u64 {
        self.last_slot
            .as_u64()
            .saturating_sub(self.first_slot.as_u64())
            .saturating_add(1)
    }

    /// Returns `true` if the given slot falls within this epoch.
    #[must_use]
    pub fn contains_slot(&self, slot: Slot) -> bool {
        slot >= self.first_slot && slot <= self.last_slot
    }

    /// The "nonce contribution window" — slots whose VRF outputs feed into
    /// the *next* epoch's nonce. This is the first 2/3 of the epoch.
    #[must_use]
    pub fn nonce_contribution_last_slot(&self) -> Slot {
        let two_thirds = self.length().saturating_mul(2).saturating_div(3);
        Slot::from(
            self.first_slot
                .as_u64()
                .saturating_add(two_thirds.saturating_sub(1)),
        )
    }
}

// ============================================================================
// EpochBoundary — tracks epoch transitions
// ============================================================================

/// Lightweight tracker for detecting epoch boundaries during chain traversal.
///
/// Feed it successive slots (in order) and it tells you when an epoch
/// transition has occurred.
#[derive(Clone, Debug)]
pub struct EpochBoundary {
    clock: SlotClock,
    current_epoch: Epoch,
}

impl EpochBoundary {
    /// Create a new tracker starting at the genesis epoch.
    #[must_use]
    pub fn new(clock: SlotClock) -> Self {
        Self {
            clock,
            current_epoch: Epoch::GENESIS,
        }
    }

    /// Create a tracker starting at the epoch that contains `slot`.
    #[must_use]
    pub fn at_slot(clock: SlotClock, slot: Slot) -> Self {
        Self {
            clock,
            current_epoch: clock.slot_to_epoch(slot),
        }
    }

    /// Process a new slot. Returns `Some(new_epoch)` if an epoch transition
    /// just happened, `None` otherwise.
    pub fn advance(&mut self, slot: Slot) -> Option<Epoch> {
        let epoch = self.clock.slot_to_epoch(slot);
        if epoch > self.current_epoch {
            self.current_epoch = epoch;
            Some(epoch)
        } else {
            None
        }
    }

    /// The epoch we are currently in.
    #[must_use]
    pub fn current_epoch(&self) -> Epoch {
        self.current_epoch
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

    fn clock() -> SlotClock {
        SlotClock::new(&ConsensusParams::mainnet(), 0)
    }

    #[test]
    fn genesis_nonce_is_nonzero() {
        assert_ne!(EpochNonce::GENESIS.0, Hash256::ZERO);
    }

    #[test]
    fn nonce_evolution_is_deterministic() {
        let n0 = EpochNonce::GENESIS;
        let entropy = b"some vrf outputs concatenated";
        let boundary = Hash256::from_bytes([0xAA; 32]);
        let n1a = n0.evolve(entropy, &boundary);
        let n1b = n0.evolve(entropy, &boundary);
        assert_eq!(n1a, n1b);
    }

    #[test]
    fn nonce_evolution_changes_on_different_input() {
        let n0 = EpochNonce::GENESIS;
        let n1 = n0.evolve(b"aaa", &Hash256::ZERO);
        let n2 = n0.evolve(b"bbb", &Hash256::ZERO);
        assert_ne!(n1, n2);
    }

    #[test]
    fn epoch_info_contains_slot() {
        let c = clock();
        let info = EpochInfo::new(&c, Epoch::from(0), EpochNonce::GENESIS);
        assert!(info.contains_slot(Slot::from(0)));
        assert!(info.contains_slot(Slot::from(21_599)));
        assert!(!info.contains_slot(Slot::from(21_600)));
    }

    #[test]
    fn epoch_info_length() {
        let c = clock();
        let info = EpochInfo::new(&c, Epoch::from(0), EpochNonce::GENESIS);
        assert_eq!(info.length(), 21_600);
    }

    #[test]
    fn nonce_contribution_window_is_two_thirds() {
        let c = clock();
        let info = EpochInfo::new(&c, Epoch::from(0), EpochNonce::GENESIS);
        // 2/3 of 21600 = 14400 → last contributing slot = 14399
        let last = info.nonce_contribution_last_slot();
        assert_eq!(last, Slot::from(14_399));
    }

    #[test]
    fn epoch_boundary_detects_transition() {
        let c = clock();
        let mut b = EpochBoundary::new(c);

        // Within epoch 0 — no transition
        assert!(b.advance(Slot::from(100)).is_none());
        assert!(b.advance(Slot::from(21_599)).is_none());

        // Cross into epoch 1
        assert_eq!(b.advance(Slot::from(21_600)), Some(Epoch::from(1)));
        assert_eq!(b.current_epoch(), Epoch::from(1));

        // Stay in epoch 1
        assert!(b.advance(Slot::from(21_700)).is_none());
    }

    #[test]
    fn epoch_boundary_at_slot() {
        let c = clock();
        let b = EpochBoundary::at_slot(c, Slot::from(50_000));
        // 50_000 / 21_600 = 2 (epoch 2)
        assert_eq!(b.current_epoch(), Epoch::from(2));
    }

    #[test]
    fn nonce_chain_three_epochs() {
        let n0 = EpochNonce::GENESIS;
        let n1 = n0.evolve(b"epoch0_vrf", &Hash256::from_bytes([1; 32]));
        let n2 = n1.evolve(b"epoch1_vrf", &Hash256::from_bytes([2; 32]));
        // All three are distinct
        assert_ne!(n0, n1);
        assert_ne!(n1, n2);
        assert_ne!(n0, n2);
    }
}
