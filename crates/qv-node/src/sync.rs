//! Bootstrap synchronization state machine (ADR-010).
//!
//! [`SyncManager`] is a **pure** decision component: given the node's local
//! chain and inputs from peers, it computes what sync action to take. All
//! network I/O — actually sending `GetHeaders` / `GetBlocks`, downloading and
//! applying blocks — is the caller's responsibility, executed against the
//! returned [`SyncAction`]. Keeping the manager free of `async` and libp2p
//! makes it fully unit-testable.
//!
//! Still TODO (ADR-010): wiring `on_tick` to a timer, routing incoming
//! `Headers` / `GetHeaders` messages from `network_handler`, and executing
//! the [`SyncAction`]s (request dispatch, block download, reorg apply).

use qv_consensus::{build_locator, maxvalid_bg, ChainEntry, ChainPreference};
use qv_core::BlockHash;

/// Whether the node is catching up or fully synced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// Actively catching up to the network.
    Syncing,
    /// Synced — following gossip only.
    Live,
}

/// An action the network layer should perform on the manager's behalf.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncAction {
    /// Send a `GetHeaders` request carrying this locator (tip→genesis).
    RequestHeaders { locator: Vec<BlockHash> },
    /// Download these full blocks — a candidate chain was adopted.
    RequestBlocks { hashes: Vec<BlockHash> },
    /// Nothing to do right now.
    Idle,
}

/// Bootstrap synchronization state machine (ADR-010).
///
/// Holds only the sync state and consensus parameters; the local chain is
/// passed in per call so the manager stays pure and testable.
#[derive(Clone, Debug)]
pub struct SyncManager {
    state: SyncState,
    /// Finality depth `k` — the shallow/deep fork boundary for maxvalid-bg.
    k_finality: u64,
    /// Density window `s`, in slots, for maxvalid-bg.
    window_slots: u64,
}

impl SyncManager {
    /// Create a manager that starts in [`SyncState::Syncing`].
    #[must_use]
    pub fn new(k_finality: u64, window_slots: u64) -> Self {
        Self {
            state: SyncState::Syncing,
            k_finality,
            window_slots,
        }
    }

    /// The current sync state.
    #[must_use]
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// Periodic tick. While syncing, ask a peer for headers past our tip;
    /// once live, there is nothing to do (gossip drives the chain).
    #[must_use]
    pub fn on_tick(&self, local_chain: &[ChainEntry]) -> SyncAction {
        match self.state {
            SyncState::Syncing => SyncAction::RequestHeaders {
                locator: build_locator(local_chain),
            },
            SyncState::Live => SyncAction::Idle,
        }
    }

    /// A peer returned a candidate header chain. Decide via Genesis
    /// maxvalid-bg whether to adopt it; if so, request its missing blocks.
    #[must_use]
    pub fn on_headers(
        &self,
        local_chain: &[ChainEntry],
        candidate: &[ChainEntry],
    ) -> SyncAction {
        match maxvalid_bg(local_chain, candidate, self.k_finality, self.window_slots) {
            ChainPreference::AdoptCandidate => SyncAction::RequestBlocks {
                hashes: missing_hashes(local_chain, candidate),
            },
            ChainPreference::KeepLocal => SyncAction::Idle,
        }
    }

    /// Mark the node caught up — switch to [`SyncState::Live`].
    pub fn mark_caught_up(&mut self) {
        self.state = SyncState::Live;
    }

    /// A gossip block arrived far ahead of our tip — re-enter `Syncing`.
    pub fn trigger_resync(&mut self) {
        self.state = SyncState::Syncing;
    }
}

/// Hashes in `candidate` that lie past its shared prefix with `local` —
/// i.e. the blocks the node must still download to adopt `candidate`.
fn missing_hashes(local: &[ChainEntry], candidate: &[ChainEntry]) -> Vec<BlockHash> {
    let common = local
        .iter()
        .zip(candidate.iter())
        .take_while(|(a, b)| a.hash == b.hash)
        .count();
    candidate.iter().skip(common).map(|e| e.hash).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use qv_core::{Hash256, Height, Slot};

    fn entry(hash_byte: u8, slot: u64) -> ChainEntry {
        ChainEntry {
            hash: BlockHash::from_bytes([hash_byte; 32]),
            parent_hash: BlockHash::ZERO,
            height: Height::from(0),
            slot: Slot::from(slot),
            producer_key_hash: Hash256::ZERO,
        }
    }

    #[test]
    fn starts_in_syncing() {
        assert_eq!(SyncManager::new(50, 2160).state(), SyncState::Syncing);
    }

    #[test]
    fn tick_requests_headers_while_syncing() {
        let m = SyncManager::new(50, 2160);
        let chain = vec![entry(0x00, 0), entry(0x01, 10)];
        assert_eq!(
            m.on_tick(&chain),
            SyncAction::RequestHeaders {
                locator: build_locator(&chain),
            }
        );
    }

    #[test]
    fn tick_is_idle_when_live() {
        let mut m = SyncManager::new(50, 2160);
        m.mark_caught_up();
        assert_eq!(m.state(), SyncState::Live);
        assert_eq!(m.on_tick(&[entry(0x00, 0)]), SyncAction::Idle);
    }

    #[test]
    fn resync_returns_to_syncing() {
        let mut m = SyncManager::new(50, 2160);
        m.mark_caught_up();
        m.trigger_resync();
        assert_eq!(m.state(), SyncState::Syncing);
    }

    #[test]
    fn adopts_denser_deep_fork_candidate() {
        // k=3, window=100: local sparse, candidate dense, deep fork.
        let m = SyncManager::new(3, 100);
        let mut local = vec![entry(0x00, 0)];
        let mut candidate = vec![entry(0x00, 0)];
        for i in 1..=6u64 {
            local.push(entry(0x10 + i as u8, i * 20));
            candidate.push(entry(0xC0 + i as u8, i * 5));
        }
        let expected: Vec<BlockHash> = (1..=6u64)
            .map(|i| BlockHash::from_bytes([0xC0 + i as u8; 32]))
            .collect();
        assert_eq!(
            m.on_headers(&local, &candidate),
            SyncAction::RequestBlocks { hashes: expected }
        );
    }

    #[test]
    fn keeps_local_when_candidate_not_better() {
        let m = SyncManager::new(3, 100);
        let local = vec![entry(0x00, 0), entry(0x01, 10), entry(0x02, 20)];
        let candidate = vec![entry(0x00, 0), entry(0x03, 11)];
        // Shallow fork, candidate shorter → keep local → Idle.
        assert_eq!(m.on_headers(&local, &candidate), SyncAction::Idle);
    }
}
