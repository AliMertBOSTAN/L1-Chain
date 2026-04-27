# Threat Model: qv-crypto

**Module**: Cryptographic primitives (hash, signing, KEM, VRF, KES, threshold)  
**Public API**: `hash()`, `sha3_256()`, `blake3()`, `sign_pqc()`, `verify_pqc()`, `encapsulate_hybrid()`, `decapsulate_hybrid()`, `SecureBytes`, `SharedSecret`  
**Threat Count**: 8 (1 Critical, 3 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Private keys** — Dilithium secret keys, Kyber private keys, X25519 scalars
   - Confidentiality: CRITICAL
   - Integrity: CRITICAL (forged signatures leak coins)
2. **Shared secrets** — KEM decapsulation output for session encryption
   - Confidentiality: CRITICAL (session compromise)
3. **Hash outputs** — Merkle root, transaction ID, merkle path consistency
   - Integrity: CRITICAL (ledger commits to these)
4. **Signature verification** — accept only valid PQC signatures
   - Integrity: CRITICAL (invalid signature acceptance = double-spend)
5. **Random number generation** — entropy for keypair generation
   - Confidentiality: CRITICAL (weak RNG = weak keys)

### Trust Boundaries
- **Upstream**: `pqcrypto-dilithium`, `pqcrypto-kyber` crates + liboqs C library
- **Consumer**: All other crates call `sign_pqc()`, `verify_pqc()`, `encapsulate_hybrid()`, `decapsulate_hybrid()`
- **Side-channel**: Timing, power, cache state during signing / KEM operations

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. PQC signature forgery (Dilithium mathematical break) | Spoofing | Critical | Deferred | Assume CRYSTALS-Dilithium secure ≥15 years; monitor NIST research |
| 2. Kyber KEM IND-CCA2 break (quantum attack) | Information Disclosure | Critical | Deferred | Assume ML-KEM secure; hybrid X25519 as fallback |
| 3. Weak entropy source → predictable private keys | Information Disclosure | Critical | Deferred | Use `OsRng` from `rand` crate; audit platform RNG |
| 4. Timing side-channel leaks private key bits | Information Disclosure | High | Partial | `subtle::ConstantTimeEq` for signature verify; more needed for signing |
| 5. KEM ciphertext tampering → incorrect shared secret | Tampering | High | Mitigated | Kyber is IND-CCA2; decapsulation rejects invalid CT |
| 6. Hash collision on transaction ID (SHA3-256 break) | Tampering | High | Deferred | Assume SHA3-256 is cryptographically sound; monitor for attacks |
| 7. `SecureBytes` zeroization failure (memleak of secrets) | Information Disclosure | High | Mitigated | Use `zeroize` crate with volatile write; test in production |
| 8. Signing side-channel (cache, power) during Dilithium | Information Disclosure | Medium | Partial | No constant-time signing guarantee yet; use HSM in production |

---

## Detailed Threat Analysis

### Threat 1: PQC Signature Forgery (Critical)

**Scenario**: Attacker breaks Dilithium via a mathematical attack, forges validator block signatures.

**Impact**: Consensus system is completely compromised; attacker can create arbitrary blocks, steal all coins.

**Likelihood**: Very Low (NIST-selected, ~15-year horizon before quantum threat).

**Mitigation Status**: Deferred
- Current: Trust in CRYSTALS-Dilithium standardization process
- Future: Formal verification of Dilithium in EasyCrypt or Coq
- Monitoring: Quarterly review of NIST post-quantum publications + CRY cryptanalysis papers

**Residual Risk**: If a break is announced, no protocol-layer fix is possible without a hard fork to a different signing scheme.

---

### Threat 2: Kyber KEM Decapsulation Attack (Critical)

**Scenario**: Attacker discovers a decryption oracle against Kyber IND-CCA2, or quantum adversary performs Shor's algorithm.

**Impact**: All encrypted mempool transactions become plaintext; MEV attacks become trivial.

**Likelihood**: Very Low (NIST ML-KEM standard, ~15-year horizon).

**Mitigation Status**: Deferred
- Current: Hybrid X25519 + Kyber provides fallback if Kyber breaks
- If X25519 alone holds, encrypted mempool degrades to classical ECC
- Future: Monitor Kyber cryptanalysis; consider migration to post-post-quantum schemes

**Residual Risk**: Threshold decryption committee knows plaintext if Kyber breaks; depends on honesty of 2/3+ committee members.

---

### Threat 3: Weak Entropy in Key Generation (Critical)

**Scenario**: `OsRng` on deployment platform (Linux, macOS, Windows) is weakly seeded; attacker predicts private keys.

**Impact**: All keys derived from the predictable RNG are compromised; attacker can forge signatures + spend coins.

**Likelihood**: Low (modern OSes have cryptographically secure RNG) but non-zero on embedded / custom platforms.

**Mitigation Status**: Partial
- Current: Use `rand::rngs::OsRng` which calls `getrandom()` on Linux, `SecRandomCopyBytes` on macOS, `BCryptGenRandom` on Windows
- Testing: `cargo test` includes entropy quality tests (not run in CI; human verification)
- Future: Audit RNG on deployment platform; use hardware RNG if available (e.g., TPM)

**Residual Risk**: If a custom platform uses weak RNG, keys are predictable. No code-level detection.

---

### Threat 4: Timing Side-Channel in Signature Verification (High)

**Scenario**: Attacker observes time to verify a signature, extracts key bits via timing analysis (Kocher attack).

**Impact**: Private key leakage; attacker can forge signatures.

**Likelihood**: Medium (requires lab access to validator hardware; less feasible remotely).

**Mitigation Status**: Partial
- Current: `subtle::ConstantTimeEq` for byte-by-byte signature verification
- Missing: Dilithium signature generation itself is not constant-time in `pqcrypto-dilithium`; uses early-exit loops
- Future: Use constant-time Dilithium implementation from `liboqs` when available; deploy on resistant hardware (ARM with cache timing mitigations)

**Residual Risk**: Timing leak in signing (not verification) can extract key bits. Validator must be isolated from network timing.

---

### Threat 5: KEM Ciphertext Tampering (High)

**Scenario**: Network attacker modifies ciphertext during ephemeral key exchange; `decapsulate()` rejects the CT.

**Impact**: Session establishment fails; attacker cannot recover the shared secret.

**Likelihood**: Low (network transport typically has integrity checks at TCP/TLS level).

**Mitigation Status**: Mitigated
- Kyber decapsulation is deterministic: malformed CT → `DecapsulationFailed` error
- Wire format includes length prefix; corrupt CT detected early
- Consumer (`qv-net`) aborts handshake on decapsulation failure

**Residual Risk**: None; Kyber IND-CCA2 + proper error handling.

---

### Threat 6: SHA3-256 Hash Collision (High)

**Scenario**: Attacker discovers two distinct transactions with the same SHA3-256 hash; both have identical TxId.

**Impact**: UTXO model breaks; attacker can replace one transaction with another without detection.

**Likelihood**: Very Low (SHA3-256 is a NIST standard with 256-bit security; 2^128 expected collision resistance).

**Mitigation Status**: Deferred
- Current: Assume SHA3-256 is cryptographically sound
- Fallback: If collision found, protocol hard-fork to SHA3-512 or BLAKE3
- Monitoring: Quarterly cryptanalysis updates from NIST + IACR

**Residual Risk**: Collision would require protocol upgrade; unavoidable during announcement-to-patch window.

---

### Threat 7: `SecureBytes` Zeroization Failure (High)

**Scenario**: Rust compiler optimizes away `zeroize` call; secret key remains in heap memory after dropping.

**Impact**: Private key leakage if attacker reads process memory (dump, VM snapshot, cold-boot attack).

**Likelihood**: Low (zeroize crate uses volatile writes; compiler is unlikely to optimize away without UB).

**Mitigation Status**: Mitigated
- Current: `zeroize` crate with `#[zeroize(drop)]` on `SecureBytes`
- Testing: Valgrind / AddressSanitizer verify no leaks (human, not CI)
- Production: Monitor for kernel-level memory protection (madvise MADV_WIPEONFORK)

**Residual Risk**: Compiler optimizations could theoretically bypass zeroization; mitigation is best-effort.

---

### Threat 8: Dilithium Signing Side-Channel (Medium)

**Scenario**: Attacker monitors power draw, cache misses, or speculative execution during `sign_pqc()` call; extracts key bits.

**Impact**: Private key compromise; forged signatures.

**Likelihood**: Low (requires specialized hardware; not practical for remote attacks).

**Mitigation Status**: Partial
- Current: No explicit constant-time Dilithium signing in `pqcrypto-dilithium`
- Missing: `pqcrypto-dilithium` uses reference implementation with variable-time loops in rejection sampling
- Future: Use constant-time Dilithium from formal verification (CRYSTALS-Dilithium ref or liboqs hardened variant)
- Deployment: Run validators on hardware with cache-timing mitigations (Intel TSX, ARM SVE masking)

**Residual Risk**: Signing is not constant-time; validator must assume isolated environment.

---

## Known Weaknesses & Future Work

1. **VRF not yet implemented** — `vrf.rs` is a stub; real Ouroboros-Praos VRF required (ADR-004)
2. **KES not yet implemented** — `kes.rs` is a stub; KES signature required (ADR-005)
3. **Threshold Kyber not yet implemented** — `threshold.rs` is a stub; distributed decryption required (ADR-006)
4. **RNG not seeded independently per thread** — single global seed could leak if threads share state
5. **No formal verification** — Dilithium, Kyber, SHA3 rely on NIST review, not machine-checked proofs

---

## Testing Strategy

### Unit Tests
- ✅ Hash determinism (same input → same output)
- ✅ Hash avalanche (1-bit input change → >128-bit output change)
- ✅ Signature roundtrip (sign → verify succeeds)
- ✅ Signature rejection (malformed signature fails)
- ✅ KEM roundtrip (encapsulate → decapsulate → same secret)
- ✅ KEM ciphertext tampering (malformed CT rejected)
- ✅ SecureBytes zeroization (debug output redacted)

### Fuzz Testing
- [x] `tx_parser` — arbitrary bytes → `Transaction::decode` (no panic)
- [x] `script_vm` — script bytes → opcode decode → execute (no panic, gas bounded)
- [x] Signature verification — malformed signatures → no panic
- [x] KEM decapsulation — malformed ciphertexts → no panic

### Integration Tests
- ✅ End-to-end: keypair generation → sign transaction → verify signature → broadcast
- ✅ Property-based (proptest): Associativity of hash chaining

---

## Audit Checklist

- [ ] Dilithium signature batch verification (SigVerifyBatch opcode if used)
- [ ] Kyber KEM in `libx25519-kyber` integration (no bit flips)
- [ ] X25519 scalar clamping (low-order point rejection)
- [ ] Shared secret derivation (KDF is non-invertible)
- [ ] Entropy source on deployment platform (pre-audit on staging)
- [ ] Zeroization testing with valgrind + asan
- [ ] No integer overflow in size calculations (KEM CT len)
- [ ] All debug impls redact secret material

---

## References

- `crates/qv-crypto/src/lib.rs` — Public API surface
- `crates/qv-crypto/src/pqc_sign.rs` — Dilithium wrapper
- `crates/qv-crypto/src/hybrid_kem.rs` — X25519 + Kyber hybrid
- [NIST PQC Standardization](https://csrc.nist.gov/projects/post-quantum-cryptography/) — ML-DSA, ML-KEM status
- [liboqs Documentation](https://openquantumsafe.org/) — liboqs implementation details
