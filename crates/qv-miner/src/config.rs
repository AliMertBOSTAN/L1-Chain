//! Configuration for the stake pool operator.

use crate::MinerError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Network selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    /// Mainnet.
    Mainnet,
    /// Testnet.
    Testnet,
    /// Devnet (ephemeral).
    Devnet,
}

/// Configuration for a stake pool operator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatorConfig {
    /// Unique pool identifier (hex or base58).
    pub pool_id: String,

    /// Pool name for logging/identification.
    pub pool_name: String,

    /// Path to the encrypted operator keystore (envanter M-04).
    ///
    /// Single Argon2id+AES-256-GCM file holding the 32-byte master seed
    /// from which `(vrf, kes, cold)` are deterministically derived plus the
    /// current KES rotation period. See `qv_miner::keystore` for format.
    pub keystore_path: PathBuf,

    /// Operator pledge in satoshis.
    pub pledge: u64,

    /// Margin as basis points (0-10000, where 10000 = 100%).
    pub margin_bps: u32,

    /// Fixed cost deducted per epoch in satoshis.
    pub fixed_cost: u64,

    /// Reward account address (where block rewards are sent).
    pub reward_account: String,

    /// Network (mainnet, testnet, devnet).
    pub network: Network,

    /// Node RPC endpoint (e.g., http://localhost:8080).
    pub node_rpc_url: String,

    /// Node gossip address (multiaddr, e.g., /ip4/127.0.0.1/tcp/30000).
    pub node_gossip_addr: Option<String>,

    /// Clear mempool capacity (max transactions).
    pub clear_mempool_capacity: Option<usize>,

    /// Encrypted mempool capacity.
    pub encrypted_mempool_capacity: Option<usize>,

    /// Path to decryption committee share (if operator is on the committee).
    pub decryption_committee_share_path: Option<PathBuf>,

    /// Genesis time (Unix seconds) — defaults to protocol params if not specified.
    pub genesis_time: Option<u64>,

    /// KES rotation period (in epochs; if not set, rotates every epoch).
    pub kes_rotation_period_epochs: Option<u64>,
}

impl OperatorConfig {
    /// Load configuration from a TOML file.
    pub fn load_toml<P: AsRef<Path>>(path: P) -> Result<Self, MinerError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| MinerError::Config(format!("failed to read config: {e}")))?;

        toml::from_str::<Self>(&content)
            .map_err(|e| MinerError::Config(format!("invalid TOML: {e}")))
    }

    /// Save configuration to a TOML file.
    pub fn save_toml<P: AsRef<Path>>(&self, path: P) -> Result<(), MinerError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| MinerError::Config(format!("TOML serialization failed: {e}")))?;

        std::fs::write(path.as_ref(), content)
            .map_err(|e| MinerError::Config(format!("failed to write config: {e}")))
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), MinerError> {
        if self.pool_id.is_empty() {
            return Err(MinerError::Config("pool_id must not be empty".to_string()));
        }

        if self.margin_bps > 10000 {
            return Err(MinerError::Config(format!(
                "margin_bps {} exceeds 10000",
                self.margin_bps
            )));
        }

        if self.node_rpc_url.is_empty() {
            return Err(MinerError::Config(
                "node_rpc_url must not be empty".to_string(),
            ));
        }

        if self.reward_account.is_empty() {
            return Err(MinerError::Config(
                "reward_account must not be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Get the clear mempool capacity, or default to 10000.
    pub fn get_clear_mempool_capacity(&self) -> usize {
        self.clear_mempool_capacity.unwrap_or(10000)
    }

    /// Get the encrypted mempool capacity, or default to 5000.
    pub fn get_encrypted_mempool_capacity(&self) -> usize {
        self.encrypted_mempool_capacity.unwrap_or(5000)
    }

    /// Get the KES rotation period, or default to 1 epoch.
    pub fn get_kes_rotation_period_epochs(&self) -> u64 {
        self.kes_rotation_period_epochs.unwrap_or(1)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn sample_config() -> OperatorConfig {
        OperatorConfig {
            pool_id: "pool_abc123".to_string(),
            pool_name: "MyPool".to_string(),
            keystore_path: PathBuf::from("keys/operator.keystore"),
            pledge: 1_000_000_000,
            margin_bps: 300,
            fixed_cost: 10_000_000,
            reward_account: "qvaddr_...".to_string(),
            network: Network::Testnet,
            node_rpc_url: "http://localhost:8080".to_string(),
            node_gossip_addr: Some("/ip4/127.0.0.1/tcp/30000".to_string()),
            clear_mempool_capacity: Some(10000),
            encrypted_mempool_capacity: Some(5000),
            decryption_committee_share_path: None,
            genesis_time: None,
            kes_rotation_period_epochs: None,
        }
    }

    #[test]
    fn config_defaults() {
        let cfg = sample_config();
        assert_eq!(cfg.get_clear_mempool_capacity(), 10000);
        assert_eq!(cfg.get_encrypted_mempool_capacity(), 5000);
        assert_eq!(cfg.get_kes_rotation_period_epochs(), 1);
    }

    #[test]
    fn config_validate_ok() {
        let cfg = sample_config();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn config_validate_empty_pool_id() {
        let mut cfg = sample_config();
        cfg.pool_id = String::new();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_validate_margin_out_of_range() {
        let mut cfg = sample_config();
        cfg.margin_bps = 10001;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_network_variants() {
        assert_eq!(Network::Mainnet, Network::Mainnet);
        assert_ne!(Network::Mainnet, Network::Testnet);
    }
}
