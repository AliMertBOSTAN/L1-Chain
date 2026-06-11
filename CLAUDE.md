# QuantumVault L1 — Proje Rehberi (v2)

## Proje Kimliği
Kuantum korumalı, UTXO tabanlı, istemci tarafı doğrulamalı Katman 1 blokzincir.
Rust çekirdeği. Hedef: ultra hızlı, gizlilik odaklı, DeFi-uyumlu, matematiksel olarak
doğrulanabilir mutabakat katmanı.

**Pivot**: Proje 2026-04-15'te C++'tan Rust'a geçti. v1 kodu `archive/cpp-v1/`
altında referans olarak duruyor. Aktif geliştirme `crates/` altında.

## Mimari Kararlar (Değiştirilemez)

### Kriptografi
- **P2P Tünelleme**: X25519 (klasik) + Kyber/ML-KEM (PQC) hibrit model
- **İşlem İmzaları**: Dilithium/ML-DSA (PQC) — Level 3 varsayılan
- **Kütüphane**: `pqcrypto-dilithium`, `pqcrypto-kyber` crate'leri (liboqs bağlaması)
- **Neden hibrit?** PQC-only backdoor riskini klasik kriptografi ile sigortala
- **Ek**: VRF (Ouroboros için), KES (forward-secure imza), Threshold Kyber (encrypted mempool)

### Durum Modeli
- **UTXO + Cardano eUTXO tarzı datum/validator**
- Her UTXO: `value`, `locking_script`, opsiyonel `datum` (ekstra veri), opsiyonel `stealth_info`
- L1 çekirdeği sadece şunları bilir: PQC imza doğruluğu + double-spend + script doğrulama
- Akıllı kontratlar L1'de çalıştırılmaz; script VM sadece UTXO harcanabilir mi kontrol eder
- Account modeli bilinçli olarak reddedildi (paralellik + gizlilik kısıtları)
- **Shared UTXO Pattern**: AMM/lending için tek UTXO + script invariant (x·y=k)

### Mutabakat — Ouroboros Praos (Pure Nakamoto PoS)
- VRF ile stake-orantılı slot lider seçimi
- Slot süresi: 2 saniye
- Epoch: 12 saat (21600 slot)
- k-deep finality: k=50 blok (~100 saniye); blok-bazlı, yapışkan/monoton (geri alınamaz)
- KES imzalar: uzun menzilli saldırılara karşı forward secrecy
- Fork choice: longest-chain (yükseklik) + deterministik tie-break (en düşük hash),
  yapışkan k-deep finalite ile birleşik. Genesis maxvalid-bg (bootstrap density
  tabanlı seçim) çekirdeği uygulandı; sync entegrasyonu açık — ADR-008
- Hibrit PoW+PoS bırakıldı — DeFi için gereken finalite ve latency PoS ile daha iyi

### Tokenomics
- **Sabit arz** (21M, deflasyonist, Bitcoin modeli — korundu)
- Halving dönemleri ile azalan blok ödülleri
- İşlem ücretleri stake pool operatörlerine + delegator'lara dağıtılır (burn yok)
- Parametreler `config/genesis.toml` içinde (Aşama 4'te yazılacak)

### Akıllı Kontratlar — Script VM
- Stack-based, deterministik, gas-limitli
- Opcode'lar: stack ops + aritmetik + kripto (CHECKSIG_PQC) + introspection + covenants
- Script L1'de **doğrulanır**, **yürütülmez** — sadece UTXO harcanabilir mi diye bakar
- Standart template'ler: `p2pkh_pqc`, `multisig_pqc`, AMM swap, lending

### Gizlilik — Opt-in Model
- **Varsayılan**: KEM tabanlı Stealth Addresses (Kyber view + Dilithium spend)
  - Not: primitifler var ama uçtan uca entegrasyon henüz tamam değil — ADR-011
- **Privacy mode (opsiyonel)**: Confidential amounts — Bulletproofs (klasik curve)
  - Not: Bulletproofs PQC değil; kullanıcı bilinçli trade-off
  - STARK range proof migration gelecek sürümde
- Ring Signatures kullanılmaz (blok şişmesi nedeniyle reddedildi)

### MEV Stratejisi — Encrypted Mempool
- İşlemler mempool'a **threshold Kyber** ile şifreli girer
- Slot lider + validator komitesi (t-of-n) blok önerirken çözer
- Deterministik batch sıralama → MEV fırsatı kapatılır
- Detaylar: ADR-003

### Veri Kullanılabilirliği (DA)
- Erasure coding destekli bağımsız sidechain (gelecek aşamada)
- Kademeli veri yaşlandırma: Hot → Warm → Cold
- Özyinelemeli PQC-STARK kanıtları ile cold veri sıkıştırma
- libp2p + Gossip protokolü ile dağıtım

### Ağ
- libp2p üzerinden P2P iletişim (Kademlia + GossipSub)
- Hibrit KEM handshake (X25519 + Kyber)
- libp2p `rust-libp2p` 0.54+

## Build Sistemi
- **Nix flake** ile reproducible ortam (`nix develop`)
- **Cargo workspace** — 13 crate (10 library + 3 binary)
- Rust stable 1.78+, edition 2021
- Task runner: `just`

## Dizin Yapısı
```
crates/
  qv-common/       # Shared types, errors
  qv-crypto/       # Hash, PQC sign, hybrid KEM, VRF, KES, threshold
  qv-core/         # UTXO, Transaction, Block, Merkle, Protocol params
  qv-script/       # Script VM (opcode, interpreter, templates, gas)
  qv-consensus/    # Ouroboros Praos (slot, epoch, leader, finality)
  qv-privacy/      # Stealth addresses + opt-in confidential amounts
  qv-storage/      # RocksDB blocks, UTXO set, chain state
  qv-net/          # libp2p transport, gossip, node wiring
  qv-mempool/      # Clear + encrypted mempool, batcher
  qv-defi/         # AMM, lending, oracle, intents
  qv-node/         # Full node binary
  qv-wallet/       # CLI wallet binary
  qv-miner/        # Stake pool operator binary
docs/              # Mimari belgeler, ADR'ler, ROADMAP
archive/cpp-v1/    # Önceki C++ implementasyon (referans)
config/            # genesis.toml, ağ parametreleri (gelecek)
.github/workflows/ # CI pipeline
```

## Kodlama Kuralları (Rust)
- Edition 2021, stable 1.78+
- `#![forbid(unsafe_code)]` varsayılan — `unsafe` bloğu mutlaka `// SAFETY:` yorumu içermeli
- `cargo fmt` + `cargo clippy -D warnings` zorunlu (CI gate)
- Hata yönetimi: `thiserror` ile crate-specific enum; tüketici tarafta `anyhow`
- Bellek güvenliği: `zeroize` ile secret drop-zeroing; `secrecy::Secret<T>` wrapper
- Panic yok: `unwrap`, `expect`, `panic!`, `indexing`, `integer_division` clippy ile deny
- Serializasyon: `serde` + `bincode` (internal), `rkyv` (zero-copy hot paths)
- Loglama: `tracing` + structured fields (`?err`, `%addr`)
- Async runtime: `tokio`
- Test framework: `cargo-nextest` + `proptest` (property-based)
- Benchmark: `criterion`
- Namespace: tüm crate'ler `qv-*`, modül adları `qv` prefix'siz

## Geliştirme Akışı
1. `nix develop` ile devshell'e gir
2. `just build` — workspace derle
3. `just test` — nextest ile testleri çalıştır
4. `just clippy` — lint
5. `just ci` — tam CI pipeline lokal koş (commit öncesi)
6. `pre-commit install` ile hook'ları kur (ilk sefer)

## Önemli Notlar
- L1 çekirdeği ASLA akıllı kontrat çalıştırmaz — sadece UTXO + imza + script doğrulama
- Her kriptografik primitif hibrit olmalı (klasik + PQC); istisna: Bulletproofs (opt-in, belirtilen)
- Gizlilik varsayılan stealth; miktar gizliliği opt-in
- Script VM deterministik olmak zorunda — float yok, overflow=wrap, gas sınırlı
- Bağımlılık ekleme politikası: yeni crate eklerken PR'da gerekçe ve license+audit

## Referanslar
- [MEMORY.md](MEMORY.md) — proje hafızası, crate durumları, açık kararlar
- [PROJECT_STATUS.md](PROJECT_STATUS.md) — güncel durum ve aşama detayları
- [ARCHITECTURE_V2.md](docs/ARCHITECTURE_V2.md) — üst düzey tasarım
- [ROADMAP.md](docs/ROADMAP.md) — açık işler için tek doğru kaynak (Placeholder ve Mock Envanteri; eski MASTER_PLAN `archive/docs-v1/` altında)
- [ADR-001](docs/ADR/001-testing-framework.md) — test framework kararı (v1)
- [ADR-002](docs/ADR/002-defi-architecture.md) — DeFi mimarisi (Cardano eUTXO tercihi)
- [ADR-003](docs/ADR/003-mev-encrypted-mempool.md) — MEV stratejisi
- [ADR-008](docs/ADR/008-genesis-maxvalid-bg.md) — Genesis maxvalid-bg çatal seçimi (çekirdek uygulandı)
- [ADR-009](docs/ADR/009-deterministic-leader-check.md) — deterministik (sabit-nokta) lider kontrolü
- [ADR-010](docs/ADR/010-bootstrap-sync.md) — bootstrap senkronizasyon altyapısı (çekirdek uygulandı)
- [ADR-011](docs/ADR/011-stealth-address-integration.md) — stealth adres uçtan uca entegrasyonu (Faz 1-5 uygulandı)
- [ADR-012](docs/ADR/012-transaction-sighash.md) — işlem sighash'i; imzayı işleme bağlama (uygulandı)
- [ADR-013](docs/ADR/013-lending-covenant-oracle.md) — lending kovenantı + imzalı oracle fiyat doğrulama (uygulandı)
- [Çatallanma & Finalite Denetimi](docs/security/qv-consensus-fork-finality-audit.md) — fork/finalite güvenlik denetimi
- [ABSTRACT.md](docs/ABSTRACT.md) — proje felsefesi ve kime hitap ediyor
