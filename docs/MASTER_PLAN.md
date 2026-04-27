# QuantumVault v2 — Master Execution Plan

_Tüm görevler sıralı. Süre kutusu yok. Her maddeyi tamamlayıp bir sonrakine._
_Son güncelleme: 2026-04-24 (Aşama 0-8 tamamlandı, Aşama 9 sırada)_

**Pivot kararları:** Rust, UTXO+eUTXO/Plutus-tarzı script, Ouroboros Praos PoS,
Stealth addresses (opt-in confidential amounts), Encrypted mempool (threshold Kyber).

---

## ✅ AŞAMA 0 — Pivot Temizliği ve Rust İskeleti (2026-04-15)

### 0.1 Arşivleme
- [x] `archive/cpp-v1/` dizini oluştur
- [x] Mevcut `src/`, `include/`, `tests/`, `tools/`, `cmake/` dizinlerini taşı
- [x] Mevcut `flake.nix`, `CMakeLists.txt`, `CMakePresets.json`, `.clang-*` → arşive
- [x] `archive/cpp-v1/README.md` yaz: "C++ referans implementasyon, artık geliştirilmiyor"

### 0.2 Rust Workspace Kurulumu
- [x] `Cargo.toml` workspace manifest oluştur
- [x] `rust-toolchain.toml` (stable, nightly opsiyonları)
- [x] `rustfmt.toml`, `clippy.toml` (stil)
- [x] `.cargo/config.toml` (profiller: dev, release, bench)
- [x] Boş crate'ler: `qv-crypto`, `qv-core`, `qv-script`, `qv-consensus`,
      `qv-privacy`, `qv-net`, `qv-storage`, `qv-mempool`, `qv-defi`,
      `qv-node`, `qv-wallet`, `qv-miner`, `qv-common`

### 0.3 Nix ve Build
- [x] `flake.nix` Rust için güncelle: rustc, cargo, rust-analyzer, liboqs, rocksdb
- [x] `devShell` tanımla: clippy, rustfmt, cargo-audit, cargo-deny, cargo-fuzz
- [x] `just` veya `cargo-make` ile task runner

### 0.4 CI ve Kod Kalitesi
- [x] `.github/workflows/ci.yml`: fmt, clippy -Dwarnings, test, doc-test, audit
- [x] `cargo-deny.toml`: license/banned/source policy
- [x] Pre-commit hook: fmt + clippy (husky-rs veya manual)
- [x] `codecov.io` entegrasyonu

### 0.5 Dokümantasyon Güncelleme
- [x] `CLAUDE.md` v2'ye güncelle (Rust, yeni kararlar)
- [x] `README.md` proje tanıtımı
- [x] `docs/ABSTRACT.md` değişmeden kalır (felsefe aynı)

---

## ✅ AŞAMA 1 — Kriptografi Çekirdeği (`qv-crypto`) (2026-04-15)

### 1.1 Hash Fonksiyonları
- [x] SHA3-256, SHA3-512 (RustCrypto `sha3`)
- [x] BLAKE3 (`blake3` crate)
- [x] Streaming hasher wrapper
- [x] Double-hash (Merkle için)
- [x] NIST KAT test vektörleri

### 1.2 PQC İmzalar (Dilithium / ML-DSA)
- [x] `pqcrypto-dilithium` entegrasyonu (veya `oqs-rs`)
- [x] Keypair, sign, verify API
- [x] 3 parameter set (Level 2/3/5)
- [x] Serialization (bincode veya custom)
- [x] Property testler + tamper reddi

### 1.3 Hibrit KEM
- [x] X25519 (`x25519-dalek`)
- [x] Kyber / ML-KEM (`pqcrypto-kyber`)
- [x] Transcript-bound KDF (SHA3-256 tabanlı)
- [x] Wire format: `eph_x25519_pk || kyber_ct`
- [x] Tamper tests

### 1.4 VRF (Ouroboros için)
- [ ] Pallas veya Ristretto tabanlı VRF (Cardano uyumlu)
- [ ] Veya PQC VRF araştır (lattice VRF literatürü var ama olgun değil)
- [ ] KES (Key Evolving Signatures) — forward-secure
- [ ] Slot leader proof API
> **Not:** İskelet oluşturuldu (vrf.rs, kes.rs, threshold.rs). Gerçek implementasyon ADR-004/005 sonrası.

### 1.5 Threshold Kriptografisi (ADR-003 için hazırlık)
- [ ] Shamir secret sharing (scalar)
- [ ] Threshold Kyber DKG skeleton (araştırma, ayrı ADR gerekecek)
- [ ] Komite imza şeması (Dilithium multi-signature)
> **Not:** İskelet oluşturuldu (threshold.rs). Gerçek implementasyon Aşama 7 ile paralel.

### 1.6 Secure Memory
- [x] `zeroize` crate kullan, custom `SecureBytes` tip
- [x] Compile-time checks: secret material Copy olamaz
- [x] `secrecy` crate ile `Secret<T>` wrapper

### 1.7 Benchmarkler
- [x] `criterion` ile: hash throughput, sign/verify ops/sec, KEM latency
- [x] Baseline dokümantasyonu

---

## ✅ AŞAMA 2 — Core Tipler ve Serializasyon (`qv-core`) (2026-04-16)

### 2.1 Temel Tipler
- [x] `TxId`, `BlockHash`, `Height`, `Amount`, `OutPoint`, `Timestamp`
- [x] Hex serialization, Display, FromStr
- [x] `bincode` ile binary codec (`rkyv` gereksizdi, çıkarıldı)

### 2.2 Transaction
- [x] `TxInput { prev_output, signature, witness }`
- [x] `TxOutput { value, locking_script, datum, stealth_info }` (eUTXO — datum eklendi!)
- [x] `Transaction { version, inputs, outputs, lock_time, validity_interval }`
- [x] TxId = hash(canonical_bytes)
- [x] Roundtrip testler

### 2.3 Block
- [x] `BlockHeader { prev_hash, merkle_root, utxo_commitment, slot, vrf_proof, kes_sig }`
- [x] `Block { header, transactions }`
- [x] Merkle tree (power-of-2 pad)

### 2.4 UTXO Set (in-memory, trait-based)
- [x] `UtxoSet` trait (add, spend, get, contains)
- [x] `InMemoryUtxoSet` implementation
- [x] `commitment_root()` — UTXO seti taahhüdü (sparse Merkle)

### 2.5 Protokol Parametreleri
- [x] `ProtocolParams` struct (tüm sabit değerler tek yerde)
- [x] Genesis serialization (TOML veya JSON)
- [x] Validity interval (before/after slot)

---

## ✅ AŞAMA 3 — Script VM (`qv-script`) (2026-04-17)

### 3.1 Opcode Seti Tasarımı
- [x] Stack ops: PUSH, DUP, SWAP, DROP, PICK, ROLL
- [x] Arithmetic: ADD, SUB, MUL, DIV, MOD, NEG
- [x] Compare: EQ, NEQ, LT, GT, LE, GE
- [x] Crypto: CHECKSIG_PQC, CHECKMULTISIG_PQC, HASH_SHA3, HASH_BLAKE3
- [x] Introspection: READ_INPUT_VALUE, READ_OUTPUT_VALUE, READ_OUTPUT_SCRIPT,
      READ_OUTPUT_DATUM, TX_HASH, SLOT_NUMBER
- [x] Covenant: ASSERT_OUTPUT_SCRIPT_HASH, ASSERT_DATUM_HASH, ASSERT_VALUE

### 3.2 Interpreter
- [x] Stack-based VM (Vec<Value>)
- [x] Gas/step model (DoS koruması)
- [x] Deterministik yürütme (float yok, overflow=wrap)
- [x] Hata kodları

### 3.3 Script Builder (Rust API)
- [x] `ScriptBuilder` fluent API
- [x] Standart template: `p2pkh_pqc(pubkey)`
- [x] Standart template: `multisig_pqc(m, pubkeys)`
- [x] Template: AMM swap script (covenant-only; tam invariant Aşama 9'da)

### 3.4 AMM Referans Implementasyonu
- [ ] Shared UTXO pattern: havuz = tek UTXO, datum = (x, y, fee_bps)
- [ ] Swap script: output'un x_new * y_new >= x_old * y_old * (1 - fee) invariant'ı
- [ ] Liquidity provide/withdraw scriptleri
- [ ] Integration test: 100 swap simülasyonu
> **Not:** AMM swap template covenant-only olarak yazıldı (script hash + datum varlığı).
> Tam `x*y >= x_old*y_old` invariant kontrolü datum→int dönüşüm opcode'u gerektirir,
> bu qv-defi (Aşama 9) ile gelecek.

---

## ✅ AŞAMA 4 — Consensus: Ouroboros Praos (`qv-consensus`) (2026-04-17)

### 4.1 Slot ve Epoch Mekaniği
- [x] `SlotClock` — slot↔epoch mapping, slot↔wall-clock, `SlotInfo`
- [x] Slot süresi parametresi (2 sn), epoch uzunluğu (21600 slot = 12 saat)
- [x] `EpochNonce` — evolving nonce (SHA3-256 chain), `EpochInfo`, `EpochBoundary`

### 4.2 Slot Leader Seçimi
- [x] `VrfEvaluator` trait + `TestVrf` (deterministik mock)
- [x] Praos threshold: `T = 1 − (1−f)^σ`, `ACTIVE_SLOT_COEFF = 0.05`
- [x] `check_leadership()`, `verify_leadership()`, `vrf_input()` (domain-separated)
- [x] Epoch randomness (nonce) — SHA3(prev_nonce || vrf_entropy || boundary_hash)

### 4.3 Blok Üretimi ve Doğrulama
- [x] `validate_block_header()`: 7 check (version, prev_hash, slot mono, height, timestamp, VRF, KES)
- [x] `KesVerifier` trait + `TestKesVerifier`
- [x] Fork choice: longest-chain, tie-break by lower hash
- [x] k-deep finality (k=50), `is_final()`, `finality_height()`
- [x] `ChainState`: in-memory BTreeMap chain index, `add_block()`, `ancestors()`, `chain_density()`

### 4.4 Stake Delegation
- [x] `StakePool` (id, vrf_key, kes_key, pledge, margin, fixed_cost)
- [x] `Delegation` (delegator→pool mapping)
- [x] `StakeDistribution::snapshot()` — epoch-frozen per-pool stake (BTreeMap, deterministic)

### 4.5 Ödüller
- [x] `block_subsidy()` — Bitcoin halving: `initial >> halvings`
- [x] `cumulative_emission()`, `is_emission_exhausted()`, supply cap koruması
- [x] `distribute_reward()` — fixed_cost + margin + pro-rata delegator split, dust → operator
- [ ] Çift imza tespiti (slashable) — gelecek aşamada (Aşama 12 ile)

### 4.6 Testler
- [x] 67 unit test + 12 integration test
- [x] Simülatör: 1000 slot, 10 stake pool, multi-epoch
- [x] Adversarial: %30 stake saldırgan, fairness istatistikleri (50k slot)
- [x] Fork resolution, finality guarantee, nonce chain, reward conservation

---

## ✅ AŞAMA 5 — Depolama Katmanı (`qv-storage`)

### 5.1 Backend Abstraction
- [x] `KvStore` trait (get/put/delete/scan_prefix/batch)
- [x] RocksDB implementation (`RocksKvStore`)
- [x] `redb` (pure Rust) fallback (`RedbKvStore`)
- [x] In-memory backend (`MemoryKvStore`) — test/simülasyon

### 5.2 Block Store
- [x] put_block, get_block, get_block_by_height
- [x] Header-only retrieval (light client) — `get_header_by_height`
- [x] Height + hash secondary index, duplicate rejection

### 5.3 UTXO Store (persistent)
- [x] Batch apply (connect block), batch revert (disconnect block)
- [x] Merkle commitment root (sorted BTreeMap + SHA3-256 leaf hash)
- [x] Snapshot / rollback (`create_snapshot`, `restore_snapshot`, `rollback_to_snapshot`)
- [x] Intra-block chained spending (staged_new pattern)
- [x] Double-spend detection

### 5.4 Chain State
- [x] Ledger state: stake pools, delegations, reward balances
- [x] Epoch snapshot persistence + `latest_epoch_snapshot()` scan
- [x] Chain entry CRUD + tip hash tracking

### 5.5 Test Coverage
- [x] 14 unit test (kv: 5, block: 4, utxo: 3, state: 3) — redb dahil
- [x] 12 integration test: cross-module flow, multi-block apply/revert, commitment stability, snapshot, namespace isolation, intra-block chaining, 100-block stress

---

## ✅ AŞAMA 6 — Ağ Katmanı (`qv-net`)

### 6.1 Transport
- [x] `libp2p` yapılandırması (TCP + Noise XX + Yamux)
- [x] `TransportConfig` presets (mainnet/testnet/ephemeral)
- [x] `NodeIdentity` (Ed25519 keypair wrapper)
- [x] Peer discovery: Kademlia DHT (`kad::Behaviour<MemoryStore>`)
- [x] Identify protocol ile peer metadata değişimi
- [ ] Hybrid KEM handshake (X25519 + Kyber) — beklemede, `snow`/libp2p pluggable KEM desteği gerekiyor

### 6.2 Gossip
- [x] `libp2p-gossipsub` entegrasyonu (`build_gossipsub` builder)
- [x] 4 topic: `/qv/blocks/1`, `/qv/tx/1`, `/qv/vrf/1`, `/qv/votes/1`
- [x] SHA3-256 content-addressed deduplication (`SeenCache`)
- [x] `GossipConfig` presets (mainnet/testnet/ephemeral)

### 6.3 Mesaj Protokolü
- [x] `NetworkMessage` enum (9 variant: Block, Transaction, VrfProof, Vote, GetHeaders, Headers, GetBlocks, Ping, Pong)
- [x] `Envelope` wire format (version tag + bincode payload)
- [x] Protocol versioning (`PROTOCOL_VERSION = 1`, mismatch rejection)
- [x] Size limits (`MAX_MESSAGE_SIZE = 4MiB`)
- [x] `RateLimiter` (per-peer token bucket)

### 6.4 Node Orchestrator
- [x] `QvBehaviour` composite: GossipSub + Kademlia + Identify + Ping
- [x] `NetworkNode`: Swarm construction, event loop, publish/subscribe
- [x] `NetEvent` channel (Message, PeerConnected, PeerDisconnected)
- [x] `PeerStore` + `PeerInfo` (reputation, ban/evict, idle eviction)

### 6.5 Test Coverage
- [x] 22 unit test (peer: 5, message: 7, transport: 5, gossip: 4, node: 4, lib: 3)
- [x] 12 integration test: message roundtrips, topic routing, peer lifecycle, rate limiter, dedup, config validation, node construction

---

## ✅ AŞAMA 7 — Mempool + Encrypted Pool (`qv-mempool`)

### 7.1 Clear Mempool
- [x] Fee-sorted priority queue (`ClearPool`, fee-density + FIFO + hash tiebreak)
- [x] UTXO dependency tracking (spent_outpoints map, double-spend detection)
- [x] Eviction policy: age-based (`max_age_secs`), capacity-based (lowest-fee eviction)
- [x] `ClearPoolConfig` presets (mainnet/testnet/ephemeral)
- [x] `get_batch()`, `remove_confirmed()`, `all_sorted()` API

### 7.2 Encrypted Mempool (ADR-003)
- [x] `EncryptedTx` wrapper (kem_ciphertext, encrypted_body, target_epoch)
- [x] `EncryptedPool` (epoch-scoped, capacity-limited, epoch advance flush)
- [x] `ThresholdDecryptor` trait (pluggable DKG/decrypt backend)
- [x] `MockThresholdDecryptor` (XOR-based test mock)
- [x] `decrypt_batch()` — bulk threshold decryption with skip-on-error
- [x] `DecryptionShare` struct for committee share collection
- [ ] Real threshold Kyber DKG — beklemede (openfhe-rust veya custom lattice impl)

### 7.3 Deterministic Ordering
- [x] `OrderKey` (fee_density DESC, timestamp ASC, tx_id ASC)
- [x] `deterministic_sort()` canonical sorter
- [x] `verify_order()` verifier (validators re-derive and check)

### 7.4 Batcher Logic
- [x] `OrderIntent` (order UTXO datum decoder: pool_id, direction, offer, min_receive)
- [x] `build_amm_batch()` — constant-product AMM executor with 0.3% fee
- [x] `SlashingEvidence` struct + `is_valid()` verifier
- [x] Deterministic order within batch (canonical sort applied before execution)

### 7.5 Test Coverage
- [x] 24 unit test (clear: 8, ordering: 6, encrypted: 7, batcher: 6, lib: 3)
- [x] 12 integration test: clear→ordering pipeline, double-spend prevention, encrypted decrypt roundtrip, epoch lifecycle, AMM multi-order, invariant verification, slashing evidence, capacity eviction, deterministic ordering, full pipeline, dependency cleanup, encrypted→ordering

---

## AŞAMA 8 — Gizlilik (`qv-privacy`)

### 8.1 Stealth Addresses ✅ (2026-04-24)
- [x] StealthKeys { view_kyber_pk, spend_dilithium_pk }
- [x] `create_stealth_output()`: gönderen Kyber KEM encapsulate → shared secret → view tag + onetime pk hash
- [x] `scan_output()`: alıcı Kyber decapsulate → view tag pre-filter → pk hash doğrulama
- [x] `recover_spend_key()`: SpendKeyDeriver trait + MockSpendKeyDeriver

### 8.2 Opt-in Confidential Amounts ✅ (2026-04-24)
- [x] `ConfidentialAmount`: `Plain(u64)` | `Confidential(Commitment, RangeProof)`
- [x] Committer trait + MockCommitter (SHA3-256 tabanlı Pedersen mock)
- [x] RangeProver/RangeVerifier traitlari + MockRangeProver/MockRangeVerifier
- [x] verify_balance_mock() — karışık plain+confidential balance doğrulama
- [x] Uyarı: PQC değil, opsiyonel; PrivacyMode::Full ile aktif

### 8.3 View Key Mekanizması ✅ (2026-04-24)
- [x] ViewKey: Kyber hybrid keypair export — audit için 3. taraf tarama
- [x] DisclosureProof: per-output seçici ifşa (shared_secret + amount? + blinding?)
- [x] PrivacyMode enum: StealthOnly | Full | Transparent

### 8.4 STARK Range Proof (gelecek için altyapı)
- [ ] Prototip: `winterfell` ile range proof
- [ ] Performans baseline
- [ ] Migration plan dokümanı
- Not: trait altyapısı hazır (RangeProver/RangeVerifier), winterfell entegrasyonu gelecek

---

## AŞAMA 9 — DeFi Primitifleri (`qv-defi`)

### 9.1 AMM (Constant Product)
- [ ] Pool UTXO data model
- [ ] Swap entry-point (intent üretimi)
- [ ] Liquidity add/remove
- [ ] LP token modeli (stealth-aware)

### 9.2 Lending (Basit)
- [ ] Single-pool borrow/lend
- [ ] Interest rate model (linear)
- [ ] Collateralization ratio
- [ ] Liquidation flow

### 9.3 Oracle
- [ ] Validator median fiyat (stake pool'lar imzalı fiyat yayınlar)
- [ ] TWAP hesaplama (havuz rezervlerinden)
- [ ] Oracle manipulation detection

### 9.4 Intent-based Order System
- [ ] Order UTXO schema
- [ ] Cüzdan tarafı intent SDK
- [ ] Test: 10k intent/sn batch simülasyonu

---

## AŞAMA 10 — Node Binary (`qv-node`)

### 10.1 Daemon
- [ ] CLI (clap) — config file, data dir, ports
- [ ] Tokio runtime
- [ ] Component wiring: storage, net, consensus, mempool, rpc
- [ ] Graceful shutdown (SIGINT)

### 10.2 RPC
- [ ] JSON-RPC 2.0 server (`jsonrpsee`)
- [ ] Metodlar: getblock, gettx, sendtx, getutxo, getbalance, scan_stealth
- [ ] WebSocket subscriptions (new blocks, new tx)

### 10.3 Metrics ve Observability
- [ ] `metrics` crate + Prometheus endpoint
- [ ] Structured logs (`tracing` + jaeger)

---

## AŞAMA 11 — Wallet (`qv-wallet`)

### 11.1 Key Yönetimi
- [ ] BIP-39 benzeri mnemonic (24 kelime, PQC key tohumu için)
- [ ] HD wallet — Dilithium anahtar türetme
- [ ] Encrypted keystore (Argon2id + AES-256-GCM)

### 11.2 Stealth Tarama
- [ ] Arka plan tarama daemon'u
- [ ] İşaretli UTXO'ları yerel DB'ye yaz

### 11.3 Transaction Building
- [ ] Coin selection (UTXO seçimi)
- [ ] Script derleme (standart template'ler için)
- [ ] PQC imzalama
- [ ] Gönder via RPC veya doğrudan gossip

### 11.4 DeFi Entegrasyonu
- [ ] Swap intent oluştur (AMM)
- [ ] LP pozisyonu yönet
- [ ] Lending pozisyonu izle

---

## AŞAMA 12 — Stake Pool Operator (`qv-miner`)

### 12.1 Pool Registrasyonu
- [ ] Pool kayıt tx'i üretme
- [ ] Operator anahtar yönetimi (KES)
- [ ] Delegator listesi

### 12.2 Block Production
- [ ] Slot lider kontrolü (VRF her slot)
- [ ] Mempool'dan tx seçimi
- [ ] Encrypted mempool decryption (komite üyesi olarak)
- [ ] Blok imzalama ve gossip

### 12.3 Dashboard
- [ ] TUI (`ratatui`) — canlı slot/epoch, ödül geçmişi

---

## AŞAMA 13 — Testnet ve E2E

### 13.1 Local Devnet
- [ ] `docker-compose.yml` — 3 node + explorer
- [ ] Genesis üretme aracı
- [ ] Faucet

### 13.2 E2E Senaryolar
- [ ] Simple transfer (Alice → Bob)
- [ ] Stealth transfer (scan ve spend)
- [ ] AMM swap (two-leg)
- [ ] Lending lifecycle (deposit, borrow, repay, withdraw)
- [ ] Consensus fork resolution
- [ ] Encrypted mempool round-trip

### 13.3 Public Testnet
- [ ] Milestone gate (Aşamalar 0-12 yeşil)
- [ ] Explorer (basit, read-only)
- [ ] Seed node listesi, bootstrap config

---

## AŞAMA 14 — Güvenlik Sıkılaştırma

### 14.1 Audit Hazırlığı
- [ ] Her crate için threat model dokümanı
- [ ] Fuzz testleri: tx parser, script VM, network messages
- [ ] `cargo-fuzz` ile 24 saat sürekli fuzz çalıştır

### 14.2 Performans
- [ ] Kritik path profilleme (perf, flamegraph)
- [ ] Allocation hot paths azaltma (arena alloc)
- [ ] Block validation < 500ms hedefi

### 14.3 Dış Audit
- [ ] Kriptografi audit (Trail of Bits, NCC Group vs.)
- [ ] Konsensüs audit (Runtime Verification vs.)
- [ ] Bug bounty başlatma

---

## AŞAMA 15 — Mainnet Launch

### 15.1 Genesis Ceremonisi
- [ ] Trusted setup (threshold Kyber DKG)
- [ ] Initial stake dağılımı (airdrop / pre-sale modeline göre)
- [ ] Genesis block

### 15.2 Dokümantasyon
- [ ] API docs (rustdoc + mdBook)
- [ ] Kullanıcı kılavuzu
- [ ] Validator kılavuzu
- [ ] DeFi geliştirici SDK

### 15.3 Topluluk
- [ ] Discord, forum, blog
- [ ] Geliştirici grant programı
- [ ] İlk DeFi dApp partnerliği

---

## Yardımcı: Her Aşama İçin Genel "Definition of Done"

Bir aşama **bitti** sayılır, eğer:
- Tüm public API'ler doc comment'li
- `cargo fmt` ve `cargo clippy -D warnings` temiz
- Unit test kapsaması > %80 (kritik crate'ler için %90)
- Integration testler geçiyor
- Benchmark baseline kaydedilmiş
- `cargo-audit` ve `cargo-deny` temiz
- ADR referansları güncel
