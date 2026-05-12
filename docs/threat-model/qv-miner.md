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
| 1. VRF key theft (impersonate leader) | Information Disclosure | Critical | Mitigated (Argon2id+AES-GCM keystore, M-04) / Partial (HSM optional) | Encrypted keystore single-file (master seed); HSM gelecek faz |
| 2. KES key compromise (sign arbitrary blocks) | Information Disclosure | Critical | Mitigated (Sum-KES forward security, depth=11) | Per-period leaf zeroize'lı; eski periyot kaybedilemez (ADR-005) |
| 3. Double-sign (produce competing blocks same slot) | Spoofing | High | Partial | Slashing evidence struct'ı var (`qv_mempool::SlashingEvidence`); on-chain enforcement N-03 finality vote akışıyla birlikte gelecek |
| 4. Committee member collusion (decrypt early) | Tampering | High | Partial | t-of-n honest gerekli; T-01 (Pedersen DKG/Feldman VSS asymmetry) açık — MP-01 wiring eksik |
| 5. Block production timing (slot miss) | Denial of Service | Medium | Mitigated | Slot clock synced via NTP; tolerance ± 1 second |
| 6. Reward theft (pool operator skims fees) | Tampering | Medium | Mitigated | Reward distribution on-chain `qv_consensus::rewards`; auditable; tests `distribute_*` |
| 7. KES key rotation race (sign with stale key) | Tampering | Medium | Mitigated | `kes_period` keystore'da; load'da `evolve` ile geriye gitmek imkansız (forward-only) |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: VRF Key Theft (Critical)
- **Scenario**: Attacker steals VRF private key from disk or memory
- **Impact**: Attacker can forge VRF proofs; produce blocks at any time; steal all rewards
- **Status**: Mitigated (on-disk) / Partial (in-memory). M-04 keystore Argon2id (64 MiB / 3 iter) + AES-256-GCM ile şifreli; master seed dosyadan plain çıkmaz. Root erişiminde belleği dump etme tehdidi devam ediyor.
- **Mitigation**: Encrypted keystore (M-04); HSM gelecek; production'da `SecureBytes` zeroize-on-drop in-memory; HSM optional in future.

### Threat 2: KES Key Compromise (Critical)
- **Scenario**: Attacker compromises KES key for current period; forges block signatures
- **Impact**: Attacker can produce blocks **only for the current KES period**; consensus broken for that period only
- **Status**: Mitigated — Sum-KES forward security: master seed'den 2048 leaf seed pre-derive, her `evolve` çağrısında o periyodun leaf seed'i `zeroize` edilir. Eski periyodun KES anahtarı matematiksel olarak yeniden üretilemez (ADR-005). `kes_period` keystore'da kalıcı; restart'ta `load_encrypted` geriye gidemez.
- **Mitigation**: Operator periodically rotates; on compromise alert, manual rotation via `qv-miner keys rotate-kes --target-epoch <future>`.

### Threat 3: Double-Sign (High)
- **Scenario**: Operator (bug or attack) produces two different blocks at same slot
- **Impact**: Slashing punishment; loss of stake + rewards
- **Status**: Partial — Slashing evidence struct'ları tanımlandı (`qv_mempool::SlashingEvidence`); on-chain enforcement vote/finality akışı (N-03) tamamlandığında devreye girer.
- **Mitigation**: Block producer kod yolu paralelizmsiz (`SlotTicker` tek thread); ek olarak slashing TX'i her node tarafından kabul edilebilir formatta yazıldı.

### Threat 4: Committee Collusion (High)
- **Scenario**: t-of-n committee members collude; decrypt encrypted mempool early
- **Impact**: MEV attack; unfair transaction ordering
- **Status**: Partial — `qv_crypto::threshold` Pedersen DKG + ElGamal-style threshold decrypt impl mevcut; ancak T-01 envanteri (Pedersen DKG / Feldman VSS verification asymmetry — 5 ignored test) açık. Encrypted mempool decrypt'ten block producer akışına wire (MP-01, K-06) henüz tam bağlı değil.
- **Mitigation**: Threshold encryption + commit-reveal pattern (planlı); economic incentive (stake slashing on bad decrypt share).

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
