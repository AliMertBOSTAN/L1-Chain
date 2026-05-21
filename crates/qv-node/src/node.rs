//! Core Node orchestration — wires together consensus, storage, network, mempool, and RPC.

use crate::config::NodeConfig;
use crate::metrics;
use crate::network_handler::NetworkHandler;
use crate::rpc::{QvNodeApiServer, RpcServer};
use crate::signals::shutdown_signal;
use crate::slot_ticker::SlotTicker;
use jsonrpsee::server::Server;
use qv_consensus::{
    epoch::EpochNonce, leader_schedule::TestVrf, slot::SlotClock, stake::StakeDistribution,
    ChainEntry, ChainState,
};
use qv_core::{Amount, Block, Epoch, ProtocolParams, Transaction};
use qv_crypto::{generate_pqc_keypair, DilithiumLevel};
use qv_mempool::clear::{ClearPool, ClearPoolConfig};
use qv_net::{Multiaddr, NetworkNode, NodeIdentity};
use qv_storage::block_store::BlockStore;
use qv_storage::kv::MemoryKvStore;
use qv_storage::utxo_store::UtxoStore;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Event types flowing through the node's main loop.
#[derive(Debug, Clone)]
pub enum NodeEvent {
    /// Block received from network.
    BlockReceived(Block),

    /// Transaction received from network.
    TxReceived(Transaction),

    /// Signal to shut down.
    Shutdown,
}

/// Parse a 32-byte seed from a hex string, with a descriptive error.
fn parse_seed32(hex_str: &str, field: &str) -> crate::NodeResult<[u8; 32]> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| crate::NodeError::Config(format!("invalid {field}: {e}")))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| crate::NodeError::Config(format!("{field} must be exactly 32 bytes")))
}

/// The full node.
pub struct Node {
    config: NodeConfig,
    protocol_params: ProtocolParams,
    // Storage layers
    block_store: Arc<BlockStore<MemoryKvStore>>,
    utxo_store: Arc<UtxoStore<MemoryKvStore>>,
    // Consensus
    chain_state: Arc<tokio::sync::Mutex<ChainState>>,
    /// Currently-active stake distribution (the per-epoch snapshot used
    /// for VRF leader election). Shared with the slot ticker and exposed
    /// over RPC via `qv_getStakeDistribution`. Read-heavy; mutated only
    /// at startup and at epoch boundaries (future work).
    stake_distribution: Arc<tokio::sync::RwLock<StakeDistribution>>,
    /// Currently-active epoch nonce. Shared with the slot ticker and
    /// exposed over RPC via `qv_getEpochNonce`. Evolves at every epoch
    /// boundary as described in `qv_consensus::epoch`.
    epoch_nonce: Arc<tokio::sync::RwLock<EpochNonce>>,
    // Networking — the NetworkNode is consumed by a background task in `run()`.
    network_node: Option<NetworkNode>,
    // Gossip command channel (send messages to be published by the network task).
    gossip_tx: Option<mpsc::UnboundedSender<qv_net::NetworkMessage>>,
    // Mempool
    clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
    // Event channels
    event_tx: mpsc::Sender<NodeEvent>,
    event_rx: mpsc::Receiver<NodeEvent>,
}

impl Node {
    /// Read-only access to the node's resolved configuration. Useful for
    /// integration tests and observability tooling that wants to inspect
    /// network/listen-addr/etc. without poking the private fields directly.
    #[must_use]
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Clone the event-sender side of the node's channel. Lets external
    /// driver code (e.g. integration tests, RPC bridges) push `NodeEvent`s
    /// into the running node without reaching into private state.
    #[must_use]
    pub fn event_sender(&self) -> mpsc::Sender<NodeEvent> {
        self.event_tx.clone()
    }

    /// Create a new node with the given configuration.
    pub async fn new(config: NodeConfig) -> crate::NodeResult<Self> {
        tracing::info!(network = %config.network, "initializing QuantumVault node");

        // Initialize storage backends.
        // `BlockStore::new` takes a value of `S: KvStore`, not an `Arc<S>` —
        // `Arc<MemoryKvStore>` does not auto-implement `KvStore`. We share
        // the underlying store between block & utxo stores via `Clone`
        // (`MemoryKvStore` is internally `Arc<RwLock<...>>`-shaped, so
        // cloning yields a shared handle).
        let kv_store = MemoryKvStore::new();
        let block_store = Arc::new(BlockStore::new(kv_store.clone()));
        let utxo_store = Arc::new(UtxoStore::new(kv_store));

        // Load protocol parameters from config file or use built-in preset.
        let proto_params = Self::load_protocol_params(&config)?;

        // Apply genesis block if storage is empty (first start).
        Self::maybe_apply_genesis(&config, &utxo_store, &block_store)?;

        // Initialize consensus state from the protocol params for this network.
        // `ChainState` has no `Default` impl — bootstrap from genesis.
        let chain_state = Arc::new(tokio::sync::Mutex::new(ChainState::genesis(
            &proto_params.consensus,
        )));

        // Initialize shared epoch-frozen state. The distribution starts
        // empty for the genesis epoch; `spawn_slot_ticker` will populate
        // it from the local stake-pool config when present, and pool
        // registration TXs (M-07/M-08) will extend it once on-chain
        // registrations are processed at epoch boundaries.
        let stake_distribution = Arc::new(tokio::sync::RwLock::new(
            StakeDistribution::new(
                Epoch::GENESIS,
                Vec::<(qv_consensus::stake::PoolId, Amount)>::new(),
            )
            .map_err(|e| {
                let ce: qv_consensus::ConsensusError = e.into();
                crate::NodeError::Consensus(ce)
            })?,
        ));
        let epoch_nonce = Arc::new(tokio::sync::RwLock::new(EpochNonce::GENESIS));

        // Initialize mempool. `ClearPoolConfig` exposes plain fields
        // (`max_tx_count`, `max_pool_bytes`, `min_fee`, `max_age_secs`);
        // use the testnet preset for now, override below if config provides.
        let clear_pool = Arc::new(tokio::sync::Mutex::new(ClearPool::new(
            ClearPoolConfig::testnet(),
        )));
        // Suppress dead_code on the per-network mempool config knobs until
        // we expose them through ClearPoolConfig:
        let _mempool_cfg_hint = (
            config.mempool.max_clear_pool_size,
            config.mempool.min_fee_rate,
        );

        // Initialize network node (optional, may fail gracefully).
        // We extract the gossip command sender; the NetworkNode itself is
        // consumed by a background task in `run()`.
        let (gossip_tx, network_node) = match Self::init_network_node(&config).await {
            Ok(node) => {
                let tx = node.command_sender();
                (Some(tx), Some(node))
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to initialize network node");
                (None, None)
            }
        };

        // Create event channel.
        let (event_tx, event_rx) = mpsc::channel(1000);

        let node = Self {
            config,
            protocol_params: proto_params,
            block_store,
            utxo_store,
            chain_state,
            stake_distribution,
            epoch_nonce,
            network_node,
            gossip_tx,
            clear_pool,
            event_tx,
            event_rx,
        };

        tracing::info!("node initialized successfully");
        Ok(node)
    }

    /// Load protocol parameters from config/{network}.toml or fall back to built-in preset.
    fn load_protocol_params(config: &NodeConfig) -> crate::NodeResult<ProtocolParams> {
        let config_path = PathBuf::from(format!("config/{}.toml", config.network));
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).map_err(|e| {
                crate::NodeError::Config(format!("failed to read {}: {e}", config_path.display()))
            })?;
            ProtocolParams::from_toml(&content).map_err(|e| {
                crate::NodeError::Config(format!(
                    "invalid protocol params in {}: {e}",
                    config_path.display()
                ))
            })
        } else {
            info!(
                network = %config.network,
                "no config TOML found, using built-in preset"
            );
            match config.network.as_str() {
                "mainnet" => Ok(ProtocolParams::mainnet()),
                "testnet" => Ok(ProtocolParams::testnet()),
                // "devnet" or any unrecognized network → ephemeral params.
                _ => Ok(ProtocolParams::ephemeral()),
            }
        }
    }

    /// If the UTXO store is empty, this is a fresh node — apply genesis block.
    fn maybe_apply_genesis(
        config: &NodeConfig,
        utxo_store: &UtxoStore<MemoryKvStore>,
        block_store: &BlockStore<MemoryKvStore>,
    ) -> crate::NodeResult<()> {
        let count = utxo_store.len()?;

        if count > 0 {
            info!(
                utxo_count = count,
                "existing chain state found, skipping genesis"
            );
            return Ok(());
        }

        info!(
            network = %config.network,
            "empty store detected — applying genesis block"
        );

        // Use devnet_genesis() for devnet/ephemeral, otherwise build a minimal genesis.
        let genesis_block = match config.network.as_str() {
            "devnet" | "ephemeral" => {
                let (block, keys) = crate::genesis::devnet_genesis();
                info!(
                    accounts = keys.len(),
                    "devnet genesis generated (keys available via --init)"
                );
                block
            }
            _ => {
                // For mainnet/testnet, create a minimal genesis with a single foundation output.
                // Real genesis ceremony would use threshold Kyber DKG — placeholder for now.
                let kp = generate_pqc_keypair(DilithiumLevel::Level3)
                    .map_err(|e| crate::NodeError::Other(format!("keygen failed: {e}")))?;
                crate::genesis::build_genesis_block(&[(kp.public, 2_100_000_000_000_000)])
            }
        };

        // Apply genesis to both stores.
        utxo_store.apply_block(&genesis_block)?;
        block_store.put_block(&genesis_block)?;

        info!(
            tx_count = genesis_block.transactions.len(),
            "genesis block applied successfully"
        );

        Ok(())
    }

    /// Initialize the network node from configuration.
    async fn init_network_node(config: &NodeConfig) -> Result<NetworkNode, crate::NodeError> {
        // Use a deterministic identity when the config supplies a seed, so
        // the node's PeerId is stable across restarts and the devnet
        // launcher / monitor can label each node by a known PeerId.
        let identity = match &config.node_key_seed_hex {
            Some(hex_str) => {
                let seed = parse_seed32(hex_str, "node_key_seed_hex")?;
                NodeIdentity::from_seed(seed)?
            }
            None => NodeIdentity::generate(),
        };
        let listen_addr: Multiaddr = config
            .listen_addr
            .parse()
            .map_err(|e| crate::NodeError::Other(format!("invalid listen_addr: {e}")))?;

        let mut net_config = qv_net::NodeConfig::ephemeral();
        if !config.bootstrap_peers.is_empty() {
            net_config.bootstrap_peers = config.bootstrap_peers.clone();
        }

        let mut network_node = NetworkNode::new(net_config, identity)?;
        network_node.listen(listen_addr)?;
        network_node.subscribe_all()?;

        info!(
            peer_id = %network_node.local_peer_id(),
            listen = %config.listen_addr,
            "network node initialized"
        );
        Ok(network_node)
    }

    /// Bootstrap seed nodes by dialing each configured seed node multiaddress.
    ///
    /// Parses each seed node string into a `Multiaddr` and attempts to dial it.
    /// Invalid multiaddrs are logged as warnings and skipped (no panic).
    fn bootstrap_seed_nodes(seed_nodes: &[String], net_node: &mut NetworkNode) {
        if seed_nodes.is_empty() {
            return;
        }

        info!(count = seed_nodes.len(), "bootstrapping seed nodes");

        for seed_str in seed_nodes {
            match seed_str.parse::<Multiaddr>() {
                Ok(addr) => match net_node.dial(addr.clone()) {
                    Ok(()) => {
                        info!(addr = %addr, "dialing seed node");
                    }
                    Err(e) => {
                        warn!(addr = %addr, error = %e, "failed to dial seed node");
                    }
                },
                Err(e) => {
                    warn!(seed_addr = %seed_str, error = %e, "invalid seed node multiaddr");
                }
            }
        }
    }

    /// Run the node's main event loop.
    ///
    /// Listens for:
    /// - Block gossip from the network → validate → store → broadcast
    /// - Transaction gossip from the network → mempool insertion
    /// - RPC requests → process → respond
    /// - Shutdown signal (Ctrl-C, SIGTERM) → graceful shutdown
    pub async fn run(mut self) -> crate::NodeResult<()> {
        info!("starting node main loop");

        // Bootstrap seed nodes before spawning the main network loop.
        // This must be done on the NetworkNode before it is spawned.
        if let Some(ref mut net_node) = self.network_node {
            Self::bootstrap_seed_nodes(&self.config.seed_nodes, net_node);
        }

        // Spawn network node event loop (if available).
        // Take ownership of the NetworkNode and move it into a background task.
        let net_event_rx = if let Some(mut net_node) = self.network_node.take() {
            // Extract the event receiver before spawning (it's consumed once).
            let rx = net_node.take_event_receiver();

            // Spawn the network event loop — takes ownership of NetworkNode.
            tokio::spawn(async move {
                net_node.run().await;
            });

            rx
        } else {
            None
        };

        // Spawn network handler if we have a network node event receiver.
        if let Some(net_rx) = net_event_rx {
            let network_handler = NetworkHandler::new(net_rx, self.event_tx.clone());
            tokio::spawn(async move {
                network_handler.run().await;
            });
            info!("network handler spawned");
        }

        // Spawn slot ticker if stake pool config is present.
        if let Some(ref pool_cfg) = self.config.stake_pool {
            match self.spawn_slot_ticker(pool_cfg).await {
                Ok(_) => info!("slot ticker spawned"),
                Err(e) => warn!(error = %e, "failed to spawn slot ticker"),
            }
        }

        // Bind JSON-RPC server (jsonrpsee 0.24).
        //
        // The trait `QvNodeApiServer` is generated by the `#[rpc(server)]`
        // macro on `QvNodeApi`; calling `into_rpc()` on the implementing
        // struct yields an `RpcModule` we can hand to the server.
        let rpc_addr = self.config.rpc_addr;
        let encrypted_pool = Arc::new(tokio::sync::Mutex::new(
            qv_mempool::encrypted::EncryptedPool::new(
                qv_mempool::encrypted::EncryptedPoolConfig::default(),
                Epoch::GENESIS,
            ),
        ));
        let slot_clock_for_rpc = SlotClock::new(
            &self.protocol_params.consensus,
            self.protocol_params.genesis_time,
        );
        let rpc_server_impl = RpcServer::new(
            Arc::clone(&self.block_store),
            Arc::clone(&self.utxo_store),
            Arc::clone(&self.chain_state),
            Arc::clone(&self.clear_pool),
            Arc::clone(&encrypted_pool),
            Arc::clone(&self.stake_distribution),
            Arc::clone(&self.epoch_nonce),
            slot_clock_for_rpc,
            self.event_tx.clone(),
        );
        let rpc_module = rpc_server_impl.into_rpc();

        let server = Server::builder()
            .build(rpc_addr)
            .await
            .map_err(|e| crate::NodeError::Rpc(format!("bind {rpc_addr}: {e}")))?;

        let local_addr = server
            .local_addr()
            .map_err(|e| crate::NodeError::Rpc(format!("local_addr: {e}")))?;

        let rpc_handle = server.start(rpc_module);
        info!(addr = %local_addr, "JSON-RPC server listening");

        // Create a shutdown channel.
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

        // Spawn a background task for shutdown signal handling.
        let shutdown_tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = shutdown_tx_clone.send(()).await;
        });

        // Main event loop.
        loop {
            tokio::select! {
                // Process events from network / RPC / mempool.
                Some(event) = self.event_rx.recv() => {
                    match event {
                        NodeEvent::BlockReceived(block) => {
                            debug!(height = ?block.header.height, "received block from network");
                            if let Err(e) = self.handle_block(&block).await {
                                warn!(error = %e, "failed to process block");
                            }

                            // Gossip the block to peers via command channel.
                            if let Some(ref tx) = self.gossip_tx {
                                let msg = qv_net::NetworkMessage::Block(Box::new(block.clone()));
                                if tx.send(msg).is_err() {
                                    warn!("gossip channel closed, cannot relay block");
                                }
                            }
                        }
                        NodeEvent::TxReceived(tx) => {
                            debug!(tx_id = ?tx.id(), "received transaction from network");

                            // Validate and insert into mempool.
                            let current_slot = {
                                let chain = self.chain_state.lock().await;
                                chain.tip().slot
                            };

                            match crate::validation::validate_transaction(
                                &tx,
                                &self.utxo_store,
                                current_slot,
                                self.config.mempool.min_fee_rate,
                            ) {
                                Ok(validated) => {
                                    let tx_id = validated.tx_id;
                                    let mut pool = self.clear_pool.lock().await;
                                    match crate::validation::insert_validated_tx(
                                        &mut pool,
                                        tx.clone(),
                                        validated,
                                    ) {
                                        Ok(_) => {
                                            info!(tx_id = %tx_id, "transaction accepted into mempool");
                                            drop(pool);

                                            // Relay to network via command channel.
                                            if let Some(ref gossip) = self.gossip_tx {
                                                let msg = qv_net::NetworkMessage::Transaction(
                                                    Box::new(tx.clone()),
                                                );
                                                if gossip.send(msg).is_err() {
                                                    warn!("gossip channel closed, cannot relay tx");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!(tx_id = %tx_id, error = %e, "mempool insertion failed");
                                        }
                                    }
                                }
                                Err(e) => {
                                    debug!(error = %e, "transaction validation failed");
                                }
                            }

                            metrics::record_tx_received();
                        }
                        NodeEvent::Shutdown => {
                            info!("shutdown event received");
                            break;
                        }
                    }
                }

                // Shutdown signal.
                _ = shutdown_rx.recv() => {
                    info!("shutdown signal received, gracefully shutting down");
                    break;
                }
            }
        }

        // Tear down RPC server before flushing other state.
        info!("stopping RPC server");
        if rpc_handle.stop().is_err() {
            warn!("RPC server was already stopped");
        }
        rpc_handle.stopped().await;
        info!("RPC server stopped");

        self.shutdown().await?;
        Ok(())
    }

    /// Graceful shutdown: close network channels, capture final state,
    /// and log a clean exit.
    ///
    /// Storage backends (RocksDB, redb) flush implicitly on `Drop`; the
    /// `Arc<...>` clones owned by `Node` are released when this function
    /// returns. A future `KvStore::flush()` trait method would let us call
    /// flush explicitly here for synchronous durability guarantees — tracked
    /// under the storage roadmap.
    ///
    /// On entry we:
    /// 1. Drop the gossip command channel so the network event loop
    ///    completes its `select!` and returns naturally.
    /// 2. Snapshot the chain tip + mempool sizes for the final shutdown log.
    /// 3. Emit one structured INFO line with the snapshot.
    async fn shutdown(&mut self) -> crate::NodeResult<()> {
        tracing::info!("node shutting down");

        // 1. Close the gossip command channel by dropping the sender. The
        //    network event loop is `select!`'ed on this channel and will
        //    exit its loop once it observes the `None` receive.
        if self.gossip_tx.take().is_some() {
            tracing::debug!("gossip command channel closed");
        }

        // 2. Capture the final tip + clear-pool snapshot for forensics.
        //    (Encrypted pool is owned by the slot-ticker scope; not visible
        //    here. Future improvement: hoist `encrypted_pool` into `Node`
        //    fields so we can include it in the shutdown log.)
        //
        //    `tip()` returns `&ChainEntry` borrowed from the lock guard;
        //    use the owned `tip_height()` / `tip_hash()` accessors instead
        //    so we can drop the guard immediately.
        let (tip_height, tip_hash) = {
            let chain_state = self.chain_state.lock().await;
            (chain_state.tip_height(), chain_state.tip_hash())
        };
        let clear_size = {
            let pool = self.clear_pool.lock().await;
            pool.len()
        };

        // 3. Log the final state.
        tracing::info!(
            tip_height = tip_height.as_u64(),
            tip_hash = %tip_hash.to_hex(),
            clear_mempool = clear_size,
            "node shutdown complete"
        );

        Ok(())
    }

    /// Send an event to the node's event loop (used by network/RPC handlers).
    pub async fn send_event(&self, event: NodeEvent) -> crate::NodeResult<()> {
        self.event_tx
            .send(event)
            .await
            .map_err(|e| crate::NodeError::Other(format!("failed to send event: {e}")))?;
        Ok(())
    }

    /// Spawn the slot ticker background task if stake pool config is provided.
    async fn spawn_slot_ticker(
        &self,
        pool_cfg: &crate::config::StakePoolConfig,
    ) -> crate::NodeResult<()> {
        // This node's own VRF seed and the pool id derived from it.
        let vrf_seed = parse_seed32(&pool_cfg.vrf_seed_hex, "vrf_seed_hex")?;
        let vrf = TestVrf::new(vrf_seed);
        let my_pool_id = qv_consensus::stake::PoolId::from_vrf_key(&vrf_seed);

        let slot_clock = SlotClock::new(
            &self.protocol_params.consensus,
            self.protocol_params.genesis_time,
        );

        // Build the stake distribution. When the config carries a shared
        // genesis-pool set (multi-node devnet) every pool is included, so
        // all nodes agree on the stake split and — with round-robin — on
        // the leader schedule. Otherwise fall back to the single,
        // locally-configured pool (single-node behaviour).
        let (stake_dist, round_robin) = if self.config.genesis_pools.is_empty() {
            let initial_stake = Amount::from_smallest_units(pool_cfg.initial_stake);
            let dist = StakeDistribution::new(Epoch::GENESIS, vec![(my_pool_id, initial_stake)])
                .map_err(|e| {
                    let ce: qv_consensus::ConsensusError = e.into();
                    crate::NodeError::Consensus(ce)
                })?;
            (dist, None)
        } else {
            let mut pools = Vec::with_capacity(self.config.genesis_pools.len());
            let mut my_index: Option<u64> = None;
            for (i, gp) in self.config.genesis_pools.iter().enumerate() {
                let seed = parse_seed32(&gp.vrf_seed_hex, "genesis_pools.vrf_seed_hex")?;
                let pid = qv_consensus::stake::PoolId::from_vrf_key(&seed);
                pools.push((pid, Amount::from_smallest_units(gp.stake)));
                if seed == vrf_seed {
                    my_index = Some(i as u64);
                }
            }
            let dist = StakeDistribution::new(Epoch::GENESIS, pools).map_err(|e| {
                let ce: qv_consensus::ConsensusError = e.into();
                crate::NodeError::Consensus(ce)
            })?;
            let rr = if self.config.round_robin_leader {
                let idx = my_index.ok_or_else(|| {
                    crate::NodeError::Config(
                        "this node's vrf_seed_hex is not present in genesis_pools".to_string(),
                    )
                })?;
                Some((idx, self.config.genesis_pools.len() as u64))
            } else {
                None
            };
            (dist, rr)
        };

        // Publish the distribution + nonce to the shared RwLocks so RPC
        // callers (`qv_getStakeDistribution`, `qv_getEpochNonce`) and the
        // slot ticker all observe identical data.
        {
            let mut guard = self.stake_distribution.write().await;
            *guard = stake_dist.clone();
        }
        {
            let mut guard = self.epoch_nonce.write().await;
            *guard = EpochNonce::GENESIS;
        }

        // Build the slot ticker. Block production is routed through the
        // node's event channel (`event_tx`) so produced blocks are
        // validated, stored, applied, and gossiped on exactly the same
        // path as peer-received blocks.
        let mut ticker = SlotTicker::new(
            slot_clock,
            my_pool_id,
            Arc::new(stake_dist),
            EpochNonce::GENESIS,
            vrf,
            Arc::clone(&self.block_store),
            Arc::clone(&self.utxo_store),
            Arc::clone(&self.chain_state),
            Arc::clone(&self.clear_pool),
            pool_cfg.active_slot_coeff,
            self.event_tx.clone(),
        );
        if let Some((idx, total)) = round_robin {
            info!(
                pool_index = idx,
                total_pools = total,
                "slot leader schedule: deterministic round-robin (devnet)"
            );
            ticker = ticker.with_round_robin(idx, total);
        }
        if self.config.startup_warmup_secs > 0 {
            ticker = ticker.with_warmup(self.config.startup_warmup_secs);
        }

        // Spawn the ticker on a background task.
        tokio::spawn(async move {
            if let Err(e) = ticker.run().await {
                warn!(error = %e, "slot ticker encountered error");
            }
        });

        Ok(())
    }

    /// Handle a received block: validate structure, verify chain linkage, apply to storage,
    /// update consensus state, and remove confirmed transactions from mempool.
    async fn handle_block(&self, block: &Block) -> crate::NodeResult<()> {
        // Step 1: Structural validation (merkle root, duplicate txids, tx validation).
        block
            .validate_structure()
            .map_err(|e| crate::NodeError::BlockValidation(e.to_string()))?;

        let block_hash = block
            .hash()
            .map_err(|e| crate::NodeError::BlockValidation(e.to_string()))?;

        // Step 2: Verify chain linkage and slot monotonicity.
        self.validate_chain_linkage(block).await?;

        // Step 3: Apply block effects to UTXO set.
        self.utxo_store
            .apply_block(block)
            .map_err(crate::NodeError::Storage)?;

        // Step 4: Persist the block to storage.
        self.block_store
            .put_block(block)
            .map_err(crate::NodeError::Storage)?;

        // Step 5: Update consensus chain state with new entry.
        let chain_entry = ChainEntry {
            hash: block_hash,
            parent_hash: block.header.prev_hash,
            height: block.header.height,
            slot: block.header.slot,
            producer_key_hash: block.header.producer_key_hash,
        };

        {
            let mut chain_state = self.chain_state.lock().await;
            chain_state
                .add_block(chain_entry)
                .map_err(|e| crate::NodeError::Consensus(e.into()))?;
        }

        // Step 6: Remove confirmed transactions from mempool.
        let spent_outpoints: BTreeSet<_> = block
            .transactions
            .iter()
            .flat_map(|tx| tx.inputs.iter().map(|inp| inp.prev_output))
            .collect();

        if !spent_outpoints.is_empty() {
            let mut pool = self.clear_pool.lock().await;
            let removed = pool.remove_confirmed(&spent_outpoints);
            tracing::debug!(
                count = removed.len(),
                "removed confirmed transactions from mempool"
            );
        }

        // Step 7: Log and emit metrics.
        tracing::info!(
            hash = %block_hash.to_hex(),
            height = ?block.header.height,
            tx_count = block.transactions.len(),
            "block accepted and stored"
        );
        metrics::record_block_validated();

        Ok(())
    }

    /// Validate that a block is properly linked to the chain:
    /// - prev_hash matches the current tip (or genesis if no tip)
    /// - slot is strictly greater than tip's slot
    /// - height is exactly tip's height + 1
    async fn validate_chain_linkage(&self, block: &Block) -> crate::NodeResult<()> {
        let chain_state = self.chain_state.lock().await;
        let tip = chain_state.tip();
        let prev_expected = tip.hash;
        let prev_actual = block.header.prev_hash;

        if prev_actual != prev_expected {
            return Err(crate::NodeError::BlockValidation(format!(
                "prev_hash mismatch: expected {}, got {}",
                prev_expected.to_hex(),
                prev_actual.to_hex()
            )));
        }

        // Slot must be strictly monotone increasing.
        if block.header.slot <= tip.slot {
            return Err(crate::NodeError::BlockValidation(format!(
                "slot not strictly increasing: tip slot {}, block slot {}",
                tip.slot.as_u64(),
                block.header.slot.as_u64()
            )));
        }

        // Height must increment by exactly 1.
        let expected_height = tip.height.as_u64() + 1;
        let actual_height = block.header.height.as_u64();
        if actual_height != expected_height {
            return Err(crate::NodeError::BlockValidation(format!(
                "height mismatch: expected {}, got {}",
                expected_height, actual_height
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_creation() {
        let config = NodeConfig::devnet();
        let node = Node::new(config).await.unwrap();
        assert_eq!(node.config.network, "devnet");
    }

    #[tokio::test]
    async fn test_node_send_event() {
        let config = NodeConfig::devnet();
        let node = Node::new(config).await.unwrap();
        let event = NodeEvent::Shutdown;
        node.send_event(event).await.unwrap();
    }

    #[tokio::test]
    async fn test_node_config_testnet() {
        let config = NodeConfig::testnet();
        let node = Node::new(config).await.unwrap();
        assert_eq!(node.config.network, "testnet");
    }

    #[tokio::test]
    async fn test_node_config_mainnet() {
        let config = NodeConfig::mainnet();
        let node = Node::new(config).await.unwrap();
        assert_eq!(node.config.network, "mainnet");
    }

    /// `Node` is `!Send` because it owns a libp2p `Swarm` whose trait objects
    /// are not `Sync`. `tokio::spawn` requires `Send`, so we cannot exercise
    /// the full `node.run()` lifecycle from a unit test. Documented as
    /// envanter B-03 / future cleanup: refactor `Node` to keep `NetworkNode`
    /// behind `Arc<Mutex<Option<...>>>` and `take()` it inside the spawned
    /// task (rather than capturing the whole `Node` by move).
    ///
    /// Until that refactor we restrict this test to "construction does not
    /// panic" — a tiny smoke check.
    #[tokio::test]
    async fn test_node_construction_smoke() {
        let config = NodeConfig::devnet();
        let _node = Node::new(config).await.unwrap();
        // Cannot spawn `_node.run()` while `Node: !Send`; full shutdown
        // lifecycle is exercised end-to-end in `tests/transfer_e2e.rs`
        // which runs synchronously without `tokio::spawn`.
    }
}
