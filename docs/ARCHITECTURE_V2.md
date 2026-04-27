# QuantumVault v2 — Mimari Yol Haritası

_Pivot tarihi: 2026-04-15_

Bu belge v1'den (C++ iskelet) v2'ye (Rust, DeFi odaklı, UTXO+Covenants,
Ouroboros tarzı PoS, Stealth+Confidential Amounts) geçişin gerekçelerini ve
getirdiği yeni mühendislik zorluklarını kayıt altına alır.

---

## 1. Kararlar Özeti

| Katman | v1 | v2 | Değişiklik Gerekçesi |
|---|---|---|---|
| Dil | C++20 | **Rust (stable)** | Bellek güvenliği + olgun async ekosistem + blokzincir camiası |
| Durum Modeli | UTXO + CSV | **UTXO + Covenants** | CSV felsefesini koru; DeFi'yi gelişmiş script'lerle in'a et |
| Konsensüs | Hibrit PoW+PoS | **Nakamoto PoS (Ouroboros Praos)** | DeFi için daha düşük gecikme, enerjisiz, kanıtlanabilir güvenlik |
| Gizlilik | Stealth addresses | **Stealth + Confidential Amounts** | Bireysel pozisyon gizli, havuz şeffaf → DeFi uyumlu |
| Kriptografi | X25519+Kyber, Dilithium, liboqs | **Aynı** (pqcrypto-rs / oqs-rs bağlaması) | Karar değişmedi |
| Build | Nix + CMake + Ninja | **Nix + Cargo + Workspaces** | Rust-native yaklaşım |

---

## 2. UTXO + Covenants: DeFi'yi Nasıl Kuracağız?

Bu **en kritik ve en çözümsüz** kısım. Uniswap/Aave modellerini doğrudan
taşıyamayız çünkü onlar "paylaşılan zincir üstü hesap" varsayımı yapar.
UTXO + CSV'de her UTXO tek sahipli, tek harcanan birimdir.

### 2.1 Temel Primitif: Shared UTXO Pattern

AMM havuzunu tek bir "havuz UTXO'su" olarak temsil ederiz. Kilidi (locking
script), havuzun sabit çarpım kuralını (x·y=k) matematiksel olarak
dayatır. Her swap eden kullanıcı:

1. Eski havuz UTXO'sunu harcar (input)
2. Yeni havuz UTXO'su üretir (output) — invariant korunarak
3. Kendi çıktısını alır

Script dili şu primitifleri desteklemeli:
- **Introspection**: bir girdi/çıktının miktarını ve kilit script'ini okuma
- **Invariant enforcement**: x_new · y_new ≥ x_old · y_old (swap fee hariç)
- **Covenants**: bir sonraki harcamanın kilit script'i ne olmak zorunda
- **Merkle proofs**: büyük durum (order book) için off-chain commitment

### 2.2 Araştırma Düzeyinde Sorunlar (henüz çözümsüz)

| Sorun | Neden zor? | Aday çözüm |
|---|---|---|
| **Eşzamanlı erişim** | Aynı blokta iki swap → ikisi de eski UTXO'yu harcar, biri reddedilir | **Intent-based batching**: cüzdanlar intent yayınlar, bir "aggregator" bunları tek tx'te birleştirir (CoW-Swap tarzı) |
| **Lending protokolü** | Milyonlarca borç verici için tek UTXO olmaz | **Merkle tree UTXO**: tüm pozisyonlar tek UTXO'nun içindeki Merkle kökünde, ekle/çıkar STARK ispatıyla |
| **Fiyat oracle** | Havuzun fiyatı on-chain ama dış fiyat gerekir | **Commit-reveal + median feed**: validatörler PoS slot lider olarak fiyat imzalar |
| **MEV** | Aggregator ayrıcalıklı pozisyonda → front-run | **VDF tabanlı sıra**: işlem sırası doğrulanabilir gecikmeyle belirlenir |

**Karar gereksinimleri:** Bu kısım için ayrı bir ADR (`docs/ADR/002-defi-architecture.md`)
yazıp BitVM, Ark, Cashu, Taproot Assets ve Cardano eUTXO'nun DeFi yaklaşımlarını
incelemeliyiz. Her biri farklı trade-off yapıyor.

---

## 3. Ouroboros Praos PoS — Blok Süresi ve Finalite

Pure Nakamoto PoS'u seçtik. Cardano'nun Ouroboros'u kanıtlanmış, ama
**20 saniyelik slot'u DeFi için çok yavaş**. Parametreleri yeniden tune etmeliyiz:

- **Slot süresi:** 2 saniye (DeFi için elverişli)
- **Epoch uzunluğu:** 12 saat (864 slot)
- **Slot lider seçimi:** VRF ile stake-orantılı
- **k-deep finality:** k=50 blok (~100 saniye olasılıksal finalite)
- **Forward-secure imzalar:** uzun menzilli saldırılara karşı (KES)

Ouroboros'un avantajı: saf longest-chain kuralı + VRF, BFT komite kurulumu
gerektirmiyor. UTXO + Nakamoto ile doğal uyum.

**Açık soru:** k-deep finality DeFi için yeterli mi, yoksa checkpoint
kesinleşmesi (Byzantine fault tolerant overlay) eklemeli miyiz?

---

## 4. Gizlilik: Stealth + Confidential Amounts (PQC güvenli nasıl?)

Standart Confidential Amounts → **Bulletproofs** (Monero) kullanır, ama
Bulletproofs **discrete log tabanlı**, Shor ile kırılabilir. PQC felsefemizle
çelişir.

### Seçenekler (karar gerekli)

| Yaklaşım | PQC güvenli? | Proof boyutu | Olgunluk |
|---|---|---|---|
| Bulletproofs | ❌ | ~670 B | Prod |
| Bulletproofs++ | ❌ | ~416 B | Prod (Monero) |
| zk-STARK range proof | ✅ (hash-based) | ~50 KB | Prod (StarkWare) |
| Lattice Bulletproofs | ✅ (Ring-LWE) | ~5-15 KB | Araştırma |
| Pedersen + STARK hybrid | Kısmi | ~20 KB | Araştırma |

**Öneri:** İlk mainnet sürümünde **klasik Bulletproofs** ile başla (çalışan
DeFi için), **STARK range proof migration path** planla. Bu, "kuantum saldırı
iminent olduğunda geçiş yapabiliriz" demek.

**Alternatif (daha radikal):** Miktar gizliliği başlangıçta opsiyonel olsun —
sadece stealth addresses varsayılan, confidential amounts "privacy mode"
olarak opt-in. Böylece PQC uyumluluğunu ertelemiş oluruz.

---

## 5. Rust Ekosistem Entegrasyonu

Sıfırdan bespoke Rust seçtin — framework yok. Kullanılacak kütüphaneler:

### Core
- **tokio** — async runtime
- **rust-libp2p** — P2P ağ (production-ready, Ethereum Lighthouse kullanıyor)
- **rocksdb** veya **redb** — depolama (redb: pure Rust, daha az bağımlılık)
- **prost** veya **rkyv** — serializasyon (rkyv zero-copy, daha hızlı)
- **tracing** — structured logging
- **rayon** — paralel işlem

### Kriptografi
- **oqs-rs** veya **pqcrypto** — liboqs Rust bağlaması
- **x25519-dalek** — klasik X25519
- **ed25519-dalek** — klasik imza (VRF için)
- **blake3** — native Rust hash
- **sha3** — RustCrypto, FIPS uyumlu
- **vrf-rs** veya özel implementasyon — Ouroboros VRF

### Test & Geliştirme
- **proptest** — property-based testing
- **criterion** — benchmark
- **cargo-fuzz** + **libFuzzer** — fuzzing
- **miri** — undefined behavior detector
- **cargo-deny** — dependency audit
- **cargo-audit** — vulnerability DB check

---

## 6. Workspace Yapısı (Rust)

```
quantumvault/
├── Cargo.toml                 # workspace root
├── flake.nix
├── rust-toolchain.toml        # rustc pin
├── crates/
│   ├── qv-crypto/             # hash, PQC, hybrid KEM, VRF
│   ├── qv-core/               # UTXO, transaction, block primitives
│   ├── qv-script/             # Covenant script VM (stack-based)
│   ├── qv-consensus/          # Ouroboros Praos
│   ├── qv-privacy/            # Stealth addresses, range proofs
│   ├── qv-net/                # libp2p transport, gossip
│   ├── qv-storage/            # RocksDB/redb abstraction, UTXO set
│   ├── qv-mempool/            # Intent-aware mempool + aggregator
│   ├── qv-defi/               # AMM, lending primitives (on-top of script)
│   ├── qv-node/               # Full node binary
│   ├── qv-wallet/             # CLI wallet binary
│   └── qv-miner/              # Stake pool operator binary (slot leader)
├── fuzz/
├── benches/
└── docs/
    └── ADR/
```

---

## 7. Yeniden Düşünülmesi Gereken Noktalar (Bu Oturumda Seninle)

1. **MEV stratejisi:** Intent-based batch auctions (CoW), encrypted mempool,
   veya VDF tabanlı sıra — hangisi?
2. **Oracle tasarımı:** Validator median, Chainlink-like pull, veya zincir içi
   TWAP tabanlı?
3. **Cross-chain:** IBC-benzeri, optimistic bridge, veya zk-bridge?
4. **Stablecoin:** Native protokol parçası mı (MakerDAO tarzı CDP), yoksa
   sadece primitive sağla, topluluk mu kursun?
5. **Governance:** On-chain (stake-weighted voting) mi, off-chain mi?
6. **Tokenomics revizyon:** Sabit arz aynı kalıyor, ama PoS ödül dağıtımı
   ve "stake delegation" modeli nasıl olacak?

---

## 8. v1'den v2'ye Geçiş Stratejisi

v1 C++ kodu **referans implementasyon** olarak arşivlenir. Yeniden yazmak
yerine şunu yapıyoruz:

1. `archive/cpp-v1/` dizini oluştur, mevcut src/include/tests'i oraya taşı
2. `docs/ABSTRACT.md` ve mimari kararları koru (felsefe aynı)
3. Yeni Rust workspace'i root'ta kur
4. v1 CLAUDE.md'yi v2 için güncelle (Rust konvansiyonları)
5. v2'de her commit bir ADR'a bağlansın

---

## 9. Sonraki Adımlar (Teklif)

### Aşama 0 — Temel Temizlik & Altyapı (1 hafta)
- [ ] C++ v1'i arşive taşı
- [ ] Rust workspace iskeleti (tüm crate'ler boş)
- [ ] flake.nix Rust için güncelle
- [ ] CLAUDE.md v2'ye güncelle
- [ ] CI (GitHub Actions: fmt, clippy, test, audit)

### Aşama 1 — Kriptografi (2 hafta)
- [ ] qv-crypto: hash, Dilithium, hybrid KEM (pqcrypto-rs)
- [ ] VRF (Ouroboros Praos için)
- [ ] Property-based testler + NIST KAT

### Aşama 2 — Core UTXO + Script VM (3 hafta)
- [ ] UTXO, Transaction, Block yapıları
- [ ] Script VM (introspection + covenants)
- [ ] "Shared UTXO" örneği: basit AMM işleyen test

### Aşama 3 — Consensus (3 hafta)
- [ ] Ouroboros Praos slot lider seçimi (VRF)
- [ ] KES imzalar (forward-secure)
- [ ] Longest-chain + k-deep finality

### Aşama 4 — DeFi Primitifleri (4 hafta)
- [ ] AMM (constant product) — shared UTXO pattern
- [ ] Intent-based batch aggregator (mempool'da)
- [ ] Lending primitive (Merkle-tree UTXO)

### Aşama 5 — Gizlilik (3 hafta)
- [ ] Stealth addresses (Kyber KEM tabanlı)
- [ ] Range proof entegrasyonu (Bulletproofs başlangıç)
- [ ] STARK range proof prototip (migration path)

### Aşama 6 — Ağ & Depolama (2 hafta)
- [ ] libp2p transport (PQC KEM handshake)
- [ ] RocksDB UTXO store, block store
- [ ] Gossip protokolü

### Aşama 7 — Node Binary & Test Ağı (2 hafta)
- [ ] qv-node daemon
- [ ] qv-wallet CLI
- [ ] 3-node local testnet
- [ ] E2E entegrasyon testleri

Toplam: ~20 hafta (~5 ay) MVP için.

---

## 10. Finalize Edilmiş Kararlar (2026-04-15)

| Konu | Karar | Detay |
|---|---|---|
| **DeFi mimarisi** | **Cardano eUTXO modeli** (QuantumVault'a uyarlanmış) | ADR-002. Shared UTXO Pattern, slot-lider = batcher |
| **MEV stratejisi** | **Encrypted mempool + threshold decryption** | ADR-003. Kyber distributed KEM, komite = slot lider + N-1 validator |
| **Gizlilik** | **Opt-in**: varsayılan stealth addresses, confidential amounts privacy mode olarak | PQC range proof sorunu v2+ için ertelendi |
| **Yol haritası** | Zaman kutusu YOK, **tüm adımlar sıralı** | Aşama sıralaması korundu, süreler çıkarıldı |
| **v1 C++ kodu** | `archive/cpp-v1/` dizinine arşivlenecek | Referans olarak kalır, silme yok |
| **Tokenomics** | Sabit 21M arz korunur; PoS ödül modeli Aşama 3'te detaylandırılır | — |

**Açık tutulan kararlar** (sıra geldiğinde verilecek):
- Oracle tasarımı (validator median / TWAP / pull)
- Cross-chain (IBC-like / zk-bridge) — v2 sonrası
- Stablecoin primitive'i (native mi, topluluk mu)
- Governance (on-chain vs off-chain)
- Komite boyutu n, eşik t (ADR-003'te açık)

---

## 11. Referanslar

- [ADR-002: DeFi Architecture](./ADR/002-defi-architecture.md)
- [ADR-003: MEV Encrypted Mempool](./ADR/003-mev-encrypted-mempool.md)
- [MASTER_PLAN.md](./MASTER_PLAN.md) — sıralı görev listesi
