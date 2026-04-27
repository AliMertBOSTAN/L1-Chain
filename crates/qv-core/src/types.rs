//! Core primitive types shared across the ledger.
//!
//! Everything in this module is intentionally small, `Copy` where possible,
//! and has a canonical binary encoding (via `serde` + `bincode`). Hex is the
//! standard textual form for all 32-byte hash types.
//!
//! # Types at a glance
//!
//! | Type         | Size    | Purpose                                    |
//! |--------------|---------|--------------------------------------------|
//! | [`Hash256`]  | 32 B    | Generic 256-bit cryptographic digest       |
//! | [`TxId`]     | 32 B    | Transaction identifier (hash of canonical) |
//! | [`BlockHash`]| 32 B    | Block header hash                          |
//! | [`ScriptHash`]| 32 B   | Hash of a locking script / validator       |
//! | [`DatumHash`]| 32 B    | Hash of an eUTXO datum                     |
//! | [`Height`]   | u64     | Chain height                               |
//! | [`Slot`]     | u64     | Ouroboros Praos slot number                |
//! | [`Epoch`]    | u64     | Ouroboros Praos epoch number               |
//! | [`Timestamp`]| u64     | Unix seconds                               |
//! | [`Amount`]   | u64     | Token quantity in the smallest unit        |
//! | [`OutPoint`] | 40 B    | `(TxId, u32)` — an unspent output ref      |

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Errors
// ============================================================================

/// Errors produced when parsing or decoding a ledger primitive.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TypeError {
    /// Hex string had the wrong length for a 32-byte digest.
    #[error("expected {expected} hex chars, got {actual}")]
    HexLength {
        /// Required number of hex characters (always 64 for a 32-byte digest).
        expected: usize,
        /// The length we actually observed.
        actual: usize,
    },
    /// Input contained non-hex characters.
    #[error("invalid hex encoding")]
    HexFormat,
    /// A numeric field was out of range (e.g. a non-numeric output index).
    #[error("numeric value out of range: {0}")]
    OutOfRange(&'static str),
}

// ============================================================================
// Hash256 — the shared 32-byte primitive
// ============================================================================

/// A 32-byte cryptographic digest. Used as the underlying representation for
/// every identifier-like type in the ledger (transactions, blocks, scripts,
/// datums, ...). Types below are thin newtypes over `Hash256`.
///
/// Textual encoding is lowercase hex without any `0x` prefix.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Hash256(pub [u8; 32]);

impl Hash256 {
    /// A zero-valued digest. Useful as a sentinel (e.g. the genesis block's
    /// `prev_hash` field) but should never appear as a real TxId.
    pub const ZERO: Self = Self([0u8; 32]);

    /// Length in bytes.
    pub const LEN: usize = 32;

    /// Construct from a fixed-size byte array.
    #[must_use]
    pub const fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    /// Borrow the underlying bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Copy the underlying bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Lowercase hex rendering without a `0x` prefix.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from a 64-character lowercase or uppercase hex string.
    pub fn from_hex(s: &str) -> Result<Self, TypeError> {
        if s.len() != 64 {
            return Err(TypeError::HexLength {
                expected: 64,
                actual: s.len(),
            });
        }
        let raw = hex::decode(s).map_err(|_| TypeError::HexFormat)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        Ok(Self(out))
    }

    /// Return `true` if every byte is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

impl Default for Hash256 {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Debug for Hash256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Short form for logs: first 4 + last 4 bytes. Keeps lines readable.
        let hex = self.to_hex();
        let (head, tail) = hex.split_at(8);
        let tail_start = tail.len().saturating_sub(8);
        write!(f, "Hash256({}…{})", head, &tail[tail_start..])
    }
}

impl fmt::Display for Hash256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Hash256 {
    type Err = TypeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl From<[u8; 32]> for Hash256 {
    fn from(b: [u8; 32]) -> Self {
        Self(b)
    }
}

impl From<Hash256> for [u8; 32] {
    fn from(h: Hash256) -> Self {
        h.0
    }
}

impl AsRef<[u8]> for Hash256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// ----------------------------------------------------------------------------
// Convenience: derive an identifier newtype over Hash256 without copy-paste.
// ----------------------------------------------------------------------------

macro_rules! define_hash_newtype {
    ($(#[$outer:meta])* $name:ident) => {
        $(#[$outer])*
        #[repr(transparent)]
        #[derive(
            Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize,
        )]
        pub struct $name(pub Hash256);

        impl $name {
            /// Zero-valued identifier (sentinel only).
            pub const ZERO: Self = Self(Hash256::ZERO);
            /// Length in bytes (always 32).
            pub const LEN: usize = Hash256::LEN;

            /// Wrap a fixed-size byte array.
            #[must_use]
            pub const fn from_bytes(b: [u8; 32]) -> Self {
                Self(Hash256::from_bytes(b))
            }

            /// Borrow the underlying bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                self.0.as_bytes()
            }

            /// Copy the underlying bytes.
            #[must_use]
            pub const fn to_bytes(self) -> [u8; 32] {
                self.0.to_bytes()
            }

            /// Lowercase hex rendering without a `0x` prefix.
            #[must_use]
            pub fn to_hex(&self) -> String {
                self.0.to_hex()
            }

            /// Parse from a 64-character hex string.
            pub fn from_hex(s: &str) -> Result<Self, TypeError> {
                Hash256::from_hex(s).map(Self)
            }

            /// True iff all bytes are zero.
            #[must_use]
            pub fn is_zero(&self) -> bool {
                self.0.is_zero()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let hex = self.to_hex();
                let (head, tail) = hex.split_at(8);
                let tail_start = tail.len().saturating_sub(8);
                write!(
                    f,
                    concat!(stringify!($name), "({}…{})"),
                    head,
                    &tail[tail_start..]
                )
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = TypeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::from_hex(s)
            }
        }

        impl From<[u8; 32]> for $name {
            fn from(b: [u8; 32]) -> Self {
                Self::from_bytes(b)
            }
        }

        impl From<Hash256> for $name {
            fn from(h: Hash256) -> Self {
                Self(h)
            }
        }

        impl From<$name> for Hash256 {
            fn from(x: $name) -> Self {
                x.0
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                self.0.as_ref()
            }
        }
    };
}

define_hash_newtype! {
    /// Transaction identifier: `H(canonical_transaction_bytes)`.
    TxId
}

define_hash_newtype! {
    /// Block identifier: `H(canonical_block_header_bytes)`.
    BlockHash
}

define_hash_newtype! {
    /// Hash of a locking script / Plutus-style validator.
    ScriptHash
}

define_hash_newtype! {
    /// Hash of a datum (eUTXO attached state).
    DatumHash
}

define_hash_newtype! {
    /// Hash of a UTXO set commitment (see [`UtxoSet::commitment_root`]).
    ///
    /// [`UtxoSet::commitment_root`]: crate::utxo::UtxoSet::commitment_root
    UtxoCommitment
}

define_hash_newtype! {
    /// Hash of a Merkle tree root over transactions in a block.
    MerkleRoot
}

// ============================================================================
// Numeric newtypes
// ============================================================================

/// Chain height (0 = genesis, monotonically increasing).
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Height(pub u64);

impl Height {
    /// Genesis height.
    pub const GENESIS: Self = Self(0);

    /// Return the raw integer.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Saturating increment.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Debug for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Height({})", self.0)
    }
}

impl fmt::Display for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Height {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

/// Ouroboros-Praos slot number.
///
/// Slots tick monotonically regardless of whether a block is produced, so
/// `Slot` and `Height` diverge over time.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Slot(pub u64);

impl Slot {
    /// Genesis slot.
    pub const GENESIS: Self = Self(0);

    /// Return the raw integer.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Saturating increment.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slot({})", self.0)
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Slot {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

/// Ouroboros-Praos epoch number (groups of [`ProtocolParams::epoch_slots`]
/// slots).
///
/// [`ProtocolParams::epoch_slots`]: crate::params::ProtocolParams::epoch_slots
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Epoch(pub u64);

impl Epoch {
    /// Genesis epoch.
    pub const GENESIS: Self = Self(0);

    /// Return the raw integer.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Epoch({})", self.0)
    }
}

impl fmt::Display for Epoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Epoch {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

/// Unix timestamp in seconds.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Timestamp(pub u64);

impl Timestamp {
    /// Wrap a unix timestamp.
    #[must_use]
    pub const fn from_unix_secs(s: u64) -> Self {
        Self(s)
    }

    /// Return raw seconds.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({})", self.0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Timestamp {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

/// Token amount in the smallest indivisible unit (think satoshis).
///
/// `u64` gives a ceiling of ~1.8·10^19 units. With 8 decimals of precision
/// against a 21M nominal supply, this leaves ample headroom.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct Amount(pub u64);

impl Amount {
    /// Zero tokens.
    pub const ZERO: Self = Self(0);

    /// Construct from the raw smallest-unit integer.
    #[must_use]
    pub const fn from_smallest_units(n: u64) -> Self {
        Self(n)
    }

    /// Underlying smallest-unit integer.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Checked addition. Returns `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on underflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Sum an iterator of amounts, returning `None` on overflow.
    pub fn checked_sum<I: IntoIterator<Item = Self>>(iter: I) -> Option<Self> {
        iter.into_iter()
            .try_fold(Self::ZERO, |acc, x| acc.checked_add(x))
    }
}

impl fmt::Debug for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Amount({})", self.0)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for Amount {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

// ============================================================================
// OutPoint — pointer to a specific output of a specific transaction
// ============================================================================

/// Reference to an individual output within some transaction.
///
/// `OutPoint` is `Ord` so that it can be used as a `BTreeMap` key for the
/// UTXO set. The sort order is stable and canonical: first by `tx_id` byte
/// order, then by `index`.
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct OutPoint {
    /// Transaction that produced the output.
    pub tx_id: TxId,
    /// Zero-based index of the output within that transaction.
    pub index: u32,
}

impl OutPoint {
    /// Compose a new `OutPoint`.
    #[must_use]
    pub const fn new(tx_id: TxId, index: u32) -> Self {
        Self { tx_id, index }
    }

    /// Canonical byte representation: `tx_id || index_le`. Used anywhere we
    /// need a stable key (UTXO commitment, script introspection, etc.).
    #[must_use]
    pub fn canonical_bytes(&self) -> [u8; 36] {
        let mut out = [0u8; 36];
        let (left, right) = out.split_at_mut(32);
        left.copy_from_slice(self.tx_id.as_bytes());
        right.copy_from_slice(&self.index.to_le_bytes());
        out
    }
}

impl fmt::Debug for OutPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OutPoint({}#{})", self.tx_id, self.index)
    }
}

impl fmt::Display for OutPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.tx_id, self.index)
    }
}

impl FromStr for OutPoint {
    type Err = TypeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((tx_part, idx_part)) = s.split_once('#') else {
            return Err(TypeError::HexFormat);
        };
        let tx_id = TxId::from_hex(tx_part)?;
        let index: u32 = idx_part
            .parse()
            .map_err(|_| TypeError::OutOfRange("OutPoint::index"))?;
        Ok(Self { tx_id, index })
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

    #[test]
    fn hash256_zero_is_all_zero() {
        assert!(Hash256::ZERO.is_zero());
        assert!(Hash256::default().is_zero());
    }

    #[test]
    fn hash256_hex_roundtrip() {
        let mut raw = [0u8; 32];
        for (i, b) in raw.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(17);
        }
        let h = Hash256::from_bytes(raw);
        let s = h.to_hex();
        assert_eq!(s.len(), 64);
        let back = Hash256::from_hex(&s).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn hash256_hex_length_validation() {
        let err = Hash256::from_hex("ab").unwrap_err();
        assert_eq!(
            err,
            TypeError::HexLength {
                expected: 64,
                actual: 2
            }
        );
    }

    #[test]
    fn hash256_hex_invalid_chars() {
        let bad = "Z".repeat(64);
        let err = Hash256::from_hex(&bad).unwrap_err();
        assert_eq!(err, TypeError::HexFormat);
    }

    #[test]
    fn hash256_debug_is_compact() {
        let s = format!("{:?}", Hash256::from_bytes([0xAB; 32]));
        // Contains the ellipsis separator we insert.
        assert!(s.contains('…'));
        // And does NOT contain all 64 hex chars expanded.
        assert!(!s.contains(&"ab".repeat(32)));
    }

    #[test]
    fn hash256_display_is_full_hex() {
        let s = format!("{}", Hash256::from_bytes([0u8; 32]));
        assert_eq!(s.len(), 64);
        assert_eq!(s, "0".repeat(64));
    }

    #[test]
    fn newtype_conversions() {
        let raw = [9u8; 32];
        let tx: TxId = raw.into();
        assert_eq!(tx.to_bytes(), raw);

        let h: Hash256 = tx.into();
        assert_eq!(h.to_bytes(), raw);

        let back: TxId = h.into();
        assert_eq!(back, tx);
    }

    #[test]
    fn newtype_debug_differs_by_name() {
        let raw = [0xAAu8; 32];
        let tx = format!("{:?}", TxId::from_bytes(raw));
        let blk = format!("{:?}", BlockHash::from_bytes(raw));
        assert!(tx.starts_with("TxId("));
        assert!(blk.starts_with("BlockHash("));
    }

    #[test]
    fn outpoint_canonical_bytes_layout() {
        let tx = TxId::from_bytes([0x11; 32]);
        let op = OutPoint::new(tx, 0x0403_0201);
        let bytes = op.canonical_bytes();
        // First 32 bytes = tx id
        assert_eq!(&bytes[..32], &[0x11u8; 32]);
        // Next 4 = little-endian index
        assert_eq!(&bytes[32..], &[0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn outpoint_ord_is_lex_first_then_index() {
        let a = OutPoint::new(TxId::from_bytes([0x01; 32]), 1);
        let b = OutPoint::new(TxId::from_bytes([0x01; 32]), 2);
        let c = OutPoint::new(TxId::from_bytes([0x02; 32]), 0);
        let mut v = vec![c, b, a];
        v.sort();
        assert_eq!(v, vec![a, b, c]);
    }

    #[test]
    fn outpoint_display_roundtrip() {
        let op = OutPoint::new(TxId::from_bytes([0xCD; 32]), 42);
        let s = op.to_string();
        let back: OutPoint = s.parse().unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn outpoint_display_rejects_malformed() {
        assert!("nothash#1".parse::<OutPoint>().is_err());
        assert!("abc".parse::<OutPoint>().is_err());
    }

    #[test]
    fn amount_checked_arithmetic() {
        let a = Amount::from(100);
        let b = Amount::from(50);
        assert_eq!(a.checked_add(b), Some(Amount::from(150)));
        assert_eq!(a.checked_sub(b), Some(Amount::from(50)));
        assert_eq!(b.checked_sub(a), None);
        assert_eq!(Amount::from(u64::MAX).checked_add(Amount::from(1)), None);
    }

    #[test]
    fn amount_sum_overflow_detected() {
        let big = Amount::from(u64::MAX);
        let s = Amount::checked_sum([big, Amount::from(1)]);
        assert_eq!(s, None);

        let s2 = Amount::checked_sum([Amount::from(1), Amount::from(2), Amount::from(3)]);
        assert_eq!(s2, Some(Amount::from(6)));
    }

    #[test]
    fn height_and_slot_saturate() {
        let max = Height::from(u64::MAX);
        assert_eq!(max.next(), max);
        let s = Slot::from(u64::MAX);
        assert_eq!(s.next(), s);
    }

    #[test]
    fn serde_bincode_roundtrip_hash() {
        let h = TxId::from_bytes([0x77; 32]);
        let enc = bincode::serialize(&h).unwrap();
        let back: TxId = bincode::deserialize(&enc).unwrap();
        assert_eq!(h, back);
    }

    #[test]
    fn serde_bincode_roundtrip_outpoint() {
        let op = OutPoint::new(TxId::from_bytes([0xDE; 32]), 7);
        let enc = bincode::serialize(&op).unwrap();
        let back: OutPoint = bincode::deserialize(&enc).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn serde_json_roundtrip_amount() {
        let a = Amount::from(123_456);
        let j = serde_json::to_string(&a).unwrap();
        // Amount is a newtype, encodes as a single number.
        let back: Amount = serde_json::from_str(&j).unwrap();
        assert_eq!(a, back);
    }
}
