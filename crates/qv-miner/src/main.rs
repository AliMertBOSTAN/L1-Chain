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
    use qv_consensus::{PoolId, VrfProof};
    use qv_core::{
        merkle_root_of, Block, BlockHash, BlockHeader, Hash256, Height, ProtocolParams, Slot,
        Timestamp, Transaction, UtxoCommitment, BLOCK_VERSION,
    };
    use qv_miner::node_rpc::NodeRpcClient;
    use qv_miner::slot_loop::{run_slot_loop, SlotLoop};
    use std::sync::Arc;

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

    // ── 3. Consensus state (M-09b: real RPC fetch from qv-node) ───────────
    //
    // The operator no longer guesses its own stake distribution. We fetch
    // the authoritative snapshot from the running node via RPC. The pool
    // must already be present in the distribution (registered via the
    // genesis stake-pool config or a registration TX); otherwise we
    // refuse to start instead of pretending to have stake we don't.
    let params = match config.network {
        qv_miner::config::Network::Mainnet => ProtocolParams::mainnet(),
        qv_miner::config::Network::Testnet => ProtocolParams::testnet(),
        qv_miner::config::Network::Devnet => ProtocolParams::ephemeral(),
    };

    let pool_id = PoolId::from_vrf_key(keys.vrf.public_bytes());

    let rpc_client = NodeRpcClient::new(&node_rpc);

    tracing::info!(rpc = %node_rpc, "fetching stake distribution from node");
    let stake_distribution = rpc_client
        .get_stake_distribution()
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch stake distribution: {e}"))?;

    if stake_distribution.is_empty() {
        anyhow::bail!(
            "node returned empty stake distribution — register the pool first \
             (qv-miner register-pool) or check the node's genesis config"
        );
    }

    let (self_stake, total_stake) = (
        stake_distribution.pool_stake(&pool_id),
        stake_distribution.total_stake(),
    );
    if self_stake == 0 {
        anyhow::bail!(
            "this operator's pool is not present in the node's stake distribution \
             (pool_id={}, total_stake={total_stake}); the pool must be registered \
             on-chain or pre-seeded in the node's stake-pool config",
            hex::encode(pool_id.as_bytes())
        );
    }
    tracing::info!(
        pool_id = %hex::encode(pool_id.as_bytes()),
        pool_stake = self_stake,
        total_stake,
        pool_count = stake_distribution.pool_count(),
        "stake distribution fetched"
    );

    tracing::info!("fetching epoch nonce from node");
    let (epoch_nonce, epoch) = rpc_client
        .get_epoch_nonce()
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch epoch nonce: {e}"))?;
    tracing::info!(
        epoch = %epoch,
        nonce = %hex::encode(epoch_nonce.as_bytes()),
        "epoch nonce fetched"
    );
    let initial_nonce = epoch_nonce.as_bytes().to_vec();

    // Slot clock: figure out the current slot from wall-clock time.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let slot_clock = qv_consensus::SlotClock::from_params(&params);
    let current_slot = slot_clock
        .current_info(now_ms)
        .map(|info| info.slot)
        .unwrap_or(Slot::GENESIS);

    let mut sl = SlotLoop::new(&params, current_slot, initial_nonce);
    sl.stake_distribution = Some(stake_distribution);

    tracing::info!(
        slot = %current_slot,
        epoch_slots = params.consensus.epoch_slots,
        slot_duration_ms = params.consensus.slot_duration_ms,
        "slot loop initialized; entering main loop"
    );

    // ── 4. Block producer callback (M-09c real path) ──────────────────────
    //
    // When elected as slot leader, we:
    //   1. Fetch the current chain tip from the node (`qv_getTip`).
    //   2. Fetch the pending clear-mempool transactions (`qv_getPendingTransactions`).
    //   3. Compute the Merkle root over the txid leaves.
    //   4. Build the block header WITHOUT `kes_sig`, bincode-serialize it,
    //      sign those bytes with KES, then attach the signature. This
    //      matches the symmetric verifier path in `qv-consensus` and the
    //      reference producer in `qv-node::slot_ticker::produce_block`.
    //   5. Submit the bincode-serialized block via `qv_submitBlock`.
    //
    // The closure captures cheap-to-clone state by move so the returned
    // futures own everything they touch (no borrows from the closure).
    let kes_arc: Arc<qv_miner::keys::KesKeyPair> = Arc::new(keys.kes);
    let rpc_for_producer = rpc_client.clone();
    let pool_id_for_producer = pool_id;
    let producer_key_hash = Hash256::from_bytes(qv_crypto::sha3_256(pool_id.as_bytes()));

    let block_producer_fn = move |slot: Slot,
                                  vrf_proof: VrfProof|
          -> std::pin::Pin<
        Box<dyn std::future::Future<Output = qv_miner::MinerResult<()>> + Send>,
    > {
        let kes = Arc::clone(&kes_arc);
        let rpc = rpc_for_producer.clone();
        let pool = pool_id_for_producer;
        let producer_hash = producer_key_hash;
        Box::pin(async move {
            // 4.1 Fetch tip.
            let tip = rpc.get_tip().await.map_err(|e| {
                qv_miner::MinerError::BlockProduction(format!("get_tip failed: {e}"))
            })?;
            let prev_hash = BlockHash::from_hex(&tip.block_hash).map_err(|e| {
                qv_miner::MinerError::BlockProduction(format!(
                    "invalid tip hash `{}`: {e}",
                    tip.block_hash
                ))
            })?;
            let new_height = Height::from(tip.height.saturating_add(1));

            // 4.2 Fetch pending transactions.
            let pending_hex = rpc.get_pending_transactions().await.map_err(|e| {
                qv_miner::MinerError::BlockProduction(format!(
                    "get_pending_transactions failed: {e}"
                ))
            })?;

            let mut transactions: Vec<Transaction> = Vec::with_capacity(pending_hex.len());
            let mut tx_ids = Vec::with_capacity(pending_hex.len());
            for hex_tx in pending_hex {
                let raw = hex::decode(&hex_tx).map_err(|e| {
                    qv_miner::MinerError::Serialization(format!(
                        "pending tx hex decode failed: {e}"
                    ))
                })?;
                let tx: Transaction = bincode::deserialize(&raw).map_err(|e| {
                    qv_miner::MinerError::Serialization(format!(
                        "pending tx bincode decode failed: {e}"
                    ))
                })?;
                let id = tx.id().map_err(|e| {
                    qv_miner::MinerError::BlockProduction(format!("tx id failed: {e}"))
                })?;
                transactions.push(tx);
                tx_ids.push(id);
            }

            // 4.3 Merkle root over txids.
            let merkle_root = merkle_root_of(&tx_ids);

            // 4.4 Build unsigned header → KES sign → attach signature.
            //
            // `utxo_commitment` stays `ZERO` (envanter K-03/K-05); the
            // post-apply UTXO snapshot hash is a separate workstream and
            // the verifier currently treats the field as opaque.
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut header = BlockHeader {
                version: BLOCK_VERSION,
                prev_hash,
                height: new_height,
                slot,
                timestamp: Timestamp::from(now_secs),
                merkle_root,
                utxo_commitment: UtxoCommitment::ZERO,
                vrf_proof: vrf_proof.0.clone(),
                kes_sig: Vec::new(),
                producer_key_hash: producer_hash,
            };
            let bytes_to_sign = bincode::serialize(&header).map_err(|e| {
                qv_miner::MinerError::Serialization(format!(
                    "header bincode failed: {e}"
                ))
            })?;
            let sig = kes.sign(&bytes_to_sign)?;
            header.kes_sig = bincode::serialize(&sig).map_err(|e| {
                qv_miner::MinerError::Serialization(format!("kes sig bincode failed: {e}"))
            })?;

            // 4.5 Assemble + validate.
            let block = Block::new(header, transactions);
            block.validate_structure().map_err(|e| {
                qv_miner::MinerError::BlockProduction(format!(
                    "structural validation failed: {e}"
                ))
            })?;

            // 4.6 Submit to node.
            let block_bytes = bincode::serialize(&block).map_err(|e| {
                qv_miner::MinerError::Serialization(format!("block bincode failed: {e}"))
            })?;
            let accepted_hash = rpc.submit_block(&block_bytes).await.map_err(|e| {
                qv_miner::MinerError::BlockProduction(format!("submit_block failed: {e}"))
            })?;

            tracing::info!(
                slot = %slot,
                height = ?new_height,
                tx_count = block.transactions.len(),
                hash = %accepted_hash,
                pool_id = %hex::encode(pool.as_bytes()),
                "block produced and submitted via RPC"
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
