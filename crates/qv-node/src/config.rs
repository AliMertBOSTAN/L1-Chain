//! Node configuration: TOML parsing, validation, and presets.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

/// Full node configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Network identifier: mainnet, testnet, devnet, ephemeral
    pub network: String,

    /// Data directory for persistent storage (blocks, UTXO set, state)
    pub data_dir: PathBuf,

    /// Listen address for P2P (multiaddr format)
    pub listen_addr: String,

    /// RPC server bind address
    pub rpc_addr: SocketAddr,

    /// Metrics exporter bind address (Prometheus HTTP)
    pub metrics_addr: SocketAddr,

    /// Bootstrap peer addresses (multiaddr format)
    pub bootstrap_peers: Vec<String>,

    /// Seed node multiaddrs for initial peer discovery.
    #[serde(default)]
    pub seed_nodes: Vec<String>,

    /// Gossip configuration
    pub gossip: GossipConfig,

    /// Mempool configuration
    pub mempool: MempoolConfig,

    /// Storage backend: "rocksdb" | "redb" | "memory"
    pub storage_backend: String,

    /// Stake pool operator configuration (optional)
    #[serde(default)]
    pub stake_pool: Option<StakePoolConfig>,

    /// Optional 32-byte hex seed for a deterministic libp2p node identity.
    /// When set, the node's `PeerId` is stable across restarts.
    #[serde(default)]
    pub node_key_seed_hex: Option<String>,

    /// Shared genesis stake-pool set. When non-empty the node builds the
    /// full multi-pool stake distribution from this list (used by the
    /// multi-node devnet so every node agrees on the stake split).
    #[serde(default)]
    pub genesis_pools: Vec<GenesisPoolConfig>,

    /// Use a deterministic round-robin slot-leader schedule instead of
    /// VRF election (devnet only). The leader of slot `S` is genesis
    /// pool `S % n`. Guarantees exactly one leader per slot — no forks.
    #[serde(default)]
    pub round_robin_leader: bool,

    /// Seconds to wait after startup before the slot ticker starts
    /// producing blocks. Lets every peer connect and the gossip mesh
    /// form first, so no node produces a block before the network is up.
    #[serde(default)]
    pub startup_warmup_secs: u64,
}

/// One stake pool in the shared devnet genesis set. Every node carries
/// the identical list so they all build the same `StakeDistribution`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisPoolConfig {
    /// VRF seed (32-byte hex). Pool id is `PoolId::from_vrf_key(seed)`.
    pub vrf_seed_hex: String,
    /// Pool stake in smallest units.
    pub stake: u64,
}

/// Gossip / networking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Maximum number of peers to maintain
    pub max_peers: u32,

    /// Inbound peer limit
    pub max_inbound_peers: u32,

    /// Outbound peer target
    pub target_outbound_peers: u32,

    /// Message TTL (hops)
    pub message_ttl: u32,

    /// GossipSub heartbeat interval in milliseconds
    pub heartbeat_interval_ms: u64,
}

/// Mempool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolConfig {
    /// Maximum clear mempool size (transaction count)
    pub max_clear_pool_size: usize,

    /// Maximum encrypted mempool size (transaction count)
    pub max_encrypted_pool_size: usize,

    /// Minimum fee (satoshis per byte)
    pub min_fee_rate: u64,

    /// Transaction time-to-live (slots)
    pub tx_ttl_slots: u64,
}

/// Stake pool operator configuration (optional — only if this node produces blocks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePoolConfig {
    /// VRF seed (32 bytes, hex-encoded). Used for slot leader election.
    pub vrf_seed_hex: String,

    /// Initial stake amount (in smallest units).
    pub initial_stake: u64,

    /// Active slot coefficient (Praos f parameter, e.g. 0.05).
    pub active_slot_coeff: f64,
}

impl NodeConfig {
    /// Load configuration from a TOML file.
    pub fn from_toml(path: &std::path::Path) -> crate::NodeResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::NodeError::Config(format!("failed to read config file: {e}")))?;
        let config: NodeConfig = toml::from_str(&content)
            .map_err(|e| crate::NodeError::Config(format!("failed to parse config TOML: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    /// Write configuration to a TOML file.
    pub fn to_toml(&self, path: &std::path::Path) -> crate::NodeResult<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| crate::NodeError::Config(format!("failed to serialize config: {e}")))?;
        std::fs::write(path, content)
            .map_err(|e| crate::NodeError::Config(format!("failed to write config file: {e}")))?;
        Ok(())
    }

    /// Validate configuration constraints.
    pub fn validate(&self) -> crate::NodeResult<()> {
        if self.network.is_empty() {
            return Err(crate::NodeError::Config(
                "network field cannot be empty".to_string(),
            ));
        }

        if self.gossip.max_peers == 0 {
            return Err(crate::NodeError::Config(
                "gossip.max_peers must be > 0".to_string(),
            ));
        }

        if self.mempool.max_clear_pool_size == 0 {
            return Err(crate::NodeError::Config(
                "mempool.max_clear_pool_size must be > 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Default configuration for devnet.
    #[allow(clippy::unwrap_used)] // SAFETY: hardcoded socket addresses are infallible
    pub fn devnet() -> Self {
        Self {
            network: "devnet".to_string(),
            data_dir: PathBuf::from("./data"),
            listen_addr: "/ip4/127.0.0.1/tcp/10333".to_string(),
            rpc_addr: "127.0.0.1:8545".parse().unwrap(),
            metrics_addr: "127.0.0.1:9090".parse().unwrap(),
            bootstrap_peers: vec![],
            seed_nodes: vec![],
            gossip: GossipConfig {
                max_peers: 100,
                max_inbound_peers: 50,
                target_outbound_peers: 20,
                message_ttl: 16,
                heartbeat_interval_ms: 1000,
            },
            mempool: MempoolConfig {
                max_clear_pool_size: 10000,
                max_encrypted_pool_size: 1000,
                min_fee_rate: 1,
                tx_ttl_slots: 100,
            },
            storage_backend: "memory".to_string(),
            stake_pool: Some(StakePoolConfig {
                vrf_seed_hex: "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899"
                    .to_string(),
                initial_stake: 1_000_000_000,
                active_slot_coeff: 0.05,
            }),
            node_key_seed_hex: None,
            genesis_pools: Vec::new(),
            round_robin_leader: false,
            startup_warmup_secs: 0,
        }
    }

    /// Default configuration for testnet.
    #[allow(clippy::unwrap_used)] // SAFETY: hardcoded socket addresses are infallible
    pub fn testnet() -> Self {
        Self {
            network: "testnet".to_string(),
            data_dir: PathBuf::from("./data-testnet"),
            listen_addr: "/ip4/0.0.0.0/tcp/10334".to_string(),
            rpc_addr: "0.0.0.0:8546".parse().unwrap(),
            metrics_addr: "127.0.0.1:9091".parse().unwrap(),
            bootstrap_peers: vec![],
            seed_nodes: vec![],
            gossip: GossipConfig {
                max_peers: 200,
                max_inbound_peers: 100,
                target_outbound_peers: 50,
                message_ttl: 16,
                heartbeat_interval_ms: 1000,
            },
            mempool: MempoolConfig {
                max_clear_pool_size: 20000,
                max_encrypted_pool_size: 2000,
                min_fee_rate: 1,
                tx_ttl_slots: 200,
            },
            storage_backend: "redb".to_string(),
            stake_pool: None,
            node_key_seed_hex: None,
            genesis_pools: Vec::new(),
            round_robin_leader: false,
            startup_warmup_secs: 0,
        }
    }

    /// Default configuration for mainnet.
    #[allow(clippy::unwrap_used)] // SAFETY: hardcoded socket addresses are infallible
    pub fn mainnet() -> Self {
        Self {
            network: "mainnet".to_string(),
            data_dir: PathBuf::from("./data-mainnet"),
            listen_addr: "/ip4/0.0.0.0/tcp/10333".to_string(),
            rpc_addr: "0.0.0.0:8545".parse().unwrap(),
            metrics_addr: "127.0.0.1:9090".parse().unwrap(),
            bootstrap_peers: vec![],
            seed_nodes: vec![],
            gossip: GossipConfig {
                max_peers: 500,
                max_inbound_peers: 250,
                target_outbound_peers: 100,
                message_ttl: 16,
                heartbeat_interval_ms: 1000,
            },
            mempool: MempoolConfig {
                max_clear_pool_size: 100000,
                max_encrypted_pool_size: 10000,
                min_fee_rate: 1,
                tx_ttl_slots: 1000,
            },
            storage_backend: "rocksdb".to_string(),
            stake_pool: None,
            node_key_seed_hex: None,
            genesis_pools: Vec::new(),
            round_robin_leader: false,
            startup_warmup_secs: 0,
        }
    }

    /// Get preset based on network name, or load from TOML.
    pub fn for_network(
        network: &str,
        config_path: Option<&std::path::Path>,
    ) -> crate::NodeResult<Self> {
        match network {
            "mainnet" => Ok(Self::mainnet()),
            "testnet" => Ok(Self::testnet()),
            "devnet" => Ok(Self::devnet()),
            _ => {
                if let Some(path) = config_path {
                    Self::from_toml(path)
                } else {
                    Err(crate::NodeError::Config(format!(
                        "unknown network '{network}' and no config file provided",
                        network = network
                    )))
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_config_devnet_defaults() {
        let cfg = NodeConfig::devnet();
        assert_eq!(cfg.network, "devnet");
        assert!(cfg.data_dir.to_string_lossy().contains("data"));
        assert_eq!(cfg.gossip.max_peers, 100);
    }

    #[test]
    fn test_config_testnet_defaults() {
        let cfg = NodeConfig::testnet();
        assert_eq!(cfg.network, "testnet");
        assert_eq!(cfg.gossip.max_peers, 200);
    }

    #[test]
    fn test_config_mainnet_defaults() {
        let cfg = NodeConfig::mainnet();
        assert_eq!(cfg.network, "mainnet");
        assert_eq!(cfg.gossip.max_peers, 500);
    }

    #[test]
    fn test_config_validate_invalid_network() {
        let mut cfg = NodeConfig::devnet();
        cfg.network.clear();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_invalid_peers() {
        let mut cfg = NodeConfig::devnet();
        cfg.gossip.max_peers = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validate_invalid_mempool() {
        let mut cfg = NodeConfig::devnet();
        cfg.mempool.max_clear_pool_size = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_for_network() {
        let cfg = NodeConfig::for_network("mainnet", None).unwrap();
        assert_eq!(cfg.network, "mainnet");
    }
}
