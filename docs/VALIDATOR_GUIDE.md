# QuantumVault L1 — Validator & Stake Pool Operator Guide

**Audience**: Stake pool operators, node validators  
**Target Deployment**: Devnet / iç-testnet (mainnet HENÜZ YOK — mainnet'e özgü bölümler "gelecek" işaretli)  
**Status**: Devnet/iç-testnet rehberi — proje mainnet-ready DEĞİL; açık işler `docs/ROADMAP.md` envanterinde  
**Version**: 2.1 (Updated 2026-06-10)

> **Gerçeklik notu (2026-06-10).** Bu rehberin bir kısmı hedef-mimariyi anlatır;
> `qv-miner`'ın bugünkü fiili durumu şudur:
>
> - **Çalışan:** Operatör keystore'u gerçek — tek dosya, Argon2id + AES-256-GCM
>   (`OperatorConfig.keystore_path`, envanter M-04). `qv-miner run` keystore'u
>   yükler (parola `QV_KEYSTORE_PASS` env'i veya interaktif prompt), node'dan
>   RPC ile gerçek stake distribution + epoch nonce çeker (M-09b, kapatıldı
>   2026-05-15), `SlotLoop`'u çalıştırır ve lider seçildiğinde bloğu üretip
>   `qv_submitBlock` ile node'a gönderir (M-09c, kapatıldı 2026-05-15).
>   Bu akış yalnızca devnet ölçeğinde denendi.
> - **Açık:** `qv-miner init` şu an yalnızca `operator.toml` yazar — keystore
>   dosyasını CLI'dan üretmez. `register-pool` zinciri placeholder'dır
>   (M-06/M-07/M-08: boş locking script, dummy input, `submit_via_rpc`
>   `"txid_placeholder"` döner). Dashboard/metrics stub'dır (M-10/M-11;
>   `--metrics-addr` flag'i henüz bağlı değil). Encrypted mempool threshold
>   komitesi (T-01 Pedersen DKG) uygulanmadı. On-chain delegation ve ödül
>   dağıtımı protokol seviyesinde henüz zorlanmıyor.

---

## 1. Overview

A validator on QuantumVault L1 is a network participant who:
- **Runs a full node** (downloads and verifies all blocks)
- **Operates a stake pool** (receives delegated stake from users — on-chain delegation *gelecek*)
- **Produces blocks** (elected by VRF lottery proportional to stake)
- **Participates in committee** (decrypts threshold-encrypted mempool as 2/3+ required — *gelecek*, T-01 Pedersen DKG açık)
- **Earns rewards** (block rewards + transaction fees, shared with delegators — protokol dağıtımı *gelecek*)

### Why Run a Validator?

1. **Revenue**: Earn block rewards + transaction fees, split with delegators
2. **Network Security**: Participate in Ouroboros Praos consensus
3. **Decentralization**: Reduce reliance on large pools
4. **DeFi Integration**: Direct interaction with encrypted mempool protocol

---

## 2. Hardware Requirements

QuantumVault uses post-quantum cryptography (Dilithium + Kyber) which impose higher computational overhead than classical ECDSA. These estimates assume:
- Linux/UNIX OS (Ubuntu 22.04 LTS recommended)
- No containerization overhead
- Full validator (not light client)

### Minimum (Small Pool, <1M stake)

| Component | Specification |
|-----------|---------------|
| CPU | 4 cores, 2.4 GHz (x86-64) |
| RAM | 8 GB DDR4 |
| Storage | 500 GB NVMe SSD (RAID-1 backup) |
| Network | 50 Mbps symmetric |
| UPS | 10 kWh backup (for graceful shutdown) |

**Caveats**: May miss slots under high network congestion; not recommended for mainnet.

### Recommended (Active Pool, 1M–10M stake)

| Component | Specification |
|-----------|---------------|
| CPU | 8 cores, 3.2+ GHz (Intel/AMD Xeon) |
| RAM | 32 GB DDR4 |
| Storage | 2 TB NVMe SSD (RAID-1 or RAID-10) |
| Network | 200 Mbps symmetric (fiber recommended) |
| UPS | 50 kWh battery bank |

**Justification**: 12-hour epoch = 21,600 slots; PQC signature verification (~10ms per block); encrypted mempool decryption (2/3 threshold overhead).

### Optimized (Large Pool, >10M stake)

| Component | Specification |
|-----------|---------------|
| CPU | 16 cores, 3.4+ GHz (Dual socket) |
| RAM | 64 GB DDR4 ECC |
| Storage | 4–8 TB NVMe RAID-10 + HSM (SSD for KES keys) |
| Network | 1 Gbps dedicated uplink (CDN recommended) |
| UPS | 500 kWh + diesel generator |
| Compliance | Hardware Security Module (Thales Luna HSM) |

**Rationale**: Lower miss-slot probability; faster encrypted mempool decryption; HSM-backed key storage.

### Network Topology

Validator should be placed behind:
- **Firewall**: Restrict inbound to libp2p port only (default: 30333)
- **Load Balancer**: If running multiple validator nodes
- **IPFS/libp2p Proxy**: For validator anonymization (optional)

---

## 3. Installation

### 3.1 Prerequisites

```bash
# Install system dependencies (Ubuntu 22.04)
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  pkg-config \
  libssl-dev \
  curl \
  git \
  ntp \
  htop

# Install Nix (single-user, no systemd required)
curl -L https://nixos.org/nix/install | bash
source ~/.bashrc  # Reload shell
```

### 3.2 Clone & Build

```bash
# Clone QuantumVault repository
git clone https://github.com/quantumvault/l1.git
cd l1

# Enter reproducible dev environment
nix develop

# Build qv-miner binary
just build

# Verify binary exists
ls -lh target/release/qv-miner
```

**Build time**: ~5–10 minutes (depending on hardware)

### 3.3 Binary Locations

After build, key binaries are:
- **`target/release/qv-miner`** — Stake pool operator daemon (validator)
- **`target/release/qv-node`** — Full node (dependency)

For production, symlink to standard location:
```bash
sudo mkdir -p /opt/quantumvault
sudo cp target/release/qv-miner /opt/quantumvault/qv-miner
sudo chmod 755 /opt/quantumvault/qv-miner
```

---

## 4. Key Management

### 4.1 Key Types & Rotation

QuantumVault uses four asymmetric keypairs:

| Key Type | Purpose | Lifetime | Storage | Rotation |
|----------|---------|----------|---------|----------|
| **VRF** | Prove slot leadership eligibility | Lifetime (pool) | HSM or offline vault | Never (unless compromised) |
| **KES** | Sign blocks in current epoch | 1 epoch (12 hrs) | Encrypted file + memory | Automatic, every epoch |
| **Cold** | Register/update pool on-chain | 1–2 years | Air-gapped device (USB/HSM) | Manual ceremony, quarterly |
| **Wallet** | User funds (delegator transfer) | Lifetime | Hardware wallet recommended | Never (BIP32-HD derived) |

### 4.2 Initial Key Generation

Operatör anahtarları **tek bir 32-byte master seed**'den deterministik olarak
türetilir (`OperatorKeys::from_seed`); master seed Argon2id+AES-256-GCM ile
**tek bir keystore dosyasında** şifrelenir (envanter M-04, 2026-05-12).

Fiili CLI (2026-06-10):

```bash
# Operatör config'ini üret (operator.toml). Pool adı + ekonomik parametreler.
qv-miner init \
  --pool-name "MyValidator" \
  --pledge 1000000000 \
  --margin-bps 300 \
  --fixed-cost 10000000 \
  --output operator.toml
```

> **Sınır (açık iş):** `init` şu an yalnızca `operator.toml` yazar;
> `keystore_path` alanına işaret edilen keystore **dosyasını üretmez**.
> Keystore, `OperatorKeys::save_encrypted(path, password)` API'siyle
> oluşturulur (5 unit test + roundtrip testi mevcut); bunun CLI'a
> bağlanması (`init --keystore --password-file` benzeri bir akış) gelecek
> iş. Keystore formatı: JSON envelope `{ version: 1, master_seed,
> kes_period }`, Argon2id + AES-256-GCM ile şifreli, tek dosya.

Anahtar türetme şeması:
```
vrf_seed  = SHA3-256(master_seed || "vrf")
kes_seed  = SHA3-256(master_seed || "kes")
cold_seed = SHA3-256(master_seed || "cold")
```
Üç child seed bağımsız primitiflere besleniyor (Ristretto255-VRF, Sum-KES,
ML-DSA-65). Master seed compromise olursa üçü de compromise olur; bu yüzden
keystore parolası güçlü olmalı (Argon2id-64MiB-3-1) ve yedek master seed
**yalnızca air-gapped** ortamda saklanmalıdır.

### 4.3 Key Rotation Schedule

**VRF**: Never (lifetime of pool)

**KES**: Automatic every epoch boundary (every 12 hours)
```bash
# Operator software handles this transparently:
# - At epoch boundary (UTC 00:00, 12:00, 24:00, etc.)
# - Derives next KES key from current
# - Loads into memory (old key zeroed & deleted)
# - Blocks signed with new key for remainder of epoch
```

> **Fiili durum:** `kes_evolve` primitifi gerçek (leaf zeroize + period
> advance) ve keystore load sırasında kayıtlı `kes_period`'a kadar evolve
> edilir. Ancak çalışan daemon'ın epoch sınırında otomatik evolve edip
> yeni period'u keystore'a **geri yazması** henüz bağlanmadı (`cmd_run`
> içinde "future work" olarak işaretli). Uzun süreli çalıştırmada bunu
> göz önünde bulundurun.

**Cold**: Quarterly manual ceremony (requires 3+ operators)
```bash
# 1. Operators meet physically or on secure conference call
# 2. Load vault with Shamir shares (3-of-5 threshold)
# 3. Generate new cold key
# 4. Sign pool update transaction rotating keys
# 5. Broadcast from online machine
# 6. Seal new key in vault

./scripts/cold_key_ceremony.sh --participants 3
```

### 4.4 Cold Key Security (Critical)

**NEVER** put cold key on validator machine or any online computer:

```bash
# Correct: Offline signing
# Machine A (Offline, air-gapped):
qv-wallet sign-transaction \
  --input /mnt/usb/pool_update.tx \
  --key /mnt/vault/cold_key.pem \
  --output /mnt/usb/pool_update.signed

# Machine B (Online, connected to network):
qv-wallet broadcast /mnt/usb/pool_update.signed

# Incorrect (DO NOT DO THIS):
qv-miner --cold-key-file /path/to/cold_key.pem run  # DANGER!
```

---

## 5. Pool Registration

### 5.1 Registration Transaction

Fiili CLI (2026-06-10) — pool parametreleri `operator.toml`'dan okunur:

```bash
# Register stake pool on-chain
qv-miner register-pool \
  --config operator.toml \
  --node-rpc http://localhost:8080 \
  --wait-blocks 10
```

> **Açık iş (M-06/M-07/M-08):** Registration zinciri henüz placeholder.
> Locking script boş (`Script::standard_registration_lock()` TODO), UTXO
> seçimi/change/cold-key imzalama yok (tek dummy input) ve `submit_via_rpc`
> gerçek RPC çağırmayıp `"txid_placeholder"` döner. Devnet'te pool'lar
> bunun yerine node'un genesis stake-pool config'i ile pre-seed edilir;
> `qv-miner run` pool'u node'un stake distribution'ında bulamazsa net hata
> ile çıkar. Gerçek on-chain registration *gelecek* (Faz 1/6).

### 5.2 Pool Parameters

| Parameter | Min | Max | Meaning |
|-----------|-----|-----|---------|
| Pledge | 0 | Pool stake | Amount operator commits (builds trust) |
| Margin | 0% | 100% | Operator fee % of block rewards |
| Cost | 0 | ∞ | Fixed satoshis per epoch before margin |
| Relays | 0 | ∞ | How many stake pools can delegate through |

**Best Practices**:
- High pledge (>1M stake) = higher delegator trust
- Margin 2–5% = competitive
- Cost 200–500 satoshis = covers infrastructure

### 5.3 Delegators (gelecek)

Once registered, delegators can stake to your pool (*hedef tasarım —
`qv-wallet delegate` komutu henüz yok*):

```bash
# Delegator action (not operator) — GELECEK:
qv-wallet delegate \
  --pool-id <pool_id> \
  --amount 50000 \
  --password <wallet_pass>

# Results in UTXO locked to pool for that epoch
# Rewards flow to delegator address each epoch
```

**Operator does NOT touch delegator keys** — rewards are automatic via
protocol (*tasarım hedefi; on-chain delegation + ödül dağıtımı henüz
uygulanmadı, sadece `qv-consensus::rewards` matematiği ve testleri var*).

---

## 6. Running the Node

### 6.1 Configuration

Fiili operatör config'i `operator.toml`'dur (`OperatorConfig`, `qv-miner
init` üretir). Gerçek alanlar (2026-06-10):

```toml
pool_id = "pool_a1b2c3d4"
pool_name = "MyValidator"

# Single encrypted master keystore (envanter M-04).
# Argon2id (64 MiB / 3 iter / 1 lane) + AES-256-GCM. Contains:
#   - 32-byte master seed (vrf/kes/cold deterministic derivation)
#   - kes_period: u32 (forward-secure rotation pointer)
keystore_path = "keys/operator.keystore"

pledge = 1000000000
margin_bps = 300        # 3%
fixed_cost = 10000000
reward_account = "qvaddr_reward"
network = "testnet"     # devnet | testnet | mainnet (mainnet GELECEK)
node_rpc_url = "http://localhost:8080"

# Opsiyonel alanlar:
# node_gossip_addr = "/ip4/127.0.0.1/tcp/30333"
# clear_mempool_capacity = 10000
# encrypted_mempool_capacity = 5000
# decryption_committee_share_path = "..."   # T-01 sonrası anlamlı (gelecek)
# genesis_time = 1780000000
# kes_rotation_period_epochs = 1
```

Node tarafı (`qv-node`) ayrı bir config kullanır; bu rehberin önceki
sürümündeki `[node]/[consensus]/[storage]/[mempool]/[monitoring]/[hsm]`
blokları **hedef-mimari** idi — mainnet operasyon config'i olarak
*gelecek* işaretlidir.

### 6.2 Start Validator

```bash
# Foreground (development/testing) — parola QV_KEYSTORE_PASS env'inden
# (devnet kolaylığı) ya da interaktif prompt'tan okunur.
qv-miner run --config operator.toml --node-rpc http://localhost:8080

# Background (systemd service — mainnet/production akışı GELECEK)
sudo systemctl start quantumvault-miner
sudo systemctl enable quantumvault-miner

# Check status
sudo journalctl -u quantumvault-miner -f

# Stop gracefully (daemon Ctrl+C / SIGINT ile graceful kapanır)
sudo systemctl stop quantumvault-miner
```

### 6.3 Systemd Unit File (gelecek — mainnet/production)

Create `/etc/systemd/system/quantumvault-miner.service`:

```ini
[Unit]
Description=QuantumVault L1 Stake Pool Operator
After=network-online.target

[Service]
Type=simple
User=quantumvault
ExecStart=/opt/quantumvault/qv-miner run --config /etc/quantumvault/operator.toml --node-rpc http://localhost:8080
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/var/quantumvault /var/log/quantumvault

# Resource limits
LimitNOFILE=65535
LimitNPROC=16384

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable quantumvault-miner
sudo systemctl start quantumvault-miner
```

---

## 7. Delegation (gelecek — hedef tasarım)

> On-chain delegation henüz uygulanmadı. `qv-miner delegate` bugün yalnızca
> bilgilendirme metni basan bir yardımcıdır; `qv-wallet delegate` komutu yok.
> Aşağıdaki akış hedef tasarımdır.

### 7.1 How Delegators Stake

Delegators **do not** interact with the operator; they stake directly on-chain:

```bash
# Delegator's wallet (GELECEK):
qv-wallet delegate \
  --pool <pool_id> \
  --amount <satoshis> \
  --key <delegator_key>
```

This creates a UTXO locked to the pool for the next epoch. The operator's validator software automatically:
1. Detects UTXO
2. Aggregates delegator stake
3. Uses total stake for VRF leadership check
4. Distributes rewards (block reward − operator margin − fixed cost) back to delegators

### 7.2 Reward Distribution

**Example: Pool with 5M stake**

```
Block reward: 50,000 satoshis
Operator stake: 1,000,000 (20%)
Delegator stake: 4,000,000 (80%)
Margin: 5%
Cost: 300 satoshis/epoch

Distribution:
  Total reward: 50,000
  - Margin (5%): 2,500 → Operator
  - Cost (fixed): 300 → Operator
  - Available to share: 47,200

  To Operator: 1,000,000 / 5,000,000 × 47,200 = 9,440
  To Delegators: 4,000,000 / 5,000,000 × 47,200 = 37,760

  Operator total: 2,500 + 300 + 9,440 = 12,240
  Delegators total: 37,760 (pro-rata split)
```

**Operator cannot skim**: Rewards computed on-chain; protocol enforces distribution.

---

## 8. Block Production

### 8.1 Leadership Check (VRF)

Once per slot (every 2 seconds), the slot loop:

```rust
fn check_leadership(vrf_key: &VRFKey, pool_stake: u64, total_stake: u64) -> bool {
    slot_nonce = hash(prev_block_hash || current_slot)
    vrf_proof = sign_vrf(vrf_key, slot_nonce)
    
    // Probability = pool_stake / total_stake
    let threshold = (pool_stake as f64 / total_stake as f64) * 2^256
    let vrf_output = interpret_vrf(vrf_proof) as u256
    
    vrf_output < threshold  // Is leader for this slot?
}
```

> Yukarıdaki pseudo-code kavramsaldır; fiili implementasyon **float
> kullanmaz** — lider eşiği ADR-009 uyarınca deterministik sabit-nokta
> aritmetiğiyle hesaplanır (`T = 1 − (1−f)^σ`).

If elected (2-4. adımlar bugün `qv-miner::cmd_run` block producer'ında
gerçek; encrypted-mempool komite adımı *gelecek*, T-01):

```rust
fn produce_block(kes_key: &KESKey, vrf_proof: &VRFProof) -> Block {
    // 1. Gather transactions from mempool (encrypted + clear)
    txs = mempool.pull(max_block_size)
    
    // 2. If committee member: decrypt encrypted mempool
    if is_committee_member() {
        my_threshold_share = get_my_share()
        combined_txs = decrypt_with_threshold(my_threshold_share)
        txs.extend(combined_txs)
    }
    
    // 3. Assemble block
    merkle_root = merkle_tree(txs)
    body = BlockBody {
        slot: current_slot,
        transactions: txs,
        merkle_root,
        prev_block_hash,
    }
    
    // 4. Sign with KES (current epoch key)
    signature = kes_key.sign(&body)
    
    block = Block { header: ..., body, signature }
    
    // 5. Broadcast
    broadcast_to_peers(block)
}
```

### 8.2 Slot Miss

If not elected, validator skips the slot. Consecutive misses = lower throughput, but **no slashing**.

---

## 9. KES Key Rotation

### 9.1 Automatic Rotation

At each epoch boundary (12 hours), the validator automatically:

```rust
fn rotate_kes_key_at_epoch_boundary() {
    if current_slot % 21600 == 0 {  // Epoch boundary
        // 1. Derive next KES key (deterministic)
        new_kes = kes_key.evolve()
        
        // 2. Load into memory
        MEMORY.set("current_kes", new_kes)
        
        // 3. Zero old key (irreversible)
        old_kes.zeroize()
        
        log!("KES rotated at epoch {}", current_epoch)
    }
}
```

**Key design**: KES (Key-Evolving Signature) keys cannot be used retroactively. Each key signs only its epoch; previous keys are destroyed, not recoverable.

### 9.2 If Rotation Fails

Scenario: Validator crashes before epoch boundary, KES not rotated.

**Recovery**:
1. Restart validator
2. Software detects stale KES (old epoch)
3. Derives correct KES for current epoch
4. Resumes block production
5. **No slashing** — but blocks from previous epoch won't be accepted (already finalized)

### 9.3 Manual Rotation (Emergency) — gelecek

> `qv-miner keys rotate-kes` alt komutu henüz CLI'da yok (mevcut tek key
> alt komutu `keys-show`). Aşağıdaki akış hedef tasarımdır; bugün manuel
> evolve `OperatorKeys` API'si üzerinden yapılabilir.

```bash
# Only if automatic rotation failed or suspected compromise.
# Reads the keystore, evolves KES forward to target epoch, re-encrypts
# with the same password, writes back to the single keystore file.
qv-miner keys rotate-kes \
  --keystore /etc/quantumvault/secrets/operator.keystore \
  --target-epoch <epoch>
```

---

## 10. Monitoring & Alerting (gelecek — hedef tasarım)

> `qv-miner run`'daki `--metrics-addr` flag'i parse edilir ama henüz bir
> exporter'a bağlanmaz; `qv-miner dashboard` da stub'dır (M-10/M-11).
> Aşağıdaki metrik adları ve Grafana/Alertmanager kurulumları hedef
> tasarımdır. (Not: `qv-node` tarafında Prometheus exporter mevcuttur.)

### 10.1 Metrics Endpoint

Validator exposes Prometheus metrics on `:9090/metrics`:

```bash
curl http://localhost:9090/metrics | grep qv_

# Key metrics:
# qv_blocks_produced{pool_id="..."} = 42
# qv_blocks_missed{pool_id="..."} = 2
# qv_slot_miss_rate = 0.045  (4.5%)
# qv_mempool_size = 850
# qv_committee_decryption_time_ms{percentile="p99"} = 120
# qv_network_peers = 48
# qv_sync_height = 123456
```

### 10.2 Grafana Dashboard

Import dashboard template:
```bash
curl https://raw.githubusercontent.com/quantumvault/monitoring/main/grafana-validator.json | \
  curl -X POST http://localhost:3000/api/dashboards/db \
    -H "Content-Type: application/json" \
    -d @-
```

**Dashboard panels**:
- Block production rate (blocks/epoch)
- Slot miss rate
- Peers connected
- Mempool size
- Committee decryption latency
- VRF key access (audit log)
- Network latency (to 5 largest peers)
- KES key age (warns before rotation)
- Sync height (diff from network)

### 10.3 Critical Alerts

Set up alerts for:

| Alert | Condition | Action |
|-------|-----------|--------|
| **High slot miss rate** | >10% misses in epoch | Review CPU/network; check clock sync |
| **Peers dropping** | <5 connected | Restart node; check firewall |
| **Mempool stuck** | >1000 txs for 10 min | Investigate memory leak; restart |
| **Committee decryption slow** | p99 > 500ms | Reduce block size; check CPU |
| **KES key age** | Within 1 hour of rotation | Monitor KES rotation completion |
| **Sync lagging** | Height diff > 10 blocks | Network issue; check connectivity |
| **VRF key accessed outside slot loop** | HSM alert | Potential compromise; investigate |

Setup example (Alertmanager):
```yaml
groups:
  - name: quantumvault_validator
    rules:
      - alert: HighSlotMissRate
        expr: qv_slot_miss_rate > 0.1
        for: 30m
        annotations:
          summary: "Validator {{ $labels.pool_id }} missing >10% slots"
          action: "Review hardware; check network/clock"
```

---

## 11. Troubleshooting

### 11.1 Missing Slots

**Symptom**: Frequent gaps in `qv_blocks_produced` metric.

**Causes**:
1. **Clock drift**: System time skewed >1 second
   - Fix: `sudo ntpdate -s pool.ntp.org`
2. **Low CPU**: PQC signature verification bottleneck
   - Fix: Upgrade CPU; monitor `top` during slots
3. **Network latency**: Block broadcast takes >1 second
   - Fix: Check peer latency; move closer to datacenter
4. **Mempool congestion**: Assembling block takes too long
   - Fix: Reduce `max_block_size` in config

### 11.2 Sync Issues

**Symptom**: `qv_sync_height` stuck; not advancing.

**Causes**:
1. **No peers**: Cannot download blocks
   - Fix: Check firewall rules; ensure port 30333 open
   - `qv-miner dashboard` → peers section shows connections (*gelecek* — dashboard şu an stub, M-10/M-11)
2. **Bad blocks in chain**: Node rejects due to signature failure
   - Fix: Check logs for `PQC signature verification failed`; restart from last-known-good block
3. **Disk full**: Cannot write blocks to storage
   - Fix: Increase SSD; run `qv-miner prune-state` (*gelecek* — komut henüz yok)

### 11.3 Key Errors

**Symptom**: `ERR: keystore load failed: wrong password or corrupted keystore` in logs.

**Causes**:
1. **Keystore file corrupted**: Usually after disk failure (M-04 keystore writes
   are not torn-write safe — single file replacement only).
   - Fix: Restore from offline backup of the keystore file; recreate from master
     seed if backed up separately.
2. **Password wrong**: Argon2id KDF + AES-GCM tag mismatch
   - Fix: `export QV_KEYSTORE_PASS=<password>; systemctl restart quantumvault-miner`
3. **Keystore version mismatch**: Older miner version reading newer envelope
   - Fix: Upgrade miner binary; current version: 1 (envelope schema).

### 11.4 Double-Sign Protection

**Symptom**: Two blocks found for same slot from the same pool.
(Not: Ouroboros Praos tasarımında **slashing yok** — bkz. 8.2; double-sign
çatallanma/itibar sorunu yaratır, otomatik stake cezası uygulamaz.)

**Causes**:
1. **Bug in slot loop**: Running two validators for same pool
   - Fix: Ensure only ONE validator process per pool at a time
2. **Clock jumped backward**: System clock adjusted; same slot processed twice
   - Fix: Use NTP with `tinyNTP` or `chrony` for safety; never manually adjust clock

**Prevention**:
```bash
# Run single validator check
pgrep -f "qv-miner run" | wc -l
# Output should be exactly 1; if >1, kill extras immediately

# Monitor for double-sign attempt:
sudo journalctl -u quantumvault-miner | grep "double.sign\|slot.*already.*produced"
```

---

## 12. Security Best Practices

### 12.1 Air-Gapped Cold Key Management

```
┌────────────────────────────────────┐
│  OFFLINE MACHINE (no network)       │
├────────────────────────────────────┤
│  - Cold key in vault                │
│  - qv-wallet binary (read-only)     │
│  - Transaction drafts (USB import)  │
└────────────────────────────────────┘
            ↓ USB sneaker net
┌────────────────────────────────────┐
│  ONLINE MACHINE (connected)         │
├────────────────────────────────────┤
│  - Signed transaction (USB import)  │
│  - Broadcast via qv-wallet          │
└────────────────────────────────────┘
```

**Process**:
1. Create pool update on offline machine
2. Physically transfer USB to online machine
3. Broadcast only — no keys leave offline machine
4. Erase USB after successful broadcast

### 12.2 Firewall Rules

```bash
# Only libp2p port (validator-to-validator)
sudo ufw default deny incoming
sudo ufw allow 30333/tcp from any
sudo ufw allow 22/tcp from <operator_ip_only>

# Metrics port (internal only)
sudo ufw allow 9090/tcp from 127.0.0.1

# SSH hardening
sudo sed -i 's/#PermitRootLogin yes/PermitRootLogin no/' /etc/ssh/sshd_config
sudo systemctl reload sshd
```

### 12.3 Key Backup Checklist

- [ ] VRF key: encrypted + backed up to vault (3-of-5 Shamir)
- [ ] KES key: ephemeral (not backed up; rotates every epoch)
- [ ] Cold key: encrypted USB × 3 (distributed to operators)
- [ ] Mnemonic: seed phrase in safe + password-protected copy
- [ ] Test recovery: Restore from backup; verify keys load correctly

### 12.4 Update Procedure

```bash
# 1. Stop validator
sudo systemctl stop quantumvault-miner

# 2. Backup state
sudo tar -czf /backup/qv-state-$(date +%s).tar.gz \
  /var/quantumvault/data

# 3. Update binary
cd ~/l1
git pull origin main
nix develop
just build
sudo cp target/release/qv-miner /opt/quantumvault/qv-miner.new

# 4. Verify binary (check signature)
sha256sum /opt/quantumvault/qv-miner.new | \
  grep $(cat CHECKSUMS.sha256 | grep qv-miner)

# 5. Swap binary
sudo mv /opt/quantumvault/qv-miner \
  /opt/quantumvault/qv-miner.old
sudo mv /opt/quantumvault/qv-miner.new \
  /opt/quantumvault/qv-miner

# 6. Restart
sudo systemctl start quantumvault-miner

# 7. Verify sync
sleep 30
curl http://localhost:9090/metrics | grep qv_sync_height
```

### 12.5 Regular Maintenance Schedule

| Task | Frequency | Owner |
|------|-----------|-------|
| Monitor slot miss rate | Daily | Operator |
| Review access logs (HSM) | Weekly | Security team |
| Test KES rotation | Monthly | Operator |
| Verify backups | Monthly | Operator |
| Cold key rotation ceremony | Quarterly | Quorum (3+) |
| Full disaster recovery test | Annually | All operators |
| Security audit | Annually | External firm |

---

## References

- **[Key Management Guide](security/key-management.md)** — Detailed key lifecycle, HSM setup, Shamir recovery
- **[Threat Model: qv-miner](threat-model/qv-miner.md)** — Security assumptions, STRIDE analysis
- **[ADR-003: Encrypted Mempool](ADR/003-mev-encrypted-mempool.md)** — Committee decryption protocol
- **[ARCHITECTURE_V2.md](ARCHITECTURE_V2.md)** — Ouroboros Praos consensus design
- **[Incident Response Runbook](security/runbook-incident.md)** — Key compromise procedures

---

**Last Updated**: 2026-06-10  
**Maintained By**: QuantumVault Core Team  
**Next Review**: Mainnet hazırlık fazı başlarken (mainnet tarihi henüz yok; açık işler `docs/ROADMAP.md`)
