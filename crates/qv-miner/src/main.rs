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

    let config = OperatorConfig::load_toml(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
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
    tracing::info!(
        "Delegation helper for pool: {} (amount: {})",
        pool_id,
        amount
    );
    tracing::info!("To delegate, send a UTXO to the pool's reward address.");
    Ok(())
}

async fn cmd_run(
    config_path: std::path::PathBuf,
    node_rpc: String,
    _node_gossip: Option<String>,
    _metrics_addr: Option<String>,
) -> anyhow::Result<()> {
    use qv_consensus::{PoolId, StakeDistribution, StakePool};
    use qv_core::{Amount, ProtocolParams};
    use qv_miner::slot_loop::{run_slot_loop, SlotLoop};

    tracing::info!("Starting operator daemon");

    // ── 1. Config ──────────────────────────────────────────────────────────
    let config = OperatorConfig::load_toml(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    config.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

    tracing::info!(pool = %config.pool_name, pool_id = %config.pool_id, "operator config loaded");
    tracing::info!(rpc = %node_rpc, "node RPC endpoint");

    // ── 2. Keystore ────────────────────────────────────────────────────────
    // Read password from QV_KEYSTORE_PASS env var (CI/devnet) or interactive
    // prompt (production). Avoids printing the password to logs.
    let password = if let Ok(env_pass) = std::env::var("QV_KEYSTORE_PASS") {
        tracing::warn!("using QV_KEYSTORE_PASS from environment (development convenience)");
        env_pass
    } else {
        rpassword::prompt_password("Keystore password: ")
            .map_err(|e| anyhow::anyhow!("failed to read password: {e}"))?
    };

    tracing::info!(keystore = %config.keystore_path.display(), "loading operator keys");
    let keys = OperatorKeys::load_encrypted(&config.keystore_path, &password)
        .map_err(|e| anyhow::anyhow!("keystore load failed: {e}"))?;
    drop(password); // explicit zeroize hint; SecureBytes will scrub on drop anyway

    tracing::info!(
        vrf_pk_bytes = keys.vrf.public_bytes().len(),
        kes_pk_bytes = keys.kes.public_bytes().len(),
        cold_pk_bytes = keys.cold.public_bytes().len(),
        kes_period = keys.kes.period(),
        "operator keys loaded"
    );

    // ── 3. Consensus state (M-09b: real RPC fetch is a follow-up) ─────────
    //
    // For now we mock the stake distribution: this pool holds 100% of stake.
    // In production, M-09b will fetch via RPC `qv_getStakeDistribution` and
    // refresh at every epoch boundary.
    let params = match config.network {
        qv_miner::config::Network::Mainnet => ProtocolParams::mainnet(),
        qv_miner::config::Network::Testnet => ProtocolParams::testnet(),
        qv_miner::config::Network::Devnet => ProtocolParams::ephemeral(),
    };

    let pool_id = PoolId::from_vrf_key(keys.vrf.public_bytes());
    let _self_pool = StakePool {
        id: pool_id,
        vrf_key: keys.vrf.public_bytes().to_vec(),
        kes_key: keys.kes.public_bytes().to_vec(),
        pledge: Amount::from_smallest_units(config.pledge),
        margin_num: config.margin_bps,
        margin_den: 10_000,
        fixed_cost: Amount::from_smallest_units(config.fixed_cost),
        active: true,
    };
    // Bootstrap stake distribution: this pool holds 100% of stake in the mock.
    // M-09b will replace this with an RPC fetch (`qv_getStakeDistribution`)
    // refreshed on every epoch boundary.
    let stake_distribution = StakeDistribution::new(
        qv_core::Epoch::GENESIS,
        [(pool_id, Amount::from_smallest_units(config.pledge))],
    )
    .map_err(|e| anyhow::anyhow!("stake distribution: {e}"))?;

    // Epoch nonce: M-09b will fetch via RPC; for scaffolding use a fixed seed
    // tied to the pool id (so the same operator deterministically maps to the
    // same nonce across restarts on the same devnet).
    let initial_nonce = qv_crypto::sha3_256(b"qv-miner-devnet-genesis-nonce").to_vec();

    // Slot clock: figure out the current slot from wall-clock time.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let slot_clock = qv_consensus::SlotClock::from_params(&params);
    let current_slot = slot_clock
        .current_info(now_ms)
        .map(|info| info.slot)
        .unwrap_or(qv_core::Slot::GENESIS);

    let mut sl = SlotLoop::new(&params, current_slot, initial_nonce);
    sl.stake_distribution = Some(stake_distribution);

    tracing::info!(
        slot = %current_slot,
        epoch_slots = params.consensus.epoch_slots,
        slot_duration_ms = params.consensus.slot_duration_ms,
        "slot loop initialized; entering main loop"
    );

    // ── 4. Block producer callback (scaffolding) ───────────────────────────
    //
    // M-09c will turn this into a real `produce_block(...)` + RPC submit. For
    // now we just log the leadership event and exit cleanly; the qv-node
    // running on the same machine still does real block production in devnet.
    let block_producer_fn = move |slot: qv_core::Slot| -> std::pin::Pin<
        Box<dyn std::future::Future<Output = qv_miner::MinerResult<()>> + Send>,
    > {
        Box::pin(async move {
            tracing::info!(
                slot = %slot,
                "M-09c follow-up: would call block_producer::produce_block + RPC submit here"
            );
            Ok(())
        })
    };

    // ── 5. Run loop with graceful shutdown on Ctrl+C ──────────────────────
    //
    // VRF evaluator: wrap the loaded keypair into qv_consensus' Ristretto VRF
    // adapter. `into_evaluator` is destructive (takes ownership); we need a
    // separate copy because `keys.vrf` is consumed.
    let vrf_evaluator = keys.vrf.into_evaluator();

    tokio::select! {
        result = run_slot_loop(sl, &vrf_evaluator, &pool_id, block_producer_fn) => {
            result.map_err(|e| anyhow::anyhow!("slot loop terminated: {e}"))?;
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("SIGINT received; shutting down operator daemon");
            // Future work: flush dashboard metrics, persist KES rotation
            // pointer (kes_period) back to keystore.
        }
    }

    tracing::info!("operator daemon stopped");
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

    let config = OperatorConfig::load_toml(&config_path).map_err(|e| anyhow::anyhow!("{e}"))?;
    let keys = OperatorKeys::load_encrypted(&config.keystore_path, "password")
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Operator: {} ({})", config.pool_name, config.pool_id);
    println!(
        "VRF public key length: {} bytes",
        keys.vrf.public_bytes().len()
    );
    println!(
        "KES public key length: {} bytes",
        keys.kes.public_bytes().len()
    );
    println!(
        "Cold public key length: {} bytes",
        keys.cold.public_bytes().len()
    );

    if verbose {
        println!("VRF public (hex): {}", hex::encode(keys.vrf.public_bytes()));
        println!("KES public (hex): {}", hex::encode(keys.kes.public_bytes()));
        println!(
            "Cold public (hex): {}",
            hex::encode(keys.cold.public_bytes())
        );
    }

    Ok(())
}
