# Threat Model: qv-wallet

**Module**: CLI wallet with key derivation, stealth scanning, transaction building  
**Public API**: `Mnemonic`, `WalletKeystore`, `scan_outputs()`, `build_transaction()`  
**Threat Count**: 8 (1 Critical, 2 High, 5 Medium)

---

## Assets & Trust Boundaries

### Assets
1. **Mnemonic seed phrase** — all keys derived from this
   - Confidentiality: CRITICAL (leak = all coins stolen)
2. **Private keys** — spending keys for transactions
   - Confidentiality: CRITICAL (leak = coins stolen)
3. **Output scanning** — identify received outputs
   - Confidentiality: HIGH (leak = balance exposed to attacker)
4. **Transaction signing** — cryptographic proof of authorization
   - Integrity: CRITICAL (weak signature = forgeable)

### Trust Boundaries
- **Input**: User input (mnemonic), RPC responses (chain state), local filesystem (keystore)
- **Processing**: Key derivation, output scanning, transaction building
- **Output**: Signed transactions (broadcast to network)
- **Attacker**: Keystore file access, RPC poisoning, local machine compromise

---

## STRIDE Threat Matrix

| Threat | STRIDE | Severity | Status | Mitigation |
|--------|--------|----------|--------|------------|
| 1. Mnemonic leaked in memory (plaintext) | Information Disclosure | Critical | Partial | Use `secrecy::Secret<T>` + zeroize; review required |
| 2. Keystore file unencrypted (local theft) | Information Disclosure | High | Mitigated | Keystore encrypted with user password — **Argon2id** (OWASP-2023: 64 MiB / 3 iter / 1 lane) + AES-256-GCM, fresh per-save salt+nonce (W-06, M-04 ile aynı pattern) |
| 3. RPC poisoning (false balance returned) | Tampering | High | Partial | Wallet should verify balance against multiple nodes |
| 4. Signature randomness weak (forge) | Spoofing | Medium | Partial | Use `OsRng`; audit RNG quality |
| 5. Output scanning leaks to observer (timing) | Information Disclosure | Medium | Partial | Scanning is local; no network timing leak |
| 6. HD derivation index collision (same key twice) | Spoofing | Medium | Mitigated | Derivation is deterministic; indexes are unique |
| 7. Transaction size limit bypass (invalid tx) | Tampering | Medium | Mitigated | Script VM enforces size; builder checks |
| 8. Coin selection algorithm bias (privacy leak) | Information Disclosure | Medium | Partial | Coin selection is not randomized; could leak intent |

---

## Detailed Threat Analysis (Abbreviated)

### Threat 1: Mnemonic in Plaintext (Critical)
- **Scenario**: Mnemonic phrase is kept in RAM as String; attacker reads memory dump
- **Impact**: All derived keys compromised; complete wallet theft
- **Status**: Partial — Code uses `secrecy::Secret<T>`; implementation review needed
- **Mitigation**: Mnemonic wrapped in Secret; zeroized on drop; no debug output

### Threat 2: Keystore File Unencrypted (High)
- **Scenario**: Keystore file stolen from disk; attacker reads keys
- **Impact**: All keys accessible without password
- **Status**: Mitigated — Keystore encrypted with scrypt KDF (cost params: N=2^16)
- **Mitigation**: File encryption + password protection; only plaintext in memory during use

### Threat 3: RPC Poisoning (High)
- **Scenario**: RPC returns false balance (attacker controls RPC); wallet sends coins
- **Impact**: Wallet believes it has coins it doesn't; transaction rejected
- **Status**: Partial — Wallet should verify balance via multiple RPC nodes
- **Mitigation**: Connect to trusted node; in future, verify block headers locally

### Threat 4: Signature Randomness (Medium)
- **Scenario**: RNG is weak; attacker predicts k value in signature
- **Impact**: Private key recovery; coins stolen
- **Status**: Partial — Uses `OsRng`; depends on OS RNG quality
- **Mitigation**: Audit RNG on deployment platform; use deterministic nonce in future (RFC 6979)

### Threats 5–8: Covered briefly
- **Scanning timing**: Local operation; no network leak
- **HD collision**: Deterministic derivation; indexes unique
- **Tx size bypass**: Script VM enforces limit
- **Coin selection bias**: Not randomized; analyst could infer intent

---

## Testing Strategy

- ✅ Mnemonic generation: deterministic, valid BIP39
- ✅ HD derivation: deterministic roundtrip
- ✅ Keystore encryption: encrypt → decrypt → plaintext matches
- ✅ Output scanning: known output found + unknown rejected
- ✅ Transaction building: inputs + outputs valid
- ✅ Signature verification: signed tx validates correctly

---

## Audit Checklist

- [ ] Mnemonic is wrapped in `secrecy::Secret<T>`
- [ ] No plaintext mnemonic in debug output
- [ ] Keystore encryption uses scrypt with proper cost params (N ≥ 2^16)
- [ ] Zeroization happens on Secret drop
- [ ] RNG uses OsRng (not thread_rng)
- [ ] HD derivation matches BIP32 standard
- [ ] Coin selection does not leak privacy (no specific ordering)
- [ ] Transaction size is checked before building

---

## References

- `crates/qv-wallet/src/mnemonic.rs` — Mnemonic generation, BIP39
- `crates/qv-wallet/src/hd.rs` — HD key derivation, BIP32
- `crates/qv-wallet/src/keystore.rs` — Encrypted key storage
- `crates/qv-wallet/src/scanner.rs` — Output scanning, stealth address recovery
- `crates/qv-wallet/src/tx_builder.rs` — Transaction building, signing
- [BIP39](https://github.com/trezor/python-mnemonic) — Mnemonic standard
- [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) — HD wallet standard
