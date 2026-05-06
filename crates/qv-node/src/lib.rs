//! QuantumVault full node library — orchestrates consensus, storage, networking, and RPC.
//!
//! # Overview
//!
//! The `qv-node` crate provides a full node implementation that:
//! - Manages persistent storage (blocks, UTXO set, chain state) via `qv-storage`
//! - Participates in P2P consensus via `qv-net` and `qv-consensus`
//! - Validates transactions and blocks via `qv-core` and `qv-script`
//! - Maintains a transaction mempool via `qv-mempool`
//! - Exposes JSON-RPC methods for wallet and application queries
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                      Node (main loop)                │
//! │  – tokio mpsc: block_events, tx_events, rpc_reqs    │
//! │  – select! on ctrl_c + shutdown channel             │
//! │  – pipeline: gossip → validate → store → relay      │
//! └─────────────────────────────────────────────────────┘
//!           │              │              │
//!    ┌──────▼──┐   ┌──────▼──┐   ┌──────▼──┐
//!    │ Storage │   │  RPC    │   │ Network │
//!    │ (rocks) │   │ (json-  │   │ (libp2p)│
//!    │         │   │  rpsee) │   │ + gossip│
//!    └─────────┘   └─────────┘   └─────────┘
//!
//! - **Storage**: BlockStore + UtxoStore + StateStore (backed by RocksDB in production)
//! - **RPC**: jsonrpsee HTTP + WebSocket server with subscription support
//! - **Network**: libp2p swarm with GossipSub topics for blocks + transactions
//! ```
//!
//! # Error Handling
//!
//! Each layer exposes its own error enum; the `Node` aggregates them via `NodeError`.

#![forbid(unsafe_code)]

pub mod ceremony;
pub mod cli;
pub mod config;
pub mod genesis;
pub mod metrics;
pub mod network_handler;
pub mod node;
pub mod rpc;
pub mod signals;
pub mod slot_ticker;
pub mod validation;

use thiserror::Error;

/// Top-level error type for the node.
#[derive(Debug, Error)]
pub enum NodeError {
    /// Configuration error (parsing, validation, missing fields).
    #[error("config error: {0}")]
    Config(String),

    /// I/O error (file read/write, network).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Storage layer error.
    #[error("storage error: {0}")]
    Storage(#[from] qv_storage::StorageError),

    /// Networking layer error.
    #[error("network error: {0}")]
    Network(#[from] qv_net::NetError),

    /// Consensus layer error.
    #[error("consensus error: {0}")]
    Consensus(#[from] qv_consensus::ConsensusError),

    /// Block validation error.
    #[error("block validation error: {0}")]
    BlockValidation(String),

    /// Transaction validation error.
    #[error("transaction validation error: {0}")]
    TxValidation(String),

    /// Mempool error.
    #[error("mempool error: {0}")]
    Mempool(#[from] qv_mempool::MempoolError),

    /// RPC server error.
    #[error("RPC error: {0}")]
    Rpc(String),

    /// Generic runtime error.
    #[error("{0}")]
    Other(String),
}

/// Result alias for node operations.
pub type NodeResult<T> = Result<T, NodeError>;

// Re-export headline types at crate root for ergonomic imports.
pub use cli::CliArgs;
pub use config::NodeConfig;
pub use node::Node;
