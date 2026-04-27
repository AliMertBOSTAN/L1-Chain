//! Stake pools, delegation, and epoch-level stake snapshots.
//!
//! Ouroboros Praos elects slot leaders in proportion to their *relative stake*.
//! This module models the entities involved:
//!
//! - [`PoolId`] — unique identifier for a stake pool (hash of the operator key).
//! - [`StakePool`] — registration metadata (operator key, margin, pledge, …).
//! - [`Delegation`] — a delegator→pool mapping with the delegated amount.
//! - [`StakeDistribution`] — the frozen per-pool stake snapshot used for a
//!   whole epoch of leader election.
//!
//! # Snapshot timing
//!
//! The distribution used in epoch *e* is snapshotted at the start of epoch
//! *e − 1* (one-epoch look-back). The ledger layer will call
//! [`StakeDistribution::snapshot`] at the appropriate boundary.

use std::collections::BTreeMap;

use qv_core::{Amount, Epoch, Hash256};
use qv_crypto::sha3_256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// PoolId
// ============================================================================

/// Unique identifier for a stake pool: `SHA3-256(operator_vrf_key)`.
///
/// Using the VRF key rather than the cold key means a pool cannot silently
/// swap its VRF identity without re-registering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PoolId(pub Hash256);

impl PoolId {
    /// Derive a pool id from raw operator VRF public-key bytes.
    #[must_use]
    pub fn from_vrf_key(vrf_pk: &[u8]) -> Self {
        Self(Hash256::from_bytes(sha3_256(vrf_pk)))
    }

    /// Zero-valued sentinel (used only in tests / genesis).
    pub const ZERO: Self = Self(Hash256::ZERO);

    /// Underlying bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

// ============================================================================
// StakePool
// ============================================================================

/// On-chain registration record for a stake pool.
///
/// In a full implementation this would be stored in the ledger state;
/// here we model the fields the consensus layer needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakePool {
    /// Unique pool identifier.
    pub id: PoolId,
    /// Operator's VRF public key (opaque bytes, validated by `qv-crypto`).
    pub vrf_key: Vec<u8>,
    /// Operator's KES public key (opaque bytes, validated by `qv-crypto`).
    pub kes_key: Vec<u8>,
    /// Operator's pledge (amount staked by the operator themselves).
    pub pledge: Amount,
    /// Margin: the fraction of rewards the operator takes, as a rational
    /// number `margin_num / margin_den`. Must be in `[0, 1]`.
    pub margin_num: u32,
    pub margin_den: u32,
    /// Fixed cost deducted from the pool's total rewards each epoch before
    /// the margin split.
    pub fixed_cost: Amount,
    /// Whether this pool is currently active (registered and not retired).
    pub active: bool,
}

/// Errors related to stake operations.
#[derive(Debug, Error)]
pub enum StakeError {
    /// A pool with the same id already exists.
    #[error("pool {0:?} already registered")]
    PoolAlreadyRegistered(PoolId),
    /// Attempted to delegate to an unknown pool.
    #[error("pool {0:?} not found")]
    PoolNotFound(PoolId),
    /// Attempted to delegate zero stake.
    #[error("delegation amount must be > 0")]
    ZeroDelegation,
    /// Margin is out of range (must be ≤ 1).
    #[error("margin {num}/{den} is invalid (must be <= 1, den > 0)")]
    InvalidMargin { num: u32, den: u32 },
    /// Arithmetic overflow in stake calculation.
    #[error("stake arithmetic overflow")]
    Overflow,
}

impl StakePool {
    /// Validate the pool's parameters.
    pub fn validate(&self) -> Result<(), StakeError> {
        if self.margin_den == 0 || self.margin_num > self.margin_den {
            return Err(StakeError::InvalidMargin {
                num: self.margin_num,
                den: self.margin_den,
            });
        }
        Ok(())
    }

    /// Compute the margin as a `f64` in `[0.0, 1.0]`.
    ///
    /// This is used only for display / reward computation; consensus-critical
    /// paths use the rational representation.
    #[must_use]
    pub fn margin_ratio(&self) -> f64 {
        if self.margin_den == 0 {
            return 0.0;
        }
        f64::from(self.margin_num) / f64::from(self.margin_den)
    }
}

// ============================================================================
// Delegation
// ============================================================================

/// A single delegation: some amount of stake delegated to a pool.
///
/// The `delegator_id` is an opaque 32-byte identifier (typically the hash of
/// the delegator's staking key).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    /// Hash of the delegator's staking key.
    pub delegator_id: Hash256,
    /// Target pool.
    pub pool_id: PoolId,
    /// Amount delegated (in smallest units).
    pub amount: Amount,
}

// ============================================================================
// StakeDistribution — the epoch-frozen snapshot
// ============================================================================

/// Frozen snapshot of per-pool stake used for one epoch of leader election.
///
/// Once built, a `StakeDistribution` is immutable for the entire epoch.
/// The consensus layer queries it via [`relative_stake`] and
/// [`is_slot_leader_eligible`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StakeDistribution {
    /// Epoch this snapshot applies to.
    pub epoch: Epoch,
    /// Per-pool total stake (pool pledge + delegated stake).
    pool_stakes: BTreeMap<PoolId, u64>,
    /// Sum of all pool stakes (cached for O(1) relative-stake lookups).
    total_stake: u64,
}

impl StakeDistribution {
    /// Build a new distribution from raw pool→stake data.
    ///
    /// `entries` is an iterator of `(pool_id, total_stake_amount)`.
    pub fn new(
        epoch: Epoch,
        entries: impl IntoIterator<Item = (PoolId, Amount)>,
    ) -> Result<Self, StakeError> {
        let mut pool_stakes = BTreeMap::new();
        let mut total: u64 = 0;
        for (pid, amount) in entries {
            let val = amount.as_u64();
            if val == 0 {
                continue; // skip pools with zero stake
            }
            pool_stakes
                .entry(pid)
                .and_modify(|s: &mut u64| {
                    *s = s.saturating_add(val);
                })
                .or_insert(val);
            total = total.checked_add(val).ok_or(StakeError::Overflow)?;
        }
        Ok(Self {
            epoch,
            pool_stakes,
            total_stake: total,
        })
    }

    /// Snapshot from a registry of pools and delegations.
    ///
    /// Convenience builder: sums pledge + delegated for each active pool.
    pub fn snapshot(
        epoch: Epoch,
        pools: &[StakePool],
        delegations: &[Delegation],
    ) -> Result<Self, StakeError> {
        let mut stakes: BTreeMap<PoolId, u64> = BTreeMap::new();

        // Add each active pool's pledge.
        for pool in pools {
            if !pool.active {
                continue;
            }
            stakes
                .entry(pool.id)
                .and_modify(|s| *s = s.saturating_add(pool.pledge.as_u64()))
                .or_insert(pool.pledge.as_u64());
        }

        // Add delegations (only to known active pools).
        let active_pools: std::collections::HashSet<PoolId> =
            pools.iter().filter(|p| p.active).map(|p| p.id).collect();

        for d in delegations {
            if !active_pools.contains(&d.pool_id) {
                continue; // silently skip delegations to inactive/unknown pools
            }
            stakes
                .entry(d.pool_id)
                .and_modify(|s| *s = s.saturating_add(d.amount.as_u64()))
                .or_insert(d.amount.as_u64());
        }

        let entries: Vec<_> = stakes
            .into_iter()
            .map(|(pid, s)| (pid, Amount::from_smallest_units(s)))
            .collect();

        Self::new(epoch, entries)
    }

    /// Total stake across all pools.
    #[must_use]
    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    /// Absolute stake for a given pool.
    #[must_use]
    pub fn pool_stake(&self, pool: &PoolId) -> u64 {
        self.pool_stakes.get(pool).copied().unwrap_or(0)
    }

    /// Relative stake of a pool as a ratio `(numerator, denominator)`.
    ///
    /// Returns `(0, 1)` if the pool has no stake or total is zero.
    #[must_use]
    pub fn relative_stake(&self, pool: &PoolId) -> (u64, u64) {
        let s = self.pool_stake(pool);
        if self.total_stake == 0 || s == 0 {
            return (0, 1);
        }
        (s, self.total_stake)
    }

    /// Number of registered pools with nonzero stake.
    #[must_use]
    pub fn pool_count(&self) -> usize {
        self.pool_stakes.len()
    }

    /// Iterate over all `(PoolId, stake)` entries in deterministic order.
    pub fn iter(&self) -> impl Iterator<Item = (&PoolId, &u64)> {
        self.pool_stakes.iter()
    }

    /// Whether the distribution is empty (no pools or no stake).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_stake == 0
    }

    /// Look up the pool registration that owns a given VRF key hash.
    ///
    /// Returns `None` if no pool matches. `pools` is the pool registry;
    /// this is a linear scan — fine for the expected pool count.
    #[must_use]
    pub fn pool_for_vrf_key(pools: &[StakePool], vrf_key: &[u8]) -> Option<PoolId> {
        let candidate = PoolId::from_vrf_key(vrf_key);
        pools
            .iter()
            .find(|p| p.id == candidate && p.active)
            .map(|p| p.id)
    }
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

    fn make_pool(id_byte: u8, pledge: u64) -> StakePool {
        let vrf_key = vec![id_byte; 32];
        StakePool {
            id: PoolId::from_vrf_key(&vrf_key),
            vrf_key,
            kes_key: vec![id_byte; 32],
            pledge: Amount::from_smallest_units(pledge),
            margin_num: 5,
            margin_den: 100,
            fixed_cost: Amount::from_smallest_units(340_000_000),
            active: true,
        }
    }

    fn make_delegation(delegator_byte: u8, pool: &StakePool, amount: u64) -> Delegation {
        Delegation {
            delegator_id: Hash256::from_bytes([delegator_byte; 32]),
            pool_id: pool.id,
            amount: Amount::from_smallest_units(amount),
        }
    }

    #[test]
    fn pool_id_deterministic() {
        let key = vec![0xAA; 32];
        let id1 = PoolId::from_vrf_key(&key);
        let id2 = PoolId::from_vrf_key(&key);
        assert_eq!(id1, id2);
    }

    #[test]
    fn pool_id_different_keys() {
        let id1 = PoolId::from_vrf_key(&[0xAA; 32]);
        let id2 = PoolId::from_vrf_key(&[0xBB; 32]);
        assert_ne!(id1, id2);
    }

    #[test]
    fn pool_validate_margin() {
        let mut pool = make_pool(1, 1000);
        pool.margin_num = 5;
        pool.margin_den = 100;
        assert!(pool.validate().is_ok());

        pool.margin_num = 101;
        pool.margin_den = 100;
        assert!(pool.validate().is_err());

        pool.margin_den = 0;
        assert!(pool.validate().is_err());
    }

    #[test]
    fn margin_ratio() {
        let mut pool = make_pool(1, 1000);
        pool.margin_num = 5;
        pool.margin_den = 100;
        let r = pool.margin_ratio();
        assert!((r - 0.05).abs() < 1e-9);
    }

    #[test]
    fn distribution_from_entries() {
        let p1 = PoolId::from_vrf_key(&[1; 32]);
        let p2 = PoolId::from_vrf_key(&[2; 32]);
        let dist = StakeDistribution::new(
            Epoch::from(5),
            vec![
                (p1, Amount::from_smallest_units(700)),
                (p2, Amount::from_smallest_units(300)),
            ],
        )
        .unwrap();

        assert_eq!(dist.total_stake(), 1000);
        assert_eq!(dist.pool_stake(&p1), 700);
        assert_eq!(dist.pool_stake(&p2), 300);
        assert_eq!(dist.pool_count(), 2);
        assert_eq!(dist.relative_stake(&p1), (700, 1000));
    }

    #[test]
    fn distribution_skips_zero_stake() {
        let p1 = PoolId::from_vrf_key(&[1; 32]);
        let dist = StakeDistribution::new(Epoch::GENESIS, vec![(p1, Amount::ZERO)]).unwrap();
        assert!(dist.is_empty());
        assert_eq!(dist.pool_count(), 0);
    }

    #[test]
    fn snapshot_aggregates_pledge_and_delegations() {
        let pool_a = make_pool(0xAA, 5000);
        let pool_b = make_pool(0xBB, 3000);
        let pools = vec![pool_a.clone(), pool_b.clone()];

        let delegations = vec![
            make_delegation(1, &pool_a, 2000),
            make_delegation(2, &pool_a, 1000),
            make_delegation(3, &pool_b, 4000),
        ];

        let dist = StakeDistribution::snapshot(Epoch::from(1), &pools, &delegations).unwrap();

        // pool_a: 5000 pledge + 2000 + 1000 = 8000
        assert_eq!(dist.pool_stake(&pool_a.id), 8000);
        // pool_b: 3000 pledge + 4000 = 7000
        assert_eq!(dist.pool_stake(&pool_b.id), 7000);
        assert_eq!(dist.total_stake(), 15_000);
    }

    #[test]
    fn snapshot_ignores_inactive_pools() {
        let mut pool = make_pool(0xCC, 5000);
        pool.active = false;
        let delegations = vec![make_delegation(1, &pool, 2000)];
        let dist = StakeDistribution::snapshot(Epoch::GENESIS, &[pool], &delegations).unwrap();
        assert!(dist.is_empty());
    }

    #[test]
    fn relative_stake_unknown_pool() {
        let dist = StakeDistribution::new(Epoch::GENESIS, std::iter::empty()).unwrap();
        assert_eq!(dist.relative_stake(&PoolId::ZERO), (0, 1));
    }

    #[test]
    fn pool_for_vrf_key_lookup() {
        let pool = make_pool(0xDD, 1000);
        let pools = vec![pool.clone()];
        let found = StakeDistribution::pool_for_vrf_key(&pools, &pool.vrf_key);
        assert_eq!(found, Some(pool.id));

        let not_found = StakeDistribution::pool_for_vrf_key(&pools, &[0xFF; 32]);
        assert!(not_found.is_none());
    }

    #[test]
    fn distribution_deterministic_iteration() {
        let p1 = PoolId::from_vrf_key(&[1; 32]);
        let p2 = PoolId::from_vrf_key(&[2; 32]);
        let p3 = PoolId::from_vrf_key(&[3; 32]);
        let dist = StakeDistribution::new(
            Epoch::GENESIS,
            vec![
                (p3, Amount::from_smallest_units(100)),
                (p1, Amount::from_smallest_units(200)),
                (p2, Amount::from_smallest_units(300)),
            ],
        )
        .unwrap();

        // BTreeMap guarantees sorted order by PoolId
        let ids: Vec<PoolId> = dist.iter().map(|(id, _)| *id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }
}
