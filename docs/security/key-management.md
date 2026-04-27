# QuantumVault L1 — Key Management Guidance

**Audience**: Validators, stake pool operators, exchange operators  
**Phase**: AŞAMA 14 — Security Hardening  
**Status**: Production-ready guidance

---

## Key Types & Lifecycle

### 1. VRF Key (Verifiable Random Function)

**Purpose**: Prove leader election eligibility each slot  
**Scope**: PoS validator only  
**Rotation**: Never (lifetime key for pool)  
**Compromise**: Attacker can forge any block at any time

**Storage**:
- **Offline (Recommended)**: Hardware Security Module (HSM) in vault
- **Warm**: Encrypted file on validator machine, accessed only during slot loop
- **Hot (Not Recommended)**: Plaintext in process memory (unacceptable)

**Backup**:
- Master key stored in vault + encrypted copy on secondary HSM
- Recovery procedure: Reconstruct from Shamir secret shares (3-of-5 shares required)

---

### 2. KES Key (Key-Evolving Signature)

**Purpose**: Sign blocks at current epoch  
**Scope**: Block producers (validators)  
**Rotation**: Every epoch (~12 hours)  
**Compromise**: Attacker can forge blocks this epoch only

**Storage**:
- **Warm (Required)**: Encrypted file on validator machine, evolves per epoch
- **Offline**: Previous KES keys archived + deleted (not encrypted; cannot be recovered)
- **Hot (Required)**: Decrypted copy in process memory during block signing

**Rotation Procedure**:
1. 1 hour before epoch boundary: Derive next KES keypair
2. At epoch boundary: Load new key into memory
3. Delete old key from memory (zeroize)
4. Archive (don't backup) previous key signature

**Key Versioning**:
```
Epoch 100: KES_100 = evolve(KES_99)
Epoch 101: KES_101 = evolve(KES_100)  ← current
Epoch 102: KES_102 = evolve(KES_101)  ← derived but not yet used
```

---

### 3. Cold Key (Pool Control)

**Purpose**: Register / update stake pool on-chain  
**Scope**: Pool operator only  
**Rotation**: Every 1–2 years (or on suspicion of compromise)  
**Compromise**: Attacker can update pool parameters (fee, addresses, margin)

**Storage**:
- **Offline (Required)**: Air-gapped machine or hardware wallet (e.g., Ledger, Trezor)
- **Warm**: Encrypted on secure USB, stored in safe deposit box
- **Hot (Never)**: Never load into validator process

**Usage**:
1. Operator disconnects from network
2. Loads cold key into offline signing device
3. Signs pool registration transaction
4. Broadcasts transaction from separate online machine
5. Stores transaction receipt

---

### 4. Wallet Keys (User Coins)

**Purpose**: Spend coins (sign transactions)  
**Scope**: End-users  
**Rotation**: Never (derivation-based; unlimited addresses)  
**Compromise**: Attacker steals all coins

**Storage**:
- **Offline (Recommended)**: Hardware wallet (Ledger Nano, Trezor, etc.)
- **Warm (Acceptable)**: Encrypted wallet file with strong password (scrypt + 256-bit AES)
- **Hot (Never)**: Unencrypted plaintext on machine or cloud

**Backup**:
- Mnemonic phrase (12–24 words) stored in safe + password-protected encrypted copy
- **Never** store plaintext mnemonic on computer or cloud
- Test recovery: Restore from backup mnemonic to verify correctness

---

## Cold/Hot Split Architecture

### Validator Cold Split (Recommended)

```
┌─────────────────────────────────────────────────┐
│  OFFLINE VAULT (Air-Gapped)                      │
├─────────────────────────────────────────────────┤
│  - Master VRF key (encrypted, 3-of-5 shares)    │
│  - Cold pool registration key                    │
│  - Emergency recovery key                        │
│                                                  │
│  Security: Faraday cage, no network access      │
│  Access: Quorum signature (3+ operators)         │
└─────────────────────────────────────────────────┘
                       │
          ┌────────────┴────────────┐
          │                         │
  ┌───────▼────────┐      ┌────────▼──────────┐
  │ HSM in Vault   │      │ Encrypted USB     │
  │ (VRF backup)   │      │ (Cold backup)     │
  │                │      │                   │
  │ - Biometric    │      │ - Stored in safe  │
  │ - Quorum req.  │      │ - 1 per operator  │
  └─────────────────┘      └─────────────────┘
                       │
  ┌─────────────────────▼─────────────────────┐
  │  ONLINE VALIDATOR NODE                      │
  ├─────────────────────────────────────────────┤
  │  - Hot KES key (encrypted file)             │
  │  - Rotating KES in memory                    │
  │  - Clear mempool (public)                    │
  │  - Encrypted mempool (2/3 shares needed)     │
  │                                              │
  │  Security: Firewall, rate limiting, HSM     │
  │  Access: Only during block production        │
  └─────────────────────────────────────────────┘
```

---

## Key Rotation Schedule

### VRF Key
- **Frequency**: Never (lifetime)
- **Action**: Only if compromise suspected
- **Procedure**: 
  1. Generate new VRF key
  2. Offline: Sign pool update TX with cold key
  3. Broadcast: Update pool on-chain with new VRF pubkey
  4. Validators: Accept blocks from new key only

### KES Key
- **Frequency**: Every epoch (12 hours)
- **Action**: Automatic by operator software
- **Procedure**:
  1. `qv-miner`: Derives next KES key at epoch boundary
  2. Load into memory for signing
  3. Delete previous key (irreversible)

### Cold Key
- **Frequency**: Every 2 years (or after compromise)
- **Action**: Manual ceremony (requires quorum)
- **Procedure**:
  1. Operators meet in person (or secure 3-party call)
  2. Load master key from Shamir shares
  3. Generate new cold key
  4. Sign pool update + operator key rotation TX
  5. Reseal in new Shamir threshold scheme

---

## Hardware Security Module (HSM) Integration

### Recommended Setup

**Device**: Thales HSM Luna Network 7 (or equivalent)
- FIPS 140-2 Level 3 certified
- Supports RSA, ECC, AES encryption
- Network-accessible via PKCS#11

**Configuration**:
```yaml
# qv-miner/config.toml
[hsm]
enabled = true
device = "Luna Network 7"
slot = 1
pin = "123456"  # Read from env var (never hardcode)

[keys]
vrf_key_id = "QuantumVault-VRF-Master"
kes_key_id = "QuantumVault-KES-Epoch{N}"
```

**PKCS#11 Integration**:
```rust
// qv-miner/src/keys.rs
use pkcs11::types::*;

fn sign_with_hsm(message: &[u8]) -> Result<PqcSignature> {
    let hsm = HSMContext::new()?;
    let session = hsm.open_session()?;
    
    // Verify message hash first (fail-safe)
    let hash = sha3_256(message);
    
    // HSM never reveals private key
    let signature = session.sign_with_key("QuantumVault-VRF", &hash)?;
    
    Ok(PqcSignature::from_bytes(&signature)?)
}
```

### Operational Procedures

**Initialization** (First-time setup):
```bash
# 1. Set HSM admin password
lunash:> lunash> open session
lunash:> changePIN -newPIN <password>

# 2. Initialize partition for QuantumVault keys
lunash:> createPartition -partition QuantumVault -password <password>

# 3. Import or generate master VRF key
lunash:> importPrivateKey -file vrf_master.pem -partition QuantumVault
```

**Key Rotation**:
```bash
# Before epoch boundary: Derive next KES key
lunash:> generateECCKeyPair -partition QuantumVault \
  -label "QuantumVault-KES-Epoch${NEXT_EPOCH}" \
  -tokenObjects

# Validator software reads new key ID, signs blocks
# Old key is deleted (irreversible in HSM)
```

**Emergency Access**:
```bash
# Recover from Shamir shares (3-of-5 required)
# Each operator inputs their share
lunash:> recoverPartition -shares <share1> <share2> <share3>
```

---

## Shamir Secret Sharing (Recovery)

### Master Key Splitting

**Scheme**: (3, 5) threshold — requires 3 of 5 shares to recover

```
Master VRF Key
       │
       ├─────────────────────────────────┬─────────┬─────────┐
       │                                 │         │         │
    Share 1 → Operator A (Vault)      Share 2 → Operator B  Share 3 → Operator C
    Share 4 → Operator D              Share 5 → Lawyer (3rd party)
```

**Distribution**:
- Share 1: Operator A keeps in personal vault
- Share 2: Operator B keeps in personal vault
- Share 3: Operator C keeps in personal vault
- Share 4: Operator D keeps (for quorum if A or B unavailable)
- Share 5: Lawyer retains (neutral 3rd party, emergency only)

**Recovery Procedure**:
1. Three operators physically meet (or call with video verification)
2. Each unseals their share
3. Use `ssss-split` tool to reconstruct master key
4. Load into HSM via PKCS#11
5. Re-seal in new (2, 3) threshold scheme if key was compromised

---

## Secure Backup Strategy

### Backup Matrix

| Key | Offline | Warm | Encryption | Redundancy |
|-----|---------|------|------------|-----------|
| VRF | ✅ HSM 2x | ❌ Never | AES-256 | 3-of-5 Shamir |
| KES | ❌ Delete old | ✅ File | AES-256 | None (ephemeral) |
| Cold | ✅ USB 3x | ❌ Never | AES-256 | 1 copy per operator |
| Wallet | ✅ Hardware | ✅ Encrypted file | AES-256 | Mnemonic + USB |

### Backup Storage

**Tier 1 (Primary)**: Operator personal vault
- Safe deposit box, home safe, or physical vault
- Temperature/humidity controlled
- Biometric + combination lock

**Tier 2 (Secondary)**: Bank vault
- Off-site, geographically diverse
- Annual backup rotation
- Accessible only by authorized officers

**Tier 3 (Emergency)**: Lawyer/notary
- Neutral 3rd party holds (5th Shamir share)
- Access only with 2+ operators present
- Sealed and notarized

---

## Access Control & Audit Logging

### HSM Access Logging

```bash
# All HSM operations are logged
lunash:> setLogLevel -level DEBUG

# Review access logs
lunash:> getLog -type "user_authentication" -since "2026-04-01"

# Example log entry:
# [2026-04-27 14:23:45] User=operator_alice Operation=SignMessage Status=Success
```

### Cold Key Access

```bash
# Ceremony log (physical)
Date: 2026-04-27
Time: 10:00–10:30 UTC
Participants: Alice (Operator), Bob (Operator), Charlie (Lawyer)
Purpose: Emergency key rotation (suspected compromise)
Action: Generated new cold key; signed pool update
Witnesses: [signatures]
```

### Monitor Access Patterns

```bash
# Alert if VRF key is accessed outside of slot loop
# Alert if Cold key is used more than quarterly
# Alert if more than 2 shares are accessed in same month
```

---

## Incident Response: Key Compromise

### If VRF Key is Compromised

1. **Stop block production** immediately (stop validator)
2. **Announce compromise** to network (governance)
3. **Update pool key** (requires 3+ operator quorum):
   - Retrieve new VRF key from HSM backup
   - Sign pool update TX with cold key
   - Broadcast to network
4. **Slashing**: Attacker may have already produced unauthorized blocks (double-sign detected)

### If KES Key is Compromised

1. **Current epoch blocks are at risk** (KES signs blocks)
2. **Wait for epoch boundary** (12 hours max)
3. **New KES key is auto-derived** and old key destroyed
4. **Investigate**: How was KES compromised? (memory dump? SSH breach?)
5. **No slashing**: KES cannot be used post-rotation (one-way evolution)

### If Cold Key is Compromised

1. **Attacker can only update pool parameters** (not sign blocks)
2. **Retrieve cold key backup** from vault
3. **Generate new cold key** (Shamir ceremony)
4. **Sign pool update** rotating out old key
5. **Check if attacker updated pool**: Review on-chain history of pool updates

---

## Testing & Validation

### Annual Key Rotation Exercise

```bash
# 1. Test recovery from Shamir shares (without live keys)
./scripts/test_shamir_recovery.sh --dry-run

# 2. Test KES rotation (on testnet)
./scripts/test_kes_rotation.sh --epoch 100

# 3. Verify backup integrity
./scripts/verify_backups.sh --all

# 4. Audit access logs
./scripts/audit_logs.sh --month=202604
```

### Disaster Recovery Simulation

- **Scenario**: Validator machine is destroyed (fire, theft)
- **Recovery goal**: Resume block production within 4 hours
- **Test**: Spin up new machine, restore from backup, verify keys load correctly

---

## Operator Onboarding Checklist

- [ ] Read this guide (Key Management)
- [ ] Read Incident Response runbook (runbook-incident.md)
- [ ] Set up HSM (or cold storage device)
- [ ] Generate initial keys (VRF, KES, cold)
- [ ] Create Shamir shares (3-of-5)
- [ ] Distribute shares to operators
- [ ] Test recovery procedure (dry-run)
- [ ] Set up monitoring + alerting
- [ ] Review access logs weekly
- [ ] Participate in annual rotation exercise

---

## References

- [Incident Response Runbook](./runbook-incident.md) — Key compromise procedures
- [SECURITY.md](../../SECURITY.md) — Disclosure policy
- [Thales HSM Docs](https://thalesdocs.com/) — HSM configuration
- [SSSS (Shamir's Secret Sharing Scheme)](http://point-at-infinity.org/ssss/) — Tool reference
- [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki) — Hierarchical deterministic wallets

---

**Last Updated**: 2026-04-27  
**Maintained By**: Security + Key Management Team  
**Review Cycle**: Annually (or after incident)
