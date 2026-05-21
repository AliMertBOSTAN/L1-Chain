//! Backend abstraction for persistent key-value storage.
//!
//! `KvStore` is intentionally small: get/put/delete, batch writes, and prefix
//! scans. `qv-storage` higher-level stores compose these primitives.
//!
//! Three backends are provided:
//! - [`MemoryKvStore`]: deterministic in-memory store for tests/simulations.
//! - [`RocksKvStore`]: production-grade, C-backed (RocksDB).
//! - [`RedbKvStore`]: pure-Rust fallback (redb) — no C toolchain required.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use redb::{Database, TableDefinition};

use crate::{StorageError, StorageResult};

/// Mutation batch for a [`KvStore`] backend.
pub trait KvBatch {
    /// Add/replace a key-value entry.
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>);

    /// Delete an entry by key.
    fn delete(&mut self, key: Vec<u8>);

    /// Whether this batch contains no operations.
    fn is_empty(&self) -> bool;
}

/// Minimal key-value backend abstraction used by storage submodules.
pub trait KvStore: Clone + Send + Sync + 'static {
    /// Backend-specific batch type.
    type Batch: KvBatch;

    /// Create an empty batch.
    fn new_batch(&self) -> Self::Batch;

    /// Atomically apply all operations in a batch.
    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()>;

    /// Get value by key.
    fn get(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    /// Put/replace value by key.
    fn put(&self, key: &[u8], value: &[u8]) -> StorageResult<()>;

    /// Delete value by key.
    fn delete(&self, key: &[u8]) -> StorageResult<()>;

    /// Return all entries whose key starts with the provided prefix.
    fn scan_prefix(&self, prefix: &[u8]) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>>;
}

#[derive(Clone, Debug)]
enum BatchOp {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}

/// In-memory deterministic KV backend used by tests and simulations.
#[derive(Clone, Debug, Default)]
pub struct MemoryKvStore {
    inner: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryKvStore {
    /// Create an empty in-memory KV store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// In-memory batch implementation.
#[derive(Clone, Debug, Default)]
pub struct MemoryBatch {
    ops: Vec<BatchOp>,
}

impl KvBatch for MemoryBatch {
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(BatchOp::Put(key, value));
    }

    fn delete(&mut self, key: Vec<u8>) {
        self.ops.push(BatchOp::Delete(key));
    }

    fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl KvStore for MemoryKvStore {
    type Batch = MemoryBatch;

    fn new_batch(&self) -> Self::Batch {
        MemoryBatch::default()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        if batch.ops.is_empty() {
            return Ok(());
        }
        let mut map = self
            .inner
            .write()
            .map_err(|_| StorageError::Backend("memory kv lock poisoned".to_owned()))?;

        for op in batch.ops {
            match op {
                BatchOp::Put(k, v) => {
                    map.insert(k, v);
                }
                BatchOp::Delete(k) => {
                    map.remove(&k);
                }
            }
        }
        Ok(())
    }

    fn get(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let map = self
            .inner
            .read()
            .map_err(|_| StorageError::Backend("memory kv lock poisoned".to_owned()))?;
        Ok(map.get(key).cloned())
    }

    fn put(&self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        let mut map = self
            .inner
            .write()
            .map_err(|_| StorageError::Backend("memory kv lock poisoned".to_owned()))?;
        map.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> StorageResult<()> {
        let mut map = self
            .inner
            .write()
            .map_err(|_| StorageError::Backend("memory kv lock poisoned".to_owned()))?;
        map.remove(key);
        Ok(())
    }

    fn scan_prefix(&self, prefix: &[u8]) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let map = self
            .inner
            .read()
            .map_err(|_| StorageError::Backend("memory kv lock poisoned".to_owned()))?;

        let start = prefix.to_vec();
        let mut out = Vec::new();

        for (k, v) in map.range(start..) {
            if !k.starts_with(prefix) {
                break;
            }
            out.push((k.clone(), v.clone()));
        }

        Ok(out)
    }
}

// RocksDB backend — gated behind the `rocksdb` feature so the workspace
// builds without a C++ toolchain / libclang (bindgen). The node runs on the
// in-memory backend; `redb` covers pure-Rust persistence. Enable the
// `rocksdb` feature for a production RocksDB-backed deployment.
#[cfg(feature = "rocksdb")]
pub use rocks_backend::{RocksBatch, RocksKvStore};

#[cfg(feature = "rocksdb")]
mod rocks_backend {
    use super::{BatchOp, KvBatch, KvStore};
    use crate::{StorageError, StorageResult};
    use rocksdb::{Direction, IteratorMode, Options, WriteBatch, DB};
    use std::path::Path;
    use std::sync::Arc;

/// RocksDB-backed KV store for persistent node state.
#[derive(Clone)]
pub struct RocksKvStore {
    db: Arc<DB>,
}

impl core::fmt::Debug for RocksKvStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RocksKvStore(..)")
    }
}

impl RocksKvStore {
    /// Open a RocksDB database at `path`, creating it if missing.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path)
            .map_err(|e| StorageError::Backend(format!("rocksdb open failed: {e}")))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Wrap an already-open RocksDB handle.
    #[must_use]
    pub fn from_db(db: DB) -> Self {
        Self { db: Arc::new(db) }
    }
}

/// RocksDB batch implementation.
#[derive(Clone, Debug, Default)]
pub struct RocksBatch {
    ops: Vec<BatchOp>,
}

impl KvBatch for RocksBatch {
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(BatchOp::Put(key, value));
    }

    fn delete(&mut self, key: Vec<u8>) {
        self.ops.push(BatchOp::Delete(key));
    }

    fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl KvStore for RocksKvStore {
    type Batch = RocksBatch;

    fn new_batch(&self) -> Self::Batch {
        RocksBatch::default()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        if batch.ops.is_empty() {
            return Ok(());
        }

        let mut write_batch = WriteBatch::default();
        for op in batch.ops {
            match op {
                BatchOp::Put(k, v) => {
                    write_batch.put(k, v);
                }
                BatchOp::Delete(k) => {
                    write_batch.delete(k);
                }
            }
        }

        self.db
            .write(write_batch)
            .map_err(|e| StorageError::Backend(format!("rocksdb write failed: {e}")))
    }

    fn get(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.db
            .get(key)
            .map(|opt| opt.map(|v| v.to_vec()))
            .map_err(|e| StorageError::Backend(format!("rocksdb get failed: {e}")))
    }

    fn put(&self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.db
            .put(key, value)
            .map_err(|e| StorageError::Backend(format!("rocksdb put failed: {e}")))
    }

    fn delete(&self, key: &[u8]) -> StorageResult<()> {
        self.db
            .delete(key)
            .map_err(|e| StorageError::Backend(format!("rocksdb delete failed: {e}")))
    }

    fn scan_prefix(&self, prefix: &[u8]) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let iter = self
            .db
            .iterator(IteratorMode::From(prefix, Direction::Forward));

        let mut out = Vec::new();
        for item in iter {
            let (k, v) =
                item.map_err(|e| StorageError::Backend(format!("rocksdb iter failed: {e}")))?;

            if !k.starts_with(prefix) {
                break;
            }

            out.push((k.to_vec(), v.to_vec()));
        }

        Ok(out)
    }
}
} // mod rocks_backend (feature = "rocksdb")

// ---------------------------------------------------------------------------
// redb (pure-Rust fallback)
// ---------------------------------------------------------------------------

/// Single-table definition used by [`RedbKvStore`].
const REDB_TABLE: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("kv");

/// Pure-Rust embedded key-value store backed by [`redb`].
///
/// This backend requires no C toolchain and is suitable for environments where
/// building RocksDB is impractical.  Its API surface is identical to
/// [`RocksKvStore`] via the shared [`KvStore`] trait.
#[derive(Clone)]
pub struct RedbKvStore {
    db: Arc<Database>,
}

impl core::fmt::Debug for RedbKvStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RedbKvStore(..)")
    }
}

impl RedbKvStore {
    /// Open (or create) a redb database at `path`.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let db = Database::create(path)
            .map_err(|e| StorageError::Backend(format!("redb create failed: {e}")))?;

        // Ensure the table exists by opening a write transaction once.
        {
            let txn = db
                .begin_write()
                .map_err(|e| StorageError::Backend(format!("redb begin_write: {e}")))?;
            let table = txn
                .open_table(REDB_TABLE)
                .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;
            drop(table); // release borrow on `txn` before commit
            txn.commit()
                .map_err(|e| StorageError::Backend(format!("redb commit: {e}")))?;
        }

        Ok(Self { db: Arc::new(db) })
    }
}

/// Redb batch implementation.
#[derive(Clone, Debug, Default)]
pub struct RedbBatch {
    ops: Vec<BatchOp>,
}

impl KvBatch for RedbBatch {
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(BatchOp::Put(key, value));
    }

    fn delete(&mut self, key: Vec<u8>) {
        self.ops.push(BatchOp::Delete(key));
    }

    fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

impl KvStore for RedbKvStore {
    type Batch = RedbBatch;

    fn new_batch(&self) -> Self::Batch {
        RedbBatch::default()
    }

    fn write_batch(&self, batch: Self::Batch) -> StorageResult<()> {
        if batch.ops.is_empty() {
            return Ok(());
        }

        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(format!("redb begin_write: {e}")))?;

        {
            let mut table = txn
                .open_table(REDB_TABLE)
                .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;

            for op in batch.ops {
                match op {
                    BatchOp::Put(k, v) => {
                        table
                            .insert(k.as_slice(), v.as_slice())
                            .map_err(|e| StorageError::Backend(format!("redb insert: {e}")))?;
                    }
                    BatchOp::Delete(k) => {
                        table
                            .remove(k.as_slice())
                            .map_err(|e| StorageError::Backend(format!("redb remove: {e}")))?;
                    }
                }
            }
        }

        txn.commit()
            .map_err(|e| StorageError::Backend(format!("redb commit: {e}")))
    }

    fn get(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StorageError::Backend(format!("redb begin_read: {e}")))?;

        let table = txn
            .open_table(REDB_TABLE)
            .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;

        let result = table
            .get(key)
            .map_err(|e| StorageError::Backend(format!("redb get: {e}")))?;

        Ok(result.map(|v| v.value().to_vec()))
    }

    fn put(&self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(format!("redb begin_write: {e}")))?;

        {
            let mut table = txn
                .open_table(REDB_TABLE)
                .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;

            table
                .insert(key, value)
                .map_err(|e| StorageError::Backend(format!("redb insert: {e}")))?;
        }

        txn.commit()
            .map_err(|e| StorageError::Backend(format!("redb commit: {e}")))
    }

    fn delete(&self, key: &[u8]) -> StorageResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| StorageError::Backend(format!("redb begin_write: {e}")))?;

        {
            let mut table = txn
                .open_table(REDB_TABLE)
                .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;

            table
                .remove(key)
                .map_err(|e| StorageError::Backend(format!("redb remove: {e}")))?;
        }

        txn.commit()
            .map_err(|e| StorageError::Backend(format!("redb commit: {e}")))
    }

    fn scan_prefix(&self, prefix: &[u8]) -> StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| StorageError::Backend(format!("redb begin_read: {e}")))?;

        let table = txn
            .open_table(REDB_TABLE)
            .map_err(|e| StorageError::Backend(format!("redb open_table: {e}")))?;

        let mut out = Vec::new();

        // redb range scan: start from prefix, iterate forward while prefix matches.
        let range = table
            .range(prefix..)
            .map_err(|e| StorageError::Backend(format!("redb range: {e}")))?;

        for entry in range {
            let entry =
                entry.map_err(|e| StorageError::Backend(format!("redb range iter: {e}")))?;
            let k = entry.0.value().to_vec();
            if !k.starts_with(prefix) {
                break;
            }
            let v = entry.1.value().to_vec();
            out.push((k, v));
        }

        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{KvBatch, KvStore, MemoryKvStore, RedbKvStore};
    #[cfg(feature = "rocksdb")]
    use super::RocksKvStore;

    fn temp_db_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("{prefix}_{nanos}"));
        path
    }

    #[test]
    fn memory_kv_put_get_delete_roundtrip() {
        let kv = MemoryKvStore::new();
        kv.put(b"k1", b"v1").unwrap();
        assert_eq!(kv.get(b"k1").unwrap(), Some(b"v1".to_vec()));

        kv.delete(b"k1").unwrap();
        assert_eq!(kv.get(b"k1").unwrap(), None);
    }

    #[test]
    fn memory_kv_batch_applies_multiple_mutations() {
        let kv = MemoryKvStore::new();
        let mut batch = kv.new_batch();
        batch.put(b"a".to_vec(), b"1".to_vec());
        batch.put(b"b".to_vec(), b"2".to_vec());
        batch.delete(b"a".to_vec());

        kv.write_batch(batch).unwrap();

        assert_eq!(kv.get(b"a").unwrap(), None);
        assert_eq!(kv.get(b"b").unwrap(), Some(b"2".to_vec()));
    }

    #[test]
    fn memory_kv_prefix_scan() {
        let kv = MemoryKvStore::new();
        kv.put(b"p:1", b"x").unwrap();
        kv.put(b"p:2", b"y").unwrap();
        kv.put(b"q:1", b"z").unwrap();

        let scanned = kv.scan_prefix(b"p:").unwrap();
        assert_eq!(scanned.len(), 2);
    }

    #[test]
    #[cfg(feature = "rocksdb")]
    fn rocks_kv_basic_roundtrip() {
        let path = temp_db_path("qv_storage_kv_test");
        let kv = RocksKvStore::open(&path).unwrap();

        kv.put(b"k", b"v").unwrap();
        assert_eq!(kv.get(b"k").unwrap(), Some(b"v".to_vec()));

        let mut batch = kv.new_batch();
        batch.put(b"pref:1".to_vec(), b"a".to_vec());
        batch.put(b"pref:2".to_vec(), b"b".to_vec());
        kv.write_batch(batch).unwrap();

        let pref = kv.scan_prefix(b"pref:").unwrap();
        assert_eq!(pref.len(), 2);

        drop(kv);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn redb_kv_basic_roundtrip() {
        let path = temp_db_path("qv_storage_redb_test");
        let kv = RedbKvStore::open(&path).unwrap();

        kv.put(b"k", b"v").unwrap();
        assert_eq!(kv.get(b"k").unwrap(), Some(b"v".to_vec()));

        kv.delete(b"k").unwrap();
        assert_eq!(kv.get(b"k").unwrap(), None);

        let mut batch = kv.new_batch();
        batch.put(b"pref:1".to_vec(), b"a".to_vec());
        batch.put(b"pref:2".to_vec(), b"b".to_vec());
        batch.put(b"other:1".to_vec(), b"c".to_vec());
        kv.write_batch(batch).unwrap();

        let pref = kv.scan_prefix(b"pref:").unwrap();
        assert_eq!(pref.len(), 2);

        // batch delete
        let mut batch2 = kv.new_batch();
        batch2.delete(b"pref:1".to_vec());
        kv.write_batch(batch2).unwrap();

        assert_eq!(kv.get(b"pref:1").unwrap(), None);
        assert_eq!(kv.get(b"pref:2").unwrap(), Some(b"b".to_vec()));

        drop(kv);
        let _ = std::fs::remove_dir_all(path);
    }
}
