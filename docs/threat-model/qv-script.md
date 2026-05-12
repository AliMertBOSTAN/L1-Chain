# Threat Model: qv-script

**Module**: Stack-based script VM for UTXO spending validation  
**Public API**: `validate_script()`, `validate_script_with_gas()`, `OpCode`, `execute()`, `GasMeter`  
**Threat Count**: 8 (1 Critical, 2 High, 5 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Script execution determinism** — same script + same context always produces same result
   - Integrity: CRITICAL (non-determinism = consensus split)
2. **Gas metering accuracy** — scripts consume ≤100K gas before termination
   - Availability: CRITICAL (gas DoS could halt consensus)
3. **Stack safety** — no stack overflow, no out-of-bounds access
   - Availability: CRITICAL (crash = node halt)
4. **Witness/Datum parsing** — untrusted user input interpreted as script instructions
   - Integrity: CRITICAL (parsing bug = spending rules bypassed)
5. **Cryptographic operations** — CHECKSIG_PQC, hash operations, no weak algorithms
   - Integrity: CRITICAL (weak crypto = signature bypass)

### Trust Boundaries
- **Input**: Witness and locking script (untrusted) → decoded as instructions
- **Processing**: Stack machine with max depth 1024, gas meter tracking
- **Output**: Pass/fail decision on script execution
- **Side-channel**: Execution time proportional to script complexity

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. Script opcode implementation bug (e.g., CHECKSIG auto-success) | Spoofing | Critical | Partial | Fuzz testing required; manual code review |
| 2. Stack underflow/overflow (panic, memory safety) | Denial of Service | High | Mitigated | Max stack 1024, bounds checked, no unsafe |
| 3. Gas metering bug (infinite loop bypasses limit) | Denial of Service | High | Mitigated | All loops bounded, gas consumed per opcode |
| 4. Integer overflow in script arithmetic | Tampering | Medium | Mitigated | i64 wrapping arithmetic, overflow is defined |
| 5. Floating-point operations in determinism-critical path | Tampering | Medium | Mitigated | No floats; i64 arithmetic only |
| 6. Timing side-channel in CHECKSIG_PQC verification | Information Disclosure | Medium | Partial | Uses `subtle::ConstantTimeEq`; Dilithium verify constant-time? |
| 7. Script size DoS (16KB compiled script) | Denial of Service | Medium | Mitigated | Hard limit 16KB; parsing enforced |
| 8. Datum deserialization panic (malformed bincode) | Denial of Service | Medium | Partial | Consumer should catch panic; VM returns error |

---

## Detailed Threat Analysis

### Threat 1: Script Opcode Implementation Bug (Critical)

**Scenario**: CHECKSIG_PQC opcode has a logic error; always returns true regardless of signature validity.

**Impact**: Consensus bypassed; attacker can spend any UTXO without correct signature; theft of all coins.

**Likelihood**: Low (code reviewed; extensive unit tests) but non-zero.

**Mitigation Status**: Partial
- Current: ~70 unit tests covering all opcodes + edge cases
- Fuzz testing: `fuzz/fuzz_targets/script_vm.rs` runs 24h on random bytecode
- Code review: Manual inspection by cryptographers (pre-audit)
- Future: Formal verification of opcode semantics in Coq/Isabelle

**Residual Risk**: Implementation bugs in critical opcodes (CHECKSIG, CHECKMULTISIG) require external audit.

---

### Threat 2: Stack Underflow/Overflow (High)

**Scenario**: Script underflows stack (pops from empty stack); causes panic or buffer overread.

**Impact**: Node crash; DoS attack.

**Likelihood**: Low (Rust bounds checking; ExecResult includes stack depth checks).

**Mitigation Status**: Mitigated
- Code: `Stack` is `Vec<Value>`; pop checks `is_empty()` before returning `StackError::Underflow`
- Test: Unit test "stack_underflow_detected" in interpreter.rs
- Max depth: 1024 elements; exceeded → `StackError::StackOverflow`
- No unsafe code in stack operations

**Residual Risk**: None; Rust's bounds checking + explicit validation.

---

### Threat 3: Gas Metering Bug (High)

**Scenario**: Gas meter is not incremented for certain opcodes; attacker crafts script with infinite loop that escapes gas limit.

**Impact**: Script execution never terminates; node hangs; DoS.

**Likelihood**: Low (all opcodes have explicit gas cost; GasMeter consumed per iteration).

**Mitigation Status**: Mitigated
- Code: `GasMeter::consume(opcode)` is called before opcode execution; `remaining()` checked
- Loops: IF/ELSE/ENDIF nesting checked; no recursive call depth
- Test: Unit test "gas_exhaustion_halts_execution" in gas.rs
- Fuzz target: `fuzz/fuzz_targets/script_vm.rs` verifies gas bounded

**Residual Risk**: None; gas consumption is mandatory and checked.

---

### Threat 4: Integer Overflow in Script Arithmetic (Medium)

**Scenario**: Script adds two large i64 values; wraps around; attacker bypasses amount check.

**Impact**: Ledger arithmetic broken; coins created or destroyed.

**Likelihood**: Very Low (i64 wrapping arithmetic is defined behavior; script should validate invariants).

**Mitigation Status**: Mitigated
- Code: Arithmetic opcodes (ADD, SUB, MUL) use wrapping i64 operations
- Behavior: Wrap-around is deterministic and specified; attacker knows what happens
- Test: Unit test "arithmetic_wrapping_is_consistent" in interpreter.rs
- Consumer responsibility: Script template should validate amounts pre-arithmetic

**Residual Risk**: None; wrapping arithmetic is intentional and predictable.

---

### Threat 5: Floating-Point Operations (Medium)

**Scenario**: Script contains opcode that uses f64; different platforms produce slightly different results.

**Impact**: Consensus split; some nodes accept block, others reject.

**Likelihood**: None (design explicitly forbids floats; no FP opcodes exist).

**Mitigation Status**: Mitigated
- Code: No floating-point types in VM; only i64 integers
- Clippy: Workspace forbids `float_arithmetic` 
- Test: Property-based test verifies determinism across multiple runs

**Residual Risk**: None; floats are architecturally excluded.

---

### Threat 6: Timing Side-Channel in CHECKSIG_PQC (Medium)

**Scenario**: Attacker observes execution time of CHECKSIG_PQc opcode; infers secret key bits.

**Impact**: Private key leakage; forged signatures.

**Likelihood**: Low (requires timing measurement from network; not practical remotely).

**Mitigation Status**: Partial
- Current: `subtle::ConstantTimeEq` for byte comparison of signatures
- Missing: ML-DSA verification in `ml-dsa = 0.0.4` is not guaranteed constant-time (rejection sampling loops)
- Future: Audit `ml-dsa` for constant-time guarantees as crate matures (currently 0.x pre-stable, ADR-006 follow-up); run validators on timing-resistant hardware

**Residual Risk**: CHECKSIG_PQC timing depends on ML-DSA implementation; validator isolation required.

---

### Threat 7: Script Size DoS (Medium)

**Scenario**: Attacker submits 16KB script with >1M opcodes; parsing takes forever.

**Impact**: DoS attack on node; script validation time exhausted.

**Likelihood**: Low (16KB limit is enforced; parsing is O(n) with n ≤ ~2K opcodes).

**Mitigation Status**: Mitigated
- Code: `decode_script()` has `max_len = 16 * 1024` check; rejects larger scripts
- Parsing: Single-pass left-to-right; O(n) time
- Test: Unit test "script_size_limit_enforced" in opcode.rs

**Residual Risk**: None; size limit + linear parsing.

---

### Threat 8: Datum Deserialization Panic (Medium)

**Scenario**: Datum field contains malformed bincode (e.g., truncated message); consumer panics during deserialization.

**Impact**: Node crash; DoS.

**Likelihood**: Low (consumer should use `bincode::deserialize()` with error handling).

**Mitigation Status**: Partial
- Current: Script VM returns opcode error on malformed instruction; does not crash
- Missing: Datum deserialization is consumer's responsibility; not in `qv-script`
- Mitigation: Consumer (e.g., smart contract runtime) should use `?` operator + error handling

**Residual Risk**: Consumer must wrap deserialization in error handling; no guarantee in VM.

---

## Known Weaknesses & Future Work

1. **CHECKSIG_PQC timing is not constant-time** — depends on Dilithium implementation timing
2. **No formal verification of opcode semantics** — manual code review only
3. **Script execution time not metered separately** — only gas is tracked, not wall-clock time
4. **No script compression** — 16KB limit can be filled with useless data
5. **No script bytecode caching** — every execution re-parses bytecode (CPU waste)

---

## Testing Strategy

### Unit Tests
- ✅ Stack ops (PUSH, DUP, DROP, SWAP, NIP, OVER, ROT)
- ✅ Arithmetic (ADD, SUB, MUL, mod with wrapping)
- ✅ Comparison (EQ, LESS, GREATER, etc.)
- ✅ Crypto (CHECKSIG_PQC, CHECKMULTISIG_PQC, SHA3, BLAKE3)
- ✅ Control flow (IF/ELSE/ENDIF, nesting, VERIFY/RETURN)
- ✅ Introspection (TxHash, InputCount, OutputCount, ReadInputValue, ReadOutputValue)
- ✅ Covenants (AssertOutputScriptHash, AssertDatumHash)
- ✅ Gas metering (consumption per opcode, limit enforcement)

### Fuzz Testing
- [x] `fuzz/fuzz_targets/script_vm.rs` — random bytecode → decode + execute (no panic, gas bounded)
- [x] Script validation roundtrip: encode → decode → execute → deterministic

### Integration Tests
- ✅ p2pkh_pqc template (DUP, HASH_SHA3, CHECKSIG_PQC)
- ✅ multisig_pqc template (M-of-N key validation)
- ✅ AMM swap template (covenant enforcement)
- ✅ Lending pool template (interest accrual computation)
- ✅ Complex scripts (nested IF/ELSE, multiple CHECKSIG)

---

## Audit Checklist

- [ ] CHECKSIG_PQC does not auto-succeed on empty signature
- [ ] CHECKMULTISIG_PQC validates M <= N before key iteration
- [ ] Stack bounds checking on every pop (not just at end)
- [ ] Gas consumed before opcode execution (not after)
- [ ] IF/ELSE/ENDIF nesting depth limit (prevent deep recursion)
- [ ] No unhandled panics in VM (all error paths return ScriptError)
- [ ] Hash operations produce consistent output (no endianness bugs)
- [ ] Introspection opcodes (ReadInputValue, etc.) use correct indexing
- [ ] Covenant opcodes (AssertOutputScriptHash) match actual hashes
- [ ] No unsafe code in interpreter

---

## References

- `crates/qv-script/src/opcode.rs` — OpCode enum, Instruction encoding/decoding
- `crates/qv-script/src/interpreter.rs` — Execute function, stack machine
- `crates/qv-script/src/gas.rs` — GasMeter, gas cost table
- `crates/qv-script/src/templates.rs` — Standard script builders
- `crates/qv-script/src/script.rs` — High-level validation API
- [Stack machine design](https://en.bitcoin.it/wiki/Script) — Bitcoin script for reference
