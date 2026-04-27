//! Persistent storage (blocks, UTXO set, chain state) via pluggable KV backends.
//!
//! This crate is the persistence layer for QuantumVault's Rust pivot:
//! - `block_store`: block body + indexes (hash, height, header)
//! - `utxo_store`: persistent UTXO map with apply/revert and snapshots
//! - `state_store`: chain metadata, ledger state, epoch snapshots
//! - `kv`: backend abstraction (`MemoryKvStore`, `RocksKvStore`)

#![forbid(unsafe_code)]

pub mod block_store;
pub mod kv;
pub mod state_store;
pub mod utxo_store;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Error type for storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Backend-specific error (RocksDB, lock poisoning, etc.).
    #[error("backend error: {0}")]
    Backend(String),

    /// Encoding failure while serializing persisted values.
    #[error("encoding error: {0}")]
    Encode(String),

    /// Decoding failure while deserializing persisted values.
    #[error("decoding error: {0}")]
    Decode(String),

    /// Data exists but is malformed or violates expected layout.
    #[error("corrupted data: {0}")]
    Corrupted(&'static str),

    /// Requested entity is absent.
    #[error("not found: {0}")]
    NotFound(&'static str),

    /// Attempted to create an entity that already exists.
    #[error("already exists: {0}")]
    AlreadyExists(&'static str),

    /// Invalid method input.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Error bubbled from `qv-core` block routines.
    #[error(transparent)]
    Block(#[from] qv_core::BlockError),

    /// Error bubbled from `qv-core` transaction routines.
    #[error(transparent)]
    Transaction(#[from] qv_core::TransactionError),
}

/// Storage result alias.
pub type StorageResult<T> = core::result::Result<T, StorageError>;

/// Encode a value with canonical bincode serialization.
pub(crate) fn encode<T: Serialize>(value: &T) -> StorageResult<Vec<u8>> {
    bincode::serialize(value).map_err(|e| StorageError::Encode(e.to_string()))
}

/// Decode a value with canonical bincode serialization.
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> StorageResult<T> {
    bincode::deserialize(bytes).map_err(|e| StorageError::Decode(e.to_string()))
}
