//! QuantumVault full node daemon.
//!
//! Orchestrates consensus, storage, networking, and RPC endpoints for a quantum-safe,
//! PoS-based L1 blockchain.

#![forbid(unsafe_code)]

use clap::Parser;
use qv_node::{CliArgs, Node, NodeConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments.
    let args = CliArgs::parse();

    // Initialize logging based on log_level argument.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        network = %args.network,
        "QuantumVault node starting"
    );

    // Load configuration based on CLI arguments or network preset.
    let config = if args.init {
        // Initialize mode: generate keys and config skeleton, then exit.
        tracing::info!("initializing configuration (--init mode)");
        let mut cfg = NodeConfig::for_network(&args.network, Some(&args.config))?;

        // Override with CLI arguments if provided.
        if let Some(listen) = &args.listen {
            cfg.listen_addr = listen.clone();
        }
        cfg.rpc_addr = args.rpc_addr;
        cfg.metrics_addr = args.metrics_addr;
        if let Some(bootstrap) = &args.bootstrap {
            cfg.bootstrap_peers = args.parse_bootstrap_addrs();
        }

        // Write config to file.
        cfg.to_toml(&args.config)?;
        tracing::info!(path = %args.config.display(), "configuration written");
        return Ok(());
    } else {
        // Normal mode: load from file or use preset.
        let mut cfg = if args.config.exists() {
            NodeConfig::from_toml(&args.config)?
        } else {
            NodeConfig::for_network(&args.network, None)?
        };

        // Allow CLI arguments to override config file.
        if let Some(listen) = &args.listen {
            cfg.listen_addr = listen.clone();
        }
        if args.rpc_addr != "127.0.0.1:8545".parse().unwrap() {
            cfg.rpc_addr = args.rpc_addr;
        }
        if args.metrics_addr != "127.0.0.1:9090".parse().unwrap() {
            cfg.metrics_addr = args.metrics_addr;
        }
        if let Some(bootstrap) = &args.bootstrap {
            cfg.bootstrap_peers = args.parse_bootstrap_addrs();
        }

        cfg
    };

    tracing::info!(
        network = %config.network,
        data_dir = %config.data_dir.display(),
        rpc_addr = %config.rpc_addr,
        metrics_addr = %config.metrics_addr,
        "configuration loaded"
    );

    // Initialize metrics exporter.
    qv_node::metrics::init_exporter(config.metrics_addr)?;

    // Create and run the node.
    let node = Node::new(config).await?;
    node.run().await?;

    tracing::info!("QuantumVault node shutdown complete");
    Ok(())
}
