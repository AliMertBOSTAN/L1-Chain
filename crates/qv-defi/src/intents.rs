//! Intent-based order encoding for DeFi transactions.
//!
//! In QuantumVault's DeFi model, users submit intent orders that are batched
//! and executed deterministically by the slot leader (batcher). This module
//! provides the types and codecs for intent encoding.
//!
//! - **OrderIntent**: The high-level order specification.
//! - **IntentBundle**: Wraps multiple intents for batch execution.
//! - **Wallet SDK**: Helper to build intents from user actions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use qv_core::{Amount, Hash256, TxId};

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur with intents.
#[derive(Debug, Clone, Error)]
pub enum IntentError {
    /// Order is expired (deadline < current slot).
    #[error("order expired at slot {deadline}")]
    Expired { deadline: u64 },

    /// Invalid amount (zero or overflow).
    #[error("invalid amount: {0}")]
    InvalidAmount(u64),

    /// Encoding/decoding failure.
    #[error("encoding failed: {0}")]
    InvalidEncoding(String),

    /// Decoding failed.
    #[error("decoding failed: {0}")]
    DecodeFailed(String),
}

pub type Result<T> = core::result::Result<T, IntentError>;

// ============================================================================
// OrderKind
// ============================================================================

/// The type of operation requested in an intent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderKind {
    /// Swap exact input for variable output.
    Swap {
        /// Pool identifier.
        pool_id: Hash256,

        /// Swap direction.
        side: SwapDirection,

        /// Amount of token to offer.
        offer: u64,

        /// Minimum amount to receive (slippage protection).
        min_receive: u64,
    },

    /// Limit order (price-based).
    LimitOrder {
        /// Pool identifier.
        pool_id: Hash256,

        /// Side (A->B or B->A).
        side: SwapDirection,

        /// Limit price in Q64.
        limit_price_q64: u128,

        /// Size to sell.
        size: u64,
    },

    /// Add liquidity to AMM pool.
    LiquidityAdd {
        /// Pool identifier.
        pool_id: Hash256,

        /// Amount of token A to deposit.
        deposit_a: u64,

        /// Amount of token B to deposit.
        deposit_b: u64,

        /// Minimum LP tokens to receive.
        min_lp: u64,
    },

    /// Remove liquidity from pool.
    LiquidityRemove {
        /// Pool identifier.
        pool_id: Hash256,

        /// LP tokens to burn.
        lp_burn: u64,

        /// Minimum token A to receive.
        min_a: u64,

        /// Minimum token B to receive.
        min_b: u64,
    },

    /// Deposit into lending pool.
    LendingDeposit {
        /// Pool identifier.
        pool_id: Hash256,

        /// Collateral amount.
        amount: u64,
    },

    /// Borrow from lending pool.
    LendingBorrow {
        /// Pool identifier.
        pool_id: Hash256,

        /// Debt amount.
        amount: u64,
    },

    /// Repay lending debt.
    LendingRepay {
        /// Pool identifier.
        pool_id: Hash256,

        /// Repayment amount.
        amount: u64,
    },

    /// Withdraw collateral from lending.
    LendingWithdraw {
        /// Pool identifier.
        pool_id: Hash256,

        /// Collateral amount.
        amount: u64,
    },
}

// ============================================================================
// SwapDirection
// ============================================================================

/// Direction of a swap operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SwapDirection {
    /// Swap from token A to token B.
    AtoB,

    /// Swap from token B to token A.
    BtoA,
}

// ============================================================================
// OrderIntent
// ============================================================================

/// A user intent to execute a DeFi operation.
///
/// The intent is submitted to the mempool (encrypted), batched by the slot leader,
/// and executed against the current pool state. This design ensures fairness and
/// MEV-resistance via threshold decryption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderIntent {
    /// Order type and parameters.
    pub kind: OrderKind,

    /// Deadline slot (order must be executed before this).
    pub deadline_slot: u64,

    /// Owner's stealth address public key (optional).
    pub owner_stealth_pk: Option<Vec<u8>>,

    /// Nonce for replay protection.
    pub nonce: u64,

    /// Offer amount (for simple swaps).
    pub offer_amount: Amount,

    /// Minimum receive amount.
    pub min_receive: Amount,
}

impl OrderIntent {
    /// Create a new swap intent.
    #[must_use]
    pub fn new_swap(
        _order_id: TxId,
        pool_id: Hash256,
        offer_amount: Amount,
        min_receive: Amount,
        _slippage_bps: u16,
        deadline_slot: u64,
    ) -> Self {
        Self {
            kind: OrderKind::Swap {
                pool_id,
                side: SwapDirection::AtoB,
                offer: offer_amount.0,
                min_receive: min_receive.0,
            },
            deadline_slot,
            owner_stealth_pk: None,
            nonce: 0,
            offer_amount,
            min_receive,
        }
    }

    /// Add stealth address info.
    #[must_use]
    pub fn with_stealth(mut self, pk_bytes: Vec<u8>) -> Self {
        self.owner_stealth_pk = Some(pk_bytes);
        self
    }

    /// Validate the intent.
    pub fn validate(&self, current_slot: u64) -> Result<()> {
        if self.deadline_slot <= current_slot {
            return Err(IntentError::Expired {
                deadline: self.deadline_slot,
            });
        }

        match &self.kind {
            OrderKind::Swap {
                offer,
                min_receive,
                ..
            } => {
                if *offer == 0 || *min_receive == 0 {
                    return Err(IntentError::InvalidAmount(0));
                }
            }
            OrderKind::LimitOrder { size, .. } => {
                if *size == 0 {
                    return Err(IntentError::InvalidAmount(0));
                }
            }
            OrderKind::LiquidityAdd {
                deposit_a,
                deposit_b,
                ..
            } => {
                if *deposit_a == 0 || *deposit_b == 0 {
                    return Err(IntentError::InvalidAmount(0));
                }
            }
            OrderKind::LiquidityRemove { lp_burn, .. } => {
                if *lp_burn == 0 {
                    return Err(IntentError::InvalidAmount(0));
                }
            }
            OrderKind::LendingDeposit { amount, .. }
            | OrderKind::LendingBorrow { amount, .. }
            | OrderKind::LendingRepay { amount, .. }
            | OrderKind::LendingWithdraw { amount, .. } => {
                if *amount == 0 {
                    return Err(IntentError::InvalidAmount(0));
                }
            }
        }

        Ok(())
    }

    /// Encode intent to bincode bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| IntentError::InvalidEncoding(e.to_string()))
    }

    /// Decode intent from bincode bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| IntentError::DecodeFailed(e.to_string()))
    }
}

// ============================================================================
// IntentBundle
// ============================================================================

/// A batch of intents to be executed together.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentBundle {
    /// Batch identifier.
    pub batch_id: Hash256,

    /// Slot in which the batch will be executed.
    pub batch_slot: u64,

    /// Orders in the batch.
    pub intents: Vec<OrderIntent>,
}

impl IntentBundle {
    /// Create a new intent bundle.
    #[must_use]
    pub fn new(batch_id: Hash256, batch_slot: u64) -> Self {
        Self {
            batch_id,
            batch_slot,
            intents: Vec::new(),
        }
    }

    /// Add an order to the bundle.
    pub fn add_order(&mut self, intent: OrderIntent) {
        self.intents.push(intent);
    }

    /// Validate all intents in the bundle.
    pub fn validate(&self, current_slot: u64) -> Result<()> {
        for intent in &self.intents {
            intent.validate(current_slot)?;
        }
        Ok(())
    }

    /// Get orders targeting a specific pool.
    #[must_use]
    pub fn orders_for_pool(&self, pool_id: Hash256) -> Vec<&OrderIntent> {
        self.intents
            .iter()
            .filter(|intent| {
                let order_pool = match &intent.kind {
                    OrderKind::Swap { pool_id: p, .. }
                    | OrderKind::LimitOrder { pool_id: p, .. }
                    | OrderKind::LiquidityAdd { pool_id: p, .. }
                    | OrderKind::LiquidityRemove { pool_id: p, .. }
                    | OrderKind::LendingDeposit { pool_id: p, .. }
                    | OrderKind::LendingBorrow { pool_id: p, .. }
                    | OrderKind::LendingRepay { pool_id: p, .. }
                    | OrderKind::LendingWithdraw { pool_id: p, .. } => *p,
                };
                order_pool == pool_id
            })
            .collect()
    }

    /// Encode bundle to bincode bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| IntentError::InvalidEncoding(e.to_string()))
    }

    /// Decode bundle from bincode bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| IntentError::DecodeFailed(e.to_string()))
    }
}

// ============================================================================
// SwapIntentBuilder
// ============================================================================

/// Fluent builder for swap intents.
#[derive(Clone, Debug, Default)]
pub struct SwapIntentBuilder {
    order_id: Option<TxId>,
    pool_id: Option<Hash256>,
    offer_amount: Option<Amount>,
    max_slippage_bps: Option<u16>,
    deadline_slot: Option<u64>,
    owner_pk_hash: Option<[u8; 32]>,
    nonce: u64,
}

impl SwapIntentBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the order ID.
    #[must_use]
    pub fn order_id(mut self, id: TxId) -> Self {
        self.order_id = Some(id);
        self
    }

    /// Set the pool ID.
    #[must_use]
    pub fn pool_id(mut self, id: Hash256) -> Self {
        self.pool_id = Some(id);
        self
    }

    /// Set the offer amount.
    #[must_use]
    pub fn offer_amount(mut self, amount: Amount) -> Self {
        self.offer_amount = Some(amount);
        self
    }

    /// Set maximum slippage in basis points.
    #[must_use]
    pub fn max_slippage_bps(mut self, bps: u16) -> Self {
        self.max_slippage_bps = Some(bps);
        self
    }

    /// Set deadline slot.
    #[must_use]
    pub fn deadline_slot(mut self, slot: u64) -> Self {
        self.deadline_slot = Some(slot);
        self
    }

    /// Set owner's stealth public key hash.
    #[must_use]
    pub fn owner_pk_hash(mut self, hash: [u8; 32]) -> Self {
        self.owner_pk_hash = Some(hash);
        self
    }

    /// Build the intent.
    pub fn build(self) -> Result<OrderIntent> {
        let offer = self.offer_amount.ok_or(IntentError::InvalidAmount(0))?;
        let pool_id = self.pool_id.ok_or(IntentError::InvalidAmount(0))?;
        let deadline = self.deadline_slot.ok_or(IntentError::InvalidAmount(0))?;

        let slippage_bps = self.max_slippage_bps.unwrap_or(100); // 1% default
        let min_receive_amount = (offer.0 as u128)
            .saturating_mul(10_000_u128 - slippage_bps as u128)
            .saturating_div(10_000);

        let min_receive = Amount::from_smallest_units(
            core::cmp::min(min_receive_amount, u64::MAX as u128) as u64
        );

        let mut intent = OrderIntent::new_swap(
            self.order_id.unwrap_or(TxId::ZERO),
            pool_id,
            offer,
            min_receive,
            slippage_bps,
            deadline,
        );

        if let Some(hash) = self.owner_pk_hash {
            intent.owner_stealth_pk = Some(hash.to_vec());
        }

        intent.nonce = self.nonce;
        Ok(intent)
    }
}

/// Convenience function to build a swap intent.
pub fn build_swap_intent(
    pool_id: Hash256,
    side: SwapDirection,
    offer: u64,
    min_receive: u64,
    deadline_slot: u64,
    owner_pk_hash: [u8; 32],
    nonce: u64,
) -> Result<OrderIntent> {
    if offer == 0 || min_receive == 0 {
        return Err(IntentError::InvalidAmount(0));
    }

    let intent = OrderIntent {
        kind: OrderKind::Swap {
            pool_id,
            side,
            offer,
            min_receive,
        },
        deadline_slot,
        owner_stealth_pk: Some(owner_pk_hash.to_vec()),
        nonce,
        offer_amount: Amount::from_smallest_units(offer),
        min_receive: Amount::from_smallest_units(min_receive),
    };

    intent.validate(0)?; // Basic validation (non-expiry)
    Ok(intent)
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

    #[test]
    fn swap_intent_new() {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        assert_eq!(intent.offer_amount.0, 1000);
        assert_eq!(intent.min_receive.0, 900);
    }

    #[test]
    fn intent_validate_ok() {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        assert!(intent.validate(500).is_ok());
    }

    #[test]
    fn intent_validate_expired() {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        assert!(intent.validate(1001).is_err());
    }

    #[test]
    fn intent_validate_zero_amount() {
        let intent = OrderIntent {
            kind: OrderKind::Swap {
                pool_id: Hash256::from_bytes([2; 32]),
                side: SwapDirection::AtoB,
                offer: 0,
                min_receive: 100,
            },
            deadline_slot: 1000,
            owner_stealth_pk: None,
            nonce: 0,
            offer_amount: Amount::from_smallest_units(0),
            min_receive: Amount::from_smallest_units(100),
        };
        assert!(intent.validate(500).is_err());
    }

    #[test]
    fn intent_with_stealth() {
        let mut intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        let stealth_bytes = vec![0xFF; 32];
        intent = intent.with_stealth(stealth_bytes.clone());
        assert_eq!(intent.owner_stealth_pk_hash.as_ref(), stealth_bytes.as_slice());
    }

    #[test]
    fn bundle_new() {
        let bundle = IntentBundle::new(Hash256::from_bytes([1; 32]), 500);
        assert_eq!(bundle.intents.len(), 0);
    }

    #[test]
    fn bundle_add_order() {
        let mut bundle = IntentBundle::new(Hash256::from_bytes([1; 32]), 500);
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([2; 32]),
            Hash256::from_bytes([3; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        bundle.add_order(intent);
        assert_eq!(bundle.intents.len(), 1);
    }

    #[test]
    fn bundle_validate_all_ok() {
        let mut bundle = IntentBundle::new(Hash256::from_bytes([1; 32]), 500);
        for i in 0..3 {
            let intent = OrderIntent::new_swap(
                TxId::from_bytes([i as u8; 32]),
                Hash256::from_bytes([3; 32]),
                Amount::from_smallest_units(1000),
                Amount::from_smallest_units(900),
                50,
                1000,
            );
            bundle.add_order(intent);
        }
        assert!(bundle.validate(600).is_ok());
    }

    #[test]
    fn bundle_validate_one_expired() {
        let mut bundle = IntentBundle::new(Hash256::from_bytes([1; 32]), 500);
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([3; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            500, // deadline == batch_slot
        );
        bundle.add_order(intent);
        assert!(bundle.validate(600).is_err());
    }

    #[test]
    fn bundle_orders_for_pool() {
        let pool_1 = Hash256::from_bytes([1; 32]);
        let pool_2 = Hash256::from_bytes([2; 32]);

        let mut bundle = IntentBundle::new(Hash256::from_bytes([0xAA; 32]), 500);

        for i in 0..3 {
            let intent = OrderIntent::new_swap(
                TxId::from_bytes([i as u8; 32]),
                if i % 2 == 0 { pool_1 } else { pool_2 },
                Amount::from_smallest_units(1000),
                Amount::from_smallest_units(900),
                50,
                1000,
            );
            bundle.add_order(intent);
        }

        let pool_1_orders = bundle.orders_for_pool(pool_1);
        assert_eq!(pool_1_orders.len(), 2); // i=0, 2

        let pool_2_orders = bundle.orders_for_pool(pool_2);
        assert_eq!(pool_2_orders.len(), 1); // i=1
    }

    #[test]
    fn builder_swap_intent() {
        let intent = SwapIntentBuilder::new()
            .order_id(TxId::from_bytes([1; 32]))
            .pool_id(Hash256::from_bytes([2; 32]))
            .offer_amount(Amount::from_smallest_units(1000))
            .max_slippage_bps(100)
            .deadline_slot(1000)
            .build()
            .unwrap();

        assert_eq!(intent.offer_amount.0, 1000);
        // min_receive = 1000 - (1000 * 100 / 10000) = 1000 - 10 = 990
        assert_eq!(intent.min_receive.0, 990);
    }

    #[test]
    fn builder_missing_required() {
        let result = SwapIntentBuilder::new()
            .order_id(TxId::from_bytes([1; 32]))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn roundtrip_intent() {
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        let bytes = intent.to_bytes().unwrap();
        let decoded = OrderIntent::from_bytes(&bytes).unwrap();
        assert_eq!(intent, decoded);
    }

    #[test]
    fn roundtrip_bundle() {
        let mut bundle = IntentBundle::new(Hash256::from_bytes([0xAA; 32]), 1000);
        let intent = OrderIntent::new_swap(
            TxId::from_bytes([1; 32]),
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(1000),
            Amount::from_smallest_units(900),
            50,
            1000,
        );
        bundle.add_order(intent);
        let bytes = bundle.to_bytes().unwrap();
        let decoded = IntentBundle::from_bytes(&bytes).unwrap();
        assert_eq!(bundle, decoded);
    }

    #[test]
    fn build_swap_intent_helper() {
        let intent = build_swap_intent(
            Hash256::from_bytes([1; 32]),
            SwapDirection::AtoB,
            1000,
            800,
            1000,
            [0u8; 32],
            0,
        )
        .unwrap();

        assert_eq!(intent.offer_amount.0, 1000);
        assert_eq!(intent.min_receive.0, 800);
    }

    #[test]
    fn build_swap_intent_zero_amount() {
        let result = build_swap_intent(
            Hash256::from_bytes([1; 32]),
            SwapDirection::AtoB,
            0,
            800,
            1000,
            [0u8; 32],
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn lending_deposit_kind() {
        let intent = OrderIntent {
            kind: OrderKind::LendingDeposit {
                pool_id: Hash256::from_bytes([1; 32]),
                amount: 1000,
            },
            deadline_slot: 1000,
            owner_stealth_pk: None,
            nonce: 0,
            offer_amount: Amount::from_smallest_units(1000),
            min_receive: Amount::from_smallest_units(0),
        };
        assert!(intent.validate(500).is_ok());
    }

    #[test]
    fn liquidity_add_kind() {
        let intent = OrderIntent {
            kind: OrderKind::LiquidityAdd {
                pool_id: Hash256::from_bytes([1; 32]),
                deposit_a: 1000,
                deposit_b: 2000,
                min_lp: 1500,
            },
            deadline_slot: 1000,
            owner_stealth_pk: None,
            nonce: 0,
            offer_amount: Amount::from_smallest_units(1000),
            min_receive: Amount::from_smallest_units(1500),
        };
        assert!(intent.validate(500).is_ok());
    }
}
