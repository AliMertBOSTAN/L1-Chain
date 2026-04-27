//! Deterministic transaction ordering for block building.
//!
//! After the slot leader decrypts the encrypted pool (or collects from the
//! clear pool), all candidate transactions must be ordered **deterministically**
//! so that every honest validator can independently verify the ordering.
//!
//! Ordering rule (from ADR-003):
//! 1. Primary: fee density (descending — highest fee/byte first).
//! 2. Tiebreak: submission timestamp (ascending — FIFO among equal fees).
//! 3. Final tiebreak: tx hash (ascending — lexicographic, for full determinism).

use qv_core::TxId;

/// A lightweight ordering key that can be computed for any candidate transaction.
///
/// The slot leader constructs one `OrderKey` per transaction, sorts them, and
/// includes transactions in the resulting order.  Validators re-derive the keys
/// from the decrypted transaction set and verify the order matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderKey {
    /// Negated fee density so ascending sort = descending fee density.
    neg_fee_density: u64,
    /// Unix-epoch milliseconds when the transaction was first observed.
    timestamp_ms: u64,
    /// Transaction id for final deterministic tiebreak.
    tx_id: TxId,
}

impl OrderKey {
    /// Create an ordering key.
    ///
    /// `fee_density` is `fee * 1000 / size` as computed by [`super::clear::MempoolEntry`].
    /// `timestamp_ms` is the millisecond-precision observation time.
    #[must_use]
    pub fn new(fee_density: u64, timestamp_ms: u64, tx_id: TxId) -> Self {
        Self {
            neg_fee_density: u64::MAX - fee_density,
            timestamp_ms,
            tx_id,
        }
    }

    /// The fee density (un-negated).
    #[must_use]
    pub fn fee_density(&self) -> u64 {
        u64::MAX - self.neg_fee_density
    }

    /// The observation timestamp in milliseconds.
    #[must_use]
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }

    /// The transaction id.
    #[must_use]
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }
}

impl PartialOrd for OrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.neg_fee_density
            .cmp(&other.neg_fee_density)
            .then(self.timestamp_ms.cmp(&other.timestamp_ms))
            .then(self.tx_id.cmp(&other.tx_id))
    }
}

/// Sort a slice of `OrderKey`s deterministically.
///
/// This is the canonical ordering function.  The slot leader and every
/// verifier must call this on the same set of keys and obtain the same result.
pub fn deterministic_sort(keys: &mut [OrderKey]) {
    keys.sort();
}

/// Verify that a sequence of `OrderKey`s is in canonical order.
///
/// Returns `true` iff the sequence is sorted and contains no duplicates.
#[must_use]
pub fn verify_order(keys: &[OrderKey]) -> bool {
    keys.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::TxId;

    use super::*;

    #[test]
    fn higher_fee_density_comes_first() {
        let k1 = OrderKey::new(100, 1000, TxId::from_bytes([1; 32]));
        let k2 = OrderKey::new(200, 1000, TxId::from_bytes([2; 32]));

        let mut keys = vec![k1.clone(), k2.clone()];
        deterministic_sort(&mut keys);

        assert_eq!(keys[0].fee_density(), 200);
        assert_eq!(keys[1].fee_density(), 100);
    }

    #[test]
    fn same_fee_fifo_tiebreak() {
        let k1 = OrderKey::new(100, 500, TxId::from_bytes([1; 32]));
        let k2 = OrderKey::new(100, 1000, TxId::from_bytes([2; 32]));

        let mut keys = vec![k2.clone(), k1.clone()];
        deterministic_sort(&mut keys);

        // k1 has earlier timestamp → comes first
        assert_eq!(keys[0].timestamp_ms(), 500);
        assert_eq!(keys[1].timestamp_ms(), 1000);
    }

    #[test]
    fn same_fee_same_time_hash_tiebreak() {
        let k1 = OrderKey::new(100, 1000, TxId::from_bytes([1; 32]));
        let k2 = OrderKey::new(100, 1000, TxId::from_bytes([2; 32]));

        let mut keys = vec![k2.clone(), k1.clone()];
        deterministic_sort(&mut keys);

        assert_eq!(keys[0].tx_id(), TxId::from_bytes([1; 32]));
        assert_eq!(keys[1].tx_id(), TxId::from_bytes([2; 32]));
    }

    #[test]
    fn verify_order_accepts_sorted() {
        let mut keys = vec![
            OrderKey::new(300, 100, TxId::from_bytes([1; 32])),
            OrderKey::new(200, 200, TxId::from_bytes([2; 32])),
            OrderKey::new(100, 300, TxId::from_bytes([3; 32])),
        ];
        deterministic_sort(&mut keys);
        assert!(verify_order(&keys));
    }

    #[test]
    fn verify_order_rejects_unsorted() {
        let keys = vec![
            OrderKey::new(100, 100, TxId::from_bytes([1; 32])),
            OrderKey::new(200, 200, TxId::from_bytes([2; 32])),
        ];
        assert!(!verify_order(&keys));
    }

    #[test]
    fn verify_order_empty_and_single() {
        assert!(verify_order(&[]));
        let single = vec![OrderKey::new(100, 100, TxId::from_bytes([1; 32]))];
        assert!(verify_order(&single));
    }
}
