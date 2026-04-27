//! Persistent UTXO store with block connect/disconnect and snapshots.

use std::collections::{BTreeMap, BTreeSet};

use qv_core::{
    commitment_root_of_sorted_entries, Block, BlockHash, OutPoint, Transaction, TxId, TxOutput,
    UtxoCommitment,
};
use serde::{Deserialize, Serialize};

use crate::kv::{KvBatch, KvStore};
use crate::{decode, encode, StorageError, StorageResult};

const UTXO_ENTRY_PREFIX: &[u8] = b"utxo:entry:";
const UTXO_UNDO_PREFIX: &[u8] = b"utxo:undo:";
const UTXO_SNAPSHOT_PREFIX: &[u8] = b"utxo:snapshot:";

/// Undo information required to revert one applied block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UndoLog {
    /// Outputs consumed from the prior UTXO set.
    spent: Vec<(OutPoint, TxOutput)>,
}

/// Persistent UTXO storage facade.
#[derive(Clone, Debug)]
pub struct UtxoStore<S: KvStore> {
    kv: S,
}

impl<S: KvStore> UtxoStore<S> {
    /// Create a UTXO store backed by `kv`.
    #[must_use]
    pub fn new(kv: S) -> Self {
        Self { kv }
    }

    /// Insert a single UTXO entry.
    pub fn insert(&self, outpoint: OutPoint, output: TxOutput) -> StorageResult<()> {
        let key = Self::key_outpoint(&outpoint);
        if self.kv.get(&key)?.is_some() {
            return Err(StorageError::AlreadyExists("utxo outpoint"));
        }
        self.kv.put(&key, &encode(&output)?)
    }

    /// Remove and return a UTXO entry if present.
    pub fn remove(&self, outpoint: &OutPoint) -> StorageResult<Option<TxOutput>> {
        let key = Self::key_outpoint(outpoint);
        let Some(bytes) = self.kv.get(&key)? else {
            return Ok(None);
        };

        self.kv.delete(&key)?;
        Ok(Some(decode::<TxOutput>(&bytes)?))
    }

    /// Get one UTXO entry.
    pub fn get(&self, outpoint: &OutPoint) -> StorageResult<Option<TxOutput>> {
        let key = Self::key_outpoint(outpoint);
        let Some(bytes) = self.kv.get(&key)? else {
            return Ok(None);
        };

        Ok(Some(decode::<TxOutput>(&bytes)?))
    }

    /// True iff an outpoint is currently unspent.
    pub fn contains(&self, outpoint: &OutPoint) -> StorageResult<bool> {
        Ok(self.get(outpoint)?.is_some())
    }

    /// Number of entries currently in the persistent UTXO set.
    pub fn len(&self) -> StorageResult<usize> {
        Ok(self.kv.scan_prefix(UTXO_ENTRY_PREFIX)?.len())
    }

    /// Return all UTXO entries.
    pub fn entries(&self) -> StorageResult<Vec<(OutPoint, TxOutput)>> {
        let pairs = self.kv.scan_prefix(UTXO_ENTRY_PREFIX)?;
        let mut out = Vec::with_capacity(pairs.len());

        for (key, value) in pairs {
            let outpoint = Self::outpoint_from_key(&key)?;
            let tx_out = decode::<TxOutput>(&value)?;
            out.push((outpoint, tx_out));
        }

        Ok(out)
    }

    /// Commitment root of the current persistent UTXO set.
    pub fn commitment_root(&self) -> StorageResult<UtxoCommitment> {
        let mut sorted: BTreeMap<OutPoint, TxOutput> = BTreeMap::new();
        for (outpoint, output) in self.entries()? {
            sorted.insert(outpoint, output);
        }
        Ok(commitment_root_of_sorted_entries(sorted.iter()))
    }

    /// Apply all transaction effects in `block` to the persistent UTXO set.
    ///
    /// Stores an undo log under `block.hash()` so [`revert_block`] can restore
    /// the prior state.
    pub fn apply_block(&self, block: &Block) -> StorageResult<()> {
        block.validate_structure()?;

        let mut spent: Vec<(OutPoint, TxOutput)> = Vec::new();
        let mut consumed: BTreeSet<OutPoint> = BTreeSet::new();
        let mut staged_new: BTreeMap<OutPoint, TxOutput> = BTreeMap::new();

        for tx in &block.transactions {
            self.consume_inputs(tx, &mut spent, &mut consumed, &mut staged_new)?;
            self.produce_outputs(tx, &consumed, &mut staged_new)?;
        }

        let mut batch = self.kv.new_batch();

        for (outpoint, _) in &spent {
            batch.delete(Self::key_outpoint(outpoint));
        }

        for (outpoint, output) in staged_new {
            batch.put(Self::key_outpoint(&outpoint), encode(&output)?);
        }

        let block_hash = block.hash()?;
        let undo = UndoLog { spent };
        batch.put(Self::key_undo(&block_hash), encode(&undo)?);

        self.kv.write_batch(batch)
    }

    /// Revert a previously-applied block using its persisted undo log.
    pub fn revert_block(&self, block: &Block) -> StorageResult<()> {
        let block_hash = block.hash()?;
        let undo_key = Self::key_undo(&block_hash);

        let Some(undo_bytes) = self.kv.get(&undo_key)? else {
            return Err(StorageError::NotFound("utxo undo log"));
        };
        let undo = decode::<UndoLog>(&undo_bytes)?;

        let mut batch = self.kv.new_batch();

        for tx in &block.transactions {
            let tx_id = tx.id()?;
            for (idx, _) in tx.outputs.iter().enumerate() {
                let output_index = u32::try_from(idx)
                    .map_err(|_| StorageError::InvalidInput("output index overflow"))?;
                let outpoint = OutPoint::new(tx_id, output_index);
                batch.delete(Self::key_outpoint(&outpoint));
            }
        }

        for (outpoint, output) in undo.spent {
            batch.put(Self::key_outpoint(&outpoint), encode(&output)?);
        }

        batch.delete(undo_key);
        self.kv.write_batch(batch)
    }

    /// Persist a full UTXO snapshot under `snapshot_id`.
    pub fn create_snapshot(&self, snapshot_id: &[u8]) -> StorageResult<()> {
        if snapshot_id.is_empty() {
            return Err(StorageError::InvalidInput("snapshot id must not be empty"));
        }

        let key = Self::key_snapshot(snapshot_id);
        if self.kv.get(&key)?.is_some() {
            return Err(StorageError::AlreadyExists("utxo snapshot id"));
        }

        let entries = self.entries()?;
        self.kv.put(&key, &encode(&entries)?)
    }

    /// Restore the UTXO set from a previously-persisted snapshot.
    pub fn restore_snapshot(&self, snapshot_id: &[u8]) -> StorageResult<()> {
        if snapshot_id.is_empty() {
            return Err(StorageError::InvalidInput("snapshot id must not be empty"));
        }

        let key = Self::key_snapshot(snapshot_id);
        let Some(bytes) = self.kv.get(&key)? else {
            return Err(StorageError::NotFound("utxo snapshot"));
        };

        let snapshot_entries = decode::<Vec<(OutPoint, TxOutput)>>(&bytes)?;
        let current_entries = self.kv.scan_prefix(UTXO_ENTRY_PREFIX)?;

        let mut batch = self.kv.new_batch();
        for (current_key, _) in current_entries {
            batch.delete(current_key);
        }

        for (outpoint, output) in snapshot_entries {
            batch.put(Self::key_outpoint(&outpoint), encode(&output)?);
        }

        self.kv.write_batch(batch)
    }

    /// Alias for `restore_snapshot`.
    pub fn rollback_to_snapshot(&self, snapshot_id: &[u8]) -> StorageResult<()> {
        self.restore_snapshot(snapshot_id)
    }

    fn consume_inputs(
        &self,
        tx: &Transaction,
        spent: &mut Vec<(OutPoint, TxOutput)>,
        consumed: &mut BTreeSet<OutPoint>,
        staged_new: &mut BTreeMap<OutPoint, TxOutput>,
    ) -> StorageResult<()> {
        for input in &tx.inputs {
            let outpoint = input.prev_output;
            if !consumed.insert(outpoint) {
                return Err(StorageError::InvalidInput("double spend inside block"));
            }

            if staged_new.remove(&outpoint).is_some() {
                continue;
            }

            let key = Self::key_outpoint(&outpoint);
            let Some(bytes) = self.kv.get(&key)? else {
                return Err(StorageError::NotFound("input outpoint"));
            };

            let prev_output = decode::<TxOutput>(&bytes)?;
            spent.push((outpoint, prev_output));
        }
        Ok(())
    }

    fn produce_outputs(
        &self,
        tx: &Transaction,
        consumed: &BTreeSet<OutPoint>,
        staged_new: &mut BTreeMap<OutPoint, TxOutput>,
    ) -> StorageResult<()> {
        let tx_id = tx.id()?;
        for (idx, output) in tx.outputs.iter().enumerate() {
            let output_index = u32::try_from(idx)
                .map_err(|_| StorageError::InvalidInput("output index overflow"))?;
            let outpoint = OutPoint::new(tx_id, output_index);

            if consumed.contains(&outpoint) || staged_new.contains_key(&outpoint) {
                return Err(StorageError::AlreadyExists("new utxo outpoint collision"));
            }

            let key = Self::key_outpoint(&outpoint);
            if self.kv.get(&key)?.is_some() {
                return Err(StorageError::AlreadyExists("new utxo outpoint collision"));
            }

            staged_new.insert(outpoint, output.clone());
        }

        Ok(())
    }

    fn key_outpoint(outpoint: &OutPoint) -> Vec<u8> {
        let mut key = Vec::with_capacity(UTXO_ENTRY_PREFIX.len() + 36);
        key.extend_from_slice(UTXO_ENTRY_PREFIX);
        key.extend_from_slice(&outpoint.canonical_bytes());
        key
    }

    fn key_undo(block_hash: &BlockHash) -> Vec<u8> {
        let mut key = Vec::with_capacity(UTXO_UNDO_PREFIX.len() + BlockHash::LEN);
        key.extend_from_slice(UTXO_UNDO_PREFIX);
        key.extend_from_slice(block_hash.as_bytes());
        key
    }

    fn key_snapshot(snapshot_id: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(UTXO_SNAPSHOT_PREFIX.len() + snapshot_id.len());
        key.extend_from_slice(UTXO_SNAPSHOT_PREFIX);
        key.extend_from_slice(snapshot_id);
        key
    }

    fn outpoint_from_key(key: &[u8]) -> StorageResult<OutPoint> {
        if !key.starts_with(UTXO_ENTRY_PREFIX) {
            return Err(StorageError::Corrupted("invalid utxo key prefix"));
        }

        let raw = &key[UTXO_ENTRY_PREFIX.len()..];
        if raw.len() != 36 {
            return Err(StorageError::Corrupted("invalid utxo key length"));
        }

        let (tx_part, idx_part) = raw.split_at(32);
        let mut tx_bytes = [0u8; 32];
        tx_bytes.copy_from_slice(tx_part);

        let mut idx_bytes = [0u8; 4];
        idx_bytes.copy_from_slice(idx_part);

        let tx_id = TxId::from_bytes(tx_bytes);
        let index = u32::from_le_bytes(idx_bytes);
        Ok(OutPoint::new(tx_id, index))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use qv_core::{
        Amount, Block, BlockHash, BlockHeader, Height, OutPoint, Script, Transaction, TxId,
        TxInput, TxOutput,
    };

    use crate::kv::MemoryKvStore;
    use crate::utxo_store::UtxoStore;
    use crate::StorageError;

    fn tx_output(value: u64, marker: u8) -> TxOutput {
        TxOutput::new(
            Amount::from_smallest_units(value),
            Script::new(vec![marker]),
        )
    }

    fn transfer_block(prev_outpoint: OutPoint, marker: u8, value_out: u64, height: u64) -> Block {
        let tx = Transaction::new(
            vec![TxInput::new(prev_outpoint)],
            vec![tx_output(value_out, marker)],
        );

        let mut header = BlockHeader::genesis_template();
        header.height = Height::from(height);
        header.prev_hash = BlockHash::from_bytes([marker; 32]);

        let mut block = Block::new(header, vec![tx]);
        block.header.merkle_root = block.compute_merkle_root().unwrap();
        block
    }

    #[test]
    fn apply_and_revert_block_roundtrip() {
        let store = UtxoStore::new(MemoryKvStore::new());

        let genesis_op = OutPoint::new(TxId::from_bytes([1u8; 32]), 0);
        store.insert(genesis_op, tx_output(100, 1)).unwrap();

        let block = transfer_block(genesis_op, 9, 95, 1);
        store.apply_block(&block).unwrap();

        let tx_id = block.transactions[0].id().unwrap();
        let new_op = OutPoint::new(tx_id, 0);

        assert!(!store.contains(&genesis_op).unwrap());
        assert!(store.contains(&new_op).unwrap());

        store.revert_block(&block).unwrap();

        assert!(store.contains(&genesis_op).unwrap());
        assert!(!store.contains(&new_op).unwrap());
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let store = UtxoStore::new(MemoryKvStore::new());

        let op1 = OutPoint::new(TxId::from_bytes([2u8; 32]), 0);
        let op2 = OutPoint::new(TxId::from_bytes([3u8; 32]), 1);
        let op3 = OutPoint::new(TxId::from_bytes([4u8; 32]), 2);

        store.insert(op1, tx_output(10, 2)).unwrap();
        store.insert(op2, tx_output(20, 3)).unwrap();

        store.create_snapshot(b"snap-1").unwrap();

        store.remove(&op1).unwrap();
        store.insert(op3, tx_output(30, 4)).unwrap();

        assert!(!store.contains(&op1).unwrap());
        assert!(store.contains(&op3).unwrap());

        store.restore_snapshot(b"snap-1").unwrap();

        assert!(store.contains(&op1).unwrap());
        assert!(store.contains(&op2).unwrap());
        assert!(!store.contains(&op3).unwrap());
    }

    #[test]
    fn revert_without_undo_log_errors() {
        let store = UtxoStore::new(MemoryKvStore::new());

        let genesis_op = OutPoint::new(TxId::from_bytes([7u8; 32]), 0);
        let block = transfer_block(genesis_op, 8, 1, 1);

        let err = store.revert_block(&block).unwrap_err();
        assert!(matches!(err, StorageError::NotFound("utxo undo log")));
    }
}
