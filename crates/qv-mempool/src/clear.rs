//! Fee-sorted cleartext mempool.
//!
//! The [`ClearPool`] stores unconfirmed transactions sorted by fee density
//! (fee-per-byte), tracks UTXO dependencies to prevent double-spends, and
//! supports configurable eviction policies (size cap, age cap, min fee).
//!
//! This pool is for *unencrypted* transactions.  Encrypted transactions
//! flow through [`super::encrypted::EncryptedPool`].

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use qv_core::{Amount, OutPoint, Transaction, TxId};
use serde::{Deserialize, Serialize};

use crate::MempoolError;

/// Configuration for the clear mempool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClearPoolConfig {
    /// Maximum number of transactions in the pool.
    pub max_tx_count: usize,
    /// Maximum aggregate serialized size (bytes) of transactions.
    pub max_pool_bytes: usize,
    /// Minimum fee (smallest units) to accept a transaction.
    pub min_fee: u64,
    /// Maximum age in seconds before a transaction is evicted.
    pub max_age_secs: u64,
}

impl Default for ClearPoolConfig {
    fn default() -> Self {
        Self {
            max_tx_count: 50_000,
            max_pool_bytes: 64 * 1024 * 1024, // 64 MiB
            min_fee: 1,
            max_age_secs: 3600, // 1 hour
        }
    }
}

impl ClearPoolConfig {
    /// Conservative mainnet configuration.
    #[must_use]
    pub fn mainnet() -> Self {
        Self::default()
    }

    /// Testnet with relaxed limits.
    #[must_use]
    pub fn testnet() -> Self {
        Self {
            max_tx_count: 100_000,
            max_pool_bytes: 128 * 1024 * 1024,
            min_fee: 0,
            max_age_secs: 7200,
        }
    }

    /// Minimal config for local testing.
    #[must_use]
    pub fn ephemeral() -> Self {
        Self {
            max_tx_count: 1_000,
            max_pool_bytes: 4 * 1024 * 1024,
            min_fee: 0,
            max_age_secs: 300,
        }
    }
}

/// A mempool entry wrapping a transaction with metadata.
#[derive(Clone, Debug)]
pub struct MempoolEntry {
    /// The unconfirmed transaction.
    pub tx: Transaction,
    /// Pre-computed transaction id.
    pub tx_id: TxId,
    /// Pre-computed fee (supplied by the caller who resolved UTXO values).
    pub fee: Amount,
    /// Estimated serialized size (bytes).
    pub size: usize,
    /// Fee density (fee * 1000 / size) for sorting.  Higher is better.
    pub fee_density: u64,
    /// Unix-epoch seconds when the transaction was added.
    pub added_at: u64,
}

impl MempoolEntry {
    /// Create a new mempool entry.
    ///
    /// `fee` must be pre-computed by the caller (sum of resolved inputs − sum
    /// of outputs).  `size` is the estimated wire size in bytes.
    pub fn new(tx: Transaction, tx_id: TxId, fee: Amount, size: usize) -> Self {
        let fee_density = if size > 0 {
            fee.0.saturating_mul(1000) / (size as u64)
        } else {
            fee.0
        };
        Self {
            tx,
            tx_id,
            fee,
            size,
            fee_density,
            added_at: now_secs(),
        }
    }
}

/// Sort key for the fee-priority queue: (fee_density DESC, added_at ASC, tx_id ASC).
///
/// Higher fee density comes first.  Among equal fee densities, older
/// transactions are preferred (FIFO tiebreak).  Final tiebreak by tx_id for
/// full determinism.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PriorityKey {
    /// Negated fee density (so BTreeMap ascending order = descending fee density).
    neg_fee_density: u64,
    added_at: u64,
    tx_id: TxId,
}

impl PartialOrd for PriorityKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.neg_fee_density
            .cmp(&other.neg_fee_density)
            .then(self.added_at.cmp(&other.added_at))
            .then(self.tx_id.cmp(&other.tx_id))
    }
}

impl PriorityKey {
    fn from_entry(entry: &MempoolEntry) -> Self {
        Self {
            neg_fee_density: u64::MAX - entry.fee_density,
            added_at: entry.added_at,
            tx_id: entry.tx_id,
        }
    }
}

/// Fee-sorted cleartext mempool.
#[derive(Debug)]
pub struct ClearPool {
    config: ClearPoolConfig,
    /// TxId → entry lookup.
    by_id: HashMap<TxId, MempoolEntry>,
    /// Fee-priority sorted index.
    by_priority: BTreeMap<PriorityKey, TxId>,
    /// OutPoint → spending TxId (UTXO dependency tracker).
    spent_outpoints: HashMap<OutPoint, TxId>,
    /// Aggregate byte size of all entries.
    total_bytes: usize,
}

impl ClearPool {
    /// Create an empty clear mempool with the given configuration.
    #[must_use]
    pub fn new(config: ClearPoolConfig) -> Self {
        Self {
            config,
            by_id: HashMap::new(),
            by_priority: BTreeMap::new(),
            spent_outpoints: HashMap::new(),
            total_bytes: 0,
        }
    }

    /// Number of transactions currently in the pool.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Aggregate byte size of all pooled transactions.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Check whether a transaction is already in the pool.
    #[must_use]
    pub fn contains(&self, tx_id: &TxId) -> bool {
        self.by_id.contains_key(tx_id)
    }

    /// Get a transaction by id.
    #[must_use]
    pub fn get(&self, tx_id: &TxId) -> Option<&MempoolEntry> {
        self.by_id.get(tx_id)
    }

    /// Add a transaction to the pool.
    ///
    /// Returns the `TxId` on success.  Rejects duplicates, double-spends,
    /// transactions below the minimum fee, and transactions that would exceed
    /// the pool capacity (after eviction).
    pub fn add(&mut self, entry: MempoolEntry) -> Result<TxId, MempoolError> {
        // Duplicate check
        if self.by_id.contains_key(&entry.tx_id) {
            return Err(MempoolError::DuplicateTx(entry.tx_id));
        }

        // Min fee check
        if entry.fee.0 < self.config.min_fee {
            return Err(MempoolError::FeeTooLow {
                fee: entry.fee.0,
                min: self.config.min_fee,
            });
        }

        // Double-spend check
        for input in &entry.tx.inputs {
            if let Some(existing_tx) = self.spent_outpoints.get(&input.prev_output) {
                return Err(MempoolError::DoubleSpend {
                    outpoint: input.prev_output,
                    existing_tx: *existing_tx,
                });
            }
        }

        // Evict expired before capacity check
        self.evict_expired();

        // Capacity: evict lowest-fee txs if needed
        while self.by_id.len() >= self.config.max_tx_count
            || self.total_bytes.saturating_add(entry.size) > self.config.max_pool_bytes
        {
            if !self.evict_lowest_fee() {
                return Err(MempoolError::PoolFull);
            }
        }

        // Insert
        let tx_id = entry.tx_id;
        let key = PriorityKey::from_entry(&entry);

        for input in &entry.tx.inputs {
            self.spent_outpoints.insert(input.prev_output, tx_id);
        }
        self.total_bytes = self.total_bytes.saturating_add(entry.size);
        self.by_priority.insert(key, tx_id);
        self.by_id.insert(tx_id, entry);

        Ok(tx_id)
    }

    /// Remove a transaction by id (e.g. when it's been confirmed in a block).
    pub fn remove(&mut self, tx_id: &TxId) -> Option<MempoolEntry> {
        let entry = self.by_id.remove(tx_id)?;
        let key = PriorityKey::from_entry(&entry);
        self.by_priority.remove(&key);

        for input in &entry.tx.inputs {
            self.spent_outpoints.remove(&input.prev_output);
        }
        self.total_bytes = self.total_bytes.saturating_sub(entry.size);
        Some(entry)
    }

    /// Remove all transactions that spend any of the given outpoints
    /// (called when a block is connected and those outputs are now spent).
    pub fn remove_confirmed(&mut self, spent: &BTreeSet<OutPoint>) -> Vec<TxId> {
        let mut removed = Vec::new();
        let tx_ids: Vec<TxId> = spent
            .iter()
            .filter_map(|op| self.spent_outpoints.get(op).copied())
            .collect();

        for tx_id in tx_ids {
            if self.remove(&tx_id).is_some() {
                removed.push(tx_id);
            }
        }
        removed
    }

    /// Get the top `n` transactions by fee density (for block building).
    pub fn get_batch(&self, n: usize) -> Vec<&MempoolEntry> {
        self.by_priority
            .values()
            .take(n)
            .filter_map(|tx_id| self.by_id.get(tx_id))
            .collect()
    }

    /// Get all transactions sorted by fee density (highest first).
    pub fn all_sorted(&self) -> Vec<&MempoolEntry> {
        self.by_priority
            .values()
            .filter_map(|tx_id| self.by_id.get(tx_id))
            .collect()
    }

    /// Evict transactions older than `max_age_secs`.  Returns the count evicted.
    pub fn evict_expired(&mut self) -> usize {
        let cutoff = now_secs().saturating_sub(self.config.max_age_secs);
        let expired_ids: Vec<TxId> = self
            .by_id
            .values()
            .filter(|e| e.added_at < cutoff)
            .map(|e| e.tx_id)
            .collect();

        let count = expired_ids.len();
        for tx_id in expired_ids {
            self.remove(&tx_id);
        }
        count
    }

    /// Evict the single lowest-fee-density transaction.
    /// Returns `true` if something was evicted, `false` if pool is empty.
    fn evict_lowest_fee(&mut self) -> bool {
        // Last entry in BTreeMap = highest neg_fee_density = lowest actual fee density
        let last_key = self.by_priority.keys().next_back().cloned();
        if let Some(key) = last_key {
            let tx_id = key.tx_id;
            self.remove(&tx_id);
            true
        } else {
            false
        }
    }

    /// Check whether an outpoint is currently being spent by a pooled transaction.
    #[must_use]
    pub fn is_spent(&self, outpoint: &OutPoint) -> bool {
        self.spent_outpoints.contains_key(outpoint)
    }

    /// Return the spending transaction for an outpoint, if any.
    #[must_use]
    pub fn spending_tx(&self, outpoint: &OutPoint) -> Option<TxId> {
        self.spent_outpoints.get(outpoint).copied()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::{Amount, OutPoint, Script, Transaction, TxId, TxInput, TxOutput};

    use super::*;

    fn make_tx(marker: u8) -> (Transaction, TxId) {
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(
                TxId::from_bytes([marker; 32]),
                0,
            ))],
            vec![TxOutput::new(
                Amount::from_smallest_units(100),
                Script::new(vec![marker]),
            )],
        );
        let id = tx.id().unwrap();
        (tx, id)
    }

    fn entry(marker: u8, fee: u64, size: usize) -> MempoolEntry {
        let (tx, id) = make_tx(marker);
        MempoolEntry::new(tx, id, Amount::from_smallest_units(fee), size)
    }

    #[test]
    fn add_and_get() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let e = entry(1, 100, 200);
        let tx_id = e.tx_id;

        pool.add(e).unwrap();
        assert_eq!(pool.len(), 1);
        assert!(pool.contains(&tx_id));
        assert!(pool.get(&tx_id).is_some());
    }

    #[test]
    fn duplicate_rejected() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let e = entry(2, 100, 200);
        let tx_id = e.tx_id;

        pool.add(e.clone()).unwrap();
        let err = pool.add(e).unwrap_err();
        assert!(matches!(err, MempoolError::DuplicateTx(id) if id == tx_id));
    }

    #[test]
    fn double_spend_rejected() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

        // Two different transactions spending the same outpoint
        let (tx1, id1) = make_tx(3);
        let e1 = MempoolEntry::new(tx1, id1, Amount::from_smallest_units(100), 200);
        pool.add(e1).unwrap();

        // tx2 spends the same outpoint as tx1 (marker 3 → OutPoint([3;32], 0))
        let tx2 = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([3; 32]), 0))],
            vec![TxOutput::new(
                Amount::from_smallest_units(90),
                Script::new(vec![33]),
            )],
        );
        let id2 = tx2.id().unwrap();
        let e2 = MempoolEntry::new(tx2, id2, Amount::from_smallest_units(200), 200);

        let err = pool.add(e2).unwrap_err();
        assert!(matches!(err, MempoolError::DoubleSpend { .. }));
    }

    #[test]
    fn fee_too_low_rejected() {
        let config = ClearPoolConfig {
            min_fee: 50,
            ..ClearPoolConfig::ephemeral()
        };
        let mut pool = ClearPool::new(config);
        let e = entry(4, 10, 200); // fee=10 < min=50

        let err = pool.add(e).unwrap_err();
        assert!(matches!(err, MempoolError::FeeTooLow { .. }));
    }

    #[test]
    fn remove_clears_spent_tracking() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let e = entry(5, 100, 200);
        let tx_id = e.tx_id;
        let outpoint = e.tx.inputs[0].prev_output;

        pool.add(e).unwrap();
        assert!(pool.is_spent(&outpoint));

        pool.remove(&tx_id);
        assert!(!pool.is_spent(&outpoint));
        assert!(pool.is_empty());
    }

    #[test]
    fn get_batch_returns_highest_fee_first() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

        // Add with varying fees (same size → fee density directly proportional to fee)
        pool.add(entry(10, 100, 200)).unwrap(); // density = 500
        pool.add(entry(11, 300, 200)).unwrap(); // density = 1500
        pool.add(entry(12, 200, 200)).unwrap(); // density = 1000

        let batch = pool.get_batch(3);
        assert_eq!(batch.len(), 3);
        assert!(batch[0].fee_density >= batch[1].fee_density);
        assert!(batch[1].fee_density >= batch[2].fee_density);
    }

    #[test]
    fn capacity_eviction() {
        let config = ClearPoolConfig {
            max_tx_count: 3,
            max_pool_bytes: 1_000_000,
            min_fee: 0,
            max_age_secs: 3600,
        };
        let mut pool = ClearPool::new(config);

        pool.add(entry(20, 10, 100)).unwrap(); // lowest fee
        pool.add(entry(21, 100, 100)).unwrap();
        pool.add(entry(22, 200, 100)).unwrap();
        assert_eq!(pool.len(), 3);

        // Adding a 4th should evict the lowest-fee tx
        pool.add(entry(23, 50, 100)).unwrap();
        assert_eq!(pool.len(), 3);

        // The lowest-fee (marker=20, fee=10) should have been evicted
        let (_, id20) = make_tx(20);
        assert!(!pool.contains(&id20));
    }

    #[test]
    fn remove_confirmed_bulk() {
        let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

        let e1 = entry(30, 100, 200);
        let e2 = entry(31, 200, 200);
        let op1 = e1.tx.inputs[0].prev_output;
        let op2 = e2.tx.inputs[0].prev_output;

        pool.add(e1).unwrap();
        pool.add(e2).unwrap();
        assert_eq!(pool.len(), 2);

        let mut spent = BTreeSet::new();
        spent.insert(op1);
        spent.insert(op2);

        let removed = pool.remove_confirmed(&spent);
        assert_eq!(removed.len(), 2);
        assert!(pool.is_empty());
    }
}
