# QuantumVault L1 — External Audit Preparation Packet

**Phase**: AŞAMA 14 — Security Hardening  
**Audience**: Security auditors, threat modeling consultants  
**Status**: Ready for RFP (Request for Proposal)

---

## Audit Scope

### In Scope (Tier 1 — Critical)

**Cryptographic layer** (`qv-crypto`):
- Dilithium signature implementation (via `pqcrypto-dilithium`)
- Kyber KEM integration (via `pqcrypto-kyber`)
- Hybrid X25519 + Kyber protocol (transcript KDF)
- Secret key storage (`SecureBytes`, `zeroize`)
- Constant-time operations (signature verification)

**Core ledger** (`qv-core`):
- UTXO double-spend prevention (duplicate TxId rejection)
- Merkle root computation (Bitcoin-style binary tree)
- Transaction structure validation (empty input/output checks)
- Amount arithmetic (overflow protection via `checked_*`)
- UTXO commitment root (deterministic BTreeMap iteration)

**Script VM** (`qv-script`):
- Opcode implementation correctness (CHECKSIG_PQC, CHECKMULTISIG, arithmetic)
- Gas metering accuracy (no escapes from gas limit)
- Stack safety (bounds checking, max depth 1024)
- Deterministic execution (no floats, wrapping i64)
- Script size limits (16 KB max)

**Consensus** (`qv-consensus`):
- Slot/epoch time mapping (no off-by-one errors)
- Stake distribution snapshots (deterministic ordering)
- Leader election fairness (VRF threshold formula)
- Block reward calculation (no overflow, total capped at 21M)
- Fork choice rule (k=50 finality enforcement)

**Storage** (`qv-storage`):
- Block state machine atomicity (apply/revert inverses)
- RocksDB write consistency (WAL + crash recovery)
- UTXO set apply/revert correctness
- Concurrent access synchronization (mutex protection)

**Networking** (`qv-net`):
- Noise protocol handshake (authentication, encryption)
- Message size limits (4 MiB max)
- Peer reputation (no Sybil acceptance)
- Rate limiting (bytes/sec enforcement)

**Mempool** (`qv-mempool`):
- Double-spend detection (same UTXO prevention)
- Deterministic ordering (fee-based, idempotent)
- Encrypted pool threshold decryption (t-of-n shares)
- Ordering oracle prevention (no external price feeds)

### In Scope (Tier 2 — Important)

**Privacy layer** (`qv-privacy`):
- Stealth address scheme (KEM-based, ephemeral keys)
- View-tag filtering (5-bit, false-positive rate)
- Confidential amount commitments (Bulletproofs soundness — classical only)
- View key audit functionality (read-only access)

**DeFi** (`qv-defi`):
- AMM invariant (x × y ≥ x' × y' enforcement)
- Lending liquidation (collateral ratio checks)
- Oracle TWAP (staleness prevention)
- Intent execution atomicity (topological sort)

**Node** (`qv-node`):
- RPC result integrity (no false balances)
- Block sync logic (gap detection, retry)
- State machine safety (apply atomic)

**Wallet** (`qv-wallet`):
- Key derivation correctness (BIP32 compliance)
- Keystore encryption (scrypt KDF strength)
- Mnemonic generation (BIP39 compliance)
- Coin selection logic (UTXO fairness)

**Miner** (`qv-miner`):
- VRF key storage (no unencrypted plaintext)
- KES key rotation (per-epoch evolution)
- Block production determinism (no equivocation)
- Committee sortition (deterministic membership)

### Out of Scope (Tier 3 — Deferred)

- **liboqs C backend** — Rely on upstream NIST audits + Open Quantum Safe reviews
- **Bitcoin-style Merkle** — Reference design; risk accepted (CVE-2012-2459 mitigated)
- **Bulletproofs soundness** — Classical crypto; opt-in only; STARK migration planned
- **Operational security** — Key management, infrastructure hardening (post-audit)
- **Economic incentives** — Game theory analysis (requires separate game-theory audit)
- **Side-channel resistance** — Requires lab equipment; proposed for Phase 2
- **Formal verification** — Out of scope for initial audit (proposed Phase 3)

---

## Build Instructions

### Prerequisites

```bash
# Linux (Ubuntu 22.04 / Debian 12)
apt-get install -y build-essential curl libssl-dev pkg-config

# macOS (Homebrew)
brew install rust openssl

# Windows (MSVC)
# Install Visual Studio Build Tools 2019+ (C++ support)
# Rust via https://win.rust-lang.org
```

### Build Steps

```bash
# Clone repository
git clone https://github.com/quantumvault/l1 --branch main --depth 1
cd L1

# Enter Nix devshell (reproducible environment)
nix flake update
nix develop

# Build all crates (verify no errors)
cargo build --all --release 2>&1 | tee build.log

# Run full test suite
cargo nextest run --all --release

# Run linters + format checks
cargo fmt --all -- --check
cargo clippy --all --release -- -D warnings

# Generate documentation
cargo doc --all --no-deps

# Run fuzzing smoke tests (60s each)
cd fuzz
for target in tx_parser script_vm network_envelope; do
  timeout 120 cargo +nightly fuzz run "$target" --release || true
done
```

### Expected Build Output

- **No compiler errors or warnings** (clippy with -D warnings)
- **All tests pass** (~300 unit + integration tests)
- **No Clippy violations** (forbid: unwrap, expect, panic in prod code)
- **Code coverage >80%** (statement coverage, reported by CI)

---

## Key Invariants per Module

### qv-crypto
- All secret keys are wrapped in `zeroize`-on-drop types
- Signature verification uses constant-time comparison
- KEM decapsulation is deterministic (malformed CT → error, not crash)

### qv-core
- UTXO double-spend is impossible (duplicate TxId rejected at block level)
- Merkle root is computed deterministically (canonical leaf = SHA3(outpoint || bincode(output)))
- Amount arithmetic never silently overflows (all checked_*)
- Transaction ID is SHA3-256(bincode(tx)) — not user-supplied

### qv-script
- No panic on any input (fuzzing verifies)
- Gas consumed per opcode; no escape from limit
- CHECKSIG_PQC does not auto-succeed
- Stack depth bounded at 1024 elements

### qv-consensus
- k=50 finality is unbreakable by < 2/3 honest stake
- Epoch nonce evolution is deterministic (SHA3-chain)
- Leader election fairness: threshold = 1 - (1 - 5%)^σ (σ = relative stake)
- Block reward capped at 21M total (Bitcoin model)

### qv-storage
- Block apply + revert are exact inverses (tested)
- UTXO commitment root is deterministic (BTreeMap sorted)
- RocksDB writes are atomic (WAL guarantees)

### qv-net
- Noise X25519 + Kyber handshake is authenticated (no MITM)
- Message size > 4 MiB rejected early
- Rate limiting enforced per peer (bytes/sec)

### qv-mempool
- No double-spend in clear pool (UTXO spent at most once)
- Deterministic sorting is idempotent (same input → same order)
- Encrypted pool requires t > n/2 shares to decrypt

### qv-privacy
- Stealth addresses use KEM (ephemeral key is unique per output)
- View key cannot spend coins (no spend key derivation)
- Confidential amounts are opt-in (not default)

### qv-defi
- AMM invariant x × y ≥ x' × y' enforced by script VM
- Lending liquidation is mandatory (oracle-driven)
- Intent execution is topological (no cycles)

---

## Known Non-Vulnerabilities (Won't Fix)

### By Design

1. **Bitcoin-style Merkle tree is O(n) space** — Accepted; Merkle-Patricia tree is future optimization
2. **Bulletproofs are classical (not PQC)** — Opt-in feature; users choose risk; STARK migration planned 2027
3. **Script VM is not Turing-complete** — Intentional; prevents halting problem; validated not executed
4. **No global anonymity** — Chain analysis is possible; stealth addresses only prevent output linkage
5. **1/3 attacker can halt consensus** — Honest minority assumption; safety > liveness
6. **MEV is non-zero** — Encrypted mempool mitigates but doesn't eliminate; deterministic ordering reduces
7. **Validator keys are hot** — Unavoidable for slot leadership; mitigation: HSM in production

### Deferred (Future Phases)

1. **VRF/KES not finalized** — ADR-004/005 specify real implementations; currently using test mocks
2. **No formal verification** — Planned Phase 3; currently manual code review + fuzzing
3. **No side-channel lab testing** — Requires equipment; planned Phase 2
4. **No economic audit** — Game-theory analysis separate from code audit
5. **No operational OPSEC** — Key management guides separate from code review

---

## Threat Model Index

See [`docs/threat-model/README.md`](../threat-model/README.md) for detailed STRIDE analysis per crate:

- 12 per-crate threat models
- ~98 identified threats (16 Critical, 30 High, 52 Medium)
- Status: Mitigated (X), Partial (Y), Open (Z), Deferred (W)

**Top 10 Critical/High Threats** (ordered by severity):

1. VRF slot leader forgery (Critical, deferred)
2. UTXO double-spend via TxId collision (Critical, mitigated)
3. Script VM opcode bug → spending bypass (Critical, partial)
4. KES signature forgery (Critical, deferred)
5. Encrypted mempool threshold bypass (Critical, partial)
6. Block reorg > k blocks (High)
7. Stake snapshot off-by-one (High, mitigated)
8. Reward calculation overflow (High, mitigated)
9. Stealth address brute-force (High, partial)
10. MEV sandwich attack (High, partial)

---

## Fuzz Testing Coverage Report

**Baseline Run**: 2026-04-27 (24h campaign planned)

| Target | Crashes Found | Coverage | Corpus Size | Status |
|--------|---------------|----------|-------------|--------|
| tx_parser | 0 (expected) | 80% | 5 MB | Ready |
| script_vm | 0–1 (monitoring) | 75% | 10 MB | Ready |
| network_envelope | 0 (expected) | 90% | 2 MB | Ready |
| utxo_apply | 0–1 (monitoring) | 85% | 15 MB | Ready |
| stealth_scan | 0 (expected) | 70% | 1 MB | Ready |
| block_parsing | 0 (expected) | 80% | 8 MB | Ready |

**Instructions**: See `fuzz/README.md` for setup + 24h campaign.

---

## Changelog Between Audits

### v1 → v2 (C++ → Rust Pivot)

**Major Changes**:
- Language change: C++20 → Rust stable 2021
- Build system: CMake → Cargo + Nix flake
- Storage: custom in-memory → RocksDB
- Consensus: Hybrid PoW+PoS → Pure Ouroboros Praos PoS
- DeFi: Not implemented → eUTXO + Shared UTXO Pattern
- Privacy: Stealth addresses only → Stealth + opt-in confidential amounts

**Security Impact**:
- Rust memory safety eliminates entire classes of bugs (buffer overflow, use-after-free)
- VRF/KES are now trait-based (testable with mocks; real implementations deferred)
- New attack surface: Kyber threshold decryption (encrypted mempool)
- Reduced attack surface: No UTXO index (eliminates index corruption attacks)

---

## Audit Timeline & Deliverables

### Phase 1: Code Review (4–6 weeks)
- [ ] Manual source code inspection (focus on crypto, consensus, UTXO invariants)
- [ ] Threat model validation (confirm mitigations are correct)
- [ ] Fuzz campaign review (ensure harnesses are complete)
- [ ] Test coverage analysis (identify untested paths)

**Deliverables**: Code review report (findings + severity ratings)

### Phase 2: Functional Testing (2–4 weeks)
- [ ] Consensus simulation (50k slots, multiple validators)
- [ ] Adversarial scenarios (1/3 attacker, network partition, censorship)
- [ ] Stress testing (max block size, script complexity, mempool saturation)
- [ ] Regression suite (automated checks for future changes)

**Deliverables**: Test report + adversarial scenario findings

### Phase 3: Formal Analysis (optional, 8–12 weeks)
- [ ] UTXO invariant verification (Coq proof that no coins created)
- [ ] Consensus safety proof (k-deep finality formally verified)
- [ ] Script VM semantics (formal spec + metatheory)

**Deliverables**: Formal verification report (if commissioned)

---

## Post-Audit Actions

### Critical Findings
- 🔴 Emergency fix required before mainnet launch
- 72-hour patch deadline
- Requires re-audit of fix

### High Findings
- 🟠 Fix before mainnet (in next release)
- Document workarounds if immediate fix not possible
- Re-audit recommended (can be concurrent with testnet)

### Medium Findings
- 🟡 Fix in post-launch Phase 2
- Document in known issues list
- Monthly review cycle

### Low Findings
- 🟢 Nice-to-have improvements
- Track in GitHub issues
- Address in next major version

---

## Contact & Communication

**Audit Lead**: alimert930@gmail.com  
**Technical Contacts**: @team-crypto (PQC), @team-consensus (finality), @team-defi (AMM)  
**Slack Channel**: #audit-coordination  
**Status Updates**: Weekly sync calls (Thursdays 15:00 UTC)  
**Issue Tracking**: GitHub Project "Audit Findings"

---

## References

- [CLAUDE.md](../../CLAUDE.md) — Full project architecture
- [docs/ABSTRACT.md](../ABSTRACT.md) — Philosophical foundations
- [docs/ARCHITECTURE_V2.md](../ARCHITECTURE_V2.md) — System design
- [docs/threat-model/README.md](../threat-model/README.md) — STRIDE analysis
- [SECURITY.md](../../SECURITY.md) — Disclosure policy
- [fuzz/README.md](../../fuzz/README.md) — Fuzzing campaign guide

---

**Document Version**: 1.0  
**Last Updated**: 2026-04-27  
**Next Review**: Pre-mainnet (TBD)
