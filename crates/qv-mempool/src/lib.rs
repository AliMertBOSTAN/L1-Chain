//! Mempool for QuantumVault: clear pool, encrypted pool, ordering, and batcher.
//!
//! - **`clear`**: Fee-sorted cleartext mempool with UTXO dependency tracking.
//! - **`ordering`**: Deterministic transaction ordering for block building.
//! - **`encrypted`**: Threshold-Kyber encrypted pool (ADR-003).
//! - **`batcher`**: Order intent decoding, AMM batch execution, slashing evidence.

#![forbid(unsafe_code)]

pub mod batcher;
pub mod clear;
pub mod encrypted;
pub mod ordering;

use qv_core::{Epoch, OutPoint, TxId};
use thiserror::Error;

/// Error type for mempool operations.
#[derive(Debug, Error)]
pub enum MempoolError {
    /// A transaction with this id is already in the pool.
    #[error("duplicate transaction: {0:?}")]
    DuplicateTx(TxId),

    /// The transaction's fee is below the minimum.
    #[error("fee too low: {fee} < {min}")]
    FeeTooLow { fee: u64, min: u64 },

    /// An input outpoint is already being spent by another pooled transaction.
    #[error("double spend: outpoint {outpoint:?} already spent by {existing_tx:?}")]
    DoubleSpend {
        outpoint: OutPoint,
        existing_tx: TxId,
    },

    /// The pool is at capacity and could not evict enough to make room.
    #[error("pool full")]
    PoolFull,

    /// Encrypted transaction targets the wrong epoch.
    #[error("wrong epoch: got {got:?}, expected {expected:?}")]
    WrongEpoch { got: Epoch, expected: Epoch },

    /// Threshold decryption failure.
    #[error("decryption error: {0}")]
    Decryption(String),

    /// Not enough decryption shares provided.
    #[error("insufficient shares: got {got}, need {need}")]
    InsufficientShares { got: u32, need: u32 },

    /// Batch building error.
    #[error("batch error: {0}")]
    Batch(String),
}

/// Result alias for mempool operations.
pub type MempoolResult<T> = Result<T, MempoolError>;

// Re-export headline types at crate root.
pub use batcher::{BatchResult, OrderIntent, PoolState, SlashingEvidence, SwapDirection};
pub use clear::{ClearPool, ClearPoolConfig, MempoolEntry};
pub use encrypted::{
    create_envelope_share, encrypt_envelope, encrypt_envelope_random, DecryptionShare,
    DkgEnvelopeDecryptor, EncryptedPool, EncryptedPoolConfig, EncryptedTx,
    MockThresholdDecryptor, ThresholdDecryptor, AES_KEY_BYTES, AES_NONCE_BYTES,
    ENVELOPE_CIPHERTEXT_BYTES,
};
pub use ordering::{deterministic_sort, verify_order, OrderKey};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = MempoolError::PoolFull;
        assert_eq!(format!("{err}"), "pool full");

        let err2 = MempoolError::FeeTooLow { fee: 5, min: 10 };
        assert!(format!("{err2}").contains("5"));
    }

    #[test]
    fn error_double_spend_display() {
        let err = MempoolError::DoubleSpend {
            outpoint: OutPoint::new(TxId::from_bytes([1; 32]), 0),
            existing_tx: TxId::from_bytes([2; 32]),
        };
        let s = format!("{err}");
        assert!(s.contains("double spend"));
    }

    #[test]
    fn error_wrong_epoch_display() {
        let err = MempoolError::WrongEpoch {
            got: Epoch::from(5),
            expected: Epoch::from(3),
        };
        let s = format!("{err}");
        assert!(s.contains("wrong epoch"));
    }
}
