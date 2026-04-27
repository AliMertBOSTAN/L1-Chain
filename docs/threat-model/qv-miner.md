# Threat Model: qv-miner

**Module**: Stake pool operator (key management, block production, committee sortition)  
**Public API**: `OperatorConfig`, `run_slot_loop()`, `produce_block()`, `check_committee_membership()`  
**Threat Count**: 7 (1 Critical, 2 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **VRF private key** — used to prove block leadership
   - Confidentiality: CRITICAL (leak = impersonation + theft of rewards)
2. **KES private key** — forward-secure signing key
   - Confidentiality: CRITICAL (leak = current epoch signatures forged)
3. **Cold key** — offline key for pool registration
   - Confidentiality: CRITICAL (leak = pool control lost)
4. **Block production consistency** — same block on all nodes
   - Integrity: CRITICAL (divergent blocks = slashing)

### Trust Boundaries
- **Input**: Slot ticks from clock, mempool from node, committee shares from other validators
- **Processing**: VRF slot check, block production, signature
- **Output**: Signed blocks broadcast to network
- **Attacker**: Network (block relay), collusion (committee), timing attacks

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. VRF key theft (impersonate leader) | Information Disclosure | Critical | Partial | HSM storage recommended; no on-disk plaintext |
| 2. KES key compromise (sign arbitrary blocks) | Information Disclosure | Critical | Partial | Key rotation each epoch; old key destroyed |
| 3. Double-sign (produce competing blocks same slot) | Spoofing | High | Partial | Slashing punishment; detection via blockchain |
| 4. Committee member collusion (decrypt early) | Tampering | High | Partial | Requires 2/3+ honest committee; economic incentive |
| 5. Block production timing (slot miss) | Denial of Service | Medium | Mitigated | Slot clock synced via NTP; tolerance ± 1 second |
| 6. Reward theft (pool operator skims fees) | Tampering | Medium | Mitigated | Reward distribution on-chain; auditable |
| 7. KES key rotation race (sign with stale key) | Tampering | Medium | Partial | Rotation happens at epoch boundary; grace period |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: VRF Key Theft (Critical)
- **Scenario**: Attacker steals VRF private key from disk or memory
- **Impact**: Attacker can forge VRF proofs; produce blocks at any time; steal all rewards
- **Status**: Partial — No protection against root access; HSM recommended
- **Mitigation**: Store on HSM if available; in-memory encryption; avoid disk storage

### Threat 2: KES Key Compromise (Critical)
- **Scenario**: Attacker compromises KES key for current epoch; forges block signatures
- **Impact**: Attacker can produce arbitrary blocks; consensus broken for this epoch
- **Status**: Partial — KES key rotation each epoch limits damage
- **Mitigation**: Old KES keys are destroyed; compromise affects only current epoch

### Threat 3: Double-Sign (High)
- **Scenario**: Operator (bug or attack) produces two different blocks at same slot
- **Impact**: Slashing punishment; loss of stake + rewards
- **Status**: Partial — Blockchain detects double-sign; slashing enforced
- **Mitigation**: Block production logic ensures single block per slot (no parallelization)

### Threat 4: Committee Collusion (High)
- **Scenario**: 2/3+ of committee members collude; decrypt encrypted mempool early
- **Impact**: MEV attack; unfair transaction ordering
- **Status**: Partial — Requires 2/3 honest assumption; economic incentive to be honest
- **Mitigation**: Slashing condition for unauthorized decryption (future)

### Threats 5–7: Covered briefly
- **Slot timing**: NTP sync; grace period for clock skew
- **Reward theft**: Distribution auditable on-chain
- **KES rotation**: Happens at epoch boundary with grace period

---

## Testing Strategy

- ✅ Key generation: VRF, KES, cold key roundtrip
- ✅ Key rotation: KES evolves correctly each epoch
- ✅ Slot loop: checks leadership, produces block if elected
- ✅ Committee sortition: deterministic membership check
- ✅ Block production: assembles mempool + signs with KES
- ✅ Committee decryption: reconstructs threshold shares

---

## Audit Checklist

- [ ] VRF key never written to disk unencrypted
- [ ] KES key rotation happens at epoch boundary (not mid-epoch)
- [ ] Old KES keys are zeroized (cannot be recovered)
- [ ] Block production happens only if leadership check passes
- [ ] Double-sign protection: only one block per slot
- [ ] Committee membership is deterministic (same result on all nodes)
- [ ] Reward distribution includes proper fee splits (no skimming)

---

## References

- `crates/qv-miner/src/keys.rs` — VRF, KES, cold key management
- `crates/qv-miner/src/slot_loop.rs` — Main event loop, leadership check
- `crates/qv-miner/src/block_producer.rs` — Block assembly + signing
- `crates/qv-miner/src/committee.rs` — Committee sortition + decryption
- `crates/qv-miner/src/config.rs` — Operator configuration
- [KES Forward Secrecy](https://www.researchgate.net/publication/270016197_Forward_Secure_Signatures_with_Untrusted_Update) — Key-evolving signature design
