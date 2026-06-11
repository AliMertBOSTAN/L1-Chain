//! Constant-product AMM (Automated Market Maker) using the Shared UTXO Pattern.
//!
//! This module implements a Uniswap v2-style AMM with the invariant `x·y ≥ k`,
//! where a single UTXO holds the pool state (reserves) and a datum stores metadata.
//! Every swap consumes the old pool UTXO and produces a new one with updated reserves.
//!
//! # Architecture
//!
//! - **PoolDatum**: Encodes reserves (x, y), fee bps, LP total supply, and pool metadata.
//! - **PoolState**: In-memory snapshot of a pool used by the batcher.
//! - **LpToken**: Liquidity provider token minting/burning (fixed supply, tracked in datum).
//! - **Slippage protection**: Orders with `min_receive` that cannot be satisfied are skipped.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use qv_core::Hash256;
use qv_script::templates::{
    POOL_DATUM_FEE_BPS_OFFSET, POOL_DATUM_LEN, POOL_DATUM_LP_TOTAL_OFFSET,
    POOL_DATUM_RESERVE_A_OFFSET, POOL_DATUM_RESERVE_B_OFFSET, POOL_DATUM_TOKEN_A_OFFSET,
    POOL_DATUM_TOKEN_B_OFFSET,
};

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur in AMM operations.
#[derive(Debug, Clone, Error)]
pub enum AmmError {
    /// Pool reserves are zero (no liquidity).
    #[error("no liquidity in pool {pool_id:?}")]
    InsufficientLiquidity { pool_id: Hash256 },

    /// Swap output would exceed slippage tolerance.
    #[error("slippage exceeded: expected {expected}, got {actual}")]
    SlippageExceeded { expected: u64, actual: u64 },

    /// Pool not found or invalid.
    #[error("invalid pool {pool_id:?}")]
    InvalidPool { pool_id: Hash256 },

    /// Datum encoding or decoding error.
    #[error("datum error: {0}")]
    DatumError(String),

    /// LP token supply mismatch.
    #[error("lp token mismatch: expected {expected}, got {actual}")]
    LpTokenMismatch { expected: u64, actual: u64 },

    /// Arithmetic overflow.
    #[error("arithmetic overflow")]
    Overflow,

    /// Invariant violation (x·y < k after swap).
    #[error("invariant violated: {new_product} < {old_product}")]
    InvariantViolated {
        old_product: u128,
        new_product: u128,
    },

    /// Invalid fee (must be 0..10000 bps).
    #[error("invalid fee: {fee_bps} bps")]
    InvalidFee { fee_bps: u16 },
}

/// Crate-level result type.
pub type Result<T> = core::result::Result<T, AmmError>;

// ============================================================================
// Pool Datum
// ============================================================================

/// Metadata for a liquidity pool stored in the UTXO datum.
/// This is encoded deterministically and stored on-chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolDatum {
    /// Token A identifier (script hash).
    pub token_a_id: Hash256,

    /// Token B identifier (script hash).
    pub token_b_id: Hash256,

    /// Current reserve of token A (smallest units).
    pub reserve_a: u64,

    /// Current reserve of token B (smallest units).
    pub reserve_b: u64,

    /// Total LP tokens issued (smallest units).
    pub lp_total: u64,

    /// Fee in basis points (0..10000). Default: 30 (0.3%).
    pub fee_bps: u16,
}

impl PoolDatum {
    /// Create a new pool datum.
    #[must_use]
    pub fn new(
        token_a_id: Hash256,
        token_b_id: Hash256,
        reserve_a: u64,
        reserve_b: u64,
        fee_bps: u16,
    ) -> Self {
        Self {
            token_a_id,
            token_b_id,
            reserve_a,
            reserve_b,
            lp_total: 0,
            fee_bps,
        }
    }

    /// Validate the pool datum.
    pub fn validate(&self) -> Result<()> {
        if self.fee_bps > 10_000 {
            return Err(AmmError::InvalidFee {
                fee_bps: self.fee_bps,
            });
        }
        Ok(())
    }

    /// Compute the invariant product: x · y.
    #[must_use]
    pub fn invariant(&self) -> u128 {
        (self.reserve_a as u128).saturating_mul(self.reserve_b as u128)
    }

    /// Encode datum to bincode bytes.
    ///
    /// **Off-chain / storage use only.** The on-chain pool UTXO datum must
    /// use [`Self::to_canonical_bytes`] instead: the `amm_pool_lock` script
    /// slices fields at fixed offsets and must not depend on a serializer's
    /// internal layout.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| AmmError::DatumError(e.to_string()))
    }

    /// Decode datum from bincode bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).map_err(|e| AmmError::DatumError(e.to_string()))
    }

    /// Length in bytes of the canonical (script-friendly) encoding.
    ///
    /// Defined by `qv-script` (the consumer of the layout) as
    /// [`POOL_DATUM_LEN`] so the two crates cannot drift.
    pub const CANONICAL_LEN: usize = POOL_DATUM_LEN;

    /// Encode the datum into its **canonical fixed-width layout** — the
    /// exact byte sequence the `amm_pool_lock` covenant script slices:
    ///
    /// | Bytes   | Field        | Type         |
    /// |---------|--------------|--------------|
    /// | 0..32   | `token_a_id` | 32 raw bytes |
    /// | 32..64  | `token_b_id` | 32 raw bytes |
    /// | 64..72  | `reserve_a`  | `u64` LE     |
    /// | 72..80  | `reserve_b`  | `u64` LE     |
    /// | 80..88  | `lp_total`   | `u64` LE     |
    /// | 88..90  | `fee_bps`    | `u16` LE     |
    ///
    /// This — not bincode — is what must be attached to the pool UTXO as
    /// its on-chain datum.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::CANONICAL_LEN);
        out.extend_from_slice(self.token_a_id.as_bytes());
        out.extend_from_slice(self.token_b_id.as_bytes());
        out.extend_from_slice(&self.reserve_a.to_le_bytes());
        out.extend_from_slice(&self.reserve_b.to_le_bytes());
        out.extend_from_slice(&self.lp_total.to_le_bytes());
        out.extend_from_slice(&self.fee_bps.to_le_bytes());
        out
    }

    /// Decode a datum from its canonical fixed-width layout (the inverse
    /// of [`Self::to_canonical_bytes`]).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::CANONICAL_LEN {
            return Err(AmmError::DatumError(format!(
                "canonical pool datum must be {} bytes, got {}",
                Self::CANONICAL_LEN,
                bytes.len()
            )));
        }

        fn read_32(bytes: &[u8], offset: usize) -> Result<[u8; 32]> {
            bytes
                .get(offset..offset.wrapping_add(32))
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| AmmError::DatumError("token id slice out of range".into()))
        }
        fn read_u64(bytes: &[u8], offset: usize) -> Result<u64> {
            bytes
                .get(offset..offset.wrapping_add(8))
                .and_then(|s| <[u8; 8]>::try_from(s).ok())
                .map(u64::from_le_bytes)
                .ok_or_else(|| AmmError::DatumError("u64 slice out of range".into()))
        }
        fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
            bytes
                .get(offset..offset.wrapping_add(2))
                .and_then(|s| <[u8; 2]>::try_from(s).ok())
                .map(u16::from_le_bytes)
                .ok_or_else(|| AmmError::DatumError("u16 slice out of range".into()))
        }

        Ok(Self {
            token_a_id: Hash256::from_bytes(read_32(bytes, POOL_DATUM_TOKEN_A_OFFSET)?),
            token_b_id: Hash256::from_bytes(read_32(bytes, POOL_DATUM_TOKEN_B_OFFSET)?),
            reserve_a: read_u64(bytes, POOL_DATUM_RESERVE_A_OFFSET)?,
            reserve_b: read_u64(bytes, POOL_DATUM_RESERVE_B_OFFSET)?,
            lp_total: read_u64(bytes, POOL_DATUM_LP_TOTAL_OFFSET)?,
            fee_bps: read_u16(bytes, POOL_DATUM_FEE_BPS_OFFSET)?,
        })
    }
}

// ============================================================================
// Pool State (In-Memory)
// ============================================================================

/// Current state snapshot of an AMM pool (used for batch processing).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolState {
    /// Pool identifier (typically script hash of the pool UTXO).
    pub pool_id: Hash256,

    /// Pool datum.
    pub datum: PoolDatum,
}

impl PoolState {
    /// Create a new pool state.
    #[must_use]
    pub fn new(pool_id: Hash256, datum: PoolDatum) -> Self {
        Self { pool_id, datum }
    }

    /// Update reserves after a swap.
    pub fn apply_swap(
        &mut self,
        direction: SwapDirection,
        amount_in: u64,
        amount_out: u64,
    ) -> Result<()> {
        match direction {
            SwapDirection::AtoB => {
                self.datum.reserve_a = self
                    .datum
                    .reserve_a
                    .checked_add(amount_in)
                    .ok_or(AmmError::Overflow)?;
                self.datum.reserve_b = self
                    .datum
                    .reserve_b
                    .checked_sub(amount_out)
                    .ok_or(AmmError::Overflow)?;
            }
            SwapDirection::BtoA => {
                self.datum.reserve_b = self
                    .datum
                    .reserve_b
                    .checked_add(amount_in)
                    .ok_or(AmmError::Overflow)?;
                self.datum.reserve_a = self
                    .datum
                    .reserve_a
                    .checked_sub(amount_out)
                    .ok_or(AmmError::Overflow)?;
            }
        }
        Ok(())
    }
}

// ============================================================================
// Swap Direction
// ============================================================================

/// Direction of a token swap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwapDirection {
    /// Swap token A for token B.
    AtoB,
    /// Swap token B for token A.
    BtoA,
}

// ============================================================================
// Swap Computation
// ============================================================================

/// Compute constant-product swap output.
///
/// Given input amount `amount_in` and reserves `reserve_in` / `reserve_out`,
/// compute the output `amount_out` such that:
///
/// ```text
/// (reserve_in + amount_in_net) * (reserve_out - amount_out) >= reserve_in * reserve_out
/// ```
///
/// where `amount_in_net = amount_in * (10000 - fee_bps) / 10000`.
#[must_use]
pub fn compute_swap_output(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    fee_bps: u16,
) -> Option<(u64, u64)> {
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return None;
    }

    // Fee deduction: fee = amount_in * fee_bps / 10000
    let fee = (amount_in as u128)
        .checked_mul(fee_bps as u128)?
        .checked_div(10_000)?;

    if fee > amount_in as u128 {
        return None;
    }

    let amount_in_net = (amount_in as u128).checked_sub(fee)?;

    // amount_out = reserve_out * amount_in_net / (reserve_in + amount_in_net)
    let numerator = (reserve_out as u128).checked_mul(amount_in_net)?;
    let denominator = (reserve_in as u128).checked_add(amount_in_net)?;

    if denominator == 0 {
        return None;
    }

    let amount_out = numerator.checked_div(denominator)?;

    // amount_out must fit in u64
    if amount_out > u64::MAX as u128 {
        return None;
    }

    Some((amount_out as u64, fee as u64))
}

// ============================================================================
// Liquidity Operations
// ============================================================================

/// Add liquidity to a pool.
///
/// When a user adds `amount_a` of token A and `amount_b` of token B,
/// they receive LP tokens proportional to the product of the amounts.
///
/// # Formula
///
/// If the pool is empty (lp_total == 0):
/// ```text
/// lp_issued = sqrt(amount_a * amount_b)
/// ```
///
/// Otherwise:
/// ```text
/// lp_issued = min(
///   lp_total * amount_a / reserve_a,
///   lp_total * amount_b / reserve_b
/// )
/// ```
pub fn compute_add_liquidity(
    current_reserve_a: u64,
    current_reserve_b: u64,
    current_lp_total: u64,
    amount_a: u64,
    amount_b: u64,
) -> Result<(u64, PoolDatum)> {
    if amount_a == 0 || amount_b == 0 {
        return Err(AmmError::InsufficientLiquidity {
            pool_id: Hash256::ZERO,
        });
    }

    let lp_issued = if current_lp_total == 0 {
        // Empty pool: LP = sqrt(amount_a * amount_b)
        let product = (amount_a as u128)
            .checked_mul(amount_b as u128)
            .ok_or(AmmError::Overflow)?;

        // Integer square root (Newton's method)
        sqrt_u128(product) as u64
    } else {
        // Non-empty: pro-rata share
        let share_a = (current_lp_total as u128)
            .checked_mul(amount_a as u128)
            .ok_or(AmmError::Overflow)?
            .checked_div(current_reserve_a as u128)
            .ok_or(AmmError::Overflow)?;

        let share_b = (current_lp_total as u128)
            .checked_mul(amount_b as u128)
            .ok_or(AmmError::Overflow)?
            .checked_div(current_reserve_b as u128)
            .ok_or(AmmError::Overflow)?;

        // Liquidity provider gets the minimum (balanced constraint)
        let lp = core::cmp::min(share_a, share_b);

        if lp > u64::MAX as u128 {
            return Err(AmmError::Overflow);
        }

        lp as u64
    };

    let new_reserve_a = current_reserve_a
        .checked_add(amount_a)
        .ok_or(AmmError::Overflow)?;

    let new_reserve_b = current_reserve_b
        .checked_add(amount_b)
        .ok_or(AmmError::Overflow)?;

    let new_lp_total = current_lp_total
        .checked_add(lp_issued)
        .ok_or(AmmError::Overflow)?;

    // Create dummy datum (caller will fill in token IDs and fee)
    let datum = PoolDatum {
        token_a_id: Hash256::ZERO,
        token_b_id: Hash256::ZERO,
        reserve_a: new_reserve_a,
        reserve_b: new_reserve_b,
        lp_total: new_lp_total,
        fee_bps: 30,
    };

    Ok((lp_issued, datum))
}

/// Remove liquidity from a pool.
///
/// When a user burns `lp_amount` LP tokens, they receive a proportional share
/// of both reserves.
pub fn compute_remove_liquidity(
    current_reserve_a: u64,
    current_reserve_b: u64,
    current_lp_total: u64,
    lp_burned: u64,
) -> Result<(u64, u64, PoolDatum)> {
    if current_lp_total == 0 || lp_burned == 0 {
        return Err(AmmError::InsufficientLiquidity {
            pool_id: Hash256::ZERO,
        });
    }

    if lp_burned > current_lp_total {
        return Err(AmmError::LpTokenMismatch {
            expected: current_lp_total,
            actual: lp_burned,
        });
    }

    // Proportional amounts
    let amount_a = (current_reserve_a as u128)
        .checked_mul(lp_burned as u128)
        .ok_or(AmmError::Overflow)?
        .checked_div(current_lp_total as u128)
        .ok_or(AmmError::Overflow)?;

    let amount_b = (current_reserve_b as u128)
        .checked_mul(lp_burned as u128)
        .ok_or(AmmError::Overflow)?
        .checked_div(current_lp_total as u128)
        .ok_or(AmmError::Overflow)?;

    if amount_a > u64::MAX as u128 || amount_b > u64::MAX as u128 {
        return Err(AmmError::Overflow);
    }

    let amount_a = amount_a as u64;
    let amount_b = amount_b as u64;

    let new_reserve_a = current_reserve_a
        .checked_sub(amount_a)
        .ok_or(AmmError::Overflow)?;

    let new_reserve_b = current_reserve_b
        .checked_sub(amount_b)
        .ok_or(AmmError::Overflow)?;

    let new_lp_total = current_lp_total
        .checked_sub(lp_burned)
        .ok_or(AmmError::Overflow)?;

    let datum = PoolDatum {
        token_a_id: Hash256::ZERO,
        token_b_id: Hash256::ZERO,
        reserve_a: new_reserve_a,
        reserve_b: new_reserve_b,
        lp_total: new_lp_total,
        fee_bps: 30,
    };

    Ok((amount_a, amount_b, datum))
}

// ============================================================================
// Helpers
// ============================================================================

/// Integer square root using Newton's method.
fn sqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn make_pool_datum() -> PoolDatum {
        PoolDatum {
            token_a_id: Hash256::from_bytes([1; 32]),
            token_b_id: Hash256::from_bytes([2; 32]),
            reserve_a: 10_000,
            reserve_b: 10_000,
            lp_total: 10_000,
            fee_bps: 30,
        }
    }

    #[test]
    fn pool_datum_validate_ok() {
        let datum = make_pool_datum();
        assert!(datum.validate().is_ok());
    }

    #[test]
    fn pool_datum_validate_fee() {
        let mut datum = make_pool_datum();
        datum.fee_bps = 10_001;
        assert!(datum.validate().is_err());
    }

    #[test]
    fn pool_datum_invariant() {
        let datum = make_pool_datum();
        assert_eq!(datum.invariant(), 10_000u128 * 10_000u128);
    }

    #[test]
    fn pool_datum_roundtrip() {
        let datum = make_pool_datum();
        let bytes = datum.to_bytes().unwrap();
        let decoded = PoolDatum::from_bytes(&bytes).unwrap();
        assert_eq!(datum, decoded);
    }

    #[test]
    fn pool_datum_canonical_roundtrip() {
        let mut datum = make_pool_datum();
        datum.reserve_a = u64::MAX; // exercise full-width values
        datum.fee_bps = 9_999;
        let bytes = datum.to_canonical_bytes();
        assert_eq!(bytes.len(), PoolDatum::CANONICAL_LEN);
        let decoded = PoolDatum::from_canonical_bytes(&bytes).unwrap();
        assert_eq!(datum, decoded);
    }

    #[test]
    fn pool_datum_canonical_layout_matches_script_offsets() {
        // The amm_pool_lock covenant slices these exact offsets; if this
        // test breaks, on-chain pools become unspendable. Field order:
        // token_a(32) | token_b(32) | reserve_a(8) | reserve_b(8)
        // | lp_total(8) | fee_bps(2).
        let datum = PoolDatum {
            token_a_id: Hash256::from_bytes([0xA1; 32]),
            token_b_id: Hash256::from_bytes([0xB2; 32]),
            reserve_a: 0x0102_0304_0506_0708,
            reserve_b: 0x1112_1314_1516_1718,
            lp_total: 0x2122_2324_2526_2728,
            fee_bps: 30,
        };
        let bytes = datum.to_canonical_bytes();

        assert_eq!(&bytes[POOL_DATUM_TOKEN_A_OFFSET..POOL_DATUM_TOKEN_A_OFFSET + 32], &[0xA1; 32]);
        assert_eq!(&bytes[POOL_DATUM_TOKEN_B_OFFSET..POOL_DATUM_TOKEN_B_OFFSET + 32], &[0xB2; 32]);
        assert_eq!(
            &bytes[POOL_DATUM_RESERVE_A_OFFSET..POOL_DATUM_RESERVE_A_OFFSET + 8],
            &0x0102_0304_0506_0708_u64.to_le_bytes()
        );
        assert_eq!(
            &bytes[POOL_DATUM_RESERVE_B_OFFSET..POOL_DATUM_RESERVE_B_OFFSET + 8],
            &0x1112_1314_1516_1718_u64.to_le_bytes()
        );
        assert_eq!(
            &bytes[POOL_DATUM_LP_TOTAL_OFFSET..POOL_DATUM_LP_TOTAL_OFFSET + 8],
            &0x2122_2324_2526_2728_u64.to_le_bytes()
        );
        assert_eq!(
            &bytes[POOL_DATUM_FEE_BPS_OFFSET..],
            &30_u16.to_le_bytes() // [0x1E, 0x00]
        );
    }

    #[test]
    fn pool_datum_canonical_rejects_wrong_length() {
        let datum = make_pool_datum();
        let mut bytes = datum.to_canonical_bytes();
        bytes.pop();
        assert!(matches!(
            PoolDatum::from_canonical_bytes(&bytes),
            Err(AmmError::DatumError(_))
        ));
        bytes.extend_from_slice(&[0, 0]); // now 91 bytes
        assert!(matches!(
            PoolDatum::from_canonical_bytes(&bytes),
            Err(AmmError::DatumError(_))
        ));
    }

    #[test]
    fn compute_swap_basic() {
        let (out, fee) = compute_swap_output(10_000, 10_000, 1_000, 30).unwrap();
        // fee = 1000 * 30 / 10000 = 3
        // amount_in_net = 997
        // amount_out = 10000 * 997 / (10000 + 997) = 906
        assert_eq!(fee, 3);
        assert_eq!(out, 906);
    }

    #[test]
    fn compute_swap_zero_reserves() {
        assert!(compute_swap_output(0, 1000, 100, 30).is_none());
        assert!(compute_swap_output(1000, 0, 100, 30).is_none());
        assert!(compute_swap_output(1000, 1000, 0, 30).is_none());
    }

    #[test]
    fn compute_swap_no_fee() {
        let (out, fee) = compute_swap_output(1000, 2000, 100, 0).unwrap();
        // fee = 0
        // out = 2000 * 100 / (1000 + 100) = 181
        assert_eq!(fee, 0);
        assert_eq!(out, 181);
    }

    #[test]
    fn compute_swap_high_fee() {
        let (out, fee) = compute_swap_output(10_000, 10_000, 1_000, 500).unwrap();
        // fee = 1000 * 500 / 10000 = 50
        // amount_in_net = 950
        // out = 10000 * 950 / (10000 + 950) = 867
        assert_eq!(fee, 50);
        assert_eq!(out, 867);
    }

    #[test]
    fn pool_state_apply_swap_atob() {
        let mut pool = PoolState::new(Hash256::from_bytes([0xAA; 32]), make_pool_datum());
        let old_inv = pool.datum.invariant();

        pool.apply_swap(SwapDirection::AtoB, 1000, 900).unwrap();

        assert_eq!(pool.datum.reserve_a, 11_000);
        assert_eq!(pool.datum.reserve_b, 9_100);

        let new_inv = pool.datum.invariant();
        assert!(new_inv >= old_inv, "Invariant should be preserved");
    }

    #[test]
    fn pool_state_apply_swap_btoa() {
        let mut pool = PoolState::new(Hash256::from_bytes([0xBB; 32]), make_pool_datum());
        pool.apply_swap(SwapDirection::BtoA, 500, 450).unwrap();

        assert_eq!(pool.datum.reserve_b, 10_500);
        assert_eq!(pool.datum.reserve_a, 9_550);
    }

    #[test]
    fn compute_add_liquidity_empty_pool() {
        let (lp_issued, new_datum) = compute_add_liquidity(0, 0, 0, 1000, 1000).unwrap();
        // sqrt(1000 * 1000) = 1000
        assert_eq!(lp_issued, 1000);
        assert_eq!(new_datum.reserve_a, 1000);
        assert_eq!(new_datum.reserve_b, 1000);
        assert_eq!(new_datum.lp_total, 1000);
    }

    #[test]
    fn compute_add_liquidity_balanced() {
        let (lp_issued, new_datum) =
            compute_add_liquidity(10_000, 10_000, 10_000, 1_000, 1_000).unwrap();
        // share_a = 10000 * 1000 / 10000 = 1000
        // share_b = 10000 * 1000 / 10000 = 1000
        // min = 1000
        assert_eq!(lp_issued, 1000);
        assert_eq!(new_datum.reserve_a, 11_000);
        assert_eq!(new_datum.reserve_b, 11_000);
        assert_eq!(new_datum.lp_total, 11_000);
    }

    #[test]
    fn compute_add_liquidity_unbalanced() {
        let (lp_issued, _new_datum) =
            compute_add_liquidity(10_000, 10_000, 10_000, 2_000, 1_000).unwrap();
        // share_a = 10000 * 2000 / 10000 = 2000
        // share_b = 10000 * 1000 / 10000 = 1000
        // min = 1000
        assert_eq!(lp_issued, 1000);
    }

    #[test]
    fn compute_remove_liquidity_half() {
        let (amount_a, amount_b, new_datum) =
            compute_remove_liquidity(10_000, 10_000, 10_000, 5_000).unwrap();
        // amount_a = 10000 * 5000 / 10000 = 5000
        // amount_b = 10000 * 5000 / 10000 = 5000
        assert_eq!(amount_a, 5000);
        assert_eq!(amount_b, 5000);
        assert_eq!(new_datum.reserve_a, 5000);
        assert_eq!(new_datum.reserve_b, 5000);
        assert_eq!(new_datum.lp_total, 5000);
    }

    #[test]
    fn compute_remove_liquidity_quarter() {
        let (amount_a, amount_b, _new_datum) =
            compute_remove_liquidity(10_000, 20_000, 10_000, 2_500).unwrap();
        // amount_a = 10000 * 2500 / 10000 = 2500
        // amount_b = 20000 * 2500 / 10000 = 5000
        assert_eq!(amount_a, 2500);
        assert_eq!(amount_b, 5000);
    }

    #[test]
    fn sqrt_examples() {
        assert_eq!(sqrt_u128(0), 0);
        assert_eq!(sqrt_u128(1), 1);
        assert_eq!(sqrt_u128(4), 2);
        assert_eq!(sqrt_u128(9), 3);
        assert_eq!(sqrt_u128(16), 4);
        assert_eq!(sqrt_u128(1_000_000), 1000);
    }

    #[test]
    fn pool_state_new() {
        let pool_id = Hash256::from_bytes([0xCC; 32]);
        let datum = make_pool_datum();
        let pool = PoolState::new(pool_id, datum.clone());

        assert_eq!(pool.pool_id, pool_id);
        assert_eq!(pool.datum, datum);
    }

    #[test]
    fn compute_swap_overflow_protection() {
        // Large values that could overflow in intermediate computation
        let result = compute_swap_output(u64::MAX, u64::MAX, u64::MAX / 2, 30);
        // May return None due to overflow checks
        // The important thing is it doesn't panic
        let _ = result;
    }

    #[test]
    fn compute_add_liquidity_zero_amount() {
        let result = compute_add_liquidity(1000, 1000, 1000, 0, 100);
        assert!(result.is_err());

        let result = compute_add_liquidity(1000, 1000, 1000, 100, 0);
        assert!(result.is_err());
    }

    #[test]
    fn compute_remove_liquidity_too_much() {
        let result = compute_remove_liquidity(1000, 1000, 1000, 1001);
        assert!(result.is_err());
    }

    #[test]
    fn compute_remove_liquidity_zero_total() {
        let result = compute_remove_liquidity(1000, 1000, 0, 100);
        assert!(result.is_err());
    }
}
