//! CLI argument parsing via `clap` derive macros.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Command-line arguments for the QuantumVault node.
#[derive(clap::Parser, Debug, Clone)]
#[command(
    name = "qv-node",
    version = "0.1.0",
    about = "QuantumVault full node daemon",
    long_about = "A quantum-safe, PoS-consensus, UTXO-model L1 blockchain node."
)]
pub struct CliArgs {
    /// Path to the configuration TOML file.
    #[arg(long, short = 'c', default_value = "config/qv-node.toml")]
    pub config: PathBuf,

    /// Path to the data directory (blocks, UTXO set, state).
    #[arg(long, short = 'd', default_value = "./data")]
    pub data_dir: PathBuf,

    /// Network to join: mainnet | testnet | devnet
    #[arg(long, default_value = "devnet")]
    pub network: String,

    /// Listen address for P2P (multiaddr format, e.g., /ip4/0.0.0.0/tcp/10333).
    #[arg(long)]
    pub listen: Option<String>,

    /// RPC server bind address (e.g., 127.0.0.1:8545).
    #[arg(long, default_value = "127.0.0.1:8545")]
    pub rpc_addr: SocketAddr,

    /// Metrics exporter bind address (Prometheus HTTP, e.g., 127.0.0.1:9090).
    #[arg(long, default_value = "127.0.0.1:9090")]
    pub metrics_addr: SocketAddr,

    /// Bootstrap peer multiaddrs (comma-separated).
    /// Example: /ip4/192.168.1.100/tcp/10333/p2p/QmXxxx,/ip4/192.168.1.101/tcp/10333/p2p/QmYyyy
    #[arg(long)]
    pub bootstrap: Option<String>,

    /// Initialize node (generate keys, config skeleton, exit).
    #[arg(long)]
    pub init: bool,

    /// Verbosity level: trace | debug | info | warn | error.
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl CliArgs {
    /// Parse bootstrap multiaddrs from comma-separated string.
    pub fn parse_bootstrap_addrs(&self) -> Vec<String> {
        self.bootstrap
            .as_ref()
            .map(|b| b.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let args = CliArgs {
            config: PathBuf::from("config/qv-node.toml"),
            data_dir: PathBuf::from("./data"),
            network: "devnet".to_string(),
            listen: None,
            rpc_addr: "127.0.0.1:8545".parse().unwrap(),
            metrics_addr: "127.0.0.1:9090".parse().unwrap(),
            bootstrap: None,
            init: false,
            log_level: "info".to_string(),
        };
        assert_eq!(args.network, "devnet");
        assert!(args.init == false);
    }

    #[test]
    fn test_parse_bootstrap_addrs() {
        let args = CliArgs {
            config: Default::default(),
            data_dir: Default::default(),
            network: "devnet".to_string(),
            listen: None,
            rpc_addr: "127.0.0.1:8545".parse().unwrap(),
            metrics_addr: "127.0.0.1:9090".parse().unwrap(),
            bootstrap: Some(
                "/ip4/192.168.1.100/tcp/10333/p2p/QmXxxx,/ip4/192.168.1.101/tcp/10333/p2p/QmYyyy"
                    .to_string(),
            ),
            init: false,
            log_level: "info".to_string(),
        };
        let addrs = args.parse_bootstrap_addrs();
        assert_eq!(addrs.len(), 2);
        assert!(addrs[0].contains("192.168.1.100"));
    }

    #[test]
    fn test_parse_bootstrap_addrs_empty() {
        let args = CliArgs {
            config: Default::default(),
            data_dir: Default::default(),
            network: "devnet".to_string(),
            listen: None,
            rpc_addr: "127.0.0.1:8545".parse().unwrap(),
            metrics_addr: "127.0.0.1:9090".parse().unwrap(),
            bootstrap: None,
            init: false,
            log_level: "info".to_string(),
        };
        assert!(args.parse_bootstrap_addrs().is_empty());
    }
}
