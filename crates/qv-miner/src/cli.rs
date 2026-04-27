//! Command-line interface for the stake pool operator.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Stake pool operator CLI.
#[derive(Parser, Debug)]
#[command(
    name = "qv-miner",
    version,
    about = "Stake pool operator: VRF slot leader, block production, encrypted-mempool committee."
)]
pub struct Cli {
    /// Path to the configuration file (or parent directory for subcommands).
    #[arg(long, default_value = "config")]
    pub config_dir: PathBuf,

    /// Verbosity level (can be repeated: -v, -vv, -vvv).
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands for the operator.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a new operator: generate VRF/KES/cold keys and write operator.toml.
    Init {
        /// Pool name (for logging).
        #[arg(long)]
        pool_name: Option<String>,

        /// Operator pledge in satoshis.
        #[arg(long, default_value = "1000000000")]
        pledge: u64,

        /// Operator margin (as bps, e.g. 300 = 3%).
        #[arg(long, default_value = "300")]
        margin_bps: u32,

        /// Fixed cost per epoch in satoshis.
        #[arg(long, default_value = "10000000")]
        fixed_cost: u64,

        /// Output path for operator.toml.
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Register the pool on-chain: build and broadcast pool registration transaction.
    RegisterPool {
        /// Path to operator.toml.
        #[arg(long)]
        config: PathBuf,

        /// Node RPC endpoint (e.g., http://localhost:8080).
        #[arg(long)]
        node_rpc: String,

        /// Wait for confirmation (blocks).
        #[arg(long, default_value = "10")]
        wait_blocks: u64,
    },

    /// Helper command for delegators: show delegation instructions.
    Delegate {
        /// Pool ID (hex or base58).
        #[arg(long)]
        pool_id: String,

        /// Delegation amount in satoshis.
        #[arg(long)]
        amount: u64,
    },

    /// Run the operator daemon: main event loop (leadership checks, block production).
    Run {
        /// Path to operator.toml.
        #[arg(long)]
        config: PathBuf,

        /// Node RPC endpoint.
        #[arg(long)]
        node_rpc: String,

        /// Node gossip address (multiaddr format).
        #[arg(long)]
        node_gossip: Option<String>,

        /// Listen address for operator metrics (optional, for dashboard).
        #[arg(long)]
        metrics_addr: Option<String>,
    },

    /// Interactive TUI dashboard: live metrics (slots, leadership, rewards, mempool, peers).
    Dashboard {
        /// Path to operator.toml.
        #[arg(long)]
        config: PathBuf,

        /// Metrics endpoint (if operator is running with --metrics-addr).
        #[arg(long)]
        metrics_url: String,
    },

    /// Show loaded keys (public parts only, for verification).
    KeysShow {
        /// Path to operator.toml.
        #[arg(long)]
        config: PathBuf,

        /// Show extended key info (VRF/KES public key hex).
        #[arg(long)]
        verbose: bool,
    },
}

impl Cli {
    /// Parse CLI arguments.
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Translate verbosity count to tracing filter level.
    pub fn tracing_filter(&self) -> &str {
        match self.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn cli_parse_init() {
        let args = vec!["prog", "init", "--pool-name", "MyPool"];
        let cli = Cli::try_parse_from(&args);
        assert!(cli.is_ok());
    }

    #[test]
    fn cli_parse_run() {
        let args = vec![
            "prog",
            "run",
            "--config",
            "operator.toml",
            "--node-rpc",
            "http://localhost:8080",
        ];
        let cli = Cli::try_parse_from(&args);
        assert!(cli.is_ok());
    }

    #[test]
    fn tracing_filter_levels() {
        let cli_info = Cli {
            config_dir: PathBuf::from("config"),
            verbose: 0,
            command: Command::Run {
                config: PathBuf::from("op.toml"),
                node_rpc: "http://localhost:8080".to_string(),
                node_gossip: None,
                metrics_addr: None,
            },
        };
        assert_eq!(cli_info.tracing_filter(), "info");

        let cli_debug = Cli {
            config_dir: PathBuf::from("config"),
            verbose: 1,
            command: Command::Run {
                config: PathBuf::from("op.toml"),
                node_rpc: "http://localhost:8080".to_string(),
                node_gossip: None,
                metrics_addr: None,
            },
        };
        assert_eq!(cli_debug.tracing_filter(), "debug");

        let cli_trace = Cli {
            config_dir: PathBuf::from("config"),
            verbose: 3,
            command: Command::Run {
                config: PathBuf::from("op.toml"),
                node_rpc: "http://localhost:8080".to_string(),
                node_gossip: None,
                metrics_addr: None,
            },
        };
        assert_eq!(cli_trace.tracing_filter(), "trace");
    }
}
