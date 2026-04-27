# QuantumVault L1 Fuzzing Suite

**Purpose**: Property-based fuzzing of critical parsing, validation, and execution paths.  
**Target**: 1000+ CPU-hours per target over 24h continuous runs  
**Status**: AŞAMA 14 — Security Hardening

---

## Overview

This directory contains `libfuzzer`-based harnesses for the QuantumVault L1 codebase. Each target feeds randomized input to a critical public API and asserts invariants:

- **No panics** — all error paths return Result (no unwrap, expect, panic)
- **Bounded execution** — gas meter, stack limits, script size limits enforced
- **Deterministic output** — same input always produces same result (consensus safety)
- **No memory errors** — Rust's bounds checking + AddressSanitizer catch buffer overflows

---

## Fuzz Targets

### 1. `tx_parser` — Transaction deserialization

**Input**: Arbitrary bytes (0–10 KB)  
**API**: `qv_core::Transaction::decode()` (bincode deserialization)  
**Invariants**:
- Never panics on any input
- If deserializes successfully, re-encoding produces identical bytes (roundtrip)
- Rejects structurally invalid transactions (empty inputs/outputs)

**Corpus seed**: Valid transactions from integration tests  
**Expected**: Most inputs fail with `TransactionError`, ~1% succeed

```bash
cargo +nightly fuzz run tx_parser --release
```

### 2. `script_vm` — Script execution

**Input**: Random bytes interpreted as script opcodes (0–16 KB)  
**API**: `qv_script::decode_script()` → `qv_script::execute()` with synthetic Context  
**Invariants**:
- Never panics (all opcodes bounded)
- Gas consumed ≤ 100K (GasMeter enforces limit)
- Stack depth ≤ 1024 (no overflow)
- Execution time ≤ 1 second (timeout detection)

**Corpus seed**: Script templates (p2pkh_pqc, multisig, AMM)  
**Expected**: ~90% decode failure, ~10% execute timeout/success

```bash
cargo +nightly fuzz run script_vm --release
```

### 3. `network_envelope` — Message parsing

**Input**: Arbitrary bytes (0–4 MB)  
**API**: `qv_net::Envelope::decode()` (version tag + bincode)  
**Invariants**:
- Never panics on any input
- Version mismatch detected and rejected gracefully
- Message size > 4 MB rejected early

**Corpus seed**: Valid block/transaction gossip messages  
**Expected**: ~99% rejected (size limit or version mismatch)

```bash
cargo +nightly fuzz run network_envelope --release
```

### 4. `utxo_apply` — Block state machine

**Input**: Serialized Block (0–1 MB)  
**API**: `qv_storage::UtxoStore::apply_block()` → `revert_block()` roundtrip  
**Invariants**:
- Never panics (bounds checking on UTXO updates)
- After apply → revert, commitment root == initial state (atomic consistency)
- No coins created or destroyed (conservation law)

**Corpus seed**: Valid blocks from consensus integration tests  
**Expected**: ~50% apply success, 100% of applies roundtrip correctly

```bash
cargo +nightly fuzz run utxo_apply --release
```

### 5. `stealth_scan` — Privacy address scanning

**Input**: Random bytes interpreted as ephemeral key + ciphertext (0–1 KB)  
**API**: `qv_privacy::scan_output()` with test view key  
**Invariants**:
- Never panics (bounds checking on cryptographic operations)
- View-tag pre-filter rejects ~99.6% of unmatched outputs
- Matched outputs recover spend key deterministically

**Corpus seed**: Valid stealth outputs  
**Expected**: ~99.6% rejected by view-tag, ~0.4% checked, <0.01% matched

```bash
cargo +nightly fuzz run stealth_scan --release
```

### 6. `block_parsing` — Block deserialization

**Input**: Arbitrary bytes (0–10 MB)  
**API**: `qv_core::Block::validate_structure()`  
**Invariants**:
- Never panics on any input
- Duplicate TxId in block rejected
- Merkle root mismatch detected
- Header slot/height continuity checked

**Corpus seed**: Valid blocks  
**Expected**: ~99% rejected due to validation errors

```bash
cargo +nightly fuzz run block_parsing --release
```

---

## Running Fuzzing

### Prerequisites

```bash
# Install nightly Rust (required for libfuzzer)
rustup install nightly

# Install cargo-fuzz
cargo install cargo-fuzz

# Navigate to fuzz directory
cd fuzz
```

### Single-Target Smoke Test (60 seconds)

```bash
cargo +nightly fuzz run tx_parser --release -- -max_len=10000 -timeout=1 -max_total_time=60
```

### Full Fuzzing (24 hours)

```bash
# Run all targets in parallel (requires multiple cores)
for target in tx_parser script_vm network_envelope utxo_apply stealth_scan block_parsing; do
  timeout 86400 cargo +nightly fuzz run "$target" --release &
done
wait
```

### Continuous Integration

```bash
# In .github/workflows/security.yml (see below)
- name: Fuzz smoke test
  run: |
    cd fuzz
    cargo +nightly fuzz run tx_parser --release -- -max_total_time=60
    cargo +nightly fuzz run script_vm --release -- -max_total_time=60
```

---

## Crash Triage

If fuzzer finds a crash:

```bash
# Minimize the crashing input
cargo +nightly fuzz cmin tx_parser artifacts/crash-<hash>

# Run under debugger
lldb ./target/x86_64-unknown-linux-gnu/release/tx_parser <crash-file>

# Or inspect with cargo
cargo +nightly fuzz run tx_parser artifacts/<minimized-crash>
```

### Triage Checklist

1. **Reproduce**: Run harness with crashing input; verify panic
2. **Minimize**: Use `cargo fuzz cmin` to reduce input size
3. **Root cause**: Grep for `unwrap/expect/panic` in stack trace
4. **Fix**: Remove `unwrap`; return error instead
5. **Regression test**: Add unit test with minimized input
6. **Rerun**: Verify fuzzer no longer crashes on input

---

## Corpus Management

### Growing the Corpus

```bash
# Add new valid inputs from integration tests
cp crates/qv-core/tests/fixtures/*.bin fuzz/corpus/tx_parser/
cp crates/qv-script/tests/fixtures/*.bin fuzz/corpus/script_vm/
```

### Corpus Size

Keep each corpus small (<100 MB total):

```bash
ls -sh fuzz/corpus/*/
# tx_parser: 5 MB
# script_vm: 10 MB
# network_envelope: 2 MB
# utxo_apply: 15 MB
# stealth_scan: 1 MB
# block_parsing: 8 MB
```

---

## Expected Results

After 24h continuous fuzzing, expect:

| Target | Coverage | Crashes Found |
|--------|----------|---|
| tx_parser | ~80% paths covered | 0 (parsing is well-tested) |
| script_vm | ~75% opcode paths | 0–1 (VM is critical) |
| network_envelope | ~90% message paths | 0 (envelope is simple) |
| utxo_apply | ~85% UTXO paths | 0–1 (state machine critical) |
| stealth_scan | ~70% scan paths | 0 (privacy ops are simple) |
| block_parsing | ~80% block paths | 0 (parsing validated) |

---

## Integration with CI/CD

### Weekly Fuzzing Run (GitHub Actions)

```yaml
name: Fuzz Security Check
on:
  schedule:
    - cron: '0 2 * * 0'  # Weekly, Sunday 2am UTC

jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-fuzz
      - name: Run fuzz smoke test (60s per target)
        run: |
          cd fuzz
          for target in tx_parser script_vm network_envelope; do
            timeout 120 cargo +nightly fuzz run "$target" --release || true
          done
      - name: Upload corpus
        uses: actions/upload-artifact@v3
        if: always()
        with:
          name: fuzz-corpus
          path: fuzz/corpus/
```

---

## Known Limitations

1. **No symbolic execution** — libfuzzer is feedback-driven (coverage), not constraint-based
2. **Single-threaded** — async code not directly fuzzed (use `#[tokio::test]` for integration)
3. **Limited to binary protocols** — text-based protocols harder to fuzz (try structural fuzzing)
4. **No cross-crate fuzzing** — harnesses test crates individually, not interactions

---

## Future Work

1. **Structured fuzzing** — use `arbitrary` crate to generate syntactically valid inputs
2. **Differential fuzzing** — compare against reference implementation (e.g., Bitcoin Core)
3. **Concurrency fuzzing** — use `loom` to find data races in async code
4. **Profile-guided optimization** — use PGO to speed up hot paths
5. **OSS-Fuzz integration** — submit to Google's OSS-Fuzz for 24/7 fuzzing

---

## References

- [libfuzzer Documentation](https://llvm.org/docs/LibFuzzer/) — Fuzzing engine details
- [cargo-fuzz Guide](https://docs.rs/libfuzzer-sys/) — Rust bindings + setup
- [Fuzzing Handbook](https://fuzzing-handbook.gitbook.io/) — Best practices
- [OWASP Fuzzing](https://owasp.org/www-community/attacks/Fuzzing) — Attack context

---

**Contact**: alimert930@gmail.com  
**Status Tracker**: See [MEMORY.md](../MEMORY.md) for fuzz campaign results + discovered bugs.
