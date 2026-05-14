//! Transactions — the minimal eUTXO-style ledger entry.
//!
//! A [`Transaction`] consumes a set of previously-unspent outputs
//! ([`TxInput`]s) and produces new outputs ([`TxOutput`]s). Each output
//! optionally carries:
//!
//! - A **locking script** ([`Script`]) that decides under which conditions
//!   it can later be spent. L1 only *verifies* scripts; it never executes
//!   arbitrary logic (see `qv-script`).
//! - A **datum** ([`Datum`]) — Cardano/eUTXO-style attached state. The datum
//!   is opaque bytes at the ledger level; scripts introspect it.
//! - **Stealth info** ([`StealthInfo`]) — the receiver's ephemeral key and
//!   view tag. Only relevant when the output is a stealth output.
//!
//! # Canonical encoding
//!
//! The transaction is serialised with `bincode` for hashing. `bincode`
//! produces a fixed little-endian, length-prefixed encoding, which is
//! deterministic for our purposes.
//!
//! The [`TxId`] of a transaction is `sha3_256(canonical_bytes(tx))`, and the
//! `TxId` itself is excluded from the canonical encoding (the struct has no
//! `tx_id` field; it is computed on demand).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use qv_crypto::sha3_256;

use crate::types::{Amount, DatumHash, OutPoint, ScriptHash, Slot, TxId};

// ============================================================================
// Errors
// ============================================================================

/// Errors that arise while constructing or decoding a transaction.
#[derive(Debug, Error)]
pub enum TransactionError {
    /// The transaction contained zero inputs.
    #[error("transaction has no inputs")]
    NoInputs,

    /// The transaction contained zero outputs.
    #[error("transaction has no outputs")]
    NoOutputs,

    /// The same `OutPoint` was referenced by more than one input.
    #[error("duplicate input references the same OutPoint")]
    DuplicateInput,

    /// Output values overflowed `u64` when summed.
    #[error("sum of output values overflows")]
    OutputOverflow,

    /// Canonical encoding failed.
    #[error("canonical encoding failed: {0}")]
    Encode(String),

    /// Canonical decoding failed.
    #[error("canonical decoding failed: {0}")]
    Decode(String),
}

// ============================================================================
// Script, Datum, StealthInfo — small newtypes over opaque bytes
// ============================================================================

/// A locking script (Plutus-style validator) carried by a [`TxOutput`].
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Script(pub Vec<u8>);

impl Script {
    /// Create a script from raw bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Script byte-length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True iff the script is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// SHA3-256 of the raw script bytes.
    #[must_use]
    pub fn hash(&self) -> ScriptHash {
        ScriptHash::from_bytes(sha3_256(&self.0))
    }
}

impl core::fmt::Debug for Script {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Script({} bytes)", self.0.len())
    }
}

/// eUTXO datum: opaque bytes attached to an output.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Datum(pub Vec<u8>);

impl Datum {
    /// Create a datum from raw bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Datum byte-length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True iff the datum is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// SHA3-256 of the raw datum bytes.
    #[must_use]
    pub fn hash(&self) -> DatumHash {
        DatumHash::from_bytes(sha3_256(&self.0))
    }
}

impl core::fmt::Debug for Datum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Datum({} bytes)", self.0.len())
    }
}

/// Stealth-address payload attached to a [`TxOutput`].
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StealthInfo {
    /// Ephemeral Kyber public key.
    pub ephemeral_pubkey: Vec<u8>,
    /// One-byte view tag.
    pub view_tag: u8,
}

impl core::fmt::Debug for StealthInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "StealthInfo(eph_pk={} bytes, tag=0x{:02x})",
            self.ephemeral_pubkey.len(),
            self.view_tag
        )
    }
}

/// Witness bytes supplied by an input to satisfy its prevout's locking
/// script. Opaque at this layer; interpreted by `qv-script`.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Witness(pub Vec<u8>);

impl Witness {
    /// Wrap raw bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrow the raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True iff empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl core::fmt::Debug for Witness {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Witness({} bytes)", self.0.len())
    }
}

// ============================================================================
// Inputs and outputs
// ============================================================================

/// A spent-output reference plus the witness required to unlock it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxInput {
    /// Output being consumed.
    pub prev_output: OutPoint,
    /// Satisfying witness.
    pub witness: Witness,
}

impl TxInput {
    /// Compose a fresh input with an empty witness.
    #[must_use]
    pub fn new(prev_output: OutPoint) -> Self {
        Self {
            prev_output,
            witness: Witness::default(),
        }
    }

    /// Attach / replace the witness.
    #[must_use]
    pub fn with_witness(mut self, witness: Witness) -> Self {
        self.witness = witness;
        self
    }
}

/// An on-ledger output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxOutput {
    /// The value carried by the output (smallest-unit amount).
    pub value: Amount,
    /// Locking script.
    pub locking_script: Script,
    /// Optional eUTXO datum.
    pub datum: Option<Datum>,
    /// Optional stealth-address payload.
    pub stealth_info: Option<StealthInfo>,
}

impl TxOutput {
    /// Compose a plain payment-to-script output.
    #[must_use]
    pub fn new(value: Amount, locking_script: Script) -> Self {
        Self {
            value,
            locking_script,
            datum: None,
            stealth_info: None,
        }
    }

    /// Attach a datum.
    #[must_use]
    pub fn with_datum(mut self, datum: Datum) -> Self {
        self.datum = Some(datum);
        self
    }

    /// Attach stealth payload.
    #[must_use]
    pub fn with_stealth(mut self, stealth: StealthInfo) -> Self {
        self.stealth_info = Some(stealth);
        self
    }
}

// ============================================================================
// Validity interval
// ============================================================================

/// Slot range during which a transaction is valid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValidityInterval {
    /// Transaction may not be included in a block with slot < `not_before`.
    pub not_before: Option<Slot>,
    /// Transaction may not be included in a block with slot > `not_after`.
    pub not_after: Option<Slot>,
}

impl ValidityInterval {
    /// An interval with no bounds — "always valid".
    pub const UNBOUNDED: Self = Self {
        not_before: None,
        not_after: None,
    };

    /// Compose a lower-bound-only interval.
    #[must_use]
    pub const fn at_or_after(not_before: Slot) -> Self {
        Self {
            not_before: Some(not_before),
            not_after: None,
        }
    }

    /// Compose an upper-bound-only interval.
    #[must_use]
    pub const fn at_or_before(not_after: Slot) -> Self {
        Self {
            not_before: None,
            not_after: Some(not_after),
        }
    }

    /// Compose a two-sided interval.
    #[must_use]
    pub const fn between(not_before: Slot, not_after: Slot) -> Self {
        Self {
            not_before: Some(not_before),
            not_after: Some(not_after),
        }
    }

    /// True iff `slot` is inside the interval.
    #[must_use]
    pub fn contains(&self, slot: Slot) -> bool {
        let lower_ok = match self.not_before {
            Some(lo) => slot >= lo,
            None => true,
        };
        let upper_ok = match self.not_after {
            Some(hi) => slot <= hi,
            None => true,
        };
        lower_ok && upper_ok
    }
}

// ============================================================================
// Transaction
// ============================================================================

/// Current transaction format version.
pub const TX_VERSION: u32 = 1;

/// A ledger transaction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Transaction {
    /// Format version. See [`TX_VERSION`].
    pub version: u32,
    /// Inputs being consumed.
    pub inputs: Vec<TxInput>,
    /// Outputs being created.
    pub outputs: Vec<TxOutput>,
    /// Slot range during which this tx may be included.
    pub validity_interval: ValidityInterval,
    /// Optional absolute-slot lock.
    pub lock_time: Slot,
    /// Fee paid to the slot leader.
    pub fee: Amount,
}

impl Transaction {
    /// Compose a new transaction body.
    #[must_use]
    pub fn new(inputs: Vec<TxInput>, outputs: Vec<TxOutput>) -> Self {
        Self {
            version: TX_VERSION,
            inputs,
            outputs,
            validity_interval: ValidityInterval::UNBOUNDED,
            lock_time: Slot::GENESIS,
            fee: Amount::ZERO,
        }
    }

    /// Compose a genesis transaction with no inputs (initial UTXO allocation).
    ///
    /// Genesis transactions are special: they bypass the normal structural
    /// validation that requires non-empty inputs, since they represent the
    /// initial coin distribution that bootstraps the ledger.
    #[must_use]
    pub fn genesis(outputs: Vec<TxOutput>) -> Self {
        Self {
            version: TX_VERSION,
            inputs: Vec::new(),
            outputs,
            validity_interval: ValidityInterval::UNBOUNDED,
            lock_time: Slot::GENESIS,
            fee: Amount::ZERO,
        }
    }

    /// Replace the validity interval.
    #[must_use]
    pub fn with_validity(mut self, interval: ValidityInterval) -> Self {
        self.validity_interval = interval;
        self
    }

    /// Replace the lock time.
    #[must_use]
    pub fn with_lock_time(mut self, lock_time: Slot) -> Self {
        self.lock_time = lock_time;
        self
    }

    /// Replace the fee.
    #[must_use]
    pub fn with_fee(mut self, fee: Amount) -> Self {
        self.fee = fee;
        self
    }

    /// Fast structural checks — called before hashing or submitting.
    pub fn validate_structure(&self) -> Result<(), TransactionError> {
        if self.inputs.is_empty() {
            return Err(TransactionError::NoInputs);
        }
        if self.outputs.is_empty() {
            return Err(TransactionError::NoOutputs);
        }

        let mut seen: Vec<OutPoint> = self.inputs.iter().map(|i| i.prev_output).collect();
        seen.sort_unstable();
        if seen.windows(2).any(|w| match w {
            [a, b] => a == b,
            _ => false,
        }) {
            return Err(TransactionError::DuplicateInput);
        }

        Amount::checked_sum(self.outputs.iter().map(|o| o.value))
            .ok_or(TransactionError::OutputOverflow)?;

        Ok(())
    }

    /// Encode the transaction canonically (bincode).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, TransactionError> {
        bincode::serialize(self).map_err(|e| TransactionError::Encode(e.to_string()))
    }

    /// Decode from canonical (bincode) bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, TransactionError> {
        bincode::deserialize(bytes).map_err(|e| TransactionError::Decode(e.to_string()))
    }

    /// Compute the SHA3-256 identifier of this transaction.
    pub fn id(&self) -> Result<TxId, TransactionError> {
        let bytes = self.canonical_bytes()?;
        Ok(TxId::from_bytes(sha3_256(&bytes)))
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
    clippy::cast_possible_truncation,
    clippy::cast_lossless
)]
mod tests {
    use super::*;

    fn dummy_outpoint(byte: u8, index: u32) -> OutPoint {
        OutPoint::new(TxId::from_bytes([byte; 32]), index)
    }

    fn simple_tx() -> Transaction {
        Transaction::new(
            vec![TxInput::new(dummy_outpoint(1, 0))],
            vec![TxOutput::new(
                Amount::from(100),
                Script::new(vec![0xAA, 0xBB]),
            )],
        )
    }

    #[test]
    fn txid_changes_when_any_field_changes() {
        let tx = simple_tx();
        let id1 = tx.id().unwrap();

        let mut tx2 = tx.clone();
        tx2.outputs[0].value = Amount::from(101);
        let id2 = tx2.id().unwrap();
        assert_ne!(id1, id2, "value flip must change TxId");

        let mut tx3 = tx.clone();
        tx3.lock_time = Slot::from(1);
        let id3 = tx3.id().unwrap();
        assert_ne!(id1, id3, "lock_time flip must change TxId");

        let mut tx4 = tx.clone();
        tx4.inputs.push(TxInput::new(dummy_outpoint(2, 0)));
        let id4 = tx4.id().unwrap();
        assert_ne!(id1, id4, "adding input must change TxId");
    }

    #[test]
    fn txid_is_deterministic() {
        let tx = simple_tx();
        let a = tx.id().unwrap();
        let b = tx.id().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let tx = simple_tx()
            .with_fee(Amount::from(7))
            .with_lock_time(Slot::from(42))
            .with_validity(ValidityInterval::between(Slot::from(10), Slot::from(100)));
        let bytes = tx.canonical_bytes().unwrap();
        let back = Transaction::decode(&bytes).unwrap();
        assert_eq!(tx, back);
        assert_eq!(tx.id().unwrap(), back.id().unwrap());
    }

    #[test]
    fn reject_empty_inputs() {
        let tx = Transaction::new(vec![], vec![TxOutput::default()]);
        assert!(matches!(
            tx.validate_structure(),
            Err(TransactionError::NoInputs)
        ));
    }

    #[test]
    fn reject_empty_outputs() {
        let tx = Transaction::new(vec![TxInput::new(dummy_outpoint(1, 0))], vec![]);
        assert!(matches!(
            tx.validate_structure(),
            Err(TransactionError::NoOutputs)
        ));
    }

    #[test]
    fn reject_duplicate_inputs() {
        let op = dummy_outpoint(1, 0);
        let tx = Transaction::new(
            vec![TxInput::new(op), TxInput::new(op)],
            vec![TxOutput::new(Amount::from(1), Script::default())],
        );
        assert!(matches!(
            tx.validate_structure(),
            Err(TransactionError::DuplicateInput)
        ));
    }

    #[test]
    fn reject_output_sum_overflow() {
        let big = Amount::from(u64::MAX);
        let tx = Transaction::new(
            vec![TxInput::new(dummy_outpoint(1, 0))],
            vec![
                TxOutput::new(big, Script::default()),
                TxOutput::new(Amount::from(1), Script::default()),
            ],
        );
        assert!(matches!(
            tx.validate_structure(),
            Err(TransactionError::OutputOverflow)
        ));
    }

    #[test]
    fn accept_well_formed_tx() {
        let tx = simple_tx();
        tx.validate_structure().expect("well-formed");
    }

    #[test]
    fn validity_interval_contains() {
        let iv = ValidityInterval::between(Slot::from(10), Slot::from(20));
        assert!(!iv.contains(Slot::from(9)));
        assert!(iv.contains(Slot::from(10)));
        assert!(iv.contains(Slot::from(15)));
        assert!(iv.contains(Slot::from(20)));
        assert!(!iv.contains(Slot::from(21)));
    }

    #[test]
    fn validity_interval_unbounded_accepts_everything() {
        let iv = ValidityInterval::UNBOUNDED;
        assert!(iv.contains(Slot::GENESIS));
        assert!(iv.contains(Slot::from(u64::MAX)));
    }

    #[test]
    fn script_and_datum_hash_are_stable() {
        let s = Script::new(b"OP_CHECKSIG_PQC".to_vec());
        let h1 = s.hash();
        let h2 = Script::new(b"OP_CHECKSIG_PQC".to_vec()).hash();
        assert_eq!(h1, h2);

        let d = Datum::new(b"(100, 200, 30)".to_vec());
        assert_eq!(d.hash(), Datum::new(b"(100, 200, 30)".to_vec()).hash());
        assert_ne!(d.hash(), Datum::new(b"(100, 200, 31)".to_vec()).hash());
    }

    #[test]
    fn output_with_datum_and_stealth_roundtrip() {
        let out = TxOutput::new(Amount::from(1_000), Script::new(vec![0x01]))
            .with_datum(Datum::new(vec![0x02, 0x03]))
            .with_stealth(StealthInfo {
                ephemeral_pubkey: vec![0x04; 32],
                view_tag: 0x7F,
            });
        let bytes = bincode::serialize(&out).unwrap();
        let back: TxOutput = bincode::deserialize(&bytes).unwrap();
        assert_eq!(out, back);
    }

    #[test]
    fn debug_formats_are_compact_and_secret_safe() {
        let s = format!("{:?}", Script::new(vec![0x11; 1024]));
        assert!(s.contains("1024 bytes"));
        let d = format!("{:?}", Datum::new(vec![0x22; 16]));
        assert!(d.contains("16 bytes"));
        let w = format!("{:?}", Witness::new(vec![0x33; 48]));
        assert!(w.contains("48 bytes"));
        assert!(!s.contains(&"11".repeat(32)));
    }
}
