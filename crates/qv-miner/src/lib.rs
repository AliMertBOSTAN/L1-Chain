//! Stake pool operator: VRF slot leader, block production, encrypted-mempool committee.
//!
//! # Overview
//!
//! This crate implements the stake pool operator binary for QuantumVault L1.
//! An operator runs the following tasks:
//!
//! - **Key management**: Generate and encrypt VRF/KES/cold keys.
//! - **Pool registration**: Build and broadcast pool registration transactions.
//! - **Leadership**: Each slot, evaluate VRF against the epoch nonce; if elected, produce a block.
//! - **Block production**: Assemble blocks from the clear mempool, decrypt the encrypted mempool
//!   if the operator is on the decryption committee, sort transactions deterministically,
//!   build AMM batches, sign with KES, and gossip.
//! - **Committee sortition**: Determine whether the operator is on the encrypted-mempool
//!   decryption committee for the current epoch.
//! - **Dashboard**: Optional TUI showing live operator metrics.
//!
//! # Architecture
//!
//! ```text
//! main.rs
//!   └─> CLI { init, register-pool, delegate, run, dashboard, keys-show }
//!        └─> config.rs (OperatorConfig from TOML)
//!             └─> keys.rs (VRF + KES + cold key pair)
//!                  ├─> slot_loop.rs (async slot tick → leadership check)
//!                  │    └─> block_producer.rs (build + sign + gossip block)
//!                  │         ├─> mempool snapshots (RPC or local mirror)
//!                  │         ├─> committee.rs (is_committee_member?)
//!                  │         │    └─> encrypted mempool decrypt (if member)
//!                  │         ├─> deterministic ordering + AMM batch
//!                  │         └─> KES signature + libp2p gossip
//!                  └─> committee.rs (sortition: VRF on epoch nonce)
//!                       └─> DecryptionShare collection / reconstruction
//!        └─> registration.rs (pool registration TX builder)
//!        └─> dashboard.rs (ratatui TUI)
//!
//! # Key Design Decisions
//!
//! 1. **VRF/KES abstraction**: Leadership check uses `qv_consensus::VrfEvaluator` trait,
//!    allowing testing with `TestVrf` mock while the real primitive is finalized.
//!    KES signature uses `qv_consensus::KesVerifier` trait similarly.
//!
//! 2. **Committee sortition**: Deterministic, derived from epoch nonce + pool ID + committee size.
//!    Uses a helper `is_committee_member()` that calls the VRF on a domain-separated input.
//!
//! 3. **Encrypted mempool**: The operator mirrors the node's encrypted pool via RPC.
//!    If elected to the decryption committee, it contributes a share to reconstruct
//!    the batch. Otherwise, it gossips the clear pool batch as-is.
//!
//! 4. **KES rotation**: KES key evolves once per epoch (or configurable period).
//!    Uses a mock `KesEvolver` trait; real CRYSTALS-Kyber KES from ADR-005.
//!
//! 5. **RPC integration**: Block producer pulls tx batches via `qv_getMempoolStatus` +
//!    `qv_drainMempoolBatch` RPC methods (if exposed by node), or maintains a local mirror.

#![forbid(unsafe_code)]

pub mod cli;
pub mod committee;
pub mod config;
pub mod keys;
pub mod keystore;
pub mod registration;
pub mod block_producer;
pub mod slot_loop;
pub mod dashboard;

use thiserror::Error;

/// Errors in miner operations.
#[derive(Debug, Error)]
pub enum MinerError {
    /// Configuration error.
    #[error("config error: {0}")]
    Config(String),

    /// Keystore error.
    #[error("keystore error: {0}")]
    Keystore(String),

    /// VRF evaluation failed.
    #[error("VRF error: {0}")]
    VrfError(String),

    /// KES signature error.
    #[error("KES error: {0}")]
    KesError(String),

    /// RPC call failed.
    #[error("RPC error: {0}")]
    RpcError(String),

    /// Mempool error.
    #[error("mempool error: {0}")]
    MempoolError(String),

    /// Block production error.
    #[error("block production error: {0}")]
    BlockProduction(String),

    /// Committee sortition error.
    #[error("committee error: {0}")]
    Committee(String),

    /// Consensus error (from qv-consensus).
    #[error("consensus error: {0}")]
    Consensus(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Async runtime error.
    #[error("async runtime error: {0}")]
    Runtime(String),

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(String),

    /// Key generation failed (any of VRF, KES, cold).
    #[error("key generation error: {0}")]
    KeyGeneration(String),

    /// Signing operation failed (any of VRF, KES, cold).
    #[error("signing error: {0}")]
    SigningFailed(String),
}

/// Result alias for miner operations.
pub type MinerResult<T> = Result<T, MinerError>;

// Re-export headline types at crate root for consumer convenience.
pub use cli::Cli;
pub use config::OperatorConfig;
pub use keys::OperatorKeys;
pub use registration::build_pool_registration_tx;
pub use block_producer::produce_block;
pub use committee::is_committee_member;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn miner_error_display() {
        let err = MinerError::Config("bad file".to_string());
        assert_eq!(format!("{err}"), "config error: bad file");

        let err2 = MinerError::VrfError("evaluation failed".to_string());
        assert!(format!("{err2}").contains("VRF"));
    }
}
