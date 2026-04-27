# Threat Model: qv-core

**Module**: Core ledger types (UTXO, transactions, blocks, Merkle, params)  
**Public API**: `Transaction`, `Block`, `UtxoSet`, `OutPoint`, `TxId`, `Amount`, `ProtocolParams`  
**Threat Count**: 7 (0 Critical, 2 High, 5 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **UTXO set state** — the source of truth for coin ownership and availability
   - Integrity: CRITICAL (modification = theft)
   - Availability: CRITICAL (loss = coins unspendable)
2. **Transaction ID** — unique identifier, persisted in chain
   - Integrity: CRITICAL (collision = duplicate spending)
3. **Merkle root** — commitment to transaction set, signed by validator
   - Integrity: CRITICAL (mismatch = undetectable block tampering)
4. **Block structure** — version, height, slot, previous hash linkage
   - Integrity: CRITICAL (broken chain = fork detection failure)

### Trust Boundaries
- **Input**: Block body (arbitrary bytes) → parsed `Transaction` and `Block` objects
- **Processing**: UTXO set updates (apply, revert) without atomic snapshots
- **Output**: Merkle root derivation from unsorted transaction list

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. UTXO double-spend via duplicate TxId in block | Tampering | High | Mitigated | Duplicate TxId in block rejected; CVE-2012-2459 fix |
| 2. Transaction structure bypass (empty inputs/outputs) | Tampering | High | Mitigated | `validate_structure()` rejects empty bodies |
| 3. Merkle root mismatch undetected (lazy validation) | Tampering | Medium | Mitigated | `Block::validate_structure()` checks merkle_root before apply |
| 4. Integer overflow in `Amount` arithmetic | Tampering | Medium | Mitigated | All arithmetic is `checked_*`; no silent overflow |
| 5. UTXO commitment root recomputation mismatch | Tampering | Medium | Mitigated | Deterministic BTreeMap iteration; SHA3-256 canonical |
| 6. Malformed block header (slot jump, height gap) | Spoofing | Medium | Mitigated | Consensus layer (`qv-consensus`) validates slot/height monotonicity |
| 7. Transaction validity interval bypass (time-locked) | Tampering | Medium | Mitigated | Script VM (`qv-script`) checks `ValidityInterval::contains(slot)` |

---

## Detailed Threat Analysis

### Threat 1: UTXO Double-Spend via Duplicate TxId (High)

**Scenario**: Block contains two transactions with identical TxId; attacker spends same coin twice.

**Impact**: Coin duplication; attacker gains free coins; violates ledger invariant.

**Likelihood**: Low (requires attacker to craft block; consensus layer validates block header).

**Mitigation Status**: Mitigated
- Code: `Block::validate_structure()` iterates txs, stores TxId in HashSet, rejects duplicates
- Also blocks CVE-2012-2459 (Bitcoin double-spend via coinbase duplicate in merkle tree)
- Test: `tests/integration.rs` includes "duplicate_tx_rejected" test

**Residual Risk**: None; check is in critical path before block acceptance.

---

### Threat 2: Transaction Structure Bypass (High)

**Scenario**: Attacker crafts transaction with empty inputs or outputs; bypasses validation, applies to UTXO set.

**Impact**: UTXO set becomes inconsistent; unspendable coins created; double-spend becomes possible.

**Likelihood**: Low (requires consensus layer to accept structurally invalid block).

**Mitigation Status**: Mitigated
- Code: `Transaction::validate_structure()` and `Block::validate_structure()` both check:
  - `inputs.is_empty()` → Error
  - `outputs.is_empty()` → Error
- Test: Unit tests in `transaction.rs` + integration tests

**Residual Risk**: None; double-check in both transaction and block validation.

---

### Threat 3: Merkle Root Mismatch Undetected (Medium)

**Scenario**: Block claims merkle_root R, but actual root of txs is R'; mismatch not caught.

**Impact**: Consensus layer might accept invalid block; state divergence across nodes.

**Likelihood**: Low (consensus layer validates header; `Block::validate_structure()` checks merkle_root).

**Mitigation Status**: Mitigated
- Code: `Block::validate_structure()` computes merkle root and compares to header-declared value
- Early failure: merkle mismatch rejects block before UTXO update
- Test: Integration test "merkle_root_computed_correctly"

**Residual Risk**: None; validation is in critical path.

---

### Threat 4: Integer Overflow in `Amount` Arithmetic (Medium)

**Scenario**: Transaction output sum wraps around (u64 overflow); validator accepts invalid transaction.

**Impact**: Coins created from nothing; hyperinflation; ledger invariant broken.

**Likelihood**: Very Low (all arithmetic is `checked_*`; overflow → Error, not silent wrap).

**Mitigation Status**: Mitigated
- Code: `Amount` uses `checked_add`, `checked_sub`, `checked_sum` throughout
- Overflow → `TransactionError::OutputSumOverflow`
- Test: Proptest property "amount_addition_is_associative_and_checked"

**Residual Risk**: None; checked arithmetic is a Rust best practice.

---

### Threat 5: UTXO Commitment Root Recomputation Mismatch (Medium)

**Scenario**: UTXO set is updated; commitment root is recomputed; two nodes compute different roots for same UTXO set.

**Impact**: State divergence; fork; finality delay.

**Likelihood**: Low (deterministic BTreeMap iteration + SHA3-256 canonical).

**Mitigation Status**: Mitigated
- Code: `InMemoryUtxoSet` uses `BTreeMap<OutPoint, TxOutput>`; iteration is always sorted
- Merkle: `commitment_root_of_sorted_entries()` uses canonical leaf = `SHA3-256(outpoint_bytes || bincode(output))`
- Test: Property-based test "commitment_root_insertion_order_independent"

**Residual Risk**: None; BTreeMap + canonical encoding ensure determinism.

---

### Threat 6: Malformed Block Header (Medium)

**Scenario**: Block header claims slot N+100 (skipping N+1..N+99); network accepts; consensus breaks.

**Impact**: Slot continuity assumption violated; leader election fairness degraded.

**Likelihood**: Low (consensus layer validates slot monotonicity).

**Mitigation Status**: Mitigated
- Code: `qv-consensus::validate_block_header()` checks `header.slot > parent.slot`
- Allows slot > parent.slot + 1 (empty slots are valid)
- Test: Consensus integration test "leader_schedule_handles_empty_slots"

**Residual Risk**: None; consensus layer enforces monotonicity.

---

### Threat 7: Transaction Validity Interval Bypass (Medium)

**Scenario**: Transaction claims `ValidityInterval::at_slot(100)` but is applied at slot 200; script VM is not consulted.

**Impact**: Time-locked transactions execute early; attacker locks up coins temporarily, then spends them before timeout.

**Likelihood**: Low (script VM is expected to validate `ValidityInterval::contains(slot)` during script execution).

**Mitigation Status**: Mitigated
- Code: `qv-script::validate_script()` receives `current_slot` and checks interval during execution
- ValidityInterval opcode: `VerifyTimelock` checks `current_slot ∈ interval`
- Test: Script integration test "timelock_rejected_before_validity"

**Residual Risk**: None if script VM is always called; depends on higher-layer protocol.

---

## Known Weaknesses & Future Work

1. **Merkle tree is inefficient** — Bitcoin-style binary Merkle tree O(n) space for n txs; consider Merkle-Patricia tree for sparse proofs
2. **OutPoint `Ord` is not verified in `qv-core`** — depends on consumer to sort UTXO set correctly
3. **No explicit transaction format versioning** — hard to upgrade tx structure without breaking compatibility
4. **Datum and witness are opaque** — script VM responsible for decoding (could leak errors)
5. **No tx size limit in `qv-core`** — depends on script VM to enforce max size

---

## Testing Strategy

### Unit Tests
- ✅ `Amount` checked arithmetic (overflow detection)
- ✅ `OutPoint` canonical encoding (little-endian serialization)
- ✅ `ValidityInterval` membership (slot within range)
- ✅ Merkle root computation (consistent across restarts)
- ✅ UTXO set snapshot independence

### Fuzz Testing
- [x] `tx_parser` — arbitrary bytes → `Transaction::decode` (no panic, validate roundtrip)
- [x] Block parsing — arbitrary bytes → `Block::deserialize` (no panic, merkle verification)
- [x] UTXO apply/revert — random blocks → apply → revert → check commitment matches

### Property-Based Tests
- ✅ `Amount` associativity: `(a + b) + c == a + (b + c)` (when no overflow)
- ✅ Insertion-order independence: UTXO commitment root same regardless of insertion order

---

## Audit Checklist

- [ ] Merkle root computation matches Bitcoin algorithm (no off-by-one errors)
- [ ] Duplicate TxId rejection works for blocks with >2^16 txs
- [ ] UTXO commitment root is deterministic on all platforms (even Windows big-endian?)
- [ ] Integer size checks (u32 height fits 4.3B blocks; u64 slot fits 1M years)
- [ ] Canonical encoding uniqueness (same OutPoint encodes to single byte sequence)
- [ ] No unsafe code in core types

---

## References

- `crates/qv-core/src/types.rs` — Identifier newtypes, Amount
- `crates/qv-core/src/transaction.rs` — Transaction structure, ValidityInterval
- `crates/qv-core/src/block.rs` — Block, BlockHeader, Merkle root
- `crates/qv-core/src/utxo.rs` — UTXO set trait, in-memory implementation
- CVE-2012-2459 — Bitcoin merkle duplicate coinage bug
