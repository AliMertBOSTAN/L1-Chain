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

use crate::types::{Amount, DatumHash, Height, OutPoint, ScriptHash, Slot, TxId};

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

    /// A coinbase transaction carried one or more inputs.
    #[error("coinbase transaction must not have inputs")]
    CoinbaseHasInputs,

    /// The coinbase height commitment does not match the block height.
    #[error("coinbase height mismatch: expected {expected}, got {actual}")]
    CoinbaseHeightMismatch {
        /// Height of the block embedding the coinbase.
        expected: u64,
        /// Height the coinbase actually committed to (its `lock_time`).
        actual: u64,
    },

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
///
/// Carries everything a recipient needs to detect the output with their
/// view key (ADR-011): `ephemeral_pubkey` is the hybrid-KEM ciphertext and
/// `kyber_level` is the Kyber parameter set required to decapsulate it.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StealthInfo {
    /// Ephemeral hybrid-KEM ciphertext; the recipient decapsulates it with
    /// their view secret key to recover the shared secret.
    pub ephemeral_pubkey: Vec<u8>,
    /// Kyber parameter level (1, 3, or 5) needed to decapsulate.
    pub kyber_level: u8,
    /// One-byte view tag for fast scanning.
    pub view_tag: u8,
}

impl core::fmt::Debug for StealthInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "StealthInfo(eph_pk={} bytes, kyber_level={}, tag=0x{:02x})",
            self.ephemeral_pubkey.len(),
            self.kyber_level,
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

    /// Compose a **coinbase** transaction — the block producer's reward
    /// claim. Coinbases have no inputs; their outputs mint the block
    /// subsidy plus the fees collected from the block's other transactions.
    ///
    /// # Height binding (replay / duplicate-TxId prevention)
    ///
    /// Two coinbases paying the same value to the same script would
    /// otherwise serialize identically and collide on `TxId` (Bitcoin's
    /// pre-BIP34 problem). We bind the coinbase to its block height by
    /// storing the height in the existing `lock_time` field
    /// (`lock_time = Slot(height)`), which is part of the canonical
    /// encoding and therefore of the `TxId`. This is the least-invasive
    /// deterministic equivalent of Bitcoin's BIP34 scriptSig-height rule:
    /// no new struct fields, no datum pollution on reward outputs, and it
    /// stays consistent with `lock_time`'s "not before" semantics because
    /// `slot >= height` always holds (each block advances the slot by at
    /// least one). Validators check the binding via
    /// [`Self::validate_coinbase_structure`].
    #[must_use]
    pub fn new_coinbase(height: Height, outputs: Vec<TxOutput>) -> Self {
        Self {
            version: TX_VERSION,
            inputs: Vec::new(),
            outputs,
            validity_interval: ValidityInterval::UNBOUNDED,
            lock_time: Slot::from(height.as_u64()),
            fee: Amount::ZERO,
        }
    }

    /// True iff this transaction has no inputs.
    ///
    /// At the ledger level a no-input transaction is either a coinbase
    /// (position 0 of a non-genesis block) or a genesis allocation
    /// (height-0 block). The distinction is positional and enforced by
    /// `Block::validate_structure`; this predicate only reports the shape.
    #[must_use]
    pub fn is_coinbase(&self) -> bool {
        self.inputs.is_empty()
    }

    /// The block height a coinbase committed to (see [`Self::new_coinbase`]).
    ///
    /// Returns `None` when the transaction has inputs (not a coinbase).
    #[must_use]
    pub fn coinbase_height(&self) -> Option<Height> {
        if self.is_coinbase() {
            Some(Height::from(self.lock_time.as_u64()))
        } else {
            None
        }
    }

    /// Structural checks for a coinbase transaction embedded in a block at
    /// `height`:
    ///
    /// - no inputs,
    /// - at least one output,
    /// - output values sum without overflow,
    /// - `lock_time` commits to exactly `height` (see [`Self::new_coinbase`]).
    ///
    /// The *amount* rule (`sum(outputs) <= subsidy + fees`) needs resolved
    /// input values and monetary parameters, so it lives in the node's
    /// block-validation pipeline, not here.
    pub fn validate_coinbase_structure(&self, height: Height) -> Result<(), TransactionError> {
        if !self.inputs.is_empty() {
            return Err(TransactionError::CoinbaseHasInputs);
        }
        if self.outputs.is_empty() {
            return Err(TransactionError::NoOutputs);
        }
        Amount::checked_sum(self.outputs.iter().map(|o| o.value))
            .ok_or(TransactionError::OutputOverflow)?;
        if self.lock_time.as_u64() != height.as_u64() {
            return Err(TransactionError::CoinbaseHeightMismatch {
                expected: height.as_u64(),
                actual: self.lock_time.as_u64(),
            });
        }
        Ok(())
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

    /// Compute the **signature hash** — the message every input signature
    /// commits to (ADR-012).
    ///
    /// Unlike [`Self::id`], all input witnesses are cleared before hashing,
    /// so the result is independent of the signatures themselves. This makes
    /// it safe to sign (no circular dependency: signature → witness → hash)
    /// and binds the signature to the transaction's inputs and outputs,
    /// closing the witness-replay theft vector.
    pub fn sighash(&self) -> Result<[u8; 32], TransactionError> {
        let mut bare = self.clone();
        for input in &mut bare.inputs {
            input.witness = Witness::default();
        }
        let bytes = bare.canonical_bytes()?;
        Ok(sha3_256(&bytes))
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
    fn sighash_is_deterministic() {
        let tx = simple_tx();
        assert_eq!(tx.sighash().unwrap(), tx.sighash().unwrap());
    }

    #[test]
    fn sighash_is_independent_of_witness() {
        // The whole point of ADR-012: changing a witness must NOT change the
        // sighash, so a signature can commit to it without circularity.
        let tx = simple_tx();
        let base = tx.sighash().unwrap();

        let mut signed = tx.clone();
        signed.inputs[0].witness = Witness::new(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(
            signed.sighash().unwrap(),
            base,
            "witness bytes must not affect the sighash"
        );

        let mut signed2 = tx.clone();
        signed2.inputs[0].witness = Witness::new(vec![0x01; 128]);
        assert_eq!(signed2.sighash().unwrap(), base);
    }

    #[test]
    fn sighash_changes_when_outputs_or_inputs_change() {
        let tx = simple_tx();
        let base = tx.sighash().unwrap();

        let mut redirected = tx.clone();
        redirected.outputs[0].locking_script = Script::new(vec![0xEE]);
        assert_ne!(
            redirected.sighash().unwrap(),
            base,
            "redirecting an output must change the sighash"
        );

        let mut more_value = tx.clone();
        more_value.outputs[0].value = Amount::from(101);
        assert_ne!(more_value.sighash().unwrap(), base);

        let mut extra_input = tx.clone();
        extra_input.inputs.push(TxInput::new(dummy_outpoint(9, 0)));
        assert_ne!(extra_input.sighash().unwrap(), base);
    }

    #[test]
    fn sighash_differs_from_txid_once_witnessed() {
        // txid includes witnesses; sighash excludes them. For a witnessed tx
        // the two must diverge, which is why txid cannot serve as a sighash.
        let mut tx = simple_tx();
        tx.inputs[0].witness = Witness::new(vec![0xAB; 64]);
        let txid = tx.id().unwrap();
        let sighash = tx.sighash().unwrap();
        assert_ne!(
            txid.as_bytes(),
            &sighash,
            "a witnessed tx's id must not equal its sighash"
        );
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
    fn coinbase_constructor_binds_height_via_lock_time() {
        let cb = Transaction::new_coinbase(
            Height::from(42),
            vec![TxOutput::new(Amount::from(5_000), Script::new(vec![0x01]))],
        );
        assert!(cb.is_coinbase());
        assert!(cb.inputs.is_empty());
        assert_eq!(cb.lock_time, Slot::from(42));
        assert_eq!(cb.coinbase_height(), Some(Height::from(42)));
        cb.validate_coinbase_structure(Height::from(42))
            .expect("well-formed coinbase");
    }

    #[test]
    fn coinbase_txid_is_unique_per_height() {
        // Same value, same script — only the height differs. The TxIds must
        // differ (this is the whole point of the height binding).
        let out = vec![TxOutput::new(Amount::from(100), Script::new(vec![0xAA]))];
        let cb1 = Transaction::new_coinbase(Height::from(1), out.clone());
        let cb2 = Transaction::new_coinbase(Height::from(2), out);
        assert_ne!(cb1.id().unwrap(), cb2.id().unwrap());
    }

    #[test]
    fn coinbase_rejects_height_mismatch() {
        let cb = Transaction::new_coinbase(
            Height::from(7),
            vec![TxOutput::new(Amount::from(1), Script::default())],
        );
        let err = cb.validate_coinbase_structure(Height::from(8)).unwrap_err();
        assert!(matches!(
            err,
            TransactionError::CoinbaseHeightMismatch {
                expected: 8,
                actual: 7
            }
        ));
    }

    #[test]
    fn coinbase_rejects_inputs_and_empty_outputs() {
        // A tx with inputs is not a coinbase.
        let with_inputs = simple_tx();
        assert!(!with_inputs.is_coinbase());
        assert_eq!(with_inputs.coinbase_height(), None);
        assert!(matches!(
            with_inputs.validate_coinbase_structure(Height::from(1)),
            Err(TransactionError::CoinbaseHasInputs)
        ));

        // A coinbase with no outputs is invalid.
        let empty = Transaction::new_coinbase(Height::from(1), vec![]);
        assert!(matches!(
            empty.validate_coinbase_structure(Height::from(1)),
            Err(TransactionError::NoOutputs)
        ));
    }

    #[test]
    fn coinbase_rejects_output_sum_overflow() {
        let cb = Transaction::new_coinbase(
            Height::from(1),
            vec![
                TxOutput::new(Amount::from(u64::MAX), Script::default()),
                TxOutput::new(Amount::from(1), Script::default()),
            ],
        );
        assert!(matches!(
            cb.validate_coinbase_structure(Height::from(1)),
            Err(TransactionError::OutputOverflow)
        ));
    }

    #[test]
    fn output_with_datum_and_stealth_roundtrip() {
        let out = TxOutput::new(Amount::from(1_000), Script::new(vec![0x01]))
            .with_datum(Datum::new(vec![0x02, 0x03]))
            .with_stealth(StealthInfo {
                ephemeral_pubkey: vec![0x04; 32],
                kyber_level: 3,
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
