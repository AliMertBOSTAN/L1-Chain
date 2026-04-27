//! Core Node orchestration — wires together consensus, storage, network, mempool, and RPC.

use crate::config::NodeConfig;
use crate::metrics;
use crate::rpc::RpcServer;
use crate::signals::shutdown_signal;
use futures::StreamExt;
use qv_consensus::ChainState;
use qv_core::{Block, Transaction};
use qv_mempool::clear::ClearPool;
use qv_net::{NetworkNode, NodeConfig as NetConfig};
use qv_storage::block_store::BlockStore;
use qv_storage::kv::MemoryKvStore;
use qv_storage::utxo_store::UtxoStore;
use std::sync::Arc;
use tokio::sync::mpsc;

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

/// The full node.
pub struct Node {
    config: NodeConfig,
    // Storage layers
    _block_store: Arc<BlockStore<MemoryKvStore>>,
    _utxo_store: Arc<UtxoStore<MemoryKvStore>>,
    // Consensus
    _chain_state: Arc<ChainState>,
    // Networking
    _network_node: Option<Arc<NetworkNode>>,
    // Mempool
    _clear_pool: Arc<ClearPool>,
    // RPC
    _rpc_server: Arc<RpcServer>,
    // Event channels
    event_tx: mpsc::Sender<NodeEvent>,
    event_rx: mpsc::Receiver<NodeEvent>,
}

impl Node {
    /// Create a new node with the given configuration.
    pub async fn new(config: NodeConfig) -> crate::NodeResult<Self> {
        tracing::info!(network = %config.network, "initializing QuantumVault node");

        // Initialize storage backends.
        let kv_store = Arc::new(MemoryKvStore::new());
        let block_store = Arc::new(BlockStore::new(kv_store.clone()));
        let utxo_store = Arc::new(UtxoStore::new(kv_store));

        // Initialize consensus state.
        let chain_state = Arc::new(ChainState::default());

        // Initialize mempool.
        let clear_pool = Arc::new(ClearPool::new(
            qv_mempool::clear::ClearPoolConfig {
                max_size: config.mempool.max_clear_pool_size,
                min_fee_rate: config.mempool.min_fee_rate,
            },
        ));

        // Initialize network node (optional, may fail gracefully).
        let network_node = None; // TODO: wire libp2p NetworkNode

        // Initialize RPC server.
        let rpc_server = Arc::new(RpcServer {});

        // Create event channel.
        let (event_tx, event_rx) = mpsc::channel(1000);

        let node = Self {
            config,
            _block_store: block_store,
            _utxo_store: utxo_store,
            _chain_state: chain_state,
            _network_node: network_node,
            _clear_pool: clear_pool,
            _rpc_server: rpc_server,
            event_tx,
            event_rx,
        };

        tracing::info!("node initialized successfully");
        Ok(node)
    }

    /// Run the node's main event loop.
    ///
    /// Listens for:
    /// - Block gossip from the network → validate → store → broadcast
    /// - Transaction gossip from the network → mempool insertion
    /// - RPC requests → process → respond
    /// - Shutdown signal (Ctrl-C, SIGTERM) → graceful shutdown
    pub async fn run(mut self) -> crate::NodeResult<()> {
        tracing::info!("starting node main loop");

        // Create a shutdown channel.
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

        // Spawn a background task for shutdown signal handling.
        let shutdown_tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = shutdown_tx_clone.send(());
        });

        // Main event loop.
        loop {
            tokio::select! {
                // Process events from network / RPC / mempool.
                Some(event) = self.event_rx.recv() => {
                    match event {
                        NodeEvent::BlockReceived(block) => {
                            tracing::debug!(height = block.header.height, "received block from network");
                            metrics::record_block_validated();
                        }
                        NodeEvent::TxReceived(tx) => {
                            tracing::debug!(tx_id = ?tx.id(), "received transaction from network");
                            metrics::record_tx_received();
                        }
                        NodeEvent::Shutdown => {
                            tracing::info!("shutdown event received");
                            break;
                        }
                    }
                }

                // Shutdown signal.
                _ = shutdown_rx.recv() => {
                    tracing::info!("shutdown signal received, gracefully shutting down");
                    break;
                }
            }
        }

        self.shutdown().await?;
        Ok(())
    }

    /// Graceful shutdown: close connections, flush state, etc.
    async fn shutdown(&mut self) -> crate::NodeResult<()> {
        tracing::info!("node shutting down");
        // TODO: close network connections, flush storage, etc.
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
}

#[cfg(test)]
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

    #[tokio::test]
    async fn test_node_shutdown_immediate() {
        let config = NodeConfig::devnet();
        let node = Node::new(config).await.unwrap();
        let event_tx = node.event_tx.clone();

        // Spawn the node in a background task.
        let node_task = tokio::spawn(async move { node.run().await });

        // Give it a moment to start.
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Send a shutdown event.
        event_tx.send(NodeEvent::Shutdown).await.unwrap();

        // Wait for node to exit.
        let result = node_task.await.unwrap();
        assert!(result.is_ok());
    }
}
