//\! The UTXO set — a map from [`OutPoint`] to [`TxOutput`].
//\!
//\! [`UtxoSet`] is a trait so that the higher-level validator does not care
//\! whether the set lives in memory, on disk (`qv-storage`), or behind a
//\! network RPC. This crate ships the reference [`InMemoryUtxoSet`]; the
//\! persistent variant lives in `qv-storage` (Aşama 5).
//\!
//\! # Commitment
//\!
//\! [`UtxoSet::commitment_root`] returns a [`UtxoCommitment`] — the hash of
//\! a deterministic, sorted Merkle tree over the active UTXO set.  This is
//\! the same value the block header carries in `utxo_commitment`, letting
//\! light clients verify state roots across forks without downloading the
//\! full set.
//\!
//\! Today's Merkle construction matches the one in `block.rs` (binary,
//\! duplicate-last padding). We'll replace it with a sparse Merkle tree in
//\! Aşama 5 once the persistent backend is implemented.

use std::collections::BTreeMap;

use qv_crypto::sha3_256;
use thiserror::Error;

use crate::block::merkle_root_of;
use crate::transaction::TxOutput;
use crate::types::{OutPoint, TxId, UtxoCommitment};

// ============================================================================
// Errors
// ============================================================================

/// Errors arising from UTXO-set operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum UtxoError {
    /// Attempted to insert at an `OutPoint` that already exists.
    #[error("outpoint already present in UTXO set")]
    DuplicateOutPoint,

    /// Attempted to spend an `OutPoint` that is not in the set.
    #[error("outpoint not found in UTXO set")]
    NotFound,
}

// ============================================================================
// Trait
// ============================================================================

/// Abstract UTXO set. A validator uses this trait to apply / revert blocks
/// and queries without binding to a specific storage backend.
pub trait UtxoSet {
    /// Insert a new unspent output.
    ///
    /// # Errors
    /// Returns [`UtxoError::DuplicateOutPoint`] if `outpoint` is already
    /// present.
    fn insert(&mut self, outpoint: OutPoint, output: TxOutput) -> Result<(), UtxoError>;

    /// Remove (spend) an existing output, returning the removed [`TxOutput`].
    ///
    /// # Errors
    /// Returns [`UtxoError::NotFound`] if `outpoint` is absent.
    fn remove(&mut self, outpoint: &OutPoint) -> Result<TxOutput, UtxoError>;

    /// Fetch an unspent output by reference.
    fn get(&self, outpoint: &OutPoint) -> Option<&TxOutput>;

    /// True iff `outpoint` is currently unspent.
    fn contains(&self, outpoint: &OutPoint) -> bool {
        self.get(outpoint).is_some()
    }

    /// Number of unspent outputs currently tracked.
    fn len(&self) -> usize;

    /// True iff the set is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Commit to the current state as a single 32-byte hash.
    ///
    /// The commitment MUST be a pure function of the set's contents —
    /// equal sets must produce equal commitments regardless of insertion
    /// order.
    fn commitment_root(&self) -> UtxoCommitment;
}

// ============================================================================
// In-memory implementation
// ============================================================================

/// Reference `UtxoSet` implementation backed by a [`BTreeMap`].
///
/// `BTreeMap` gives us sorted iteration for free, so the commitment is
/// deterministic without any extra work.
#[derive(Clone, Debug, Default)]
pub struct InMemoryUtxoSet {
    map: BTreeMap<OutPoint, TxOutput>,
}

impl InMemoryUtxoSet {
    /// Create an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Iterate over `(outpoint, output)` pairs in canonical (`Ord`) order.
    pub fn iter(&self) -> impl Iterator<Item = (&OutPoint, &TxOutput)> {
        self.map.iter()
    }
}

impl UtxoSet for InMemoryUtxoSet {
    fn insert(&mut self, outpoint: OutPoint, output: TxOutput) -> Result<(), UtxoError> {
        if self.map.contains_key(&outpoint) {
            return Err(UtxoError::DuplicateOutPoint);
        }
        self.map.insert(outpoint, output);
        Ok(())
    }

    fn remove(&mut self, outpoint: &OutPoint) -> Result<TxOutput, UtxoError> {
        self.map.remove(outpoint).ok_or(UtxoError::NotFound)
    }

    fn get(&self, outpoint: &OutPoint) -> Option<&TxOutput> {
        self.map.get(outpoint)
    }

    fn len(&self) -> usize {
        self.map.len()
    }

    fn commitment_root(&self) -> UtxoCommitment {
        commitment_root_of_sorted_entries(self.map.iter())
    }
}

// ============================================================================
// Commitment helper
// ============================================================================

/// Build a commitment over `(OutPoint, TxOutput)` pairs that are assumed to
/// iterate in canonical order (lexicographic on `OutPoint`).
///
/// Exposed so that `qv-storage` can reuse the same hashing convention when
/// computing commitments over a persistent backend.
#[must_use]
pub fn commitment_root_of_sorted_entries<'a, I>(entries: I) -> UtxoCommitment
where
    I: IntoIterator<Item = (&'a OutPoint, &'a TxOutput)>,
{
    // Per-entry leaf hash:
    //   leaf = SHA3-256( outpoint.canonical_bytes || bincode(output) )
    //
    // We then feed each leaf as a `TxId`-typed leaf into the shared Merkle
    // root construction. This is type juggling — the commitment is *not* a
    // TxId — but the hash function is identical and we avoid duplicating
    // Merkle logic. The returned `UtxoCommitment` is re-tagged via `From`.
    let leaves: Vec<TxId> = entries
        .into_iter()
        .map(|(op, out)| {
            let op_bytes = op.canonical_bytes();
            // bincode cannot fail on an owned, serializable value under
            // normal conditions. We fall back to an empty vec on the
            // (impossible) error so the commitment stays defined.
            let out_bytes = bincode::serialize(out).unwrap_or_default();
            let mut buf = Vec::with_capacity(op_bytes.len() + out_bytes.len());
            buf.extend_from_slice(&op_bytes);
            buf.extend_from_slice(&out_bytes);
            TxId::from_bytes(sha3_256(&buf))
        })
        .collect();

    let merkle = merkle_root_of(&leaves);
    UtxoCommitment::from_bytes(merkle.to_bytes())
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
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;
    use crate::transaction::Script;
    use crate::types::Amount;

    fn op(byte: u8, idx: u32) -> OutPoint {
        OutPoint::new(TxId::from_bytes([byte; 32]), idx)
    }

    fn out(value: u64, script_byte: u8) -> TxOutput {
        TxOutput::new(Amount::from(value), Script::new(vec\![script_byte]))
    }

    #[test]
    fn insert_get_remove_basic() {
        let mut set = InMemoryUtxoSet::new();
        let a = op(1, 0);
        let o = out(500, 0x11);
        assert\!(set.is_empty());

        set.insert(a, o.clone()).unwrap();
        assert_eq\!(set.len(), 1);
        assert\!(set.contains(&a));
        assert_eq\!(set.get(&a), Some(&o));

        let removed = set.remove(&a).unwrap();
        assert_eq\!(removed, o);
        assert\!(set.is_empty());
        assert_eq\!(set.get(&a), None);
    }

    #[test]
    fn duplicate_insert_rejected() {
        let mut set = InMemoryUtxoSet::new();
        let a = op(1, 0);
        set.insert(a, out(1, 0)).unwrap();
        let err = set.insert(a, out(2, 0)).unwrap_err();
        assert_eq\!(err, UtxoError::DuplicateOutPoint);
    }

    #[test]
    fn missing_remove_errors() {
        let mut set = InMemoryUtxoSet::new();
        let err = set.remove(&op(9, 9)).unwrap_err();
        assert_eq\!(err, UtxoError::NotFound);
    }

    #[test]
    fn commitment_empty_is_zero() {
        let set = InMemoryUtxoSet::new();
        assert_eq\!(set.commitment_root(), UtxoCommitment::ZERO);
    }

    #[test]
    fn commitment_changes_when_entry_changes() {
        let mut a = InMemoryUtxoSet::new();
        a.insert(op(1, 0), out(100, 0x11)).unwrap();
        let root_a = a.commitment_root();

        let mut b = InMemoryUtxoSet::new();
        b.insert(op(1, 0), out(101, 0x11)).unwrap();
        let root_b = b.commitment_root();
        assert_ne\!(root_a, root_b, "value change must change commitment");

        let mut c = InMemoryUtxoSet::new();
        c.insert(op(1, 0), out(100, 0x22)).unwrap();
        assert_ne\!(
            root_a,
            c.commitment_root(),
            "script change must change commitment"
        );
    }

    #[test]
    fn commitment_is_insertion_order_independent() {
        let entries = [
            (op(1, 0), out(10, 0x11)),
            (op(2, 0), out(20, 0x22)),
            (op(3, 0), out(30, 0x33)),
        ];

        let mut forward = InMemoryUtxoSet::new();
        for (k, v) in &entries {
            forward.insert(*k, v.clone()).unwrap();
        }

        let mut reverse = InMemoryUtxoSet::new();
        for (k, v) in entries.iter().rev() {
            reverse.insert(*k, v.clone()).unwrap();
        }

        assert_eq\!(forward.commitment_root(), reverse.commitment_root());
    }

    #[test]
    fn commitment_reacts_to_membership_changes() {
        let mut set = InMemoryUtxoSet::new();
        set.insert(op(1, 0), out(10, 0x11)).unwrap();
        let root1 = set.commitment_root();

        set.insert(op(2, 0), out(20, 0x22)).unwrap();
        let root2 = set.commitment_root();
        assert_ne\!(root1, root2, "adding entry must move commitment");

        set.remove(&op(2, 0)).unwrap();
        let root3 = set.commitment_root();
        assert_eq\!(
            root1, root3,
            "removing restores commitment to previous state"
        );
    }

    #[test]
    fn iter_is_sorted_by_outpoint() {
        let mut set = InMemoryUtxoSet::new();
        set.insert(op(3, 0), out(30, 0x33)).unwrap();
        set.insert(op(1, 0), out(10, 0x11)).unwrap();
        set.insert(op(2, 0), out(20, 0x22)).unwrap();

        let ordered: Vec<OutPoint> = set.iter().map(|(k, _)| *k).collect();
        assert_eq\!(ordered, vec\![op(1, 0), op(2, 0), op(3, 0)]);
    }

    #[test]
    fn large_set_commitment_is_deterministic() {
        let mut set = InMemoryUtxoSet::new();
        for i in 0u8..32 {
            set.insert(op(i, u32::from(i)), out(u64::from(i) * 7, i))
                .unwrap();
        }
        let r1 = set.commitment_root();
        let r2 = set.commitment_root();
        assert_eq\!(r1, r2);
    }
}
