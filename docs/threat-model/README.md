# QuantumVault L1 — Threat Model Index

**Current Status**: Tarihsel analiz (AŞAMA 14 dönemi, 2026-04-27 snapshot'ı) — temel STRIDE analizleri geçerli; o tarihten sonra kapanan boşluklar (gerçek VRF/KES, ML-DSA swap, hibrit handshake ADR-007, sighash ADR-012) için `PROJECT_STATUS.md` ve `docs/security/qv-consensus-fork-finality-audit.md` esas alınmalı  
**Last Updated**: 2026-04-27 (index notu: 2026-06-10)  
**Scope**: Per-crate threat analysis, attack surface inventory, mitigation tracking

---

## Overview

This directory contains systematic threat models for all QuantumVault L1 crates. Each crate receives a STRIDE-based analysis identifying:

- **Assets** — confidentiality, integrity, availability goals
- **Trust boundaries** — where adversaries can inject data
- **Threats** — 5–10 per crate, severity-ranked
- **Mitigations** — status (mitigated / partial / open)
- **Known weaknesses** — design gaps requiring future work

---

## Attacker Models

All threat analysis assumes one or more of the following adversaries operating:

### A1: Network Attacker
- Controls arbitrary nodes on the P2P network
- Crafts malformed messages, replays transactions, performs Sybil attacks
- Cannot forge valid PQC signatures or break Kyber KEM
- **Mitigations**: Message size limits, rate limiting, cryptographic validation, peer reputation

### A2: Malicious Validator (< 1/3 Stake)
- Runs a stake pool with < 1/3 total network stake
- Can produce invalid blocks, withhold blocks, equivocate (break finality by releasing competing chains)
- Cannot reach consensus unilaterally — requires honest 2/3 supermajority to finalize
- **Mitigations**: VRF slot leader proof, KES signature verification, k=50 finality + slashing

### A3: Malicious Validator (≥ 1/3 Stake)
- Controls ≥ 1/3 of network stake
- Can prevent consensus (network liveness loss) but cannot forge transactions or fake history
- Cannot compromise past-finalized blocks under k-deep finality rule
- **Mitigations**: Economic slashing, honest minority assumption for safety, liveness sacrifice

### A4: Sandboxed User Script
- Submits a malicious Datum or witness script to execute on L1 validators
- Cannot access private keys, cannot rewrite ledger, confined by gas + stack limits
- Can attempt denial-of-service (gas exhaustion, stack explosion, tight loops)
- **Mitigations**: Script VM determinism, gas metering, 16KB script size limit, 1024-element stack limit

### A5: Side-Channel Adversary
- Observes timing, power, cache behavior during cryptographic operations
- Aims to extract secret keys or break PQC assumptions
- **Mitigations**: Constant-time comparisons (`subtle`), `zeroize` on secret drop, no floating-point in signing

### A6: Supply Chain / Backdoor
- Compromises an upstream dependency or build toolchain
- Injects malicious code into binaries or libraries
- **Mitigations**: `cargo deny` + `cargo audit`, reproducible Nix builds, limited unsafe code (forbid by default)

---

## Crates & File Index

| Crate | Module | Threat Count | Severity Distribution |
|-------|--------|----------------|------------------------|
| **qv-crypto** | Hash, signing, KEM | 8 | 1 Critical, 3 High, 4 Medium |
| **qv-core** | Types, UTXO, Merkle | 7 | 0 Critical, 2 High, 5 Medium |
| **qv-script** | VM opcodes, interpreter, gas | 8 | 1 Critical, 2 High, 5 Medium |
| **qv-consensus** | Leader election, finality, rewards | 10 | 2 Critical, 3 High, 5 Medium |
| **qv-storage** | RocksDB, block store, state | 7 | 1 Critical, 2 High, 4 Medium |
| **qv-net** | P2P gossip, transport, rate limiting | 9 | 1 Critical, 3 High, 5 Medium |
| **qv-mempool** | Transaction ordering, MEV, encryption | 8 | 2 Critical, 2 High, 4 Medium |
| **qv-privacy** | Stealth addresses, confidential amounts | 9 | 1 Critical, 4 High, 4 Medium |
| **qv-defi** | AMM invariants, lending, oracle | 10 | 2 Critical, 3 High, 5 Medium |
| **qv-node** | Full node orchestration, RPC, lifecycle | 7 | 1 Critical, 2 High, 4 Medium |
| **qv-wallet** | Key derivation, transaction signing, balance scanning | 8 | 1 Critical, 2 High, 5 Medium |
| **qv-miner** | Pool operator, key rotation, committee sortition | 7 | 1 Critical, 2 High, 4 Medium |
| **Total** | — | **98 threats** | **16 Critical, 30 High, 52 Medium** |

---

## STRIDE Methodology

Each threat is classified by attack vector:

- **S (Spoofing)** — forging identity, impersonating a peer or validator
- **T (Tampering)** — modifying data in transit or at rest (ledger, block, transaction)
- **R (Repudiation)** — denying an action (e.g., claiming you did not send a transaction)
- **I (Information Disclosure)** — exposing secrets (private keys, transaction content)
- **D (Denial of Service)** — crashing nodes, consuming resources, preventing consensus
- **E (Elevation of Privilege)** — escaping the script VM, reading storage without permission

---

## Severity Rubric

| Severity | Description | Example |
|----------|-------------|---------|
| **Critical** | Breaks protocol safety, enables theft, or consensus halt | VRF forgery, UTXO duplication, undetectable double-spend |
| **High** | Significant availability or privacy impact, requires attacker resources | Large-scale Sybil attack, MEV extraction, stealth address leakage |
| **Medium** | Local DoS or privacy degradation, limited scope | Gas exhaustion in script VM, long finality reorg, timing leak |
| **Low** | Operational friction, usability impact, minimal security risk | RPC version mismatch, peer eviction edge case |

---

## Mitigation Status Definitions

- **Mitigated** — threat is prevented or detected by existing design + code
- **Partial** — threat is partially mitigated; residual risk quantified
- **Open** — threat is identified but not yet addressed; tracked for future phase
- **Deferred** — threat requires upstream primitive finalization (e.g., `ml-dsa` reaching 1.0 with constant-time guarantees)

---

## Cross-Cutting Concerns

### Cryptographic Trust

All crates relying on PQC assume:
- FIPS 204 ML-DSA (`ml-dsa = 0.0.4`, RustCrypto; ADR-006) is unbroken for ≥15 years
- FIPS 203 ML-KEM (Kyber via `pqcrypto-kyber`) is IND-CCA2 secure
- Hybrid X25519 + Kyber provides defense-in-depth against both classical and quantum breaking
- `ml-dsa` saf-Rust implementation is free of obvious side-channel leaks (constant-time audit pending — crate hâlâ 0.x; ADR-006 follow-up)

**Mitigation**: Hybrid (classical + PQC) model; constant-time ops; formal verification of threshold schemes (future)

### Resource Exhaustion

All boundary-facing code (network, RPC, script VM) must enforce limits:
- Message size: 4 MiB max
- Script size: 16 KiB max
- Gas per script: 100K gas (configurable)
- Stack depth: 1024 elements
- Peer connection count: configurable (e.g., 512)
- RPC request rate: per-peer limits

**Mitigation**: Hard limits in code; `GasMeter` for script VM; rate limiting in `qv-net`

### Finality & Consensus

k-deep finality (k=50 blocks, ~100 seconds) guarantees:
- An honest minority cannot be reorged beyond k blocks
- Past-finalized history is immutable (barring 51% stake attack)
- Slashing mechanism incentivizes honesty

**Residual risk**: 1/3 stake can halt consensus (liveness loss); 51% can rewrite history (safety loss)

### Privacy Leakage

Stealth addresses are default but not global anonymity:
- Chain observer can link sender → recipient via output appearance
- Timing analysis (same sender, same time) weakens privacy
- Optional confidential amounts expose receiver (only sender amount hidden)
- View key audit functionality leaks to auditor

**Mitigation**: Stealth addresses (KEM-based), view-tag pre-filter (99.6%), optional confidential amounts (classical)

---

## Per-Crate Documentation

See individual files for detailed threat matrices:

- [qv-crypto.md](qv-crypto.md) — Hashing, signing, KEM, VRF, KES, threshold
- [qv-core.md](qv-core.md) — Transaction structure, UTXO model, Merkle trees
- [qv-script.md](qv-script.md) — Script VM, gas, opcodes, templates
- [qv-consensus.md](qv-consensus.md) — Leader election, finality, rewards
- [qv-storage.md](qv-storage.md) — Persistence, RocksDB, snapshots
- [qv-net.md](qv-net.md) — P2P transport, gossip, rate limiting
- [qv-mempool.md](qv-mempool.md) — Ordering, encryption, threshold decryption
- [qv-privacy.md](qv-privacy.md) — Stealth, confidential amounts, view keys
- [qv-defi.md](qv-defi.md) — AMM, lending, oracle, intents
- [qv-node.md](qv-node.md) — Full node, RPC, lifecycle
- [qv-wallet.md](qv-wallet.md) — Key derivation, signing, scanning
- [qv-miner.md](qv-miner.md) — Pool operator, key rotation, committee

---

## Top 10 Critical & High-Severity Threats (System-Wide)

(Sorted by impact + likelihood)

| Rank | Threat | Crate | Severity | Status |
|------|--------|-------|----------|--------|
| 1 | VRF slot leader forgery | qv-consensus | Critical | Deferred (ADR-004) |
| 2 | UTXO double-spend via duplicate-TxId replay | qv-core | Critical | Mitigated |
| 3 | Script VM escape via opcode implementation bug | qv-script | Critical | Partial (fuzz testing required) |
| 4 | Block state machine bypass (invalid block accepted) | qv-storage | Critical | Mitigated |
| 5 | KES signature forgery or forward-secret leak | qv-consensus | Critical | Deferred (ADR-005) |
| 6 | Stealth address view-tag collision (brute-force) | qv-privacy | High | Mitigated (view-tag: 5 bits) |
| 7 | Encrypted mempool threshold reconstruction bypass | qv-mempool | High | Partial (implementation audit) |
| 8 | MEV sandwich attack (reordering despite encryption) | qv-defi | High | Partial (economic, deterministic batch) |
| 9 | Peer reputation Sybil attack + partition | qv-net | High | Partial (reputation + rate limit) |
| 10 | RocksDB corruption under concurrent access | qv-storage | High | Partial (mutex protection required) |

---

## Audit Preparation

### In Scope

- [x] Rust crate public APIs
- [x] Cryptographic libraries (via `pqcrypto-*` wrapper interface)
- [x] UTXO state machine (double-spend prevention)
- [x] Block validation (header, merkle, duplicates)
- [x] Script VM determinism + gas metering
- [x] Leader election fairness + VRF input integrity
- [x] Fork choice rule + k-deep finality
- [x] Stealth address pre-filter correctness
- [x] Transaction ordering determinism
- [x] RPC interface isolation (no leaks)

### Out of Scope (for initial audit)

- [ ] RustCrypto `ml-dsa` upstream audit (saf-Rust FIPS 204; ADR-006); liboqs/oqs-rs ADR-006 ile bırakıldı
- [ ] RocksDB internal consistency (production testing required)
- [ ] Bulletproofs range proof soundness (classical, not PQC; opt-in only)
- [ ] Full node operator security (OPSEC: key management, infrastructure)
- [ ] Economic incentive analysis (game theory modeling)
- [ ] Side-channel resistance (requires lab testing + specialized tooling)

---

## References

- [CLAUDE.md](../../CLAUDE.md) — Project architecture decisions
- [PROJECT_STATUS.md](../../PROJECT_STATUS.md) — Phase completion status
- [docs/ARCHITECTURE_V2.md](../ARCHITECTURE_V2.md) — System design overview
- [docs/ADR-003](../ADR/003-mev-encrypted-mempool.md) — MEV strategy
- [SECURITY.md](../security/audit-prep.md) — Audit scope + runbook

---

## Next Steps

1. **Fuzz Testing** — Run `fuzz/` targets for 24h on each crate (transaction parsing, script VM, network codec)
2. **External Audit** — Engage tier-1 blockchain security firm (3–6 months pre-mainnet)
3. **Formal Verification** — Model consensus invariants + UTXO double-spend prevention in Coq/Isabelle (future)
4. **Penetration Testing** — Adversarial network simulation, validator behavior testing, side-channel lab testing
5. **Bug Bounty** — Public disclosure policy + responsible 90-day timeline

---

**Questions?** Contact: alimert930@gmail.com  
**Status Tracking**: See [MEMORY.md](../../MEMORY.md) for crate implementation status + open design questions.
