//! QuantumVault full node daemon.
//!
//! Orchestrates consensus, storage, networking, and RPC endpoints for a quantum-safe,
//! PoS-based L1 blockchain.

#![forbid(unsafe_code)]
// SAFETY: hardcoded socket-address parses below are infallible. The binary
// main() may also `?`-propagate setup errors; we treat the few `unwrap()`
// calls on compile-time constant addresses as acceptable in main.
#![allow(clippy::unwrap_used)]

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
        // Initialize mode: create data dir, generate genesis + keys, write config, then exit.
        tracing::info!("initializing node (--init mode)");

        // 1. Create data directory
        std::fs::create_dir_all(&args.data_dir)
            .map_err(|e| anyhow::anyhow!("failed to create data directory: {}", e))?;
        tracing::info!(path = %args.data_dir.display(), "data directory created");

        // 2. Build and write config
        let mut cfg = NodeConfig::for_network(&args.network, Some(&args.config))?;

        // Override with CLI arguments if provided.
        if let Some(listen) = &args.listen {
            cfg.listen_addr = listen.clone();
        }
        cfg.rpc_addr = args.rpc_addr;
        cfg.metrics_addr = args.metrics_addr;
        if let Some(_bootstrap) = &args.bootstrap {
            cfg.bootstrap_peers = args.parse_bootstrap_addrs();
        }

        cfg.to_toml(&args.config)?;
        tracing::info!(path = %args.config.display(), "configuration written");

        // 3. Generate devnet genesis if network is devnet
        if cfg.network == "devnet" {
            let (genesis_block, secret_keys) = qv_node::genesis::devnet_genesis();

            // Print keys to stdout for user
            println!(
                "\n=== Devnet Genesis Keys ({} accounts) ===\n",
                secret_keys.len()
            );
            let mut keys_json = Vec::new();

            for (i, sk) in secret_keys.iter().enumerate() {
                let hex_key = hex::encode(sk.expose_secret());
                // Print shortened form for readability
                println!("  Account {}: {}", i, &hex_key[..64.min(hex_key.len())]);
                keys_json.push(serde_json::json!({
                    "index": i,
                    "secret_key_hex": hex_key,
                    "level": "Level3",
                }));
            }

            // 4. Write genesis-keys.json to data directory
            let keys_path = args.data_dir.join("genesis-keys.json");
            let json = serde_json::to_string_pretty(&keys_json)?;
            std::fs::write(&keys_path, json)
                .map_err(|e| anyhow::anyhow!("failed to write genesis keys: {}", e))?;
            println!("\nGenesis keys written to: {}", keys_path.display());

            // Log block hash
            match genesis_block.hash() {
                Ok(hash) => println!("Genesis block hash: {}", hash),
                Err(e) => {
                    tracing::warn!(error = %e, "genesis block hash computation failed");
                    println!("Genesis block hash: <error computing hash>");
                }
            }

            println!("\nNode initialized. Run without --init to start.\n");
        } else {
            println!(
                "\nNode initialized for network: {}. Run without --init to start.\n",
                cfg.network
            );
        }

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
        if let Some(_bootstrap) = &args.bootstrap {
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
