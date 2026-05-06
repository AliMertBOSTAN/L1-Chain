//! Block reward computation, fee distribution, and halving schedule.
//!
//! # Reward model
//!
//! Each block's total reward consists of two parts:
//!
//! 1. **Block subsidy** — newly minted tokens, following a Bitcoin-style
//!    halving schedule (initial reward cut in half every N blocks).
//! 2. **Transaction fees** — the sum of fees from all transactions in the
//!    block (fee = sum(inputs) − sum(outputs)).
//!
//! # Distribution
//!
//! The total reward is split between the pool operator and the delegators:
//!
//! 1. The operator takes a **fixed cost** off the top (minimum operating
//!    expense).
//! 2. From the remainder, the operator takes a **margin** (percentage).
//! 3. The rest is distributed to delegators **pro-rata** based on their
//!    delegated stake.
//!
//! # Supply cap
//!
//! The total minted supply must never exceed [`MonetaryParams::total_supply`].
//! Once cumulative emissions reach the cap, the block subsidy becomes zero
//! and only fees remain as incentive.

use qv_core::{Amount, Height, MonetaryParams};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::stake::{PoolId, StakePool};

// ============================================================================
// Errors
// ============================================================================

/// Errors from reward computation.
#[derive(Debug, Error)]
pub enum RewardError {
    /// Arithmetic overflow during reward calculation.
    #[error("reward arithmetic overflow")]
    Overflow,
    /// Pool not found in the distribution.
    #[error("pool {0:?} not found")]
    PoolNotFound(PoolId),
}

// ============================================================================
// Block subsidy (halving)
// ============================================================================

/// Compute the block subsidy for a given height according to the halving
/// schedule.
///
/// The subsidy halves every `halving_interval_blocks` blocks:
///
/// ```text
/// subsidy(h) = initial_reward >> (h / halving_interval)
/// ```
///
/// Once the halving has shifted the reward to zero, the subsidy is zero.
#[must_use]
pub fn block_subsidy(height: Height, monetary: &MonetaryParams) -> Amount {
    let halvings = height
        .as_u64()
        .saturating_div(monetary.halving_interval_blocks);
    if halvings >= 64 {
        return Amount::ZERO;
    }
    Amount::from_smallest_units(monetary.initial_block_reward.as_u64() >> halvings)
}

/// Compute the cumulative emission (total minted supply) up to and including
/// the given height.
///
/// This is the sum of all block subsidies from height 0 through `height`.
/// Used to verify the supply cap is not exceeded.
#[must_use]
pub fn cumulative_emission(height: Height, monetary: &MonetaryParams) -> Amount {
    let interval = monetary.halving_interval_blocks;
    let h = height.as_u64();
    let mut total: u64 = 0;
    let mut reward = monetary.initial_block_reward.as_u64();
    let mut remaining = h.saturating_add(1); // blocks 0..=h

    loop {
        if reward == 0 {
            break;
        }
        let blocks_at_this_rate = remaining.min(interval);
        total = total.saturating_add(blocks_at_this_rate.saturating_mul(reward));
        remaining = remaining.saturating_sub(blocks_at_this_rate);
        if remaining == 0 {
            break;
        }
        reward >>= 1;
    }

    // Cap at total supply
    let cap = monetary.total_supply.as_u64();
    Amount::from_smallest_units(total.min(cap))
}

/// Check whether any more tokens can be minted at the given height.
#[must_use]
pub fn is_emission_exhausted(height: Height, monetary: &MonetaryParams) -> bool {
    cumulative_emission(height, monetary) >= monetary.total_supply
}

// ============================================================================
// Total block reward
// ============================================================================

/// Total reward for a block: subsidy + fees.
///
/// If the subsidy would push cumulative emission past `total_supply`,
/// the subsidy is reduced to fit. This ensures the supply cap is never
/// exceeded.
#[must_use]
pub fn total_block_reward(height: Height, fees: Amount, monetary: &MonetaryParams) -> Amount {
    let raw_subsidy = block_subsidy(height, monetary);

    // Check supply cap: emission through (height-1) + raw_subsidy ≤ total_supply
    let prev_emission = if height.as_u64() == 0 {
        0
    } else {
        cumulative_emission(Height::from(height.as_u64().saturating_sub(1)), monetary).as_u64()
    };
    let cap = monetary.total_supply.as_u64();
    let room = cap.saturating_sub(prev_emission);
    let capped_subsidy = raw_subsidy.as_u64().min(room);

    // Total = capped subsidy + fees (fees cannot overflow meaningfully)
    Amount::from_smallest_units(capped_subsidy.saturating_add(fees.as_u64()))
}

// ============================================================================
// Reward distribution
// ============================================================================

/// Per-entity reward share after distribution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardShare {
    /// Recipient identifier (pool operator or delegator hash).
    pub recipient: qv_core::Hash256,
    /// Amount earned.
    pub amount: Amount,
}

/// Compute how a block's total reward is split between the operator and
/// delegators.
///
/// Returns `(operator_reward, Vec<delegator_reward>)`.
///
/// # Distribution formula
///
/// 1. `after_cost = total_reward − min(fixed_cost, total_reward)`
/// 2. `operator_share = fixed_cost + after_cost × margin`
/// 3. `delegator_pool = after_cost × (1 − margin)`
/// 4. Each delegator gets `delegator_pool × (their_stake / total_delegated_stake)`
pub fn distribute_reward(
    total_reward: Amount,
    pool: &StakePool,
    delegator_stakes: &[(qv_core::Hash256, Amount)],
) -> Result<(Amount, Vec<RewardShare>), RewardError> {
    let total = total_reward.as_u64();
    if total == 0 {
        return Ok((Amount::ZERO, Vec::new()));
    }

    // 1. Deduct fixed cost (operator gets at least this)
    let cost = pool.fixed_cost.as_u64().min(total);
    let after_cost = total.saturating_sub(cost);

    // 2. Operator margin from the remainder
    let margin_share = if pool.margin_den > 0 {
        (after_cost as u128)
            .checked_mul(pool.margin_num as u128)
            .and_then(|n| n.checked_div(pool.margin_den as u128))
            .map(|v| v.min(u64::MAX as u128) as u64)
            .unwrap_or(0)
    } else {
        0
    };

    let operator_total = cost.saturating_add(margin_share);
    let delegator_pool = after_cost.saturating_sub(margin_share);

    // 3. Pro-rata distribution to delegators
    let total_delegated: u64 = delegator_stakes
        .iter()
        .map(|(_, a)| a.as_u64())
        .fold(0u64, |acc, x| acc.saturating_add(x));

    let mut shares = Vec::with_capacity(delegator_stakes.len());
    let mut distributed: u64 = 0;

    if total_delegated > 0 && delegator_pool > 0 {
        for (id, stake) in delegator_stakes {
            let share = (delegator_pool as u128)
                .checked_mul(stake.as_u64() as u128)
                .and_then(|n| n.checked_div(total_delegated as u128))
                .map(|v| v.min(u64::MAX as u128) as u64)
                .unwrap_or(0);
            distributed = distributed.saturating_add(share);
            shares.push(RewardShare {
                recipient: *id,
                amount: Amount::from_smallest_units(share),
            });
        }
    }

    // Any rounding dust goes to the operator
    let dust = delegator_pool.saturating_sub(distributed);
    let final_operator = operator_total.saturating_add(dust);

    Ok((Amount::from_smallest_units(final_operator), shares))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division,
    clippy::float_arithmetic
)]
mod tests {
    use super::*;
    use qv_core::{Amount, Hash256, Height, MonetaryParams};

    fn mainnet_monetary() -> MonetaryParams {
        MonetaryParams::mainnet()
    }

    fn simple_monetary() -> MonetaryParams {
        MonetaryParams {
            total_supply: Amount::from_smallest_units(1_000_000),
            initial_block_reward: Amount::from_smallest_units(100),
            halving_interval_blocks: 10,
            min_fee_per_byte: 0,
        }
    }

    fn make_pool_for_reward(margin_num: u32, margin_den: u32, cost: u64) -> StakePool {
        use crate::stake::PoolId;
        StakePool {
            id: PoolId::from_vrf_key(&[0xAA; 32]),
            vrf_key: vec![0xAA; 32],
            kes_key: vec![0xAA; 32],
            pledge: Amount::from_smallest_units(1000),
            margin_num,
            margin_den,
            fixed_cost: Amount::from_smallest_units(cost),
            active: true,
        }
    }

    #[test]
    fn subsidy_at_genesis() {
        let m = mainnet_monetary();
        let subsidy = block_subsidy(Height::GENESIS, &m);
        assert_eq!(subsidy, m.initial_block_reward);
    }

    #[test]
    fn subsidy_halves() {
        let m = simple_monetary();
        assert_eq!(block_subsidy(Height::from(0), &m).as_u64(), 100);
        assert_eq!(block_subsidy(Height::from(9), &m).as_u64(), 100);
        assert_eq!(block_subsidy(Height::from(10), &m).as_u64(), 50);
        assert_eq!(block_subsidy(Height::from(19), &m).as_u64(), 50);
        assert_eq!(block_subsidy(Height::from(20), &m).as_u64(), 25);
    }

    #[test]
    fn subsidy_eventually_zero() {
        let m = simple_monetary();
        // After enough halvings the reward drops to 0
        let subsidy = block_subsidy(Height::from(1000), &m);
        assert_eq!(subsidy, Amount::ZERO);
    }

    #[test]
    fn cumulative_emission_first_era() {
        let m = simple_monetary();
        // First 10 blocks: 10 × 100 = 1000
        let em = cumulative_emission(Height::from(9), &m);
        assert_eq!(em.as_u64(), 1000);
    }

    #[test]
    fn cumulative_emission_two_eras() {
        let m = simple_monetary();
        // First 10 blocks: 1000, next 5 blocks at 50: 250, total = 1250
        let em = cumulative_emission(Height::from(14), &m);
        assert_eq!(em.as_u64(), 1000 + 5 * 50);
    }

    #[test]
    fn cumulative_emission_capped_at_total_supply() {
        let m = simple_monetary();
        // Very late height — emission should be capped at 1_000_000
        let em = cumulative_emission(Height::from(1_000_000), &m);
        assert!(em.as_u64() <= m.total_supply.as_u64());
    }

    #[test]
    fn emission_exhaustion() {
        let m = simple_monetary();
        assert!(!is_emission_exhausted(Height::from(0), &m));
        // At some point it exhausts
        let mut found = false;
        for h in 0..100_000 {
            if is_emission_exhausted(Height::from(h), &m) {
                found = true;
                break;
            }
        }
        assert!(found, "emission should exhaust eventually");
    }

    #[test]
    fn total_block_reward_caps_subsidy() {
        let m = MonetaryParams {
            total_supply: Amount::from_smallest_units(150),
            initial_block_reward: Amount::from_smallest_units(100),
            halving_interval_blocks: 10,
            min_fee_per_byte: 0,
        };
        // Block 0: subsidy=100, prev_emission=0, room=150 → subsidy=100
        let r0 = total_block_reward(Height::from(0), Amount::ZERO, &m);
        assert_eq!(r0.as_u64(), 100);

        // Block 1: prev_emission=100, room=50 → subsidy capped at 50
        let r1 = total_block_reward(Height::from(1), Amount::from_smallest_units(10), &m);
        assert_eq!(r1.as_u64(), 50 + 10); // capped subsidy + fees
    }

    #[test]
    fn distribute_all_to_operator_when_no_delegators() {
        let pool = make_pool_for_reward(10, 100, 100);
        let (op, delegators) =
            distribute_reward(Amount::from_smallest_units(1000), &pool, &[]).unwrap();
        assert_eq!(op.as_u64(), 1000); // everything goes to operator
        assert!(delegators.is_empty());
    }

    #[test]
    fn distribute_with_delegators() {
        let pool = make_pool_for_reward(10, 100, 100);
        let d1 = (
            Hash256::from_bytes([1; 32]),
            Amount::from_smallest_units(700),
        );
        let d2 = (
            Hash256::from_bytes([2; 32]),
            Amount::from_smallest_units(300),
        );

        let total = Amount::from_smallest_units(10_000);
        let (op, delegators) = distribute_reward(total, &pool, &[d1, d2]).unwrap();

        // cost = 100, after_cost = 9900
        // margin = 9900 * 10/100 = 990
        // operator_total = 100 + 990 = 1090
        // delegator_pool = 9900 - 990 = 8910
        // d1: 8910 * 700/1000 = 6237
        // d2: 8910 * 300/1000 = 2673
        // dust: 8910 - 6237 - 2673 = 0
        assert_eq!(op.as_u64(), 1090);
        assert_eq!(delegators.len(), 2);
        assert_eq!(delegators[0].amount.as_u64(), 6237);
        assert_eq!(delegators[1].amount.as_u64(), 2673);
    }

    #[test]
    fn distribute_zero_reward() {
        let pool = make_pool_for_reward(10, 100, 100);
        let (op, delegators) = distribute_reward(Amount::ZERO, &pool, &[]).unwrap();
        assert_eq!(op, Amount::ZERO);
        assert!(delegators.is_empty());
    }

    #[test]
    fn distribute_dust_goes_to_operator() {
        let pool = make_pool_for_reward(0, 100, 0); // 0% margin, 0 cost
        let d1 = (Hash256::from_bytes([1; 32]), Amount::from_smallest_units(1));
        let d2 = (Hash256::from_bytes([2; 32]), Amount::from_smallest_units(1));
        let d3 = (Hash256::from_bytes([3; 32]), Amount::from_smallest_units(1));

        let total = Amount::from_smallest_units(10); // 10 / 3 = 3 each, 1 dust
        let (op, delegators) = distribute_reward(total, &pool, &[d1, d2, d3]).unwrap();

        let del_total: u64 = delegators.iter().map(|d| d.amount.as_u64()).sum();
        assert_eq!(del_total + op.as_u64(), 10); // no tokens lost
        assert!(op.as_u64() >= 1, "dust should go to operator");
    }

    #[test]
    fn mainnet_halving_schedule() {
        let m = mainnet_monetary();
        // At height 0: 50 * 10^8
        assert_eq!(block_subsidy(Height::from(0), &m).as_u64(), 5_000_000_000);
        // At height 210_000: 25 * 10^8
        assert_eq!(
            block_subsidy(Height::from(210_000), &m).as_u64(),
            2_500_000_000
        );
        // At height 420_000: 12.5 * 10^8
        assert_eq!(
            block_subsidy(Height::from(420_000), &m).as_u64(),
            1_250_000_000
        );
    }
}
