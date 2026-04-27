# Threat Model: qv-privacy

**Module**: Stealth addresses, confidential amounts, view keys  
**Public API**: `StealthKeys`, `StealthAddress`, `scan_output()`, `create_stealth_output()`, `ConfidentialAmount`  
**Threat Count**: 9 (1 Critical, 4 High, 4 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Stealth address privacy** — recipient not linkable from blockchain
   - Confidentiality: CRITICAL (break = sender + receiver exposed)
2. **View key security** — auditor key cannot spend coins
   - Integrity: CRITICAL (leak = unauthorized spending)
3. **Confidential amounts** — transaction amounts hidden (opt-in)
   - Confidentiality: HIGH (leak = balance analysis)
4. **View-tag accuracy** — 99.6% false-positive filtering
   - Performance: MEDIUM (scanning speed degradation)

### Trust Boundaries
- **Input**: Blockchain outputs (untrusted) + private view key (secret)
- **Processing**: Scan output for ownership + recover spend key
- **Output**: Matched outputs (genuine) vs false positives (filtered by view-tag)
- **Attacker**: Chain observer, view-key compromise, Bulletproofs break

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. Stealth address brute-force (recover recipient key) | Information Disclosure | Critical | Partial | View-tag 5-bit filter (99.6% FP rejection) + KEM brute-force hard |
| 2. Confidential amount Bulletproof unsoundness | Information Disclosure | High | Deferred | Classical range proof; STARK migration planned |
| 3. View key compromise (auditor becomes spender) | Information Disclosure | High | Partial | View key is read-only; cannot derive spend key |
| 4. View-tag collision (false matches, rescan cost) | Denial of Service | High | Mitigated | 5-bit tag → 1/32 false positive rate; acceptable |
| 5. Ephemeral public key reuse (link transactions) | Information Disclosure | Medium | Mitigated | Ephemeral key is unique per output (Kyber encapsulation) |
| 6. Bulletproof range proof forgery | Tampering | Medium | Deferred | Classical Curve25519; not PQC but opt-in only |
| 7. Stealth address timing analysis (know you're scanned) | Information Disclosure | Medium | Partial | Scan is local; no timing leaked unless user broadcasts |
| 8. Privacy mode indicator (opt-in vs default) | Information Disclosure | Medium | Mitigated | Stealth is default; confidential amounts marked explicitly |
| 9. Confidential amount underflow (balance verification skip) | Tampering | Medium | Partial | Script VM must validate commitment balance |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: Stealth Address Brute-Force (Critical)
- **Scenario**: Attacker observes output with stealth_info; tries to decrypt to find recipient
- **Impact**: Recipient address recovered; sender identified (output analysis)
- **Status**: Partial — View-tag filters 99.6% of decryption attempts; rest require KEM break
- **Mitigation**: View-tag is 5 bits; only 1/32 ciphertexts are checked; KEM decapsulation is hard

### Threat 2: Bulletproof Soundness Break (High)
- **Scenario**: Attacker finds collision in Bulletproof range proof; creates fake amount
- **Impact**: Inflation; coins created from nothing
- **Status**: Deferred — Bulletproofs not PQC; opt-in feature; STARK migration planned
- **Mitigation**: Bulletproofs are classical (not post-quantum); users opt in knowingly

### Threat 3: View Key Compromise (High)
- **Scenario**: Attacker steals view key from user; can scan user's outputs
- **Impact**: Privacy lost; attacker knows all user's balances and outputs
- **Status**: Partial — View key alone cannot spend coins; spend key remains private
- **Mitigation**: View key is read-only; cannot derive spend key; attacker gains audit access only

### Threat 4: View-Tag Collision (High)
- **Scenario**: Attacker crafts outputs with colliding view-tag; forces user to check fake outputs
- **Impact**: Scanning performance degraded; DoS on wallet
- **Status**: Mitigated — 5-bit tag provides 1/32 false positive rate; acceptable cost
- **Mitigation**: View-tag is 5 bits; statistically independent; attacker cannot force collisions

### Threats 5–9: Covered briefly
- **Ephemeral reuse**: Unique per output (Kyber encapsulation)
- **Bulletproof forgery**: Classical crypto; opt-in only
- **Timing analysis**: Local scan; no network timing leak
- **Privacy mode indicator**: Stealth default; confidential marked
- **Confidential underflow**: Script VM validates commitments

---

## Testing Strategy

- ✅ Stealth key generation: deterministic roundtrip
- ✅ Scan output: view-tag filter rejects false positives
- ✅ Recovery: ephemeral → spend key recovery is deterministic
- ✅ Confidential amount: commitment + proof roundtrip
- ✅ View key audit: cannot spend, only scan
- [x] Fuzz: `stealth_scan.rs` — random outputs → scan (no panic, view-tag filter effective)

---

## Known Weaknesses

- Bulletproofs are classical (not PQC); long-term risk if Curve25519 breaks
- View-tag filter is probabilistic; false positives are unavoidable (1/32 rate)
- Timing analysis possible if user reveals scan activity
- No global anonymity (chain analysis possible via output analysis)

---

## Audit Checklist

- [ ] Stealth address scheme is IND-CPA secure (KEM + randomness)
- [ ] View key cannot be used to derive spend key (one-way function)
- [ ] View-tag computation is deterministic (same output → same tag)
- [ ] Bulletproof range proof is sound (if used)
- [ ] Confidential commitment is binding (cannot forge two balances)
- [ ] Ephemeral key is unique per output (no key reuse)
- [ ] No timing leak in scan operation (constant-time comparisons)

---

## References

- `crates/qv-privacy/src/stealth.rs` — Stealth addresses, scan, recovery
- `crates/qv-privacy/src/confidential.rs` — Confidential amounts, Bulletproofs
- `crates/qv-privacy/src/view_key.rs` — View key, audit, disclosure proofs
- [Stealth Addresses](https://en.wikipedia.org/wiki/Stealth_address) — Design rationale
- [Bulletproofs](https://eprint.iacr.org/2017/1066.pdf) — Range proof paper
