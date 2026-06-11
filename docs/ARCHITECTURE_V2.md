# QuantumVault v2 — Mimari Yol Haritası

_Pivot tarihi: 2026-04-15 — Son güncelleme: 2026-06-10 (durum notları + referans onarımı)_

Bu belge v1'den (C++ iskelet) v2'ye (Rust, DeFi odaklı, UTXO+Covenants,
Ouroboros tarzı PoS, Stealth+Confidential Amounts) geçişin gerekçelerini ve
getirdiği yeni mühendislik zorluklarını kayıt altına alır.

> **Not (2026-06-10):** Bu belge bir **gerekçe/yol haritası kaydıdır**; sistemin
> güncel snapshot'ı için `docs/SYSTEM_OVERVIEW.md`, açık işler için
> `docs/ROADMAP.md` esastır. Pivot sonrası başlıca kilometre taşları: CI tamamen
> yeşil + 735 test (2026-05-14), hibrit X25519+Kyber handshake uygulandı
> (ADR-007, 2026-05-15), 4-node libp2p devnet (2026-05-21), stealth uçtan uca +
> sighash (ADR-011/012, 2026-05-22), multi-tenant cüzdan UI (2026-06-03),
> binary release pipeline + Faz 6 DeFi başlangıcı (2026-06-05).

---

## 1. Kararlar Özeti

| Katman | v1 | v2 | Değişiklik Gerekçesi |
|---|---|---|---|
| Dil | C++20 | **Rust (stable)** | Bellek güvenliği + olgun async ekosistem + blokzincir camiası |
| Durum Modeli | UTXO + CSV | **UTXO + Covenants** | CSV felsefesini koru; DeFi'yi gelişmiş script'lerle in'a et |
| Konsensüs | Hibrit PoW+PoS | **Nakamoto PoS (Ouroboros Praos)** | DeFi için daha düşük gecikme, enerjisiz, kanıtlanabilir güvenlik |
| Gizlilik | Stealth addresses | **Stealth + Confidential Amounts** | Bireysel pozisyon gizli, havuz şeffaf → DeFi uyumlu |
| Kriptografi | X25519+Kyber, Dilithium, liboqs | **ML-DSA (FIPS 204) + Kyber + X25519 + Ristretto255-VRF + Sum-KES** (RustCrypto saf-Rust; ADR-006 ile liboqs/oqs-rs bırakıldı) | Wire format FIPS 204 final spec; tek crate (`ml-dsa`); C bağımlılığı yok |
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
- **Epoch uzunluğu:** 12 saat (21 600 slot — 12 × 3600 / 2)
- **Slot lider seçimi:** VRF ile stake-orantılı
- **k-deep finality:** k=50 blok (~100 saniye olasılıksal finalite)
- **Forward-secure imzalar:** uzun menzilli saldırılara karşı (KES)

Ouroboros'un avantajı: saf longest-chain kuralı + VRF, BFT komite kurulumu
gerektirmiyor. UTXO + Nakamoto ile doğal uyum.

**Açık soru:** k-deep finality DeFi için yeterli mi, yoksa checkpoint
kesinleşmesi (Byzantine fault tolerant overlay) eklemeli miyiz?
_(2026-06-10 itibarıyla hâlâ açık — ROADMAP'te "BFT finality gadget" önerisi
olarak takipte. Yapışkan/monoton k-deep finalite ve genesis maxvalid-bg çekirdeği
ise ADR-008 ile uygulandı.)_

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

> **Güncelleme (2026-06-10):** Bu alternatif seçildi (bkz. §10) ve stealth
> tarafı **uygulandı** — ADR-011 ile uçtan uca entegrasyon (Faz 1-5, 2026-05-22):
> stealth çıktı üretimi, `qv_scanStealth` taraması, harcama, `qvst1`/`qvfp1`
> adres formatı, cüzdan UI. Confidential amounts hâlâ mock backend (P-01);
> STARK migration path planı geçerli.

---

## 5. Rust Ekosistem Entegrasyonu

Sıfırdan bespoke Rust seçtin — framework yok. Kullanılacak kütüphaneler:

### Core
- **tokio** — async runtime
- **rust-libp2p** — P2P ağ (production-ready, Ethereum Lighthouse kullanıyor)
- **rocksdb** veya **redb** — depolama (redb: pure Rust, daha az bağımlılık).
  _2026-05-21'den beri rocksdb opsiyonel, varsayılan-kapalı feature; node
  MemoryKvStore + redb kalıcılık kullanıyor (C++ toolchain/libclang bağımlılığı kalktı)_
- **prost** veya **rkyv** — serializasyon (rkyv zero-copy, daha hızlı)
- **tracing** — structured logging
- **rayon** — paralel işlem

### Kriptografi
- **ml-dsa** (RustCrypto, FIPS 204 final) — PQC imza, ADR-006 ile seçildi (`pqcrypto-dilithium` 2026-05-07'de kaldırıldı; wire-uyumsuzdu)
- **pqcrypto-kyber** — ML-KEM (KEM tarafı; saf-Rust ml-kem alternatifi gelecekte)
- **x25519-dalek** — klasik X25519, hibrit KEM için
- **ed25519-dalek** — libp2p `NodeIdentity` için (VRF için **değil**)
- **schnorrkel** (Ristretto255-VRF) — Ouroboros Praos slot lider seçimi, ADR-004
- **merlin** — transcript için (schnorrkel bağımlılığı)
- **sha3** — RustCrypto, FIPS uyumlu (transaction id, Merkle, blok hash)
- **blake3** — native Rust hash (sıcak yol streaming)
- **argon2 + aes-gcm** — keystore (wallet + miner, M-04)

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
├── Cargo.toml                 # workspace root (13 member crate)
├── flake.nix
├── rust-toolchain.toml        # rustc pin (stable 1.78+)
├── crates/
│   ├── qv-common/             # paylaşılan tipler, error wrap'leri
│   ├── qv-crypto/             # hash, PQC sign (ml-dsa), hybrid KEM, VRF (schnorrkel), KES, threshold
│   ├── qv-core/               # UTXO, transaction, block, Merkle, protocol params
│   ├── qv-script/             # Covenant script VM (stack-based, 57 opcode, gas-limited)
│   ├── qv-consensus/          # Ouroboros Praos (slot, epoch, leader, finality, rewards)
│   ├── qv-privacy/            # Stealth addresses + opt-in confidential amounts
│   ├── qv-storage/            # RocksDB/redb/memory KV abstraction, UTXO set, undo log
│   ├── qv-net/                # libp2p transport, GossipSub, peer management
│   ├── qv-mempool/            # Clear + encrypted mempool, deterministic ordering, AMM batcher
│   ├── qv-defi/               # AMM, lending, oracle, intents
│   ├── qv-node/               # Full node binary (RPC, validation, slot ticker, ceremony)
│   ├── qv-wallet/             # CLI wallet binary (mnemonic, keystore, tx build, send)
│   └── qv-miner/              # Stake pool operator binary (VRF leader, KES sign, block producer)
├── fuzz/
├── benches/
├── spikes/                    # one-off API verification scratch projects (excluded)
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

> **Durum (2026-06-10):** Bu liste 2026-04-15'teki tarihsel tekliftir; güncel
> faz takibi `docs/ROADMAP.md`'dedir. Kabaca: Aşama 0-3 ✅; Aşama 5'in stealth
> kısmı ✅ (ADR-011), Bulletproofs mock (P-01); Aşama 6'nın "PQC KEM handshake"
> maddesi ✅ (ADR-007, hibrit X25519+Kyber, 2026-05-15); Aşama 7'nin "3-node
> local testnet" hedefi **4-node libp2p devnet** olarak gerçekleşti (2026-05-21,
> round-robin lider) + cüzdan HTTP API/UI ve binary release pipeline eklendi;
> Aşama 4 (DeFi primitifleri) Faz 6 olarak 2026-06-05'te başladı (D-1
> `ReadInputDatum` ✅, D-2 AMM kovenant sırada).

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
- [ADR-007: Hibrit X25519+Kyber Handshake](./ADR/007-hybrid-handshake.md) — uygulandı 2026-05-15
- [ADR-011: Stealth Adres Entegrasyonu](./ADR/011-stealth-address-integration.md) — Faz 1-5 uygulandı
- [ADR-012: İşlem Sighash'i](./ADR/012-transaction-sighash.md) — uygulandı
- [ROADMAP.md](./ROADMAP.md) — faz planı + Placeholder ve Mock Envanteri
  (eski `MASTER_PLAN.md` `archive/docs-v1/` altına arşivlendi)
- [SYSTEM_OVERVIEW.md](./SYSTEM_OVERVIEW.md) — sistemin güncel snapshot'ı
