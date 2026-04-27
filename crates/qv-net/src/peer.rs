//! Peer identity, metadata, and peer tracking.
//!
//! `PeerInfo` captures per-peer metadata (addresses, last activity, reputation).
//! `PeerStore` is an in-memory registry used by the network node to manage the
//! set of known peers.

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

/// Minimum reputation a peer may hold before being considered for eviction.
pub const MIN_REPUTATION: i32 = -100;

/// Default reputation for freshly discovered peers.
pub const DEFAULT_REPUTATION: i32 = 0;

/// Maximum number of addresses stored per peer.
pub const MAX_ADDRS_PER_PEER: usize = 8;

/// Peer connection direction (inbound vs outbound).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// We initiated the connection.
    Outbound,
    /// The remote peer connected to us.
    Inbound,
}

/// Connection state of a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Not currently connected.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Fully connected.
    Connected,
}

/// Metadata tracked per known peer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    /// The peer's libp2p identity (stored as bytes for serde).
    peer_id_bytes: Vec<u8>,
    /// Known multiaddress strings.
    pub addresses: Vec<String>,
    /// Current connection state.
    pub state: ConnectionState,
    /// Direction of the last (or current) connection.
    pub direction: Option<Direction>,
    /// Reputation score — positive is good, negative is bad.
    pub reputation: i32,
    /// Unix-epoch seconds of first discovery.
    pub first_seen: u64,
    /// Unix-epoch seconds of last meaningful activity.
    pub last_seen: u64,
    /// Number of successful message exchanges.
    pub successful_interactions: u64,
    /// Number of failed or misbehaving interactions.
    pub failed_interactions: u64,
    /// Protocol version string reported by the peer (via Identify).
    pub protocol_version: Option<String>,
    /// User agent string reported by the peer (via Identify).
    pub agent_version: Option<String>,
}

impl PeerInfo {
    /// Create a fresh `PeerInfo` for a newly-discovered peer.
    #[must_use]
    pub fn new(peer_id: PeerId) -> Self {
        let now = now_secs();
        Self {
            peer_id_bytes: peer_id.to_bytes(),
            addresses: Vec::new(),
            state: ConnectionState::Disconnected,
            direction: None,
            reputation: DEFAULT_REPUTATION,
            first_seen: now,
            last_seen: now,
            successful_interactions: 0,
            failed_interactions: 0,
            protocol_version: None,
            agent_version: None,
        }
    }

    /// Return the `PeerId` for this peer.
    ///
    /// # Panics
    /// Panics if the stored bytes are not a valid `PeerId` (should never happen
    /// for properly-constructed `PeerInfo`).
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_bytes(&self.peer_id_bytes)
            .expect("PeerInfo always stores a valid PeerId")
    }

    /// Record a successful interaction and bump `last_seen`.
    pub fn record_success(&mut self) {
        self.last_seen = now_secs();
        self.successful_interactions = self.successful_interactions.saturating_add(1);
        self.reputation = self.reputation.saturating_add(1);
    }

    /// Record a failed or misbehaving interaction.
    pub fn record_failure(&mut self) {
        self.last_seen = now_secs();
        self.failed_interactions = self.failed_interactions.saturating_add(1);
        self.reputation = self.reputation.saturating_sub(10);
    }

    /// Manually adjust reputation by `delta` (clamped to `i32` bounds).
    pub fn adjust_reputation(&mut self, delta: i32) {
        self.reputation = self.reputation.saturating_add(delta);
    }

    /// Whether the peer's reputation has dropped below the eviction threshold.
    #[must_use]
    pub fn is_banned(&self) -> bool {
        self.reputation <= MIN_REPUTATION
    }

    /// Add an address if not already present and within the per-peer cap.
    pub fn add_address(&mut self, addr: Multiaddr) {
        let s = addr.to_string();
        if self.addresses.len() < MAX_ADDRS_PER_PEER && !self.addresses.contains(&s) {
            self.addresses.push(s);
        }
    }

    /// Return the stored addresses as parsed `Multiaddr` values.
    pub fn multiaddrs(&self) -> Vec<Multiaddr> {
        self.addresses
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect()
    }

    /// Duration since last activity (returns `None` if clock went backwards).
    #[must_use]
    pub fn idle_duration(&self) -> Option<Duration> {
        let now = now_secs();
        if now >= self.last_seen {
            Some(Duration::from_secs(now - self.last_seen))
        } else {
            None
        }
    }
}

/// In-memory peer registry.
///
/// Keyed by stringified `PeerId`. The network node mutates this store as peers
/// connect, disconnect, and exchange messages.
#[derive(Clone, Debug, Default)]
pub struct PeerStore {
    peers: BTreeMap<String, PeerInfo>,
}

impl PeerStore {
    /// Create an empty peer store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of known peers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Insert or update a peer. If the peer already exists the info is merged:
    /// addresses are unioned (up to cap) and state/last_seen are updated.
    pub fn upsert(&mut self, info: PeerInfo) {
        let key = info.peer_id().to_string();
        if let Some(existing) = self.peers.get_mut(&key) {
            for addr in &info.multiaddrs() {
                existing.add_address(addr.clone());
            }
            existing.state = info.state;
            existing.last_seen = info.last_seen;
            if info.protocol_version.is_some() {
                existing.protocol_version.clone_from(&info.protocol_version);
            }
            if info.agent_version.is_some() {
                existing.agent_version.clone_from(&info.agent_version);
            }
        } else {
            self.peers.insert(key, info);
        }
    }

    /// Get peer info by `PeerId`.
    #[must_use]
    pub fn get(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(&peer_id.to_string())
    }

    /// Get mutable peer info by `PeerId`.
    pub fn get_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(&peer_id.to_string())
    }

    /// Remove a peer from the store.
    pub fn remove(&mut self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.remove(&peer_id.to_string())
    }

    /// Iterate over all known peers.
    pub fn iter(&self) -> impl Iterator<Item = &PeerInfo> {
        self.peers.values()
    }

    /// Return all currently-connected peers.
    pub fn connected(&self) -> Vec<&PeerInfo> {
        self.peers
            .values()
            .filter(|p| p.state == ConnectionState::Connected)
            .collect()
    }

    /// Return peers whose reputation has fallen below the ban threshold.
    pub fn banned(&self) -> Vec<&PeerInfo> {
        self.peers.values().filter(|p| p.is_banned()).collect()
    }

    /// Evict all peers that are banned. Returns the number removed.
    pub fn evict_banned(&mut self) -> usize {
        let before = self.peers.len();
        self.peers.retain(|_, info| !info.is_banned());
        before - self.peers.len()
    }

    /// Evict peers that have been idle longer than `max_idle` and are
    /// currently disconnected. Returns the number removed.
    pub fn evict_idle(&mut self, max_idle: Duration) -> usize {
        let before = self.peers.len();
        self.peers.retain(|_, info| {
            if info.state != ConnectionState::Disconnected {
                return true;
            }
            info.idle_duration()
                .map_or(true, |idle| idle < max_idle)
        });
        before - self.peers.len()
    }
}

/// Current Unix-epoch seconds (saturating at 0 on clock error).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::time::Duration;

    use libp2p::identity::Keypair;
    use libp2p::PeerId;

    use super::{ConnectionState, PeerInfo, PeerStore, DEFAULT_REPUTATION, MIN_REPUTATION};

    fn random_peer_id() -> PeerId {
        let kp = Keypair::generate_ed25519();
        PeerId::from_public_key(&kp.public())
    }

    #[test]
    fn new_peer_info_defaults() {
        let pid = random_peer_id();
        let info = PeerInfo::new(pid);

        assert_eq!(info.peer_id(), pid);
        assert_eq!(info.reputation, DEFAULT_REPUTATION);
        assert_eq!(info.state, ConnectionState::Disconnected);
        assert!(info.addresses.is_empty());
        assert_eq!(info.successful_interactions, 0);
    }

    #[test]
    fn reputation_tracking() {
        let pid = random_peer_id();
        let mut info = PeerInfo::new(pid);

        info.record_success();
        assert_eq!(info.reputation, 1);
        assert_eq!(info.successful_interactions, 1);

        info.record_failure();
        assert_eq!(info.reputation, -9);
        assert_eq!(info.failed_interactions, 1);

        assert!(!info.is_banned());

        // Drive reputation to ban threshold
        for _ in 0..10 {
            info.record_failure();
        }
        assert!(info.is_banned());
    }

    #[test]
    fn peer_store_upsert_and_eviction() {
        let mut store = PeerStore::new();
        assert!(store.is_empty());

        let pid = random_peer_id();
        let mut info = PeerInfo::new(pid);
        info.state = ConnectionState::Connected;
        store.upsert(info);

        assert_eq!(store.len(), 1);
        assert_eq!(store.connected().len(), 1);

        // Mark as bad
        let entry = store.get_mut(&pid).unwrap();
        entry.reputation = MIN_REPUTATION;

        assert_eq!(store.banned().len(), 1);
        let evicted = store.evict_banned();
        assert_eq!(evicted, 1);
        assert!(store.is_empty());
    }

    #[test]
    fn peer_store_idle_eviction() {
        let mut store = PeerStore::new();

        let pid = random_peer_id();
        let mut info = PeerInfo::new(pid);
        info.state = ConnectionState::Disconnected;
        // Set last_seen far in the past
        info.last_seen = 1;
        store.upsert(info);

        let evicted = store.evict_idle(Duration::from_secs(60));
        assert_eq!(evicted, 1);
    }

    #[test]
    fn address_cap_enforced() {
        let pid = random_peer_id();
        let mut info = PeerInfo::new(pid);

        for i in 0..20u16 {
            let addr: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", 9000 + i)
                .parse()
                .unwrap();
            info.add_address(addr);
        }

        assert_eq!(info.addresses.len(), super::MAX_ADDRS_PER_PEER);
    }
}
