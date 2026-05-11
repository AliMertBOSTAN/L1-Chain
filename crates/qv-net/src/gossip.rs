//! GossipSub configuration and topic management.
//!
//! QuantumVault uses four gossip topics for block-level protocol messages.
//! Each topic carries [`Envelope`]-encoded payloads. Message deduplication
//! is handled by a SHA3-256 content hash stored in a time-bounded cache.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use libp2p::gossipsub::{
    self, MessageAuthenticity, MessageId, ValidationMode,
};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};

use crate::message::MessageKind;
use crate::NetError;

/// Gossip topic names — one per message category.
pub const TOPIC_BLOCKS: &str = "/qv/blocks/1";
pub const TOPIC_TRANSACTIONS: &str = "/qv/tx/1";
pub const TOPIC_VRF_PROOFS: &str = "/qv/vrf/1";
pub const TOPIC_VOTES: &str = "/qv/votes/1";

/// Configuration knobs for the GossipSub behaviour.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Heartbeat interval in milliseconds.
    pub heartbeat_ms: u64,
    /// Mesh target (ideal) peers per topic.
    pub mesh_n: usize,
    /// Mesh low watermark — below this we graft.
    pub mesh_n_low: usize,
    /// Mesh high watermark — above this we prune.
    pub mesh_n_high: usize,
    /// Max transmit size (bytes) for a single gossip message.
    pub max_transmit_size: usize,
    /// Deduplication cache TTL in seconds.
    pub seen_cache_ttl_secs: u64,
    /// Maximum size of the deduplication cache.
    pub seen_cache_capacity: usize,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            heartbeat_ms: 700,
            mesh_n: 8,
            mesh_n_low: 6,
            mesh_n_high: 12,
            max_transmit_size: 4 * 1024 * 1024, // 4 MiB — matches MAX_MESSAGE_SIZE
            seen_cache_ttl_secs: 120,
            seen_cache_capacity: 10_000,
        }
    }
}

impl GossipConfig {
    /// Conservative configuration for mainnet.
    #[must_use]
    pub fn mainnet() -> Self {
        Self::default()
    }

    /// More aggressive configuration for testnets.
    #[must_use]
    pub fn testnet() -> Self {
        Self {
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 10,
            seen_cache_ttl_secs: 60,
            ..Self::default()
        }
    }

    /// Minimal config for local/ephemeral test networks.
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            heartbeat_ms: 300,
            mesh_n: 3,
            mesh_n_low: 2,
            mesh_n_high: 5,
            max_transmit_size: 1024 * 1024,
            seen_cache_ttl_secs: 30,
            seen_cache_capacity: 1_000,
        }
    }
}

/// Return the gossip topic name for a given message kind.
///
/// Only gossip-eligible kinds have a topic.  Returns `None` for
/// request-response and ping/pong messages.
#[must_use]
pub fn topic_for_kind(kind: MessageKind) -> Option<&'static str> {
    match kind {
        MessageKind::Block => Some(TOPIC_BLOCKS),
        MessageKind::Transaction => Some(TOPIC_TRANSACTIONS),
        MessageKind::VrfProof => Some(TOPIC_VRF_PROOFS),
        MessageKind::Vote => Some(TOPIC_VOTES),
        _ => None,
    }
}

/// All gossip topic strings.
#[must_use]
pub fn all_topics() -> Vec<&'static str> {
    vec![TOPIC_BLOCKS, TOPIC_TRANSACTIONS, TOPIC_VRF_PROOFS, TOPIC_VOTES]
}

/// Build the GossipSub [`gossipsub::Behaviour`] from a config and keypair.
pub fn build_gossipsub(
    keypair: &Keypair,
    config: &GossipConfig,
) -> Result<gossipsub::Behaviour, NetError> {
    // SHA3-256 content-address: hash the payload to produce the MessageId.
    let message_id_fn = |message: &gossipsub::Message| {
        let hash = qv_crypto::sha3_256(&message.data);
        MessageId::from(hash.to_vec())
    };

    // libp2p-gossipsub 0.46+ enforces `mesh_outbound_min <= mesh_n / 2`.
    // Default `mesh_outbound_min = 2` breaks our ephemeral preset
    // (`mesh_n = 3` → 2 ≤ 1 fails). Compute a safe value: at least 1, at
    // most `mesh_n / 2`. (We use `mesh_n_low` cap as a soft floor too.)
    let safe_mesh_outbound_min = (config.mesh_n / 2).max(1).min(config.mesh_n_low);

    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_millis(config.heartbeat_ms))
        .mesh_n(config.mesh_n)
        .mesh_n_low(config.mesh_n_low)
        .mesh_n_high(config.mesh_n_high)
        .mesh_outbound_min(safe_mesh_outbound_min)
        .max_transmit_size(config.max_transmit_size)
        .validation_mode(ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()
        .map_err(|e| NetError::Config(format!("gossipsub config: {e}")))?;

    gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|e| NetError::Config(format!("gossipsub behaviour: {e}")))
}

// ---------------------------------------------------------------------------
// Deduplication cache
// ---------------------------------------------------------------------------

/// A time-bounded, capacity-limited message deduplication cache.
///
/// Each entry expires after `ttl` or when the cache exceeds `capacity`
/// (oldest entries evicted first).
#[derive(Debug)]
pub struct SeenCache {
    entries: HashMap<Vec<u8>, Instant>,
    ttl: Duration,
    capacity: usize,
}

impl SeenCache {
    /// Create a new `SeenCache`.
    #[must_use]
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.min(1024)),
            ttl,
            capacity,
        }
    }

    /// Create from a `GossipConfig`.
    #[must_use]
    pub fn from_config(config: &GossipConfig) -> Self {
        Self::new(
            Duration::from_secs(config.seen_cache_ttl_secs),
            config.seen_cache_capacity,
        )
    }

    /// Insert a message hash into the cache.
    ///
    /// Returns `true` if the hash was **already seen** (duplicate),
    /// `false` if this is a new entry.
    pub fn insert(&mut self, hash: Vec<u8>) -> bool {
        self.evict_expired();

        if self.entries.contains_key(&hash) {
            return true;
        }

        // Evict oldest if at capacity
        if self.entries.len() >= self.capacity {
            self.evict_oldest();
        }

        self.entries.insert(hash, Instant::now());
        false
    }

    /// Whether a hash is currently in the cache.
    #[must_use]
    pub fn contains(&self, hash: &[u8]) -> bool {
        self.entries
            .get(hash)
            .map_or(false, |ts| ts.elapsed() < self.ttl)
    }

    /// Number of (non-expired) entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
            .values()
            .filter(|ts| ts.elapsed() < self.ttl)
            .count()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn evict_expired(&mut self) {
        let ttl = self.ttl;
        self.entries.retain(|_, ts| ts.elapsed() < ttl);
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, ts)| **ts)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn topic_mapping() {
        assert_eq!(topic_for_kind(MessageKind::Block), Some(TOPIC_BLOCKS));
        assert_eq!(topic_for_kind(MessageKind::Transaction), Some(TOPIC_TRANSACTIONS));
        assert_eq!(topic_for_kind(MessageKind::VrfProof), Some(TOPIC_VRF_PROOFS));
        assert_eq!(topic_for_kind(MessageKind::Vote), Some(TOPIC_VOTES));
        assert_eq!(topic_for_kind(MessageKind::Ping), None);
        assert_eq!(topic_for_kind(MessageKind::GetHeaders), None);
    }

    #[test]
    fn all_topics_complete() {
        let topics = all_topics();
        assert_eq!(topics.len(), 4);
        assert!(topics.contains(&TOPIC_BLOCKS));
        assert!(topics.contains(&TOPIC_TRANSACTIONS));
    }

    #[test]
    fn seen_cache_dedup() {
        let mut cache = SeenCache::new(Duration::from_secs(60), 100);

        let hash = vec![1, 2, 3];
        assert!(!cache.insert(hash.clone())); // new
        assert!(cache.insert(hash.clone())); // duplicate
        assert!(cache.contains(&hash));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn seen_cache_capacity_eviction() {
        let mut cache = SeenCache::new(Duration::from_secs(60), 3);

        cache.insert(vec![1]);
        cache.insert(vec![2]);
        cache.insert(vec![3]);
        assert_eq!(cache.len(), 3);

        // Adding a 4th should evict the oldest
        cache.insert(vec![4]);
        assert_eq!(cache.len(), 3);
        assert!(cache.contains(&[4]));
    }

    #[test]
    fn gossip_config_presets() {
        let main = GossipConfig::mainnet();
        let test = GossipConfig::testnet();
        let eph = GossipConfig::ephemeral();

        assert!(main.mesh_n > eph.mesh_n);
        assert!(test.seen_cache_ttl_secs < main.seen_cache_ttl_secs);
    }
}
