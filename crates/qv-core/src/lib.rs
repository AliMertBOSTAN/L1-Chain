//! # qv-core
//!
//! Core ledger primitives for QuantumVault L1: identifiers, amounts, UTXO
//! entries, transactions, blocks with a Merkle commitment over their body,
//! a pluggable [`UtxoSet`] trait with an in-memory implementation, and the
//! [`ProtocolParams`] bundle that pins network-wide constants.
//!
//! This crate is deliberately **consensus-light**: it knows nothing about
//! leader election, stake distribution, or script execution. Higher-level
//! crates compose on top of these types:
//!
//! - [`qv_script`] consumes `Transaction`/`TxOutput` to run the locking
//!   script VM.
//! - [`qv_consensus`] consumes `BlockHeader` + `ProtocolParams` to drive
//!   Ouroboros Praos.
//! - [`qv_storage`] persists `Block`s and the `UtxoSet`.
//!
//! ## Modules
//!
//! - [`types`] — identifier newtypes (`TxId`, `BlockHash`, …) and arithmetic
//!   wrappers (`Amount`, `Height`, `Slot`, …).
//! - [`transaction`] — eUTXO transaction structure: `TxInput`, `TxOutput`
//!   with `Datum`, `StealthInfo`, `ValidityInterval`, and canonical
//!   `TxId` derivation.
//! - [`block`] — `BlockHeader`, `Block`, and [`merkle_root_of`] (Bitcoin-style
//!   binary Merkle, duplicate-last padding, with block-level duplicate-TxId
//!   rejection as the CVE-2012-2459 mitigation).
//! - [`utxo`] — the `UtxoSet` trait, `InMemoryUtxoSet`, and the canonical
//!   [`commitment_root_of_sorted_entries`] commitment used in
//!   `BlockHeader::utxo_commitment`.
//! - [`params`] — `ProtocolParams` plus `ConsensusParams`, `LedgerParams`,
//!   `MonetaryParams`, with `mainnet`/`testnet`/`ephemeral` presets and
//!   TOML/JSON (de)serialization.
//!
//! ## Determinism
//!
//! Every piece of this crate is designed to be **bit-identical across
//! implementations**:
//!
//! - All canonical encodings go through `bincode::serialize` with a fixed
//!   field order.
//! - All hashes are SHA3-256 via `qv_crypto::sha3_256`.
//! - All arithmetic on `Amount` is `checked_*` — no silent overflow.
//! - The UTXO set iterates in `OutPoint` order (via `BTreeMap`) so the
//!   commitment root is independent of insertion order.
//!
//! ## Error model
//!
//! Each module has its own fine-grained error enum ([`TypeError`],
//! [`TransactionError`], [`BlockError`], [`UtxoError`], [`ParamsError`]).
//! They are aggregated under [`CoreError`] for consumers that only care
//! that "something in the core layer failed".

#![forbid(unsafe_code)]

pub mod block;
pub mod params;
pub mod transaction;
pub mod types;
pub mod utxo;

// ---------------------------------------------------------------------------
// Re-exports: the stable public surface of the crate.
// ---------------------------------------------------------------------------

/// Block header and full block structures; see [`crate::block`] for module docs.
pub use crate::block::{merkle_root_of, Block, BlockError, BlockHeader, BLOCK_VERSION};

/// Protocol parameter bundles; see [`crate::params`] for module docs.
pub use crate::params::{
    ConsensusParams, LedgerParams, MonetaryParams, NetworkId, ParamsError, ProtocolParams,
};

/// Transaction types and validation; see [`crate::transaction`] for module docs.
pub use crate::transaction::{
    Datum, Script, StealthInfo, Transaction, TransactionError, TxInput, TxOutput,
    ValidityInterval, Witness, TX_VERSION,
};

/// Core newtypes (amounts, hashes, identifiers, timestamps); see [`crate::types`] for module docs.
pub use crate::types::{
    Amount, BlockHash, DatumHash, Epoch, Hash256, Height, MerkleRoot, OutPoint, ScriptHash, Slot,
    Timestamp, TxId, TypeError, UtxoCommitment,
};

/// UTXO set abstraction and in-memory implementation; see [`crate::utxo`] for module docs.
pub use crate::utxo::{
    commitment_root_of_sorted_entries, InMemoryUtxoSet, UtxoError, UtxoSet,
};

// ---------------------------------------------------------------------------
// Aggregate error
// ---------------------------------------------------------------------------

use thiserror::Error;

/// Aggregate error for the `qv-core` crate.
///
/// Downstream crates that compose several core modules (e.g. apply a block
/// to a UTXO set while validating its header-declared Merkle root and
/// protocol parameters) can bubble everything up as a single `CoreError`
/// via `?` without juggling five separate enums.
///
/// Sub-errors are preserved with `#[from]`, so `match`ing on the inner
/// variant works normally:
///
/// # Examples
///
/// ```rust
/// # use qv_core::{CoreError, BlockError};
/// fn check_error(e: CoreError) {
///     if let CoreError::Block(BlockError::MerkleRootMismatch) = e {
///         println!("block had bad merkle root");
///     }
/// }
/// ```
#[derive(Debug, Error)]
pub enum CoreError {
    /// Error from the [`types`] module (e.g. malformed hex, wrong length).
    #[error(transparent)]
    Type(#[from] TypeError),

    /// Error from the [`transaction`] module.
    #[error(transparent)]
    Transaction(#[from] TransactionError),

    /// Error from the [`block`] module.
    #[error(transparent)]
    Block(#[from] BlockError),

    /// Error from the [`utxo`] module.
    #[error(transparent)]
    Utxo(#[from] UtxoError),

    /// Error from the [`params`] module.
    #[error(transparent)]
    Params(#[from] ParamsError),
}

/// Convenience alias for `Result<T, CoreError>`.
pub type CoreResult<T> = Result<T, CoreError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn core_error_wraps_block_error() {
        let e: CoreError = BlockError::MerkleRootMismatch.into();
        assert!(matches!(e, CoreError::Block(BlockError::MerkleRootMismatch)));
    }

    #[test]
    fn core_error_wraps_params_error() {
        let e: CoreError = ParamsError::Invalid("bad").into();
        assert!(matches!(e, CoreError::Params(ParamsError::Invalid(_))));
    }

    #[test]
    fn core_error_display_is_transparent() {
        let inner = BlockError::DuplicateTx;
        let outer: CoreError = inner.into();
        // Transparent means the outer Display string equals the inner one.
        assert_eq!(outer.to_string(), "block contains duplicate TxId");
    }

    #[test]
    fn public_surface_is_reachable() {
        // Sanity: the crate root re-exports the headline types so consumers
        // only need `qv_core::TxId` / `qv_core::Block` / etc.
        let _: Option<TxId> = None;
        let _: Option<BlockHeader> = None;
        let _: Option<Block> = None;
        let _: Option<Transaction> = None;
        let _: Option<InMemoryUtxoSet> = None;
        let _: Option<ProtocolParams> = None;
    }
}
