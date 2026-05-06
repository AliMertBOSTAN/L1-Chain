//! Blocks, block headers, and the Merkle tree that commits to their bodies.
//!
//! A [`Block`] is a [`BlockHeader`] plus a list of [`Transaction`]s. The
//! header commits to the body via [`BlockHeader::merkle_root`], so any
//! tampering with the transaction list is detected by header-only peers.
//!
//! # Merkle construction
//!
//! [`merkle_root`] constructs a binary Merkle tree over the transactions'
//! `TxId`s, padding the last level by **duplicating the rightmost leaf**
//! until the level's length is a power of two. This is the same scheme as
//! Bitcoin; it has a well-known "CVE-2012-2459" duplicate-txn malleability,
//! which we mitigate at the ledger level by rejecting blocks containing
//! duplicate `TxId`s (see [`Block::validate_structure`]).
//!
//! Internal nodes use `SHA3-256(left || right)`. Leaves are the raw 32-byte
//! `TxId`s; we do *not* double-hash leaves because our leaf inputs are
//! already the output of a cryptographic hash.
//!
//! The empty block's Merkle root is [`MerkleRoot::ZERO`] by convention.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use qv_crypto::sha3_256;

use crate::transaction::{Transaction, TransactionError};
use crate::types::{
    BlockHash, Hash256, Height, MerkleRoot, Slot, Timestamp, TxId, UtxoCommitment,
};

// ============================================================================
// Errors
// ============================================================================

/// Errors produced while constructing or validating a block.
#[derive(Debug, Error)]
pub enum BlockError {
    /// Block header failed to encode.
    #[error("header encoding failed: {0}")]
    Encode(String),

    /// A transaction inside the block rejected structural validation.
    #[error("transaction {index} is invalid: {source}")]
    InvalidTransaction {
        /// 0-based index of the offending transaction within the block.
        index: usize,
        /// Underlying transaction error.
        #[source]
        source: TransactionError,
    },

    /// Two transactions inside the block share a `TxId`.
    #[error("block contains duplicate TxId")]
    DuplicateTx,

    /// The declared Merkle root does not match the recomputed root.
    #[error("merkle root mismatch")]
    MerkleRootMismatch,
}

// ============================================================================
// Block format
// ============================================================================

/// Current block format version.
pub const BLOCK_VERSION: u32 = 1;

/// Header of a block. This is the minimum information a light client needs
/// to follow the chain without downloading bodies.
///
/// The hash of the header (`sha3_256(canonical_bytes(header))`) is the
/// [`BlockHash`] that uniquely identifies the block.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Format version. See [`BLOCK_VERSION`].
    pub version: u32,
    /// Hash of the parent block's header.
    pub prev_hash: BlockHash,
    /// Chain height of this block (genesis = 0).
    pub height: Height,
    /// Ouroboros-Praos slot this block was produced in.
    pub slot: Slot,
    /// Wall-clock timestamp (seconds, unix epoch).
    pub timestamp: Timestamp,
    /// Merkle root over the block's transactions.
    pub merkle_root: MerkleRoot,
    /// Commitment to the UTXO set *after* applying this block.
    pub utxo_commitment: UtxoCommitment,
    /// VRF proof proving the producer is the slot leader.
    ///
    /// Opaque bytes at this layer; `qv-consensus` interprets them.
    pub vrf_proof: Vec<u8>,
    /// KES signature binding the header to the producer's evolving key.
    ///
    /// Opaque bytes at this layer; `qv-consensus` verifies them.
    pub kes_sig: Vec<u8>,
    /// Hash of the producer's operational public key (32 bytes).
    ///
    /// Used by `qv-consensus` to look up the stake pool that claims this
    /// block. Keeping a hash rather than the full key keeps header size
    /// bounded regardless of the underlying key algorithm.
    pub producer_key_hash: Hash256,
}

impl BlockHeader {
    /// Header used for the genesis block (slot 0, height 0, no parent).
    #[must_use]
    pub fn genesis_template() -> Self {
        Self {
            version: BLOCK_VERSION,
            prev_hash: BlockHash::ZERO,
            height: Height::GENESIS,
            slot: Slot::GENESIS,
            timestamp: Timestamp::from_unix_secs(0),
            merkle_root: MerkleRoot::ZERO,
            utxo_commitment: UtxoCommitment::ZERO,
            vrf_proof: Vec::new(),
            kes_sig: Vec::new(),
            producer_key_hash: Hash256::ZERO,
        }
    }

    /// Encode the header canonically (bincode) for hashing.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BlockError> {
        bincode::serialize(self).map_err(|e| BlockError::Encode(e.to_string()))
    }

    /// Compute the canonical block hash: `SHA3-256(canonical_bytes)`.
    pub fn hash(&self) -> Result<BlockHash, BlockError> {
        let bytes = self.canonical_bytes()?;
        Ok(BlockHash::from_bytes(sha3_256(&bytes)))
    }
}

/// A full block: header plus its transactions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// Block header. Committed to by its hash (the `BlockHash`).
    pub header: BlockHeader,
    /// Ordered list of transactions included in the block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Compose a block from a header and a transaction list.
    #[must_use]
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self { header, transactions }
    }

    /// Compute the Merkle root of the block's transactions. Matches the
    /// algorithm described in the module docs.
    pub fn compute_merkle_root(&self) -> Result<MerkleRoot, BlockError> {
        let mut ids = Vec::with_capacity(self.transactions.len());
        for (index, tx) in self.transactions.iter().enumerate() {
            let id = tx
                .id()
                .map_err(|source| BlockError::InvalidTransaction { index, source })?;
            ids.push(id);
        }
        Ok(merkle_root_of(&ids))
    }

    /// Recompute the block hash from the current header.
    pub fn hash(&self) -> Result<BlockHash, BlockError> {
        self.header.hash()
    }

    /// Structural validation that does not require consensus state:
    /// - every tx is structurally valid (or is the genesis transaction)
    /// - no duplicate `TxId`s
    /// - `header.merkle_root` matches the recomputed Merkle root
    pub fn validate_structure(&self) -> Result<(), BlockError> {
        // 1. Per-tx validation + id collection
        let mut ids = Vec::with_capacity(self.transactions.len());
        for (index, tx) in self.transactions.iter().enumerate() {
            // Genesis transactions (empty inputs, non-empty outputs) are allowed
            // only in genesis blocks (height=0). They bypass the normal input
            // validation but still require non-empty outputs.
            let is_genesis_block = self.header.height == Height::GENESIS;
            let is_genesis_tx = tx.inputs.is_empty() && !tx.outputs.is_empty();

            if is_genesis_tx && !is_genesis_block {
                // Genesis transaction in a non-genesis block is invalid
                return Err(BlockError::InvalidTransaction {
                    index,
                    source: TransactionError::NoInputs,
                });
            }

            if !(is_genesis_tx && is_genesis_block) {
                // Non-genesis transactions must pass normal validation
                tx.validate_structure()
                    .map_err(|source| BlockError::InvalidTransaction { index, source })?;
            }

            ids.push(
                tx.id()
                    .map_err(|source| BlockError::InvalidTransaction { index, source })?,
            );
        }

        // 2. No duplicate TxIds (mitigates CVE-2012-2459 at the ledger level)
        let mut sorted_ids = ids.clone();
        sorted_ids.sort_unstable();
        if sorted_ids.windows(2).any(|w| match w {
            [a, b] => a == b,
            _ => false,
        }) {
            return Err(BlockError::DuplicateTx);
        }

        // 3. Merkle root consistency
        let computed = merkle_root_of(&ids);
        if computed != self.header.merkle_root {
            return Err(BlockError::MerkleRootMismatch);
        }

        Ok(())
    }
}

// ============================================================================
// Merkle tree
// ============================================================================

/// Bitcoin-style Merkle root over `TxId`s.
///
/// - Empty input → [`MerkleRoot::ZERO`].
/// - Single input → the leaf is returned directly.
/// - Otherwise, pairs are hashed with `SHA3-256(left || right)`. If a level
///   has an odd length, the last node is duplicated.
#[must_use]
pub fn merkle_root_of(ids: &[TxId]) -> MerkleRoot {
    if ids.is_empty() {
        return MerkleRoot::ZERO;
    }

    // Current level: start with the txids as 32-byte leaves.
    let mut level: Vec<[u8; 32]> = ids.iter().map(|t| t.to_bytes()).collect();

    while level.len() > 1 {
        // If odd, duplicate the last. Done inside the loop so we always pair
        // cleanly below.
        if (level.len() & 1) == 1 {
            if let Some(&last) = level.last() {
                level.push(last);
            }
        }

        let mut next = Vec::with_capacity(level.len() >> 1);
        let mut iter = level.chunks_exact(2);
        for chunk in &mut iter {
            if let [left, right] = chunk {
                let mut buf = [0u8; 64];
                let (l, r) = buf.split_at_mut(32);
                l.copy_from_slice(left);
                r.copy_from_slice(right);
                next.push(sha3_256(&buf));
            }
        }
        level = next;
    }

    // Level now has exactly one entry.
    let root = match level.first() {
        Some(r) => *r,
        None => [0u8; 32],
    };
    MerkleRoot::from_bytes(root)
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
    use crate::transaction::{Script, TxInput, TxOutput};
    use crate::types::{Amount, OutPoint};

    fn tx_with_marker(byte: u8) -> Transaction {
        Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([byte; 32]), 0))],
            vec![TxOutput::new(
                Amount::from(u64::from(byte)),
                Script::new(vec![byte]),
            )],
        )
    }

    // ---- merkle_root_of ----

    #[test]
    fn merkle_empty_is_zero() {
        let r = merkle_root_of(&[]);
        assert_eq!(r, MerkleRoot::ZERO);
    }

    #[test]
    fn merkle_single_leaf_is_leaf() {
        let leaf = TxId::from_bytes([7u8; 32]);
        let r = merkle_root_of(&[leaf]);
        assert_eq!(r.to_bytes(), leaf.to_bytes());
    }

    #[test]
    fn merkle_two_leaves_is_hash_concat() {
        let a = TxId::from_bytes([1u8; 32]);
        let b = TxId::from_bytes([2u8; 32]);
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(a.as_bytes());
        buf[32..].copy_from_slice(b.as_bytes());
        let expect = sha3_256(&buf);

        let r = merkle_root_of(&[a, b]);
        assert_eq!(r.to_bytes(), expect);
    }

    #[test]
    fn merkle_three_leaves_duplicates_last() {
        let a = TxId::from_bytes([1u8; 32]);
        let b = TxId::from_bytes([2u8; 32]);
        let c = TxId::from_bytes([3u8; 32]);

        // Expected: H( H(a||b) || H(c||c) )
        let mut buf = [0u8; 64];
        buf[..32].copy_from_slice(a.as_bytes());
        buf[32..].copy_from_slice(b.as_bytes());
        let ab = sha3_256(&buf);
        buf[..32].copy_from_slice(c.as_bytes());
        buf[32..].copy_from_slice(c.as_bytes());
        let cc = sha3_256(&buf);
        buf[..32].copy_from_slice(&ab);
        buf[32..].copy_from_slice(&cc);
        let expect = sha3_256(&buf);

        let r = merkle_root_of(&[a, b, c]);
        assert_eq!(r.to_bytes(), expect);
    }

    #[test]
    fn merkle_four_leaves_is_balanced() {
        let leaves: Vec<TxId> = (1..=4).map(|i: u8| TxId::from_bytes([i; 32])).collect();
        // sanity: root is deterministic
        let a = merkle_root_of(&leaves);
        let b = merkle_root_of(&leaves);
        assert_eq!(a, b);
    }

    #[test]
    fn merkle_is_order_sensitive() {
        let a = TxId::from_bytes([1u8; 32]);
        let b = TxId::from_bytes([2u8; 32]);
        let r1 = merkle_root_of(&[a, b]);
        let r2 = merkle_root_of(&[b, a]);
        assert_ne!(r1, r2);
    }

    // ---- BlockHeader ----

    #[test]
    fn header_hash_is_deterministic() {
        let h = BlockHeader::genesis_template();
        let a = h.hash().unwrap();
        let b = h.hash().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn header_hash_changes_on_field_mutation() {
        let mut h = BlockHeader::genesis_template();
        let id1 = h.hash().unwrap();
        h.slot = Slot::from(1);
        let id2 = h.hash().unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn header_canonical_bytes_roundtrip() {
        let h = BlockHeader {
            vrf_proof: vec![0x01, 0x02, 0x03],
            kes_sig: vec![0xAA; 64],
            ..BlockHeader::genesis_template()
        };
        let bytes = h.canonical_bytes().unwrap();
        let back: BlockHeader = bincode::deserialize(&bytes).unwrap();
        assert_eq!(h, back);
    }

    // ---- Block structural validation ----

    #[test]
    fn block_validates_with_matching_merkle_root() {
        let txs = vec![tx_with_marker(1), tx_with_marker(2)];
        let ids: Vec<TxId> = txs.iter().map(|t| t.id().unwrap()).collect();
        let root = merkle_root_of(&ids);

        let header = BlockHeader {
            merkle_root: root,
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, txs);
        block.validate_structure().expect("structurally valid");
    }

    #[test]
    fn block_rejects_merkle_mismatch() {
        let txs = vec![tx_with_marker(1), tx_with_marker(2)];
        let header = BlockHeader {
            merkle_root: MerkleRoot::from_bytes([0xFF; 32]),
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, txs);
        assert!(matches!(
            block.validate_structure(),
            Err(BlockError::MerkleRootMismatch)
        ));
    }

    #[test]
    fn block_rejects_duplicate_txids() {
        // Two identical transactions -> identical TxIds.
        let tx = tx_with_marker(1);
        let txs = vec![tx.clone(), tx.clone()];
        let ids: Vec<TxId> = txs.iter().map(|t| t.id().unwrap()).collect();
        let header = BlockHeader {
            merkle_root: merkle_root_of(&ids),
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, txs);
        assert!(matches!(
            block.validate_structure(),
            Err(BlockError::DuplicateTx)
        ));
    }

    #[test]
    fn block_rejects_structurally_invalid_tx() {
        // Transaction with zero outputs should cause BlockError::InvalidTransaction,
        // even in a genesis block (since genesis txs still need outputs).
        let bad = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![],  // zero outputs
        );
        let ids = vec![bad.id().unwrap()];
        let header = BlockHeader {
            merkle_root: merkle_root_of(&ids),
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, vec![bad]);
        let err = block.validate_structure().unwrap_err();
        assert!(
            matches!(err, BlockError::InvalidTransaction { index: 0, .. }),
            "expected InvalidTransaction, got {err:?}"
        );
    }

    #[test]
    fn genesis_block_accepts_genesis_tx() {
        // A genesis block (height=0) should accept transactions with zero inputs
        // as long as they have outputs.
        let genesis_tx = Transaction::genesis(
            vec![TxOutput::new(Amount::from(100), Script::default())],
        );
        let ids = vec![genesis_tx.id().unwrap()];
        let header = BlockHeader {
            merkle_root: merkle_root_of(&ids),
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, vec![genesis_tx]);
        block.validate_structure().expect("genesis block with genesis tx should validate");
    }

    #[test]
    fn non_genesis_block_rejects_genesis_tx() {
        // A non-genesis block should reject transactions with zero inputs,
        // even if they have outputs.
        let genesis_tx = Transaction::genesis(
            vec![TxOutput::new(Amount::from(100), Script::default())],
        );
        let ids = vec![genesis_tx.id().unwrap()];
        let header = BlockHeader {
            height: Height::from(1),  // non-genesis height
            merkle_root: merkle_root_of(&ids),
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, vec![genesis_tx]);
        let err = block.validate_structure().unwrap_err();
        assert!(
            matches!(err, BlockError::InvalidTransaction { index: 0, .. }),
            "non-genesis block should reject genesis tx, got {err:?}"
        );
    }

    #[test]
    fn empty_block_validates() {
        let header = BlockHeader {
            merkle_root: MerkleRoot::ZERO,
            ..BlockHeader::genesis_template()
        };
        let block = Block::new(header, vec![]);
        block.validate_structure().expect("empty block is valid");
    }
}
