# QuantumVault Testing Strategy

## Overview

This document defines the comprehensive testing strategy for QuantumVault, a quantum-resistant UTXO-based Layer 1 blockchain. The strategy follows a **Test Pyramid** approach with clear coverage targets, performance benchmarks, and test organization by module.

The testing infrastructure emphasizes:
- **Security-critical correctness**: All cryptographic operations thoroughly validated with Known-Answer Tests (KATs)
- **Deterministic behavior**: UTXO, consensus, and network operations must be repeatable
- **Performance validation**: Hash rates, signature throughput, and block validation times tracked
- **Comprehensive coverage**: Target 80-95% line coverage depending on module criticality

---

## Test Pyramid

```
           ▲
          / \
         /   \  Property-Based & Fuzz Tests (~5%)
        /     \
       /_______\
      /         \
     /           \  Integration Tests (~15%)
    /             \
   /_____________\
  /               \
 /                 \  Unit Tests (~80%)
/___________________\
```

### Distribution by Layer

| Layer | Coverage | Count (2026-05-12) | Focus |
|-------|----------|--------------------|-------|
| **Unit Tests** | ~85% | ~577 tests | Module behavior, edge cases |
| **Integration Tests** | n/a | ~149 tests | End-to-end workflows, multi-crate flows |
| **Doc Tests** | n/a | ~10 tests | Compile-time API doğrulama |
| **Property-Based** | n/a | proptest (qv-core, qv-crypto) | Invariant checking, randomized inputs |
| **Fuzz Targets** | n/a | 6 (`fuzz/`) | UTXO commit, script run, tx decode |

**TOPLAM: ~736 passed / 0 failed / 34 ignored** (post-ADR-006, pre-M-04 build verify;
M-04 sonrası beklenen ~741/32). 32-34 ignored test bilinçli olarak yavaş KES
(~2s leaf-tree gen) veya bağımlı bir envanter ID'sine (T-01, B-03, D-07..D-12).

---

## 1. Unit Tests (80% of test suite)

Unit tests verify individual functions and components in isolation. Each module maintains its own test suite.

### 1.1 crypto/ — Cryptographic Operations

**Critical**: All cryptographic primitives must pass Known-Answer Tests (KATs) from official sources (NIST, Open Quantum Safe).

#### Dilithium Signature Tests
- [ ] `sign()` produces valid signature with correct key
- [ ] `verify()` returns true for valid (msg, sig, pubkey) tuple
- [ ] `verify()` returns false for invalid signature
- [ ] `verify()` returns false for tampered message
- [ ] `verify()` returns false for wrong public key
- [ ] Signature determinism: same seed produces same signature
- [ ] Key generation determinism with fixed seed
- [ ] Edge case: sign empty message (0 bytes)
- [ ] Edge case: sign maximum-size message (2^31-1 bytes conceptually, practical limit ~1MB)
- [ ] Corrupted signature fails verification (bit flip in signature)
- [ ] Known-Answer Tests (KATs) from ML-DSA specification
- [ ] Thread-safe sign/verify concurrent calls

**Test Category**: `crypto_dilithium_*`
**Coverage Target**: 95%

#### Kyber Key Encapsulation Tests
- [ ] `keygen()` generates valid public/private keypair
- [ ] `keygen()` determinism with fixed seed
- [ ] `encapsulate(pubkey)` produces valid (ciphertext, shared_secret) pair
- [ ] `decapsulate(ciphertext, privkey)` recovers identical shared_secret
- [ ] `encapsulate()` produces different ciphertext each time (randomness)
- [ ] `decapsulate()` fails with wrong private key
- [ ] `decapsulate()` fails with corrupted ciphertext
- [ ] Edge case: empty plaintext encapsulation
- [ ] Known-Answer Tests (KATs) from ML-KEM specification
- [ ] Shared secret size matches specification (32 bytes)

**Test Category**: `crypto_kyber_*`
**Coverage Target**: 95%

#### Hybrid KEM (X25519 + Kyber) Tests
- [ ] Hybrid encapsulate produces both X25519 and Kyber components
- [ ] Hybrid decapsulate with both keys recovers shared secret
- [ ] Hybrid encapsulation is deterministic given same seed
- [ ] Fallback to X25519 if Kyber fails (graceful degradation)
- [ ] Combined shared secret (XOR or concat) is correct
- [ ] Performance: hybrid encapsulation < 2x single encapsulation

**Test Category**: `crypto_hybrid_kem_*`
**Coverage Target**: 90%

#### Hash Functions Tests
- [ ] SHA3-256 matches NIST test vectors
- [ ] SHA3-512 matches NIST test vectors
- [ ] BLAKE3 matches reference implementation
- [ ] Argon2id KDF (keystore) matches OWASP-2023 params (m=65540 KiB, t=3, p=1)
- [ ] Hash determinism: same input always produces same output
- [ ] Different input produces different hash (collision not found in test range)
- [ ] Hash throughput: >100 MB/s (SHA3), >200 MB/s (BLAKE3)
- [ ] Edge case: hash empty input
- [ ] Edge case: hash very large input (>100MB via streaming)

**Test Category**: `crypto_hash_*`
**Coverage Target**: 95%

#### Random Number Generation Tests
- [ ] `prng_seed()` initializes PRNG state
- [ ] `prng_next()` produces values in expected range
- [ ] `prng_next()` produces different values across calls
- [ ] PRNG thread safety

**Test Category**: `crypto_rng_*`
**Coverage Target**: 85%

---

### 1.2 core/ — Transaction and Block Data Structures

#### Transaction Tests
- [ ] Transaction creation with inputs and outputs
- [ ] TxId computation is deterministic (same tx → same ID)
- [ ] Transaction serialization → deserialization roundtrip
- [ ] Transaction size calculation matches serialized size
- [ ] Coinbase transaction creation (no inputs, fixed output)
- [ ] Regular transaction validation (must have inputs and outputs)
- [ ] Edge case: transaction with 0 inputs (should fail)
- [ ] Edge case: transaction with 0 outputs (should fail)
- [ ] Edge case: transaction with 255 inputs (boundary)
- [ ] Edge case: transaction with 255 outputs (boundary)
- [ ] Transaction fee calculation: sum(inputs) - sum(outputs)
- [ ] Negative fee detection and rejection

**Test Category**: `core_transaction_*`
**Coverage Target**: 90%

#### UTXO Tests
- [ ] UTXO set creation and initialization
- [ ] Add output to UTXO set (new OutPoint)
- [ ] Spend output from UTXO set
- [ ] Double-spend rejection (try to spend same output twice)
- [ ] OutPoint uniqueness enforcement
- [ ] UTXO proof-of-existence after add
- [ ] UTXO non-existence after spend
- [ ] Batch add operations (multiple UTXOs at once)
- [ ] Batch spend operations (multiple spends at once)
- [ ] Rollback after failed operation
- [ ] Iterator over all UTXOs in set
- [ ] Size/balance computation

**Test Category**: `core_utxo_*`
**Coverage Target**: 90%

#### Block Tests
- [ ] Block creation with header and transactions
- [ ] Block hash computation is deterministic
- [ ] Block hash changes when header changes
- [ ] Block serialization → deserialization roundtrip
- [ ] Block size calculation matches serialized size
- [ ] Merkle tree computation with 0 transactions (genesis)
- [ ] Merkle tree computation with 1 transaction
- [ ] Merkle tree computation with odd number of transactions (3, 5, 7)
- [ ] Merkle tree computation with power-of-2 transactions (2, 4, 8, 16)
- [ ] Merkle root determinism: same transactions → same root
- [ ] Merkle tree inclusion proof for transaction
- [ ] Block header validation (slot monotonic, timestamp, prev_hash, version)
- [ ] Genesis block accepts genesis tx (height=0 special case)
- [ ] Non-genesis block rejects genesis-style tx

**Test Category**: `core_block_*`
**Coverage Target**: 90%

#### OutPoint Tests
- [ ] OutPoint creation from (TxId, index)
- [ ] OutPoint equality
- [ ] OutPoint serialization roundtrip
- [ ] OutPoint uniqueness (two different (TxId, index) pairs are different)

**Test Category**: `core_outpoint_*`
**Coverage Target**: 90%

---

### 1.3 consensus/ — Ouroboros Praos PoS

**Mimari:** Saf PoS (hibrit PoW+PoS bırakıldı 2026-04-15 Rust pivotunda). VRF tabanlı
slot lider seçimi + Sum-KES forward-secure blok imzası + density-weighted longest
chain fork choice. Detay: CLAUDE.md + ADR-004 + ADR-005.

#### VRF Tests (Ristretto255 / schnorrkel)
- [x] VRF keypair generation produces correct-size keys (sk + pk both 32 byte)
- [x] `from_seed` deterministic: same 32-byte seed → same keypair
- [x] `evaluate(sk, msg) → (output, proof)` deterministic
- [x] `verify(pk, msg, proof) → output` roundtrip
- [x] Tampered proof / message / wrong pk → verify fails
- [x] Output uniformly distributed across stake-proportional threshold
- [x] Wire format: pre_out(32) || proof(64) = 96 bytes

**Test Category**: `crypto_vrf_*` + `consensus_leader_schedule_*`
**Coverage Target**: 90%

#### KES Tests (Sum-KES on Dilithium L3)
- [x] `kes_generate(master_seed) → (pk, sk)` deterministic
- [x] Master seed → 2048 leaf-seed pre-derive tree (depth=11)
- [x] `kes_sign(sk, msg)` returns leaf signature + Merkle path
- [x] `kes_verify(pk, sig, msg)` recomputes Merkle root, compares
- [x] `kes_evolve(sk)` advances period, zeroizes prior leaf
- [x] Cross-period signatures don't match (forward security)
- [x] Tampered leaf signature / Merkle path → verify fails

**Test Category**: `crypto_kes_*` + `consensus_block_validator_*`
**Coverage Target**: 90%

#### Slot / Epoch / Leader Schedule
- [x] Slot 2 saniye, epoch 21 600 slot (12 saat)
- [x] `EpochInfo` slot → epoch mapping
- [x] Genesis nonce nonzero
- [x] Nonce evolution deterministic; aynı VRF outputs sırası → aynı sonraki nonce
- [x] Nonce contribution window = epoch'un ilk 2/3'ü
- [x] Leader threshold `T = 1 − (1−f)^σ` stake oranıyla monoton
- [x] Higher stake wins more slots (stat fairness test)
- [x] Minority attacker can't dominate over many slots

**Test Category**: `consensus_slot_*` + `consensus_epoch_*` + `consensus_leader_schedule_*`
**Coverage Target**: 85%

#### Fork Choice & Finality
- [x] Density-weighted longest chain wins (`chain_density_counts_recent_blocks`)
- [x] Tip tie-break by hash (deterministic)
- [x] k-deep finality (k=50): block at tip - k considered final
- [x] Finality prevents deep reorg
- [x] Multi-pool epoch simulation: stake-proportional leader frequency

**Test Category**: `consensus_chain_state_*` + integration `*_lifecycle`
**Coverage Target**: 85%

#### Stake & Rewards
- [x] Stake distribution: per-pool relative stake aggregation
- [x] Snapshot ignores inactive pools
- [x] Subsidy halves Bitcoin-style; emission capped at total supply
- [x] Reward distribution: operator fixed_cost + margin + delegator share
- [x] No tokens lost (sum-conservation invariant)

**Test Category**: `consensus_stake_*` + `consensus_rewards_*`
**Coverage Target**: 90%

#### Block Validation Pipeline
- [x] Block header validation (slot monotonic, version, prev_hash)
- [x] Merkle root verification
- [x] Transaction validation (all tx valid via Script VM)
- [x] UTXO validity (no double-spends in block, no double-spends across)
- [x] Genesis tx structure (height=0 unique)
- [x] VRF proof verification against producer's stake snapshot
- [x] KES signature verification on header bytes
- [x] Signature verification on each tx input (ML-DSA)
- [x] Block size validation (≤ ProtocolParams::max_block_size)
- [x] Timestamp validation (within slot boundaries, not too far future)
- [x] Invalid block rejection

**Test Category**: `consensus_validation_*`
**Coverage Target**: 85%

---

### 1.4 privacy/ — Stealth Addresses

#### Stealth Address Generation
- [ ] Stealth key generation from seed
- [ ] Public key derivation from stealth key
- [ ] Private spend key recovery determinism

**Test Category**: `privacy_stealth_*`
**Coverage Target**: 85%

#### Stealth Output Creation
- [ ] `create_output()` produces valid ciphertext
- [ ] `create_output()` uses recipient's public key correctly
- [ ] Ciphertext can be decrypted by recipient
- [ ] Ciphertext size matches specification

**Test Category**: `privacy_create_output_*`
**Coverage Target**: 85%

#### Stealth Output Scanning
- [ ] `scan_output()` finds outputs created for this address
- [ ] `scan_output()` rejects outputs created for other addresses
- [ ] `scan_output()` decrypts valid stealth data

**Test Category**: `privacy_scan_*`
**Coverage Target**: 85%

#### Stealth Spend
- [ ] Spend key recovery from scanned output
- [ ] Spend key can sign transaction
- [ ] Incorrect key cannot spend output

**Test Category**: `privacy_spend_*`
**Coverage Target**: 85%

---

### 1.5 vm/ — DSL Bytecode Interpreter

#### Opcode Execution
- [ ] Each opcode (OP_PUSH, OP_DUP, OP_DROP, OP_VERIFY, etc.) executes correctly
- [ ] Stack state matches expected after opcode
- [ ] Opcode serialization roundtrip
- [ ] Invalid opcode rejection

**Test Category**: `vm_opcode_*`
**Coverage Target**: 90%

#### Script Execution
- [ ] Script serialization → deserialization roundtrip
- [ ] Script execution produces expected stack state
- [ ] Script failure on invalid state

**Test Category**: `vm_script_*`
**Coverage Target**: 90%

#### Standard Templates
- [ ] **P2PKH-PQC**: Create and verify standard pay-to-pubkey-hash with PQC key
- [ ] **MultiSig**: M-of-N signature validation
- [ ] **TimeLock**: Absolute time/block height lock
- [ ] **Stealth**: Stealth address unlock script

**Test Category**: `vm_template_*`
**Coverage Target**: 85%

#### Resource Limits
- [ ] Script execution within gas limit
- [ ] Gas limit enforcement (exceeding → failure)
- [ ] Stack overflow protection (max 1000 items)
- [ ] Memory limit enforcement

**Test Category**: `vm_limits_*`
**Coverage Target**: 85%

---

### 1.6 storage/ — Persistence Layer

#### Mempool Tests
- [ ] Transaction addition to mempool
- [ ] Fee-based ordering (highest fee first)
- [ ] Mempool size limit enforcement
- [ ] Eviction policy (lowest fee removed when full)
- [ ] Double-spend rejection (tx with same input)
- [ ] Transaction removal after inclusion in block

**Test Category**: `storage_mempool_*`
**Coverage Target**: 80%

#### Block Store Tests
- [ ] Store block to database
- [ ] Retrieve block by hash
- [ ] Retrieve block by height
- [ ] Block iteration (all blocks in order)
- [ ] Database consistency after crash (persistence check in integration tests)

**Test Category**: `storage_blockstore_*`
**Coverage Target**: 80%

#### UTXO Store Tests
- [ ] Store UTXO state
- [ ] Retrieve UTXO state
- [ ] Batch operations (add multiple UTXOs)
- [ ] Snapshot creation
- [ ] Rollback to snapshot
- [ ] Database compaction

**Test Category**: `storage_utxostore_*`
**Coverage Target**: 80%

---

### 1.7 da/ — Data Availability

#### Erasure Coding Tests
- [ ] Encode k data shards into k+m total shards
- [ ] Decode k shards recovers original data
- [ ] Decode with m missing shards (maximum loss)
- [ ] Decode with m+1 missing shards fails
- [ ] Reed-Solomon parameters correctness (k=16, m=8)
- [ ] Shard size consistency

**Test Category**: `da_erasure_*`
**Coverage Target**: 85%

#### Data Availability Proof Tests
- [ ] Proof generation for data block
- [ ] Proof verification by light client
- [ ] Invalid proof rejection

**Test Category**: `da_proof_*`
**Coverage Target**: 80%

---

## 2. Integration Tests (15% of test suite)

Integration tests verify complete workflows across multiple modules.

### 2.1 Full Transaction Lifecycle

**Test**: `integration_tx_lifecycle`

```
Create Transaction
  ↓
Sign Transaction
  ↓
Validate Transaction
  ↓
Add to Block
  ↓
Slot Leader (VRF) Produces Block
  ↓
KES-Sign Block Header
  ↓
Validate Block
  ↓
Connect Block to Chain
  ↓
UTXO State Updated
```

- [ ] Transaction created with valid inputs/outputs
- [ ] Transaction signed with sender's ML-DSA private key
- [ ] Transaction validates (signatures correct, no double-spend)
- [ ] Transaction added to block
- [ ] Slot leader chosen via VRF against stake snapshot
- [ ] Block header signed with current-period KES leaf
- [ ] Block validates (merkle root, VRF proof, KES sig, UTXOs)
- [ ] Block connects to chain (prev_hash matches)
- [ ] UTXO state reflects spent and new outputs
- [ ] Mempool updated (tx removed after block)

---

### 2.2 Block Production and Mining

**Test**: `integration_block_production`

```
Create Coinbase Transaction
  ↓
Add Regular Transactions from Mempool
  ↓
Compute Merkle Root
  ↓
VRF Leader Election (this slot, this pool)
  ↓
Fill BlockHeader (vrf_proof, utxo_commitment, ...)
  ↓
KES Sign Header (current period leaf)
  ↓
Gossip Block to Peers
  ↓
k-deep Finality (~100s later)
```

- [ ] Coinbase / reward transaction created with block subsidy + fees
- [ ] Transactions selected from mempool (highest fee density first)
- [ ] Merkle root computed correctly
- [ ] VRF leader-election check returns "elected" for this slot
- [ ] Block header filled (vrf_proof + utxo_commitment + producer hash)
- [ ] KES current-period leaf signs header
- [ ] Block gossiped via `/qv/blocks/1` topic
- [ ] After k=50 deeper blocks, this block is final

---

### 2.3 Stealth Address End-to-End

**Test**: `integration_stealth_e2e`

```
Generate Stealth Keys
  ↓
Create Stealth Output
  ↓
Broadcast Transaction
  ↓
Scan Output (Recipient)
  ↓
Recover Spend Key
  ↓
Create Spend Transaction
  ↓
Spend Output
```

- [ ] Stealth address keys generated deterministically
- [ ] Transaction with stealth output created
- [ ] Output broadcast on network
- [ ] Recipient scans chain and finds output
- [ ] Spend key recovered successfully
- [ ] Spend transaction created and signed
- [ ] Spend validated and included in block

---

### 2.4 Network Message Roundtrip

**Test**: `integration_network_roundtrip`

```
Create Message
  ↓
Serialize
  ↓
Transmit over P2P
  ↓
Receive
  ↓
Deserialize
  ↓
Validate
  ↓
Process
```

- [ ] Block message serialized correctly
- [ ] Transaction message serialized correctly
- [ ] Message transmitted via libp2p
- [ ] Message received and deserialized
- [ ] Deserialized message matches original
- [ ] Message validated (signature, format)
- [ ] Message processed (block added to chain, etc.)

---

### 2.5 Consensus Finality Integration

**Test**: `integration_consensus_finality`

```
Produce Block N
  ↓
Gather PoS Votes
  ↓
Reach 2/3 Threshold
  ↓
Block N Finalized
  ↓
Produce Block N+1
  ↓
Continue Chain
```

- [ ] Block produced by VRF-elected slot leader
- [ ] Block header signed with current-period KES leaf
- [ ] k=50 blocks added after target block → target is final
- [ ] Reorg attempts below k depth rejected (density-weighted longest chain)
- [ ] Next block references finalized parent
- [ ] Chain extends continuously

---

## 3. Property-Based and Fuzz Tests (5% of test suite)

Property-based tests use random input generation to verify invariants.

### 3.1 Cryptography Properties

**Test**: `prop_crypto_sign_verify`
- Property: `∀ msg, key: verify(sign(msg, key), msg, pubkey(key)) == true`
- Property: `∀ msg, key, wrong_key: verify(sign(msg, key), msg, pubkey(wrong_key)) == false`
- Property: `∀ msg: different_keys produce different signatures`

**Test**: `prop_crypto_kem_roundtrip`
- Property: `∀ pubkey: decapsulate(encapsulate(pubkey), privkey) succeeds`
- Property: `∀ pubkey, wrong_privkey: decapsulate fails`

---

### 3.2 Serialization Properties

**Test**: `prop_serialization_roundtrip`
- Property: `∀ tx: deserialize(serialize(tx)) == tx`
- Property: `∀ block: deserialize(serialize(block)) == block`
- Property: `∀ utxo: deserialize(serialize(utxo)) == utxo`

**Test**: `prop_fuzzing_serialization`
- Fuzz serialized messages, verify deserialization fails gracefully or recovers

---

### 3.3 UTXO Invariants

**Test**: `prop_utxo_double_spend`
- Property: `Spending same output twice always fails`
- Generate random sequences of spend/add operations
- Verify no double-spend ever succeeds

**Test**: `prop_utxo_balance`
- Property: `sum(available_utxos) <= initial_balance`
- After spending, balance decreases correctly

---

### 3.4 VM Resource Limits

**Test**: `prop_vm_gas_limit`
- Property: `No script can exceed gas_limit`
- Generate random bytecode sequences
- Verify execution halts before gas_limit exceeded

---

### 3.5 Consensus Invariants

**Test**: `prop_vrf_threshold`
- Property: `Pool with relative stake σ is leader on fraction T = 1 − (1−f)^σ of slots`
- Generate large slot sample, verify empirical frequency converges to expected
- See also `leader_election_proportional_to_stake` integration test

**Test**: `prop_pos_finality`
- Property: `Block finality requires exactly 2/3 votes`
- Less than 2/3 → not finalized
- Exactly 2/3 or more → finalized

---

## 4. Coverage Targets by Module

| Module | Target | Rationale |
|--------|--------|-----------|
| **crypto/** | 95% | Security-critical; all crypto primitives must be fully tested |
| **core/** | 90% | Data structure correctness essential for chain integrity |
| **consensus/** | 85% | Complex algorithm; focus on happy path and edge cases |
| **privacy/** | 85% | Privacy correctness important but less complex |
| **vm/** | 90% | Smart contract engine; execution must be deterministic |
| **storage/** | 80% | Database operations; covered by integration tests |
| **da/** | 85% | Data availability critical but proven algorithms |
| **net/** | 75% | Network layer covered by integration tests; focus on P2P protocols |
| **rpc/** | 70% | API layer; manual testing + integration tests sufficient |

---

## 5. Performance Benchmarks

All benchmarks measured on standard hardware. Target metrics:

### Cryptography Benchmarks
- **SHA3-256**: >100 MB/s
- **BLAKE3**: >200 MB/s
- **Dilithium Sign**: >10,000 operations/sec
- **Dilithium Verify**: >5,000 operations/sec
- **Kyber Encapsulate**: >5,000 operations/sec
- **Kyber Decapsulate**: >5,000 operations/sec

### Blockchain Benchmarks
- **Block Validation**: <100ms per block (100 tx)
- **Transaction Verification**: <1ms per tx
- **Merkle Root Computation**: <10ms per 1000 tx
- **VRF evaluate + verify**: <2ms (Ristretto255)
- **KES sign + verify**: <50ms (Sum-KES leaf + Merkle path, Dilithium L3)
- **ML-DSA sign + verify**: <5ms / <2ms (FIPS 204, ml-dsa 0.0.4)
- **Block production end-to-end**: <500ms (snapshot + order + sign + gossip)

### Storage Benchmarks
- **Mempool Insert**: <1ms per tx
- **UTXO Add**: <1ms per output
- **Block Store Write**: <10ms per block
- **Block Store Read**: <1ms per block

---

## 6. Test Organization and Execution

### Directory Structure
```
tests/
  crypto/
    test_dilithium.cpp
    test_kyber.cpp
    test_hybrid_kem.cpp
    test_hash.cpp
    test_rng.cpp
    TEST_PLAN.md
  core/
    test_transaction.cpp
    test_block.cpp
    test_utxo.cpp
  consensus/
    test_pow.cpp
    test_pos.cpp
    test_validation.cpp
  privacy/
    test_stealth.cpp
  vm/
    test_opcodes.cpp
    test_scripts.cpp
  storage/
    test_mempool.cpp
    test_blockstore.cpp
  da/
    test_erasure.cpp
  integration/
    test_tx_lifecycle.cpp
    test_block_production.cpp
    test_stealth_e2e.cpp
    test_consensus_finality.cpp
  CMakeLists.txt
```

### Running Tests

```bash
# Run all tests
ctest --test-dir build

# Run specific test category
ctest --test-dir build -R crypto

# Run with verbose output
ctest --test-dir build --verbose

# Run with code coverage
cmake --preset dev -DENABLE_COVERAGE=ON
ninja -C build test_coverage

# Run benchmarks
./build/bin/crypto_benchmark
./build/bin/consensus_benchmark
```

### Continuous Integration

CI pipeline runs:
1. All unit tests (coverage >80%)
2. All integration tests
3. Code coverage analysis
4. Static analysis (clang-tidy, cppcheck)
5. Benchmark regression detection

---

## 7. Known-Answer Tests (KATs)

### Dilithium KATs
- Source: [ML-DSA Specification](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)
- Test vectors provided in `tests/crypto/kat/dilithium/`
- Verify against reference implementation from NIST

### Kyber KATs
- Source: [ML-KEM Specification](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.203.pdf)
- Test vectors provided in `tests/crypto/kat/kyber/`
- Verify against reference implementation from `pqcrypto-kyber` (PQClean) and ML-KEM RustCrypto crate

### SHA3 KATs
- Source: [NIST CAVP](https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/)
- Test vectors for SHA3-256 and SHA3-512

### Argon2id KATs (keystore, NOT PoW)
- Source: [Argon2 Specification](https://github.com/P-H-C/phc-winner-argon2)
- Used only inside `qv_wallet::keystore` and `qv_miner::keystore` for password-based
  key derivation; **not** a consensus primitive. OWASP-2023 params: m=65540 KiB,
  t=3, p=1, output 32 bytes (AES-256 key).

### ML-DSA Test Vectors (FIPS 204)
- Source: [NIST FIPS 204](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)
- ML-DSA-44 / ML-DSA-65 / ML-DSA-87 KATs — sk size 2560/4032/4896, sig size 2420/3309/4627
- Exercised by `qv_crypto::pqc_sign::tests::fips204_size_invariants`

### Ristretto255-VRF Vectors
- Source: schnorrkel crate tests (IETF draft-irtf-cfrg-vrf-15 ECVRF-RISTRETTO255-SHA512)
- Exercised by `qv_crypto::vrf::tests::*`

---

## 8. Test Automation and Reporting

### Coverage Reports
```bash
# Generate HTML coverage report
cmake --preset dev -DENABLE_COVERAGE=ON
ninja -C build test_coverage
# Open build/coverage/index.html in browser
```

### Benchmark Reports
```bash
# Generate benchmark report
ninja -C build run_benchmarks
# Output: build/benchmark_results.json (JSON format for trend analysis)
```

### CI Reporting
- Coverage: Must be >85% across codebase
- Benchmarks: Alert if performance regresses >5%
- Test Results: All tests must pass before merge

---

## 9. Future Enhancements

1. **Fuzzing Integration**: Add libFuzzer harnesses for serialization, VM, and network message parsing
2. **Symbolic Execution**: Use symbolic execution tools to verify cryptographic correctness
3. **Mutation Testing**: Verify that test suite can detect code mutations
4. **Formal Verification**: Formal proofs for consensus and UTXO logic
5. **Chaos Engineering**: Network partition simulation, validator failure scenarios

---

## 10. References

- [Test Pyramid Pattern](https://martinfowler.com/bliki/TestPyramid.html)
- [cargo-nextest](https://nexte.st/) — Rust test runner (v2 stack; v1'de Google Test idi)
- [criterion.rs](https://github.com/bheisler/criterion.rs) — Rust benchmark (v2 stack; v1'de Google Benchmark idi)
- [proptest](https://github.com/proptest-rs/proptest) — property-based testing
- [Post-Quantum Cryptography Standards (NIST FIPS 203, 204)](https://csrc.nist.gov/projects/post-quantum-cryptography/)
- [RustCrypto `ml-dsa`](https://github.com/RustCrypto/signatures/tree/master/ml-dsa) — FIPS 204 saf-Rust ML-DSA implementation (ADR-006)

---

**Last Updated**: 2026-04-10
**Status**: APPROVED
