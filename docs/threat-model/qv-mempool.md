# Threat Model: qv-mempool

**Module**: Clear + encrypted transaction pools, ordering, batcher  
**Public API**: `ClearPool`, `EncryptedPool`, `deterministic_sort()`, `SwapDirection`, `OrderIntent`  
**Threat Count**: 8 (2 Critical, 2 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Clear pool integrity** — transactions waiting for block inclusion
   - Integrity: CRITICAL (injected tx = consensus bypass)
2. **Encrypted pool confidentiality** — unencrypted txs visible only to committee
   - Confidentiality: CRITICAL (leak = MEV extraction)
3. **Deterministic ordering** — same batch on all nodes
   - Integrity: CRITICAL (order mismatch = consensus fork)
4. **Threshold decryption** — only 2/3+ committee members can decrypt batch
   - Confidentiality: CRITICAL (single member decrypt = MEV)

### Trust Boundaries
- **Input**: Transactions from peers (clear pool) + encrypted threshold ciphertexts (encrypted pool)
- **Processing**: Fee-based sorting (clear), threshold KEM decryption (encrypted)
- **Output**: Deterministic transaction batch to consensus layer
- **Attacker**: Network + single committee member (cannot decrypt alone)

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. Double-spend in clear pool (same UTXO spent twice) | Tampering | Critical | Mitigated | UTXO dependency tracking per tx |
| 2. Encrypted pool threshold reconstruction bypass | Tampering | Critical | Partial | 3+ of t=5 shares needed; implementation audit required |
| 3. Double-spending after decryption (replay) | Tampering | High | Mitigated | Sequence numbers + nonce in transaction |
| 4. MEV sandwich attack (reorder despite encryption) | Information Disclosure | High | Partial | Deterministic batch ordering + economic incentive |
| 5. Pool eviction bias (low-fee tx removed unfairly) | Denial of Service | Medium | Mitigated | Fee-based eviction; fair queue |
| 6. Ordering oracle manipulation (external price feed) | Tampering | Medium | Partial | AMM oracle values hardcoded; no external feeds |
| 7. Batcher parallelization bug (race condition) | Tampering | Medium | Partial | No parallelization (single-threaded batcher) |
| 8. Out-of-order intents (swaps executed in wrong sequence) | Tampering | Medium | Mitigated | Intent topological sort + validation |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: Double-Spend in Clear Pool (Critical)
- **Scenario**: Two txs in pool spend same UTXO; both included in batch
- **Impact**: Double-spend; coins duplicated; ledger breaks
- **Status**: Mitigated — Each input OutPoint tracked; duplicate input rejected
- **Mitigation**: `ClearPool::insert()` checks `spent_outputs.contains(&input.outpoint)` → error

### Threat 2: Encrypted Pool Threshold Bypass (Critical)
- **Scenario**: Attacker compromises 2/3 shares; reconstructs plaintext before committee consensus
- **Impact**: MEV; sandwich attack; unfair advantage
- **Status**: Partial — Requires t=5, need >3 shares (Shamir threshold); implementation audit needed
- **Mitigation**: `threshold_reconstruct()` requires t > n/2 shares; validator checks count

### Threat 3: Double-Spend After Decryption (High)
- **Scenario**: Attacker sends different version of tx to clear pool; encrypted pool has different version
- **Impact**: One version spends coin; other version also tries
- **Status**: Mitigated — Transaction hash is deterministic; two versions = different TxId
- **Mitigation**: Batcher deduplicates by TxId; second version rejected

### Threat 4: MEV Sandwich (High)
- **Scenario**: Attacker controls sort order despite encryption; reorders to extract MEV
- **Impact**: Unfair economic advantage; price impact for honest users
- **Status**: Partial — Deterministic sorting prevents attacker reordering; but MEV itself not preventable
- **Mitigation**: Fee-based ordering is deterministic (no attacker choice); economic incentive honest

### Threats 5–8: Covered briefly
- **Eviction bias**: Fee-based eviction is fair (lowest fees first)
- **Oracle manipulation**: No external price feeds; hardcoded constants
- **Parallelization**: Single-threaded batcher (no races)
- **Intent ordering**: Topological sort validates dependency DAG

---

## Testing Strategy

- ✅ Clear pool: insert, remove, fee sorting, UTXO conflict detection
- ✅ Encrypted pool: mock threshold decryption, order reconstruction
- ✅ Deterministic sort: same input → same order across runs
- ✅ Intent validation: topological sort, cycle detection
- [x] Fuzz: `tx_ordering.rs` — random tx set → deterministic sort (idempotent)

---

## Audit Checklist

- [ ] Double-spend detection is checked before pool insertion
- [ ] Threshold reconstruction requires correct number of shares (n/2 + 1)
- [ ] Encrypted transactions are not leaked to non-committee nodes
- [ ] Deterministic sort is stable (same txs → same order always)
- [ ] Fee calculation includes all transaction fees (no missing inputs)
- [ ] Intent cycle detection prevents infinite loops in batch execution
- [ ] Nonce tracking prevents transaction replay

---

## References

- `crates/qv-mempool/src/clear.rs` — Clear pool
- `crates/qv-mempool/src/encrypted.rs` — Encrypted pool, threshold KEM
- `crates/qv-mempool/src/ordering.rs` — Deterministic sorting
- `crates/qv-mempool/src/batcher.rs` — Batch building, intent execution
- [ADR-003: MEV Encrypted Mempool](../ADR/003-mev-encrypted-mempool.md)
