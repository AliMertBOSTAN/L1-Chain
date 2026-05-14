//! Batcher logic: order decoding, AMM batch execution, slashing evidence.
//!
//! In QuantumVault's DeFi model (ADR-002, ADR-003), users submit "order"
//! transactions that express **intent** (e.g. "swap X token-A for ≥Y token-B").
//! The slot leader — acting as the batcher — deterministically matches and
//! executes orders against shared UTXO pools (AMM, lending).
//!
//! If the batcher misordering is detected (the included order differs from
//! the canonical deterministic ordering), a [`SlashingEvidence`] record is
//! produced that can be submitted on-chain to slash the misbehaving leader.

use std::collections::BTreeMap;

use qv_core::{Amount, Hash256, TxId};
use serde::{Deserialize, Serialize};

use crate::ordering::{self, OrderKey};
use crate::MempoolError;

// ---------------------------------------------------------------------------
// Order intent
// ---------------------------------------------------------------------------

/// The direction of a swap order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwapDirection {
    /// Sell token A for token B.
    AtoB,
    /// Sell token B for token A.
    BtoA,
}

/// A decoded order intent extracted from a transaction's datum.
///
/// In the Shared UTXO / eUTXO model, orders are encoded as datum fields on
/// special "order UTXOs".  The batcher extracts this struct from the datum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderIntent {
    /// Transaction id of the order UTXO.
    pub order_tx_id: TxId,
    /// Target AMM pool identifier (hash of the pool script + datum).
    pub pool_id: Hash256,
    /// Swap direction.
    pub direction: SwapDirection,
    /// Amount the user is offering.
    pub offer_amount: Amount,
    /// Minimum amount the user accepts in return.
    pub min_receive: Amount,
    /// Fee density of the order transaction (for ordering).
    pub fee_density: u64,
    /// Observation timestamp in milliseconds.
    pub timestamp_ms: u64,
}

impl OrderIntent {
    /// Produce an [`OrderKey`] for deterministic sorting.
    #[must_use]
    pub fn order_key(&self) -> OrderKey {
        OrderKey::new(self.fee_density, self.timestamp_ms, self.order_tx_id)
    }
}

// ---------------------------------------------------------------------------
// AMM batch builder
// ---------------------------------------------------------------------------

/// Result of matching a set of orders against an AMM pool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchResult {
    /// Pool identifier.
    pub pool_id: Hash256,
    /// Orders included in this batch, in canonical order.
    pub matched_orders: Vec<TxId>,
    /// New pool reserve A after the batch.
    pub new_reserve_a: Amount,
    /// New pool reserve B after the batch.
    pub new_reserve_b: Amount,
    /// Total fees collected by the batcher.
    pub total_fees: Amount,
}

/// A snapshot of an AMM pool's reserves, used as input to the batch builder.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PoolState {
    /// Pool identifier.
    pub pool_id: Hash256,
    /// Current reserve of token A.
    pub reserve_a: Amount,
    /// Current reserve of token B.
    pub reserve_b: Amount,
}

/// Build a deterministic batch of swap orders against one AMM pool.
///
/// Orders are sorted canonically, then applied sequentially using the
/// constant-product invariant (`x · y ≥ k`).  Orders that cannot be
/// satisfied (output below `min_receive`) are **skipped**, not rejected,
/// so the rest of the batch can proceed.
pub fn build_amm_batch(
    pool: &PoolState,
    orders: &mut [OrderIntent],
) -> Result<BatchResult, MempoolError> {
    // Sort orders deterministically
    let mut keys: Vec<OrderKey> = orders.iter().map(|o| o.order_key()).collect();
    ordering::deterministic_sort(&mut keys);

    // Build a lookup from tx_id → order
    let order_map: BTreeMap<TxId, &OrderIntent> =
        orders.iter().map(|o| (o.order_tx_id, o)).collect();

    let mut reserve_a = pool.reserve_a.0;
    let mut reserve_b = pool.reserve_b.0;
    let mut matched = Vec::new();
    let mut total_fees: u64 = 0;

    for key in &keys {
        let Some(order) = order_map.get(&key.tx_id()) else {
            continue;
        };

        if order.pool_id != pool.pool_id {
            continue; // wrong pool
        }

        let result = match order.direction {
            SwapDirection::AtoB => {
                // User offers A, wants at least min_receive of B.
                compute_swap_output(reserve_a, reserve_b, order.offer_amount.0)
            }
            SwapDirection::BtoA => {
                // User offers B, wants at least min_receive of A.
                compute_swap_output(reserve_b, reserve_a, order.offer_amount.0)
            }
        };

        let Some((output, fee)) = result else {
            // Overflow or zero liquidity — skip
            continue;
        };

        if output < order.min_receive.0 {
            // Slippage exceeded — skip
            continue;
        }

        // Apply to reserves
        match order.direction {
            SwapDirection::AtoB => {
                reserve_a = reserve_a.saturating_add(order.offer_amount.0);
                reserve_b = reserve_b.saturating_sub(output);
            }
            SwapDirection::BtoA => {
                reserve_b = reserve_b.saturating_add(order.offer_amount.0);
                reserve_a = reserve_a.saturating_sub(output);
            }
        }

        total_fees = total_fees.saturating_add(fee);
        matched.push(order.order_tx_id);
    }

    Ok(BatchResult {
        pool_id: pool.pool_id,
        matched_orders: matched,
        new_reserve_a: Amount::from_smallest_units(reserve_a),
        new_reserve_b: Amount::from_smallest_units(reserve_b),
        total_fees: Amount::from_smallest_units(total_fees),
    })
}

/// Constant-product swap: given reserves (in, out) and input amount `dx`,
/// compute the output `dy` such that `(in + dx) * (out - dy) >= in * out`.
///
/// Uses the standard formula: `dy = out * dx / (in + dx)`.
/// A 0.3% fee is applied: `dx_net = dx * 997 / 1000`.
///
/// Returns `(output_amount, fee_amount)` or `None` on overflow/zero.
fn compute_swap_output(reserve_in: u64, reserve_out: u64, dx: u64) -> Option<(u64, u64)> {
    if reserve_in == 0 || reserve_out == 0 || dx == 0 {
        return None;
    }

    // Fee: 0.3%
    let dx_fee = dx.checked_mul(3)?.checked_div(1000)?;
    let dx_net = dx.checked_sub(dx_fee)?;

    // dy = reserve_out * dx_net / (reserve_in + dx_net)
    let numerator = (reserve_out as u128).checked_mul(dx_net as u128)?;
    let denominator = (reserve_in as u128).checked_add(dx_net as u128)?;

    if denominator == 0 {
        return None;
    }

    let dy = numerator.checked_div(denominator)?;
    if dy > u64::MAX as u128 {
        return None;
    }

    Some((dy as u64, dx_fee))
}

// ---------------------------------------------------------------------------
// Slashing evidence
// ---------------------------------------------------------------------------

/// Evidence that a batcher mis-ordered transactions.
///
/// A verifier who detects that the block's transaction order differs from the
/// canonical deterministic order can construct this evidence and submit it
/// on-chain to slash the misbehaving slot leader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashingEvidence {
    /// The block hash where the misordering was detected.
    pub block_hash: qv_core::BlockHash,
    /// The slot number.
    pub slot: u64,
    /// The canonical ordering (as computed by the verifier).
    pub canonical_order: Vec<TxId>,
    /// The ordering found in the block.
    pub actual_order: Vec<TxId>,
    /// The producer key hash (who should be slashed).
    pub producer_key_hash: Hash256,
}

impl SlashingEvidence {
    /// Check whether this evidence is valid (canonical ≠ actual).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.canonical_order != self.actual_order
            && !self.canonical_order.is_empty()
            && self.canonical_order.len() == self.actual_order.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::{Amount, BlockHash, Hash256, TxId};

    use super::*;

    fn make_order(
        marker: u8,
        pool: Hash256,
        dir: SwapDirection,
        offer: u64,
        min_recv: u64,
        fee_density: u64,
        ts: u64,
    ) -> OrderIntent {
        OrderIntent {
            order_tx_id: TxId::from_bytes([marker; 32]),
            pool_id: pool,
            direction: dir,
            offer_amount: Amount::from_smallest_units(offer),
            min_receive: Amount::from_smallest_units(min_recv),
            fee_density,
            timestamp_ms: ts,
        }
    }

    #[test]
    fn compute_swap_basic() {
        // Pool: 1000 A, 2000 B.  User offers 100 A.
        let (dy, fee) = compute_swap_output(1000, 2000, 100).unwrap();
        // dx_net = 100 - 0 (fee=0.3 rounds to 0 for small amounts) ... actually 3*100/1000=0
        // fee = 100*3/1000 = 0 (integer div)
        // So dx_net = 100, dy = 2000*100/(1000+100) = 200000/1100 = 181
        assert_eq!(fee, 0);
        assert_eq!(dy, 181);
    }

    #[test]
    fn compute_swap_with_fee() {
        // Pool: 10000 A, 10000 B.  User offers 1000 A.
        let (dy, fee) = compute_swap_output(10000, 10000, 1000).unwrap();
        // fee = 1000*3/1000 = 3
        // dx_net = 997
        // dy = 10000*997/(10000+997) = 9970000/10997 = 906
        assert_eq!(fee, 3);
        assert_eq!(dy, 906);
    }

    #[test]
    fn compute_swap_zero_cases() {
        assert!(compute_swap_output(0, 1000, 100).is_none());
        assert!(compute_swap_output(1000, 0, 100).is_none());
        assert!(compute_swap_output(1000, 1000, 0).is_none());
    }

    #[test]
    fn build_amm_batch_basic() {
        let pool_id = Hash256::from_bytes([0xAA; 32]);
        let pool = PoolState {
            pool_id,
            reserve_a: Amount::from_smallest_units(10_000),
            reserve_b: Amount::from_smallest_units(10_000),
        };

        let mut orders = vec![
            make_order(1, pool_id, SwapDirection::AtoB, 500, 1, 100, 1000),
            make_order(2, pool_id, SwapDirection::AtoB, 300, 1, 200, 2000),
        ];

        let result = build_amm_batch(&pool, &mut orders).unwrap();
        assert_eq!(result.matched_orders.len(), 2);
        assert_eq!(result.pool_id, pool_id);
        // Reserves should have changed
        assert!(result.new_reserve_a.0 > 10_000);
        assert!(result.new_reserve_b.0 < 10_000);
    }

    #[test]
    fn build_amm_batch_slippage_skip() {
        let pool_id = Hash256::from_bytes([0xBB; 32]);
        let pool = PoolState {
            pool_id,
            reserve_a: Amount::from_smallest_units(1000),
            reserve_b: Amount::from_smallest_units(1000),
        };

        // Unreasonable min_receive — should be skipped
        let mut orders = vec![make_order(
            3,
            pool_id,
            SwapDirection::AtoB,
            100,
            999, // wants 999 B from a 1000 B pool with 100 A input — impossible
            100,
            1000,
        )];

        let result = build_amm_batch(&pool, &mut orders).unwrap();
        assert!(result.matched_orders.is_empty());
    }

    #[test]
    fn slashing_evidence_validity() {
        let valid = SlashingEvidence {
            block_hash: BlockHash::from_bytes([1; 32]),
            slot: 42,
            canonical_order: vec![TxId::from_bytes([1; 32]), TxId::from_bytes([2; 32])],
            actual_order: vec![TxId::from_bytes([2; 32]), TxId::from_bytes([1; 32])],
            producer_key_hash: Hash256::from_bytes([0xAA; 32]),
        };
        assert!(valid.is_valid());

        let invalid = SlashingEvidence {
            canonical_order: vec![TxId::from_bytes([1; 32])],
            actual_order: vec![TxId::from_bytes([1; 32])],
            ..valid.clone()
        };
        assert!(!invalid.is_valid()); // same order = not evidence

        let empty = SlashingEvidence {
            canonical_order: vec![],
            actual_order: vec![],
            ..valid
        };
        assert!(!empty.is_valid()); // empty = not evidence
    }

    #[test]
    fn build_amm_batch_deterministic_order() {
        let pool_id = Hash256::from_bytes([0xCC; 32]);
        let pool = PoolState {
            pool_id,
            reserve_a: Amount::from_smallest_units(100_000),
            reserve_b: Amount::from_smallest_units(100_000),
        };

        // Two orders with different fee densities
        let mut orders = vec![
            make_order(10, pool_id, SwapDirection::AtoB, 100, 1, 50, 1000), // low fee
            make_order(11, pool_id, SwapDirection::AtoB, 100, 1, 200, 2000), // high fee
        ];

        let result = build_amm_batch(&pool, &mut orders).unwrap();
        assert_eq!(result.matched_orders.len(), 2);
        // Higher fee should be first
        assert_eq!(result.matched_orders[0], TxId::from_bytes([11; 32]));
        assert_eq!(result.matched_orders[1], TxId::from_bytes([10; 32]));
    }
}
