//! DeFi primitives: AMM, lending, oracles, intent-based orders.
//!
//! This crate bundles all DeFi protocols for QuantumVault L1:
//!
//! | Module        | Purpose                                      |
//! |---------------|----------------------------------------------|
//! | [`amm`]       | Constant-product AMM (Uniswap v2 style)      |
//! | [`lending`]   | Single-pool lending with interest accrual    |
//! | [`oracle`]    | Validator-median price oracle with TWAP      |
//! | [`intents`]   | Intent-based order encoding for batching      |
//!
//! # Architecture
//!
//! DeFi in QuantumVault follows the **Shared UTXO Pattern** (Cardano eUTXO):
//! - Each pool is a single UTXO holding reserves in a datum.
//! - User orders are intents that the batcher executes deterministically.
//! - Smart contracts validate invariants post-execution (not during).
//! - MEV is mitigated via encrypted mempool + threshold decryption.

#![forbid(unsafe_code)]
// `missing_docs` workspace-managed; see Cargo.toml. Re-tightened in Faz 9.

pub mod amm;
pub mod intents;
pub mod lending;
pub mod oracle;

use thiserror::Error;

// ============================================================================
// Error Aggregation
// ============================================================================

/// Unified DeFi error type.
///
/// Each submodule (amm, lending, oracle, intents) has its own error type,
/// but this aggregate enum allows callers to handle all DeFi errors uniformly.
#[derive(Debug, Clone, Error)]
pub enum DefiError {
    /// AMM-related error.
    #[error("amm error: {0}")]
    Amm(#[from] AmmError),

    /// Lending-related error.
    #[error("lending error: {0}")]
    Lending(#[from] LendingError),

    /// Oracle-related error.
    #[error("oracle error: {0}")]
    Oracle(#[from] OracleError),

    /// Intent encoding/validation error.
    #[error("intent error: {0}")]
    Intent(#[from] IntentError),
}

/// Crate-level result alias.
pub type Result<T> = core::result::Result<T, DefiError>;

// ============================================================================
// Re-exports — public surface
// ============================================================================

// AMM
pub use amm::{
    compute_add_liquidity, compute_remove_liquidity, compute_swap_output, AmmError, PoolDatum,
    PoolState, SwapDirection,
};

// Lending
pub use lending::{
    compute_deposit, compute_liquidation_bonus, compute_max_borrow, LendingError, LendingPoolDatum,
    LendingPosition,
};

// Oracle
pub use oracle::{
    aggregate_median, compute_twap, OracleError, OracleWindow, PriceObservation,
};

// Intents
pub use intents::{IntentBundle, IntentError, OrderIntent, OrderKind, SwapIntentBuilder};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_core::{Amount, Hash256, TxId};

    #[test]
    fn error_aggregation_amm() {
        let amm_err = amm::AmmError::Overflow;
        let defi_err: DefiError = amm_err.into();
        assert!(matches!(defi_err, DefiError::Amm(_)));
    }

    #[test]
    fn error_aggregation_lending() {
        let lend_err = lending::LendingError::NoDebt;
        let defi_err: DefiError = lend_err.into();
        assert!(matches!(defi_err, DefiError::Lending(_)));
    }

    #[test]
    fn error_aggregation_oracle() {
        let oracle_err = oracle::OracleError::InsufficientObservations {
            have: 0,
            need: 3,
        };
        let defi_err: DefiError = oracle_err.into();
        assert!(matches!(defi_err, DefiError::Oracle(_)));
    }

    #[test]
    fn error_aggregation_intent() {
        let intent_err = intents::IntentError::InvalidAmount(0);
        let defi_err: DefiError = intent_err.into();
        assert!(matches!(defi_err, DefiError::Intent(_)));
    }

    #[test]
    fn integration_amm_pool_creation() {
        let pool = PoolDatum::new(
            Hash256::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            1_000_000,
            2_000_000,
            30,
        );
        assert!(pool.validate().is_ok());
    }

    #[test]
    fn integration_lending_pool_creation() {
        let pool = lending::LendingPoolDatum::new(
            Hash256::from_bytes([3; 32]),
            Hash256::from_bytes([4; 32]),
            100_000,
            50_000,
        );
        assert!(pool.validate().is_ok());
    }

    #[test]
    fn integration_oracle_window() {
        let mut window = OracleWindow::new(Hash256::from_bytes([5; 32]), 10);
        let obs = PriceObservation::new(
            Hash256::from_bytes([5; 32]),
            1000u128 << 64,
            100,
            Hash256::from_bytes([6; 32]),
            vec![],
        );
        assert!(window.add_observation(obs).is_ok());
    }

    #[test]
    fn integration_intent_swap() {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([7; 32]),
            Hash256::from_bytes([8; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        assert!(intent.validate(500).is_ok());
    }
}
