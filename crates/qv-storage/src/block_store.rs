//! Persistent block storage and secondary indexes.

use qv_core::{Block, BlockHash, BlockHeader, Height};

use crate::kv::{KvBatch, KvStore};
use crate::{decode, encode, StorageError, StorageResult};

const BLOCK_BY_HASH_PREFIX: &[u8] = b"block:by_hash:";
const BLOCK_HASH_BY_HEIGHT_PREFIX: &[u8] = b"block:height:";

/// Block storage facade over a generic key-value backend.
#[derive(Clone, Debug)]
pub struct BlockStore<S: KvStore> {
    kv: S,
}

impl<S: KvStore> BlockStore<S> {
    /// Create a block store backed by `kv`.
    #[must_use]
    pub fn new(kv: S) -> Self {
        Self { kv }
    }

    /// Persist a block and index it by hash and height.
    ///
    /// Returns the computed block hash.
    pub fn put_block(&self, block: &Block) -> StorageResult<BlockHash> {
        block.validate_structure()?;
        let hash = block.hash()?;

        if self.get_block(&hash)?.is_some() {
            return Err(StorageError::AlreadyExists("block hash"));
        }

        if let Some(existing_hash) = self.get_hash_by_height(block.header.height)? {
            if existing_hash == hash {
                return Ok(hash);
            }
            return Err(StorageError::AlreadyExists("block height index"));
        }

        let mut batch = self.kv.new_batch();
        batch.put(Self::key_block(&hash), encode(block)?);
        batch.put(
            Self::key_height(block.header.height),
            hash.to_bytes().to_vec(),
        );

        self.kv.write_batch(batch)?;
        Ok(hash)
    }

    /// Fetch a block by block hash.
    pub fn get_block(&self, hash: &BlockHash) -> StorageResult<Option<Block>> {
        let key = Self::key_block(hash);
        let Some(bytes) = self.kv.get(&key)? else {
            return Ok(None);
        };

        let block = decode::<Block>(&bytes)?;
        Ok(Some(block))
    }

    /// Fetch the block hash indexed at a given height.
    pub fn get_hash_by_height(&self, height: Height) -> StorageResult<Option<BlockHash>> {
        let key = Self::key_height(height);
        let Some(bytes) = self.kv.get(&key)? else {
            return Ok(None);
        };

        Ok(Some(Self::decode_block_hash(&bytes)?))
    }

    /// Fetch a full block by height.
    pub fn get_block_by_height(&self, height: Height) -> StorageResult<Option<Block>> {
        let Some(hash) = self.get_hash_by_height(height)? else {
            return Ok(None);
        };
        self.get_block(&hash)
    }

    /// Fetch a block header by height (light-client path).
    pub fn get_header_by_height(&self, height: Height) -> StorageResult<Option<BlockHeader>> {
        let Some(block) = self.get_block_by_height(height)? else {
            return Ok(None);
        };
        Ok(Some(block.header))
    }

    fn key_block(hash: &BlockHash) -> Vec<u8> {
        let mut key = Vec::with_capacity(BLOCK_BY_HASH_PREFIX.len() + BlockHash::LEN);
        key.extend_from_slice(BLOCK_BY_HASH_PREFIX);
        key.extend_from_slice(hash.as_bytes());
        key
    }

    fn key_height(height: Height) -> Vec<u8> {
        let mut key = Vec::with_capacity(BLOCK_HASH_BY_HEIGHT_PREFIX.len() + 8);
        key.extend_from_slice(BLOCK_HASH_BY_HEIGHT_PREFIX);
        key.extend_from_slice(&height.as_u64().to_be_bytes());
        key
    }

    fn decode_block_hash(bytes: &[u8]) -> StorageResult<BlockHash> {
        if bytes.len() != BlockHash::LEN {
            return Err(StorageError::Corrupted("invalid block hash length"));
        }

        let mut arr = [0u8; BlockHash::LEN];
        arr.copy_from_slice(bytes);
        Ok(BlockHash::from_bytes(arr))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::{
        Amount, Block, BlockHash, BlockHeader, Height, MerkleRoot, OutPoint, Script, Timestamp,
        Transaction, TxId, TxInput, TxOutput, UtxoCommitment,
    };

    use crate::block_store::BlockStore;
    use crate::kv::MemoryKvStore;
    use crate::StorageError;

    fn make_block(height: u64, marker: u8, out_value: u64) -> Block {
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(
                TxId::from_bytes([marker; 32]),
                0,
            ))],
            vec![TxOutput::new(
                Amount::from_smallest_units(out_value),
                Script::new(vec![marker]),
            )],
        );

        let mut header = BlockHeader::genesis_template();
        header.height = Height::from(height);
        header.prev_hash = BlockHash::from_bytes([marker; 32]);
        header.timestamp = Timestamp::from_unix_secs(height);
        header.utxo_commitment = UtxoCommitment::ZERO;
        header.merkle_root = MerkleRoot::ZERO;

        let mut block = Block::new(header, vec![tx]);
        block.header.merkle_root = block.compute_merkle_root().unwrap();
        block
    }

    #[test]
    fn put_and_get_block_roundtrip() {
        let store = BlockStore::new(MemoryKvStore::new());
        let block = make_block(1, 7, 100);

        let hash = store.put_block(&block).unwrap();
        let fetched = store.get_block(&hash).unwrap().unwrap();
        assert_eq!(fetched, block);

        let by_height = store.get_block_by_height(Height::from(1)).unwrap().unwrap();
        assert_eq!(by_height, block);
    }

    #[test]
    fn header_by_height_path_works() {
        let store = BlockStore::new(MemoryKvStore::new());
        let block = make_block(2, 9, 42);
        store.put_block(&block).unwrap();

        let header = store
            .get_header_by_height(Height::from(2))
            .unwrap()
            .unwrap();
        assert_eq!(header, block.header);
    }

    #[test]
    fn duplicate_hash_is_rejected() {
        let store = BlockStore::new(MemoryKvStore::new());
        let block = make_block(3, 1, 11);

        store.put_block(&block).unwrap();
        let err = store.put_block(&block).unwrap_err();
        assert!(matches!(err, StorageError::AlreadyExists("block hash")));
    }

    #[test]
    fn height_conflict_is_rejected() {
        let store = BlockStore::new(MemoryKvStore::new());
        let b1 = make_block(4, 2, 20);
        let b2 = make_block(4, 3, 21);

        store.put_block(&b1).unwrap();
        let err = store.put_block(&b2).unwrap_err();
        assert!(matches!(
            err,
            StorageError::AlreadyExists("block height index")
        ));
    }
}
