# QuantumVault - Proje Hafizasi

_Son guncelleme: 2026-04-24_

---

## Proje Ozeti

QuantumVault, kuantum korumali, UTXO tabanli, istemci tarafi dogrulamali bir Katman 1 blokzincirdir.
Aktif gelistirme Rust cekirdegi uzerindedir. Hedef: hizli, gizlilik odakli, DeFi-uyumlu,
matematiksel olarak dogrulanabilir mutabakat katmani.

---

## Kritik Tarihler

| Tarih | Olay |
|---|---|
| 2026-04-15 | v1 (C++) -> v2 (Rust) pivot karari |
| 2026-04-15 | Asama 0 tamamlandi |
| 2026-04-15 | Asama 1 tamamlandi |
| 2026-04-16 | Asama 2 tamamlandi |
| 2026-04-17 | Asama 3 tamamlandi |
| 2026-04-17 | Asama 4 tamamlandi |
| 2026-04-24 | Asama 5 tamamlandi |
| 2026-04-24 | Asama 6 tamamlandi |
| 2026-04-24 | Asama 7 tamamlandi |
| 2026-04-24 | Asama 8 tamamlandi |

---

## Tamamlanan Asamalar

### Asama 0 - Pivot Temizligi ve Rust Iskeleti
- v1 C++ kodu archive/cpp-v1 altina tasindi
- Cargo workspace + 13 crate iskeleti kuruldu
- Nix flake, justfile, CI, clippy/lint kurallari eklendi
- Dokumantasyon v2 mimariye guncellendi

### Asama 1 - qv-crypto
- secure_bytes, hash, pqc_sign, hybrid_kem tamamlandi
- Dilithium ve Kyber tabanli API'ler yazildi
- Integration test + benchmark altyapisi eklendi
- VRF/KES/threshold icin iskelet var, final secim acik

### Asama 2 - qv-core
- Temel tipler, tx, block, utxo, protocol params tamamlandi
- Deterministik serializasyon ve yapisal dogrulamalar eklendi
- InMemoryUtxoSet ve commitment root mantigi tamamlandi

### Asama 3 - qv-script
- Opcode seti, gas modeli, interpreter tamamlandi
- validate_script API'si ve standart script template'leri yazildi
- Script only-validation prensibi korundu

### Asama 4 - qv-consensus
- slot/epoch, stake, leader schedule, block validation tamamlandi
- chain state, fork choice, k-deep finality, rewards tamamlandi
- VRF/KES trait arkasinda mock ile test edilebilir halde

---

## Aktif Asama

### Asama 5 - qv-storage
- KvStore trait + 3 backend: MemoryKvStore, RocksKvStore, RedbKvStore (pure-Rust)
- BlockStore: put/get/index by hash+height, header-only light client path
- UtxoStore: apply/revert block (undo log), snapshot/restore, commitment root
- StateStore: chain entry/tip, ledger state, epoch snapshot persistence
- 14 unit test + 12 integration test

### Asama 6 - qv-net
- libp2p 0.54: TCP + Noise XX + Yamux transport, Kademlia DHT, GossipSub, Identify, Ping
- 4 gossip topic: blocks, tx, vrf, votes — SHA3-256 content-addressed dedup
- NetworkMessage (9 variant) + Envelope wire format (version + bincode)
- PeerStore: reputation tracking, ban/evict, idle eviction
- RateLimiter: per-peer token bucket
- NetworkNode: composite QvBehaviour, async event loop, NetEvent channel
- Hybrid KEM handshake beklemede (snow/libp2p pluggable KEM desteği yok henüz)
- 22 unit + 12 integration test

### Asama 7 - qv-mempool
- ClearPool: fee-density sorted BTreeMap, UTXO dependency tracking, age/capacity eviction
- OrderKey + deterministic_sort + verify_order: canonical 3-tuple ordering
- EncryptedPool: epoch-scoped, ThresholdDecryptor trait + MockThresholdDecryptor
- build_amm_batch: constant-product (x*y>=k), 0.3% fee, slippage skip
- SlashingEvidence: misordering proof struct
- 24 unit + 12 integration test

### Asama 8 - qv-privacy
- Stealth addresses: Kyber hybrid view key + Dilithium spend key
- create_stealth_output / scan_output / recover_spend_key API
- Confidential amounts: Plain(u64) | Confidential(Commitment, RangeProof)
- Committer/RangeProver/RangeVerifier trait + Mock impl
- View key export + DisclosureProof per-output selective disclosure
- PrivacyMode: StealthOnly (default) | Full | Transparent
- SpendKeyDeriver trait + MockSpendKeyDeriver (real Dilithium keygen beklemede)
- 31 unit + 12 integration test

### Asama 9 - qv-defi (Siradaki)
Hedef:
1. AMM (Constant Product, Shared UTXO Pattern)
2. Lending (basit)
3. Oracle entegrasyonu

---

## Degistirilemez Mimari Kararlar

- Dil: Rust stable, edition 2021
- Durum modeli: UTXO + eUTXO datum/validator
- Konsensus: Ouroboros Praos, 2sn slot, 12saat epoch, k=50
- Kriptografi: hibrit KEM (X25519 + Kyber), Dilithium imza
- Gizlilik: stealth varsayilan, confidential amounts opsiyonel
- MEV: encrypted mempool + threshold Kyber
- Script VM: deterministik, gas-limitli, validation-only
- Tokenomics: 21M sabit arz, halving, fee dagitimi pool+delegator

---

## Crate Durumu

| Crate | Durum |
|---|---|
| qv-crypto | Tamamlandi |
| qv-core | Tamamlandi |
| qv-script | Tamamlandi |
| qv-consensus | Tamamlandi |
| qv-storage | Tamamlandi |
| qv-privacy | Tamamlandi |
| qv-net | Tamamlandi |
| qv-mempool | Tamamlandi |
| qv-defi | Iskelet |
| qv-node | Iskelet |
| qv-wallet | Iskelet |
| qv-miner | Iskelet |

---

## ADR Durumu

- ADR-001: testing framework (onayli)
- ADR-002: DeFi architecture (aktif referans)
- ADR-003: MEV encrypted mempool (aktif referans)
- ADR-004: VRF secimi (yazilacak)
- ADR-005: KES secimi (yazilacak)

---

## Acik Kararlar

- VRF primitive secimi
- KES primitive secimi
- Oracle tasarimi
- Cross-chain bridge yaklasimi
- Governance modeli
- STARK range proof migration takvimi

---

## Bilinen Notlar

- Workspace'te QuantumVault klasoru artefakti gorunebiliyor; zararsiz, manuel temizlenebilir.
- Local ortamda dogrulama akisi: nix develop -> just build -> just test -> just clippy.

---

## Calisma Kurali (Kalici)

- Her somut gelistirme adiminin sonunda PROJECT_STATUS.md ve MEMORY.md birlikte guncellenir.
- Memory girdisi kisa ve eylem odakli olur: yapilan is, alinan karar, sonraki adim.
- Status girdisi asama bazli olur: tamamlanan madde, test/build dogrulama notu.


## Session Update - 2026-04-24 (Asama 5 + 6 Tamamlandi)

**Asama 5 tamamlama:**
- RedbKvStore (pure-Rust fallback) eklendi: redb 2.1.3 backend.
- qv-storage integration testleri: 12 cross-module test.
- Toplam: 14 unit + 12 integration test.

**Asama 6 (qv-net) tam implementasyon:**
- peer.rs: PeerInfo + PeerStore (reputation, ban/evict, idle eviction).
- message.rs: NetworkMessage (9 variant), Envelope (version + bincode), size limits.
- transport.rs: TransportConfig presets, NodeIdentity (Ed25519).
- gossip.rs: 4 topic, GossipConfig, build_gossipsub (SHA3-256 MessageId), SeenCache dedup.
- node.rs: QvBehaviour (GossipSub+Kad+Identify+Ping), NetworkNode (Swarm, event loop), RateLimiter.
- lib.rs: NetError (6 variant), re-exports.
- Integration testleri: 12 test.
- Toplam: 22 unit + 12 integration test, ~1400 satir src + ~300 satir test.
- Hybrid KEM handshake beklemede: snow/libp2p pluggable KEM slot yok.
- Cargo.toml: bincode dependency eklendi.
- Sonraki asama: qv-mempool (Asama 7).

**Asama 7 (qv-mempool) tam implementasyon:**
- clear.rs: ClearPool (fee-density BTreeMap, UTXO dep tracking, eviction). 8 unit test.
- ordering.rs: OrderKey + deterministic_sort + verify_order. 6 unit test.
- encrypted.rs: EncryptedPool + ThresholdDecryptor trait + MockThresholdDecryptor. 7 unit test.
- batcher.rs: OrderIntent + build_amm_batch (x*y>=k, 0.3% fee) + SlashingEvidence. 6 unit test.
- lib.rs: MempoolError (8 variant). 3 unit test.
- Integration: 12 test.
- Toplam: 24 unit + 12 integration, ~1160 satir src + ~350 satir test.
- Real threshold Kyber DKG beklemede (ThresholdDecryptor trait arkasinda).
- Sonraki asama: qv-privacy (Asama 8).

**Asama 8 (qv-privacy) tam implementasyon:**
- stealth.rs: StealthKeys (Kyber view + Dilithium spend), create_stealth_output, scan_output, recover_spend_key. SpendKeyDeriver trait + MockSpendKeyDeriver. 8 unit test.
- confidential.rs: ConfidentialAmount (Plain|Confidential), BlindingFactor, Commitment, RangeProof. Committer/RangeProver/RangeVerifier trait + Mock impl. verify_balance_mock(). 12 unit test.
- view_key.rs: ViewKey export, DisclosureProof (per-output selective disclosure), PrivacyMode enum. 8 unit test.
- lib.rs: PrivacyError (6 variant), re-exports. 3 unit test.
- Integration: 12 test (full lifecycle, wrong recipient, multi-output, stealth+confidential, disclosure, balance, privacy modes, e2e).
- Toplam: 31 unit + 12 integration = 43 test, ~930 satir src + ~350 satir test.
- Real Bulletproofs beklemede (Committer/RangeProver trait arkasinda).
- Real Dilithium deterministic keygen beklemede (SpendKeyDeriver trait arkasinda).
- STARK range proof winterfell entegrasyonu gelecek (trait hazir).
- Sonraki asama: qv-defi (Asama 9).

