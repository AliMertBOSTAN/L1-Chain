//! Stake pool operator: VRF slot leader, block production, encrypted-mempool committee.
//!
//! Entry point for the qv-miner binary.

use qv_miner::cli::{Cli, Command};
use qv_miner::config::OperatorConfig;
use qv_miner::keys::OperatorKeys;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments.
    let cli = Cli::parse_args();

    // Initialize tracing based on verbosity.
    let filter = cli.tracing_filter();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_env_filter(filter)
        .init();

    tracing::info!("qv-miner starting");

    // Dispatch to subcommand.
    match cli.command {
        Command::Init {
            pool_name,
            pledge,
            margin_bps,
            fixed_cost,
            output,
        } => {
            cmd_init(pool_name, pledge, margin_bps, fixed_cost, output).await?;
        }
        Command::RegisterPool {
            config,
            node_rpc,
            wait_blocks,
        } => {
            cmd_register_pool(config, node_rpc, wait_blocks).await?;
        }
        Command::Delegate { pool_id, amount } => {
            cmd_delegate(pool_id, amount).await?;
        }
        Command::Run {
            config,
            node_rpc,
            node_gossip,
            metrics_addr,
        } => {
            cmd_run(config, node_rpc, node_gossip, metrics_addr).await?;
        }
        Command::Dashboard {
            config,
            metrics_url,
        } => {
            cmd_dashboard(config, metrics_url).await?;
        }
        Command::KeysShow { config, verbose } => {
            cmd_keys_show(config, verbose).await?;
        }
    }

    Ok(())
}

async fn cmd_init(
    pool_name: Option<String>,
    pledge: u64,
    margin_bps: u32,
    fixed_cost: u64,
    output: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    tracing::info!("Initializing new stake pool operator");

    // Generate keys.
    let _keys = OperatorKeys::generate().map_err(|e| anyhow::anyhow!("{e}"))?;

    // Build configuration.
    let config = OperatorConfig {
        pool_id: format!("pool_{:x}", rand::random::<u32>()),
        pool_name: pool_name.unwrap_or_else(|| "MyPool".to_string()),
        keystore_path: std::path::PathBuf::from("keys/operator.keystore"),
        pledge,
        margin_bps,
        fixed_cost,
        reward_account: "qvaddr_reward".to_string(),
        network: qv_miner::config::Network::Testnet,
        node_rpc_url: "http://localhost:8080".to_string(),
        node_gossip_addr: None,
        clear_mempool_capacity: Some(10000),
        encrypted_mempool_capacity: Some(5000),
        decryption_committee_share_path: None,
        genesis_time: None,
        kes_rotation_period_epochs: None,
    };

    config.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

    // Save configuration to file.
    let output_path = output.unwrap_or_else(|| std::path::PathBuf::from("operator.toml"));
    config
        .save_toml(&output_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing::info!("Configuration written to: {}", output_path.display());
    tracing::info!("Pool ID: {}", config.pool_id);

    Ok(())
}

async fn cmd_register_pool(
    config_path: std::path::PathBuf,
    node_rpc: String,
    wait_blocks: u64,
) -> anyhow::Result<()> {
    tracing::info!("Registering pool on-chain");

    let config = OperatorConfig::load_toml(&config_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let keys = OperatorKeys::load_encrypted(&config.keystore_path, "password")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tx = qv_miner::registration::build_pool_registration_tx(
        &config,
        keys.vrf.public_bytes(),
        keys.kes.public_bytes(),
        &keys,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let txid = qv_miner::registration::submit_via_rpc(&tx, &node_rpc)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing::info!("Pool registration transaction submitted: {}", txid);
    tracing::info!("Waiting for {} blocks confirmation...", wait_blocks);

    Ok(())
}

async fn cmd_delegate(pool_id: String, amount: u64) -> anyhow::Result<()> {
    tracing::info!("Delegation helper for pool: {} (amount: {})", pool_id, amount);
    tracing::info!("To delegate, send a UTXO to the pool's reward address.");
    Ok(())
}

async fn cmd_run(
    config_path: std::path::PathBuf,
    node_rpc: String,
    _node_gossip: Option<String>,
    _metrics_addr: Option<String>,
) -> anyhow::Result<()> {
    tracing::info!("Starting operator daemon");

    let config = OperatorConfig::load_toml(&config_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    config.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing::info!("Operator: {} ({})", config.pool_name, config.pool_id);
    tracing::info!("Node RPC: {}", node_rpc);

    // In a real implementation, this would:
    // 1. Load keys from encrypted storage.
    // 2. Connect to the node RPC.
    // 3. Join the P2P network.
    // 4. Run the slot loop.
    // 5. Update the dashboard with metrics.

    // For now, sleep indefinitely to simulate running.
    tokio::time::sleep(tokio::time::Duration::from_secs(u64::MAX)).await;

    Ok(())
}

async fn cmd_dashboard(
    _config_path: std::path::PathBuf,
    metrics_url: String,
) -> anyhow::Result<()> {
    tracing::info!("Starting dashboard (connecting to: {})", metrics_url);
    // In a real implementation, render a ratatui TUI dashboard.
    Ok(())
}

async fn cmd_keys_show(config_path: std::path::PathBuf, verbose: bool) -> anyhow::Result<()> {
    tracing::info!("Loading operator keys");

    let config = OperatorConfig::load_toml(&config_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let keys = OperatorKeys::load_encrypted(&config.keystore_path, "password")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Operator: {} ({})", config.pool_name, config.pool_id);
    println!("VRF public key length: {} bytes", keys.vrf.public_bytes().len());
    println!("KES public key length: {} bytes", keys.kes.public_bytes().len());
    println!(
        "Cold public key length: {} bytes",
        keys.cold.public_bytes().len()
    );

    if verbose {
        println!(
            "VRF public (hex): {}",
            hex::encode(keys.vrf.public_bytes())
        );
        println!(
            "KES public (hex): {}",
            hex::encode(keys.kes.public_bytes())
        );
        println!(
            "Cold public (hex): {}",
            hex::encode(keys.cold.public_bytes())
        );
    }

    Ok(())
}
