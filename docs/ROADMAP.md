# QuantumVault L1 — Production Roadmap

**Doküman tarihi:** 2026-04-30
**Hedef:** Mevcut iskeletten (derleniyor, ~115 placeholder, 366 unit test) production-ready bir L1 blockchain'e giden faz-faz plan.

---

## Mevcut Durum

| Konu | Durum |
|---|---|
| Workspace derleme | ✅ 0 error, 187 warning, 13 crate |
| Unit test sayısı | 366 (qv-core 70, qv-script 58, qv-consensus 46) |
| Çalışan binary'ler | qv-node, qv-wallet, qv-miner |
| Çalışan akışlar | CLI parse, mnemonic gen, PQC key gen, config save/load |
| **Eksik** | RPC bind, libp2p networking, RocksDB persistence, gerçek consensus loop, gerçek block production, encrypted mempool, Stealth address, BulletProofs, AMM, lending |
| **Placeholder/mock** | qv-privacy: 69, qv-miner: 18, qv-node: 11, qv-mempool: 10, qv-core: 6, qv-crypto: 5, qv-script: 3, qv-wallet: 2 |
| **Boş Ok dönen public method** | 53 |

---

## Faz Bazlı Plan (10 faz)

Bir "**oturum**" = 1-2 saat odaklı çalışma + maks 5-10 dosya değişikliği + sonunda yeşil build.

### Faz 0 — Hijyen ve dokümantasyon (1-2 oturum)

**Hedef:** Refactor öncesi sağlam zemin.

Görevler:
1. `cargo fix --workspace --allow-dirty --allow-staged` ile tetikli warning fix
2. Kalan warning'leri manuel ele al — özellikle `missing_docs` (toplam ~120)
3. `rust-toolchain.toml` geri yükle (`.bak`'tan)
4. Commit edilmemiş 25 dosyayı atomik commit'lere böl
5. Bu ROADMAP.md (yapıldı ✅)
6. CI: GitHub Actions workflow (build + clippy + nextest)
7. CONTRIBUTING.md

**Başarı kriteri:** `cargo build --workspace` 0 warning, `cargo clippy -- -D warnings` temiz, CI yeşil.

---

### Faz 1 — Devnet MVP, single-node (3-5 oturum)

**Hedef:** Tek node ayağa kalksın, RPC dinlesin, wallet bağlansın, in-memory bir transaction tüm zinciri dolaşsın.

Görevler:
1. `qv-node`: `jsonrpsee::server::Server::start(addr)` ile RPC server gerçekten bind et
2. `qv-node::Node`: storage + mempool + chain_state referanslarını `RpcServer`'a aktar
3. `qv-storage::block_store/utxo_store`: in-memory KvStore üzerinden gerçek I/O round-trip
4. `qv-wallet::keystore`: Argon2 + AES-GCM ile şifreli save/load (placeholder kaldır)
5. `qv-wallet::rpc_client`: `reqwest` ile gerçek HTTP JSON-RPC çağrı
6. `qv-wallet`: BIP-32/SLIP-0010 HD path'ten Dilithium spend key + X25519 view key türetme
7. `qv-wallet`: address derivation (PQC public key → bech32m benzeri encoding)
8. `qv-wallet balance`/`scan`/`send` komutları RPC üzerinden gerçek çalışsın
9. `qv-mempool`: `add` çağrısı RPC handler'dan tetiklensin
10. `qv-node`: Ctrl-C ile graceful shutdown + storage flush

**Başarı kriteri:** Bir wallet aç, node'a göndermek istediği tx'i imzala, RPC ile mempool'a düşür, `qv_getMempoolStatus` cevap dönsün, wallet `balance` doğru rakam göstersin.

**Bağımlılık:** Faz 0.

---

### Faz 2 — RocksDB persistence + indexler (2-3 oturum)

**Hedef:** Restart-safe node. Block ve UTXO kalıcı.

Görevler:
1. `qv-storage::RocksKvStore` ile in-memory yerine RocksDB
2. Block by-hash + by-height composite key indexi
3. UTXO set snapshot (epoch boundary'de)
4. Tx by-id index (mempool ve confirmed ayrı)
5. Pruning policy (Hot/Warm/Cold yaşlandırma — CLAUDE.md ADR'ı uyumlu)
6. State sync API (snapshot + delta replay)

**Başarı kriteri:** Node'u kapat, geri aç, son durum aynı; `qv_getBlockByHeight 0` aynı genesis'i versin.

**Bağımlılık:** Faz 1.

---

### Faz 3 — Single-node consensus loop (3-4 oturum)

**Hedef:** Tek node kendi kendine blok üretsin (mining demo).

Görevler:
1. `qv-crypto`: gerçek VRF (X25519 + Dilithium hash-to-curve) — TestVrf yerine
2. `qv-crypto`: gerçek KES forward-secure imza
3. `qv-miner::slot_loop`: production'a bağla — sleep(MAX) yerine
4. `qv-miner::block_producer`: mempool'dan tx topla → KES sign → block emit
5. `qv-consensus::block_validator`: VRF verify + KES verify + structural checks gerçek
6. `qv-consensus::ChainState`: `add_block` çağrısı + finality detection (k=50)
7. `qv-node`: validated block'u `block_store`'a yaz + tip güncelle
8. Self-mining demo: tek node, peer yok, blok üretiyor

**Başarı kriteri:** `qv-node` + `qv-miner run` aynı makinede çalıştırılınca her ~2 saniyede yeni blok, `qv_getTip` height artıyor.

**Bağımlılık:** Faz 2.

---

### Faz 4 — libp2p networking (4-5 oturum)

**Hedef:** Multi-node devnet localhost'ta.

Görevler:
1. `qv-net::NetworkNode`: gerçekten wire (TCP + Noise + GossipSub + Kademlia)
2. Block + Tx + VRF gossip topic'leri
3. Bootstrap peer discovery + DHT
4. Hibrit handshake X25519 + Kyber (CLAUDE.md mimarisine uygun)
5. Peer scoring + ban + connection limits
6. Multi-node devnet: localhost'ta 3 node, gossip akıyor, hep aynı tip'e yakınsıyor
7. `devnet/docker-compose.yml` gerçekten çalıştırılabilir hale getir

**Başarı kriteri:** 3 farklı portta 3 node, `qv_getTip` hepsinde aynı, bir node'a verilen tx diğerlerine gossip ile yayılıyor.

**Bağımlılık:** Faz 3.

---

### Faz 5 — Wallet pro + Stealth address (3-4 oturum)

**Hedef:** Privacy-preserving wallet.

Görevler:
1. `qv-privacy::stealth`: gerçek KEM tabanlı stealth address (Mock kaldır) — Kyber view + Dilithium spend
2. View key + spend key ayrımı + serialization
3. `qv-wallet::scanner`: gerçek stealth scanning (block range + view tag filter)
4. HD path standardı (m/44'/QV_COIN_TYPE'/account'/change/index)
5. Address book + transaction history yerel persistence
6. CLI: `qv-wallet stealth-receive` + `stealth-send`

**Başarı kriteri:** A wallet → B'nin stealth adresine 1 QV gönder, B `scan` çalıştırınca alındı görünsün, on-chain'de A→B bağı **görünmesin**.

**Bağımlılık:** Faz 1, 3.

---

### Faz 6 — Script VM + DeFi temelleri (5-6 oturum)

**Hedef:** Programlanabilir UTXO'lar.

Görevler:
1. `qv-script::opcode`: tüm opcode set tamam (CHECKSIG_PQC, introspection, covenants)
2. `qv-script::interpreter`: gas hesaplama + execution limit
3. `qv-script::templates`: p2pkh_pqc, multisig_pqc standart şablonları
4. `qv-defi::amm`: constant-product AMM (x·y=k) gerçek state — Shared UTXO Pattern
5. `qv-defi::lending`: basit collateralized lending
6. `qv-defi::oracle`: TWAP fiyat oracle (median observation)

**Başarı kriteri:** AMM havuzunda swap yap, x·y invariant korunsun, lending'de collateral koy, borç al, geri öde.

**Bağımlılık:** Faz 3.

---

### Faz 7 — Encrypted mempool + MEV koruması (4-5 oturum)

**Hedef:** ADR-003'ün gerçek hayata geçişi.

Görevler:
1. `qv-mempool::encrypted::ThresholdDecryptor`: gerçek Threshold Kyber
2. Committee sortition (VRF tabanlı, t-of-n)
3. Decryption protocol (multi-round broadcast)
4. Batch ordering deterministik (canonical hash sıralı)
5. `qv-defi::batcher`: encrypted intent → batched swap
6. Slashing evidence: misorder + decryption-skip cezaları

**Başarı kriteri:** Encrypted tx mempool'a girsin, slot leader + komite çözsün, deterministik sıralı batch'le block'a yazılsın, MEV fırsatı kapansın.

**Bağımlılık:** Faz 4, 6.

---

### Faz 8 — Privacy: Confidential amounts (3-4 oturum)

**Hedef:** Opt-in miktar gizliliği.

Görevler:
1. `qv-privacy::confidential`: gerçek Bulletproofs (curve25519-dalek-ng veya bulletproofs crate)
2. Pedersen commitments
3. Range proofs (64-bit values)
4. `qv-wallet`: confidential tx oluşturma + alma
5. Aggregate verification (validator tarafı performans)

**Başarı kriteri:** Bir tx oluştur, miktar on-chain'de görünmesin (Pedersen commit), range proof verify olsun.

**Bağımlılık:** Faz 5.

---

### Faz 9 — Production hardening + audit-ready (4-6 oturum)

**Hedef:** Mainnet'e aday hale getirme.

Görevler:
1. Tüm warning'ler 0
2. `cargo clippy -- -D warnings -D clippy::pedantic` temiz
3. Fuzz testler (cargo-fuzz, mevcut `fuzz/` klasörünü kullan)
4. Property-based testler (proptest) — kritik invariant'lar için (UTXO conservation, AMM invariant, vs.)
5. Genesis ceremony tooling (gerçek + reproducible)
6. Monitoring stack (Prometheus + Grafana dashboard'ları)
7. Docker compose tam çalışır
8. Block explorer (`devnet/scripts/explorer.py` aktif)
9. Faucet (`devnet/scripts/faucet.py` aktif)
10. Performans benchmark'ları (criterion) — TPS, finality latency, block validation süresi

**Başarı kriteri:** Devnet 24 saat aralıksız çalışsın, fuzz test 1 saat boyunca crash bulmasın, dashboard canlı.

**Bağımlılık:** Tüm önceki fazlar.

---

### Faz 10 — Mainnet ön hazırlık (uzun vadeli, ~6-12 ay)

Görevler:
- Bağımsız security audit (en az 1, ideal 2 firma)
- Formal verification (kritik consensus + cryptography parçaları için)
- Bug bounty programı
- Reference dokümantasyon (yatırımcı + entegratör + validator için ayrı)
- Genesis ceremony (multi-party, ölçülü törensel)
- Validator onboarding programı
- Mainnet launch

**Başarı kriteri:** Mainnet ayakta, dış audit raporları yayınlandı.

---

## Tahmini Toplam Süre

| Aşama | Fazlar | Oturum | Takvim |
|---|---|---|---|
| **Devnet MVP** | 0–3 | 9–14 | ~1–2 ay |
| **Testnet candidate** | 4–6 | +12–15 | ek ~3 ay |
| **Production hardening** | 7–9 | +11–15 | ek ~3 ay |
| **Mainnet** | 10 | uzun | 6–12 ay |
| **Toplam (mainnet'e kadar)** | — | 32–44 oturum + Faz 10 | **~12–18 ay** |

Bu sürelere not: Sadece kod yazma değil; review, test, debug, bekleme dahil. Tek geliştirici tempo. Birden fazla geliştirici çoğaltır ama 1/N çıkmaz (bazı fazlar inherently sequential).

---

## Bağımlılık Grafiği

```
Faz 0 ──► Faz 1 ──► Faz 2 ──► Faz 3 ──┬──► Faz 4 ──┬──► Faz 7
                                       │            │
                                       └──► Faz 5 ──┴──► Faz 8
                                       │
                                       └──► Faz 6 ──────► Faz 7

Tüm fazlar ──► Faz 9 ──► Faz 10
```

---

## Bu Oturumda Yapılan + Sonraki Adımlar

**Bu oturumda yapıldı:**
- ✅ Disk dolu krizi çözüldü (7 GB → 54 GB)
- ✅ NULL byte corrupt `Cargo.toml` git'ten geri yüklendi
- ✅ qv-wallet, qv-miner, qv-node compile edildi (~125 hata → 0 hata)
- ✅ `cargo build --workspace --release` başarılı (22 dk)
- ✅ Bu ROADMAP.md
- ⏳ `devnet/scripts/smoke.ps1` (sırada)

**Sonraki oturumda öneri:**
- Faz 0 başla — warning temizliği, CI workflow, atomik commit'ler. Hızlı kazanım.
- Sonra Faz 1'e geç — RPC server bind + wallet keystore.

**Tek oturumda kapsam önerisi:**
Bir oturumda en fazla **bir alt-fazın 3-5 görevi** yapılabilir. Örnek: Faz 1'in 1-3-9 numaralı görevleri (RPC bind + storage I/O + mempool RPC bağlama) bir oturum.

---

## Notlar

- **CLAUDE.md** mimari ve kararlar (PQC + UTXO + Ouroboros + Encrypted mempool) DEĞİŞMEZ kabul edildi. Bu roadmap o kararları implementasyona dönüştürür.
- **ADR-001/002/003** referansları korunur; her faz ilgili ADR'ı işaret eder.
- **Test öncelik:** her yeni özellik için en az 3 unit + 1 integration test. CI gate.
- **Asla** `unwrap`/`expect`/`panic`/`indexing`/`integer_division`/`float_arithmetic` kullanmayacağız — workspace `clippy.toml` ile zaten deny seviyesinde.
- **Branch stratejisi**: `main` her zaman yeşil; her görev `feat/faz-X-task-Y` branch'inde, PR ile merge.
