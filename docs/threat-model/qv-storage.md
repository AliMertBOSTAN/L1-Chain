# Threat Model: qv-storage

**Module**: Persistent storage (RocksDB, block store, UTXO set, chain state)  
**Public API**: `BlockStore`, `UtxoStore`, `StateStore`, `apply_block()`, `revert_block()`  
**Threat Count**: 7 (1 Critical, 2 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Block state** — persisted blocks must match consensus (no corruption)
   - Integrity: CRITICAL (corrupted block = state divergence)
2. **UTXO set** — authoritative coin ledger
   - Integrity: CRITICAL (missing UTXO = unspendable coins)
   - Availability: CRITICAL (lost UTXO = permanent loss)
3. **Chain state** — block height, finality, epoch metadata
   - Integrity: CRITICAL (wrong state = fork detection broken)
4. **Atomic apply/revert** — block application must be atomic (all-or-nothing)
   - Consistency: CRITICAL (partial update = ledger breaks)

### Trust Boundaries
- **Input**: Block from consensus layer (assumed valid structure)
- **Processing**: RocksDB write operations (may fail due to I/O)
- **Output**: Persistent storage with consistency guarantees
- **Failure modes**: Disk full, corruption, concurrent access

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. Concurrent UTXO writes (race condition) | Tampering | Critical | Partial | Mutex protects writes; atomicity depends on RocksDB |
| 2. RocksDB corruption (bit flip, crash) | Tampering | High | Partial | Checksums in RocksDB; WAL for crash recovery |
| 3. Block revert on long reorg (state mismatch) | Tampering | High | Mitigated | Revert is inverse of apply; tested on k-deep reorg |
| 4. UTXO commitment root mismatch (checksum fail) | Tampering | Medium | Mitigated | Root is recomputed and verified on load |
| 5. Out-of-disk-space (incomplete write) | Denial of Service | Medium | Partial | Application should monitor disk; fail gracefully |
| 6. Concurrent access without synchronization | Denial of Service | Medium | Partial | Tokio mutex + lock guard pattern; review required |
| 7. Serialization format change (incompatibility) | Denial of Service | Medium | Deferred | Bincode encoding; document format versioning |

---

## Detailed Threat Analysis

### Threat 1: Concurrent UTXO Writes (Critical)

**Scenario**: Two threads simultaneously update UTXO set (apply block + revert block); one update is lost.

**Impact**: UTXO set becomes inconsistent; coins disappear or appear.

**Likelihood**: Low (code uses `tokio::sync::Mutex`; only one writer at a time).

**Mitigation Status**: Partial
- Current: `UtxoStore` wraps map in `Mutex<BTreeMap>`; all updates serialize through lock
- Atomicity: RocksDB write is atomic (single WAL entry); crash recovery restores consistency
- Testing: No explicit concurrency test; assume Tokio mutex is correct
- Future: Formal verification of lock ordering (no deadlock)

**Residual Risk**: RocksDB corruption under concurrent writes could bypass lock protection; depends on RocksDB implementation.

---

### Threat 2: RocksDB Corruption (High)

**Scenario**: Hardware bit flip or unclean shutdown corrupts RocksDB keyspace; block reads fail or return garbage.

**Impact**: Node unable to load persisted state; service outage; loss of recent blocks if backup unavailable.

**Likelihood**: Low (RocksDB has CRC checksums + WAL) but non-zero on aging hardware.

**Mitigation Status**: Partial
- Current: RocksDB includes CRC32 checksums per block; detects bit flips
- WAL: Write-Ahead Logging ensures crash recovery (no data loss post-sync)
- Detection: RocksDB returns error on checksum mismatch
- Mitigation: Application should alert operator; attempt repair (RocksDB repair tool)
- Future: Regular snapshots to external storage; cold backup strategy

**Residual Risk**: Undetected corruption (bit flip in checksum itself) could cause silent data loss; 1 in 2^32 probability per block.

---

### Threat 3: Block Revert on Long Reorg (High)

**Scenario**: Chain reorg of k-deep reverts k blocks; revert logic has a bug; UTXO set becomes inconsistent.

**Impact**: Node accepts invalid blocks; consensus diverges.

**Likelihood**: Low (revert logic is inverse of apply; tested).

**Mitigation Status**: Mitigated
- Code: `UtxoStore::revert_block()` does reverse of `apply_block()`:
  - Remove outputs from this block
  - Restore inputs back to UTXO set
- Test: Integration test "utxo_apply_revert_roundtrip" verifies revert == undo
- Atomic: Revert is atomic via RocksDB batch write

**Residual Risk**: None; revert is tested and atomic.

---

### Threat 4: UTXO Commitment Root Mismatch (Medium)

**Scenario**: UTXO set is modified; commitment root is recomputed; two nodes compute different roots.

**Impact**: Consensus fork; finality delay.

**Likelihood**: Very Low (commitment root is deterministic; BTreeMap iteration is sorted).

**Mitigation Status**: Mitigated
- Code: `commitment_root_of_sorted_entries()` recomputes root on load
- Verification: Root from block header is compared to computed root
- Mismatch: `UtxoError::CommitmentRootMismatch` is returned
- Test: Proptest "commitment_root_independent_of_insertion_order"

**Residual Risk**: None; root is deterministic and verified.

---

### Threat 5: Out-of-Disk-Space (Medium)

**Scenario**: Disk becomes full; RocksDB::write() fails; application does not handle error.

**Impact**: Node unable to persist new blocks; service outage; consensus halted.

**Likelihood**: Low (on well-maintained servers with monitoring) but non-zero on embedded devices.

**Mitigation Status**: Partial
- Current: `StorageError::Backend` is returned on I/O failure
- Application responsibility: Caller should check error, alert operator, halt safely
- Future: Implement disk usage monitoring; refuse blocks when disk < threshold

**Residual Risk**: Application may ignore storage errors; graceful degradation depends on caller.

---

### Threat 6: Concurrent Access Without Synchronization (Medium)

**Scenario**: Two threads access `BlockStore` without lock; one writes while other reads; reads garbage.

**Impact**: Node accepts invalid blocks; consensus breaks.

**Likelihood**: Very Low (code uses proper locking pattern; Rust's borrow checker prevents data races).

**Mitigation Status**: Partial
- Current: Tokio mutex protects all shared state
- Rust safety: Rust type system prevents &mut + & simultaneously
- Future: Formal verification of lock-free data structure (if moved to lock-free design)

**Residual Risk**: None (if locking is implemented correctly); code review required pre-audit.

---

### Threat 7: Serialization Format Change (Medium)

**Scenario**: Storage format changes between binary versions; old nodes cannot deserialize new format.

**Impact**: Breaking upgrade; nodes cannot sync; consensus halted.

**Likelihood**: Low (bincode format is stable; any change requires migration).

**Mitigation Status**: Deferred
- Current: No version field in bincode encoding
- Mitigation: Document format versioning strategy; bump version on breaking changes
- Future: Add format version byte to all serialized structures
- Rollout: Hard fork with upgrade period before format change

**Residual Risk**: Format change without proper migration could cause consensus halt; must be coordinated upgrade.

---

## Known Weaknesses & Future Work

1. **No snapshots of UTXO set** — full recomputation on load takes time; large blocks cause lag
2. **RocksDB options not tuned** — default options may not be optimal for blockchain workload
3. **No explicit backup strategy** — depends on operator to copy RocksDB files
4. **Concurrent reads not optimized** — uses lock-based synchronization; could use RCU or MVCC
5. **No data retention policy** — old blocks kept forever (pruning planned for future)

---

## Testing Strategy

### Unit Tests
- ✅ Block store: insert, get, remove by hash/height
- ✅ UTXO store: apply, revert, snapshot consistency
- ✅ State store: epoch metadata, finality tracking
- ✅ Serialization roundtrip: encode → decode → verify

### Integration Tests
- ✅ Full block lifecycle: add block → update UTXO → checkpoint state
- ✅ Reorg handling: revert 5 blocks → check state consistency
- ✅ Large dataset: 10k blocks → verify commitment root unchanged
- ✅ Crash recovery: shutdown → reopen → state intact

### Fuzz Testing
- [x] `utxo_apply.rs` — random blocks → apply → revert → commitment matches initial

---

## Audit Checklist

- [ ] Mutex is held during entire apply_block operation (atomic)
- [ ] Revert is exact inverse of apply (tested on all opcode types)
- [ ] UTXO commitment root is recomputed and verified (no skips)
- [ ] Block store is deduplicated (no duplicate blocks on disk)
- [ ] RocksDB write options include durability (fsync on commit)
- [ ] Serialization includes version field for future compatibility
- [ ] Error handling for out-of-disk-space (not silently dropped)
- [ ] No unsafe code in storage layer
- [ ] Concurrent access reviewed by 3+ reviewers

---

## References

- `crates/qv-storage/src/block_store.rs` — Block persistence
- `crates/qv-storage/src/utxo_store.rs` — UTXO set with apply/revert
- `crates/qv-storage/src/state_store.rs` — Epoch + finality metadata
- `crates/qv-storage/src/kv.rs` — RocksDB backend abstraction
- [RocksDB Documentation](https://rocksdb.org/) — Options, safety guarantees
