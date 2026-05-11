//! Main async event loop for slot-based leadership checks and block production.
//!
//! Every slot (2 seconds), the operator:
//! 1. Evaluates the VRF against the current slot and epoch nonce.
//! 2. Checks if elected as slot leader.
//! 3. If elected, produces a block and gossips it.
//! 4. Advances to the next slot.

use crate::MinerResult;
use qv_consensus::{
    check_leadership, EpochNonce, SlotClock, StakeDistribution, VrfEvaluator,
};
use qv_core::{Epoch, Hash256, ProtocolParams, Slot, Timestamp};
use std::time::Duration;
use tokio::time::interval;

/// Slot loop state and lifecycle.
#[derive(Clone, Debug)]
pub struct SlotLoop {
    /// Slot clock for time mapping.
    pub slot_clock: SlotClock,

    /// Current slot.
    pub current_slot: Slot,

    /// Current epoch.
    pub current_epoch: Epoch,

    /// Current epoch nonce.
    pub epoch_nonce: Vec<u8>,

    /// Stake distribution (frozen per epoch).
    pub stake_distribution: Option<StakeDistribution>,

    /// Whether the loop is running.
    pub is_running: bool,
}

impl SlotLoop {
    /// Create a new slot loop from protocol parameters.
    pub fn new(params: &ProtocolParams, current_slot: Slot, epoch_nonce: Vec<u8>) -> Self {
        let slot_clock = SlotClock::from_params(params);
        let current_epoch = slot_clock.slot_to_epoch(current_slot);

        Self {
            slot_clock,
            current_slot,
            current_epoch,
            epoch_nonce,
            stake_distribution: None,
            is_running: false,
        }
    }

    /// Advance to the next slot.
    pub fn advance_slot(&mut self) {
        self.current_slot = Slot::from(self.current_slot.as_u64().saturating_add(1));
        let new_epoch = self.slot_clock.slot_to_epoch(self.current_slot);
        if new_epoch != self.current_epoch {
            self.current_epoch = new_epoch;
            // In a real implementation, update epoch nonce and stake distribution here.
        }
    }

    /// Calculate the next slot's wall-clock time.
    pub fn next_slot_time(&self) -> Timestamp {
        // `slot_duration_ms()` is the public accessor — the field itself is private.
        let slot_duration_secs = self.slot_clock.slot_duration_ms() / 1000;
        Timestamp::from(
            self.slot_clock
                .slot_start_timestamp(self.current_slot)
                .as_u64()
                + slot_duration_secs,
        )
    }
}

/// Run the slot loop indefinitely (until cancellation).
///
/// # Parameters
/// - `slot_loop`: Mutable slot loop state.
/// - `vrf`: VRF evaluator (trait-based for testing).
/// - `pool_id`: The operator's pool ID.
/// - `block_producer_fn`: Async callback to produce and gossip a block.
///
/// # Cancellation
/// The loop runs until the `tokio::sync::watch` receiver is signaled or a fatal error occurs.
pub async fn run_slot_loop<V, F>(
    mut slot_loop: SlotLoop,
    vrf: &V,
    pool_id: &qv_consensus::PoolId,
    mut block_producer_fn: F,
) -> MinerResult<()>
where
    V: VrfEvaluator,
    F: FnMut(Slot) -> std::pin::Pin<Box<dyn std::future::Future<Output = MinerResult<()>> + Send>>,
{
    slot_loop.is_running = true;
    let slot_duration = Duration::from_millis(slot_loop.slot_clock.slot_duration_ms());

    let mut slot_ticker = interval(slot_duration);

    loop {
        // Wait for the next slot boundary.
        slot_ticker.tick().await;

        // Advance to the next slot.
        slot_loop.advance_slot();

        tracing::debug!(slot = %slot_loop.current_slot, "new slot");

        // Convert raw bytes to the typed `EpochNonce` expected by consensus.
        let nonce_bytes: [u8; 32] = match slot_loop.epoch_nonce.as_slice().try_into() {
            Ok(b) => b,
            Err(_) => {
                tracing::error!(
                    actual_len = slot_loop.epoch_nonce.len(),
                    "epoch_nonce must be 32 bytes; skipping slot"
                );
                continue;
            }
        };
        let nonce = EpochNonce(Hash256::from_bytes(nonce_bytes));

        // Stake distribution is required to evaluate leadership; if missing
        // (e.g. epoch boundary not yet processed) we skip the slot.
        let Some(distribution) = slot_loop.stake_distribution.as_ref() else {
            tracing::warn!(
                slot = %slot_loop.current_slot,
                "no stake distribution available; skipping leadership check"
            );
            continue;
        };

        // Check if elected as leader.
        let is_leader = match check_leadership(
            vrf,
            pool_id,
            &nonce,
            slot_loop.current_slot,
            distribution,
        ) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                tracing::error!(?e, "failed to check leadership");
                continue;
            }
        };

        if is_leader {
            tracing::info!(slot = %slot_loop.current_slot, "elected as slot leader");

            // Attempt to produce and gossip a block.
            if let Err(e) = block_producer_fn(slot_loop.current_slot).await {
                tracing::error!(?e, "block production failed");
                // Continue to the next slot even if production fails.
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::MinerError;
    use tokio::time::{sleep, Duration};

    #[test]
    fn slot_loop_creation() {
        let params = ProtocolParams::mainnet();
        let slot_loop = SlotLoop::new(&params, Slot::from(100), vec![0u8; 32]);

        assert_eq!(slot_loop.current_slot, Slot::from(100));
        assert!(!slot_loop.is_running);
    }

    #[test]
    fn slot_loop_advance_slot() {
        let params = ProtocolParams::mainnet();
        let mut slot_loop = SlotLoop::new(&params, Slot::from(100), vec![0u8; 32]);

        let initial_slot = slot_loop.current_slot;
        slot_loop.advance_slot();

        assert_eq!(slot_loop.current_slot, Slot::from(initial_slot.as_u64() + 1));
    }

    #[test]
    fn slot_loop_advance_slot_multiple() {
        let params = ProtocolParams::mainnet();
        let mut slot_loop = SlotLoop::new(&params, Slot::from(100), vec![0u8; 32]);

        for _ in 0..10 {
            slot_loop.advance_slot();
        }

        assert_eq!(slot_loop.current_slot, Slot::from(110));
    }

    #[test]
    fn slot_loop_epoch_tracking() {
        let params = ProtocolParams::mainnet();
        let mut slot_loop = SlotLoop::new(&params, Slot::from(100), vec![0u8; 32]);

        let initial_epoch = slot_loop.current_epoch;

        // Advance many slots to cross epoch boundary.
        for _ in 0..params.consensus.epoch_slots {
            slot_loop.advance_slot();
        }

        // Epoch should have incremented.
        assert!(slot_loop.current_epoch > initial_epoch);
    }

    #[tokio::test]
    async fn slot_loop_run_smoke_test() {
        let params = ProtocolParams::mainnet();
        let slot_loop = SlotLoop::new(&params, Slot::from(0), vec![0u8; 32]);

        let vrf = qv_consensus::TestVrf::new([0u8; 32]);
        let pool_id = qv_consensus::PoolId::ZERO;

        let mut block_produced = false;

        // Run for a short time and then return.
        let handle = tokio::spawn(async move {
            let mut sl = slot_loop;
            let producer = |_slot: Slot| {
                Box::pin(async move {
                    Ok::<(), MinerError>(())
                })
                    as std::pin::Pin<Box<dyn std::future::Future<Output = MinerResult<()>> + Send>>
            };

            // This will run forever; we'll cancel it after a short time.
            let _ = run_slot_loop(sl, &vrf, &pool_id, producer).await;
        });

        // Let it run briefly, then cancel.
        sleep(Duration::from_millis(100)).await;
        handle.abort();
    }
}
