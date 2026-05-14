//! Slot and epoch time mapping for Ouroboros Praos.
//!
//! This module provides the bridge between wall-clock time and the abstract
//! slot/epoch coordinate system the consensus layer uses. Every node must
//! agree on these mappings; they are fully determined by [`ProtocolParams`].
//!
//! # Key invariants
//!
//! - `slot_to_epoch(s) = s / epoch_slots` (integer division)
//! - `epoch_first_slot(e) = e * epoch_slots`
//! - `slot_to_wall_clock(s) = genesis_time + s * slot_duration_ms`
//! - All arithmetic is checked or saturating — no silent overflow.

use qv_core::{ConsensusParams, Epoch, ProtocolParams, Slot, Timestamp};
use serde::{Deserialize, Serialize};

// ============================================================================
// SlotClock — the canonical mapping between slots, epochs, and time
// ============================================================================

/// Stateless mapper between slots, epochs, and wall-clock time.
///
/// `SlotClock` is cheap to clone and `Copy`. It holds only the few scalar
/// parameters needed to compute all slot/epoch/time relationships.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotClock {
    /// Slot duration in milliseconds.
    slot_duration_ms: u64,
    /// Number of slots in one epoch.
    epoch_slots: u64,
    /// Unix-millis at which genesis slot 0 began.
    genesis_time_ms: u64,
}

impl SlotClock {
    /// Construct from full protocol params.
    #[must_use]
    pub fn from_params(params: &ProtocolParams) -> Self {
        Self {
            slot_duration_ms: params.consensus.slot_duration_ms,
            epoch_slots: params.consensus.epoch_slots,
            // genesis_time in ProtocolParams is seconds; we work in millis.
            genesis_time_ms: params.genesis_time.saturating_mul(1_000),
        }
    }

    /// Construct directly from consensus parameters and a genesis time
    /// (in seconds).
    #[must_use]
    pub fn new(consensus: &ConsensusParams, genesis_time_secs: u64) -> Self {
        Self {
            slot_duration_ms: consensus.slot_duration_ms,
            epoch_slots: consensus.epoch_slots,
            genesis_time_ms: genesis_time_secs.saturating_mul(1_000),
        }
    }

    // -- Slot ↔ Epoch -------------------------------------------------------

    /// Which epoch does `slot` belong to?
    #[must_use]
    pub fn slot_to_epoch(&self, slot: Slot) -> Epoch {
        Epoch::from(slot.as_u64().saturating_div(self.epoch_slots))
    }

    /// First slot of the given epoch.
    #[must_use]
    pub fn epoch_first_slot(&self, epoch: Epoch) -> Slot {
        Slot::from(epoch.as_u64().saturating_mul(self.epoch_slots))
    }

    /// Last slot (inclusive) of the given epoch.
    #[must_use]
    pub fn epoch_last_slot(&self, epoch: Epoch) -> Slot {
        Slot::from(
            epoch
                .as_u64()
                .saturating_add(1)
                .saturating_mul(self.epoch_slots)
                .saturating_sub(1),
        )
    }

    /// Position of `slot` within its epoch (0-based).
    #[must_use]
    pub fn slot_in_epoch(&self, slot: Slot) -> u64 {
        slot.as_u64().checked_rem(self.epoch_slots).unwrap_or(0)
    }

    /// Number of slots per epoch.
    #[must_use]
    pub fn epoch_length(&self) -> u64 {
        self.epoch_slots
    }

    // -- Slot ↔ Wall-clock --------------------------------------------------

    /// Wall-clock start time (unix millis) of the given slot.
    #[must_use]
    pub fn slot_start_time_ms(&self, slot: Slot) -> u64 {
        self.genesis_time_ms
            .saturating_add(slot.as_u64().saturating_mul(self.slot_duration_ms))
    }

    /// Wall-clock start time as a [`Timestamp`] (unix seconds, truncated).
    #[must_use]
    pub fn slot_start_timestamp(&self, slot: Slot) -> Timestamp {
        Timestamp::from_unix_secs(self.slot_start_time_ms(slot).saturating_div(1_000))
    }

    /// Which slot does a given unix-millis timestamp fall into?
    ///
    /// Returns `None` if `time_ms` is before genesis.
    #[must_use]
    pub fn time_to_slot(&self, time_ms: u64) -> Option<Slot> {
        if time_ms < self.genesis_time_ms {
            return None;
        }
        let elapsed = time_ms.saturating_sub(self.genesis_time_ms);
        Some(Slot::from(elapsed.saturating_div(self.slot_duration_ms)))
    }

    /// Slot duration in milliseconds.
    #[must_use]
    pub fn slot_duration_ms(&self) -> u64 {
        self.slot_duration_ms
    }
}

// ============================================================================
// SlotInfo — a snapshot of "what slot/epoch are we in right now?"
// ============================================================================

/// Convenience snapshot bundling a slot number with its derived epoch info.
///
/// Produced by [`SlotClock`] for anything that needs "where are we?" context
/// without doing the math itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotInfo {
    /// The absolute slot number.
    pub slot: Slot,
    /// Epoch this slot belongs to.
    pub epoch: Epoch,
    /// 0-based position of the slot within its epoch.
    pub slot_in_epoch: u64,
    /// First slot of the current epoch.
    pub epoch_first_slot: Slot,
    /// Wall-clock start time (unix millis) of this slot.
    pub start_time_ms: u64,
}

impl SlotClock {
    /// Compute full [`SlotInfo`] for a given slot.
    #[must_use]
    pub fn info(&self, slot: Slot) -> SlotInfo {
        let epoch = self.slot_to_epoch(slot);
        SlotInfo {
            slot,
            epoch,
            slot_in_epoch: self.slot_in_epoch(slot),
            epoch_first_slot: self.epoch_first_slot(epoch),
            start_time_ms: self.slot_start_time_ms(slot),
        }
    }

    /// Compute [`SlotInfo`] for the current wall-clock time.
    ///
    /// Returns `None` if `now_ms` is before genesis.
    #[must_use]
    pub fn current_info(&self, now_ms: u64) -> Option<SlotInfo> {
        self.time_to_slot(now_ms).map(|s| self.info(s))
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

    fn mainnet_clock() -> SlotClock {
        SlotClock::new(&ConsensusParams::mainnet(), 1_700_000_000)
    }

    fn ephemeral_clock() -> SlotClock {
        let params = ProtocolParams::ephemeral();
        SlotClock::from_params(&params)
    }

    #[test]
    fn genesis_slot_is_epoch_zero() {
        let c = mainnet_clock();
        assert_eq!(c.slot_to_epoch(Slot::GENESIS), Epoch::GENESIS);
    }

    #[test]
    fn last_slot_of_epoch_zero() {
        let c = mainnet_clock();
        // epoch_slots = 21_600 → last slot of epoch 0 = 21_599
        assert_eq!(c.slot_to_epoch(Slot::from(21_599)), Epoch::from(0));
    }

    #[test]
    fn first_slot_of_epoch_one() {
        let c = mainnet_clock();
        assert_eq!(c.slot_to_epoch(Slot::from(21_600)), Epoch::from(1));
    }

    #[test]
    fn epoch_first_and_last_slot() {
        let c = mainnet_clock();
        assert_eq!(c.epoch_first_slot(Epoch::from(2)), Slot::from(43_200));
        assert_eq!(c.epoch_last_slot(Epoch::from(0)), Slot::from(21_599));
        assert_eq!(c.epoch_last_slot(Epoch::from(2)), Slot::from(64_799));
    }

    #[test]
    fn slot_in_epoch() {
        let c = mainnet_clock();
        assert_eq!(c.slot_in_epoch(Slot::from(0)), 0);
        assert_eq!(c.slot_in_epoch(Slot::from(100)), 100);
        assert_eq!(c.slot_in_epoch(Slot::from(21_600)), 0); // first of epoch 1
        assert_eq!(c.slot_in_epoch(Slot::from(21_605)), 5);
    }

    #[test]
    fn slot_start_time_mainnet() {
        let c = mainnet_clock();
        let genesis_ms = 1_700_000_000_000u64;
        assert_eq!(c.slot_start_time_ms(Slot::from(0)), genesis_ms);
        assert_eq!(c.slot_start_time_ms(Slot::from(1)), genesis_ms + 2_000);
        assert_eq!(c.slot_start_time_ms(Slot::from(100)), genesis_ms + 200_000);
    }

    #[test]
    fn time_to_slot_before_genesis_is_none() {
        let c = mainnet_clock();
        assert!(c.time_to_slot(100).is_none());
    }

    #[test]
    fn time_to_slot_roundtrip() {
        let c = mainnet_clock();
        let genesis_ms = 1_700_000_000_000u64;
        // Halfway through slot 5 → still slot 5
        let mid_slot_5 = genesis_ms + 5 * 2_000 + 1_000;
        assert_eq!(c.time_to_slot(mid_slot_5), Some(Slot::from(5)));
    }

    #[test]
    fn slot_info_consistency() {
        let c = mainnet_clock();
        let info = c.info(Slot::from(43_205));
        assert_eq!(info.epoch, Epoch::from(2));
        assert_eq!(info.slot_in_epoch, 5);
        assert_eq!(info.epoch_first_slot, Slot::from(43_200));
    }

    #[test]
    fn ephemeral_short_epochs() {
        let c = ephemeral_clock();
        // epoch_slots = 50
        assert_eq!(c.epoch_length(), 50);
        assert_eq!(c.slot_to_epoch(Slot::from(49)), Epoch::from(0));
        assert_eq!(c.slot_to_epoch(Slot::from(50)), Epoch::from(1));
        assert_eq!(c.slot_to_epoch(Slot::from(149)), Epoch::from(2));
    }

    #[test]
    fn current_info_before_genesis_is_none() {
        let c = mainnet_clock();
        assert!(c.current_info(0).is_none());
    }

    #[test]
    fn slot_start_timestamp_seconds() {
        let c = mainnet_clock();
        let ts = c.slot_start_timestamp(Slot::from(10));
        // 1_700_000_000 + 10*2 = 1_700_000_020
        assert_eq!(ts.as_u64(), 1_700_000_020);
    }
}
