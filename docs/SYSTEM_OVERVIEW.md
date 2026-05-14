# QuantumVault L1 — Sistem Genel Bakışı

**Doküman tarihi:** 2026-05-12 (M-04 sonrası, devnet smoke öncesi)
**Son senkronizasyon:** 2026-05-12 — Tier 1+2 doc çelişkileri çözüldü (ABSTRACT, ARCHITECTURE_V2, PROJECT_STATUS, MEMORY)
**Yazılım sürümü:** 0.1.0 (workspace), Rust 1.78+ stable, edition 2021
**Hedef okuyucu:** Projeyi ilk kez inceleyenler + uzun aradan sonra dönen geliştiriciler
**Bu doküman ne değildir:** ADR (`docs/ADR/`), faz planı (`docs/MASTER_PLAN.md`),
veya günlük açık işler listesi (`docs/ROADMAP.md`) — bunlar ayrı doküman. Burası
**sistemin tutarlı bir snapshot'ı**.

---

## 0. Üç Cümlede QuantumVault

QuantumVault L1, kuantum sonrası kriptografi üzerine kurulmuş, UTXO tabanlı,
DeFi-uyumlu bir Katman 1 blokzincirdir. Ouroboros Praos PoS mutabakatı (saf PoS,
hibrit değil), Cardano eUTXO tarzı datum-validator script modeli, opt-in gizlilik
(stealth adresler + confidential amounts), ve encrypted mempool ile MEV koruması
sunar. v1 C++ implementasyonu 2026-04-15'te Rust'a pivot edildi; aktif geliştirme
`crates/` altında 13 crate'lik bir Cargo workspace olarak yürüyor.

---

## 1. Mevcut Olgunluk Seviyesi (2026-05-12)

| Metrik | Değer |
|---|---|
| Workspace derleme | ✅ 0 error, **0 warning**, 13/13 crate (D cilalama 2026-05-12) |
| Test suite | ✅ **744 passed / 0 failed / 36 ignored** (2026-05-12, post B+C: M-09 core + K-06 wire) |
| Devnet smoke test | ✅ İlk gerçek uçtan uca transfer (block height=18, tx_count=1, ~4.9s latency) |
| Çalışan binary | qv-node (kısmen), qv-wallet (init/send/address çalışır), qv-miner (kabuk + keystore) |
| In-memory devnet uçtan uca | ✅ `tests/transfer_e2e.rs` geçer |
| Mainnet hazırlığı | ❌ değil; testnet'e bile değil |

**Üretim-seviyesinde tamam olanlar:**
- Post-quantum imza (FIPS 204 ML-DSA via `ml-dsa = 0.0.4`)
- Ristretto255-VRF (`schnorrkel`) ile slot leader election
- Sum-KES (depth 11, N=2048) on Dilithium L3 — forward-secure blok imzası
- Argon2id + AES-256-GCM keystore (wallet + miner)
- BIP-39 mnemonic, HD wallet derivation, P2PKH-PQC script template
- 13 crate'lik temiz separation of concerns

**Henüz mock/stub veya yarım:**
- Hibrit X25519+Kyber handshake (sadece klasik Noise XX şu an)
- Bulletproofs gerçek backend (mock impl yerinde)
- Mainnet genesis ceremony tool-side (modül var, mainnet'e bağlamıyor)
- Miner daemon ana loop (`cmd_run` hâlâ `sleep(u64::MAX)`)
- Encrypted mempool decrypt akışı miner tarafında
- RPC `get_balance_for` + `scan_stealth` (stub)
- AMM batch zincirinin block producer'la entegrasyonu

Tam açık iş envanteri için **`docs/ROADMAP.md` → "Placeholder ve Mock Envanteri"**
bölümüne bak (A–L grupları, ID'li).

---

## 2. Mimari Karar Özetleri (ADR'ler)

Tam metni `docs/ADR/`'de, burada bir nefeste:

| ADR | Konu | Karar |
|---|---|---|
| 001 | Test framework | `cargo-nextest` + `proptest` (unit) + `criterion` (bench) |
| 002 | DeFi mimarisi | Cardano eUTXO datum-validator pattern, shared UTXO + script invariant (x·y=k) |
| 003 | MEV stratejisi | Encrypted mempool — threshold Kyber, t-of-n committee, deterministic batch |
| 004 | VRF | Ristretto255-VRF via `schnorrkel = 0.11` (IETF draft, Cardano-uyumlu) |
| 005 | KES | Sum-KES (MMM construction) on Dilithium L3, depth=11, N=2048 period (~12 saat × 4 epoch) |
| 006 | İmza backend | `ml-dsa = 0.0.4` (FIPS 204 final). `pqcrypto-dilithium 0.5` (NIST round-3) tamamen kaldırıldı (wire-uyumsuz) |

Bunlar **değiştirilemez** — ek karar gerektirirse yeni ADR yazılır. ADR-001..003 v1
döneminden, ADR-004..006 v2 Rust pivotundan sonra yazıldı (2026-05-06/07).

---

## 3. Crate Haritası — 13 Crate'lik Yığın

```
crates/
├── qv-common/       # Paylaşılan tipler, error wrap'leri
├── qv-crypto/       # Hash, PQC sign, hybrid KEM, VRF, KES, threshold
├── qv-core/         # UTXO, Transaction, Block, Merkle, protocol params
├── qv-script/       # Script VM (opcode, interpreter, templates, gas)
├── qv-consensus/    # Ouroboros Praos (slot, epoch, leader, finality, rewards)
├── qv-privacy/      # Stealth addresses + opt-in confidential amounts
├── qv-storage/      # RocksDB/redb/memory kv backend, block store, UTXO store
├── qv-net/          # libp2p transport, gossip topics, peer management
├── qv-mempool/      # Clear + encrypted mempool, deterministic ordering, batcher
├── qv-defi/         # AMM, lending, oracle, intents
├── qv-node/         # Full node binary (RPC, validation, slot ticker, ceremony)
├── qv-wallet/       # CLI wallet binary (mnemonic, keystore, tx build, send)
└── qv-miner/        # Stake pool operator binary (VRF leader, block producer, KES)
```

Bağımlılık yönü tek yönlü, yukarıdan aşağı doğru (qv-node, qv-wallet, qv-miner
üst binary'ler; qv-common + qv-crypto + qv-core en alttaki primitifler).

---

## 4. Kriptografi Yığını (qv-crypto)

Tüm kriptografik primitifler bir crate altında — projenin geri kalanı asla
doğrudan `pqcrypto-*`, `ml-dsa`, `schnorrkel` çağırmıyor; qv-crypto'nun
opaque yüzünden geçiyor.

**Hash.** SHA3-256 ve BLAKE3. SHA3 standart (transaction id, blok hash,
Merkle), BLAKE3 stream-friendly hot path'ler için. `qv_crypto::sha3_256(bytes)
-> [u8;32]` ve `Hasher` streaming API.

**PQC İmza — FIPS 204 ML-DSA.** Üç güvenlik seviyesi: ML-DSA-44 (NIST cat 2),
ML-DSA-65 (cat 3, default), ML-DSA-87 (cat 5). Boyutlar:

| Level | pk | sk | sig |
|---|---:|---:|---:|
| 2 (44) | 1312 | 2560 | 2420 |
| 3 (65) ⭐ | 1952 | 4032 | 3309 |
| 5 (87) | 2592 | 4896 | 4627 |

API: `generate_pqc_keypair`, `from_seed_pqc`, `sign_pqc`, `verify_pqc`. `PqcKeyPair`,
`PqcPublicKey`, `PqcSecretKey`, `PqcSignature` opaque newtype'lar; `PqcSecretKey`
`SecureBytes` ile zeroize-on-drop. **`from_seed_pqc` deterministik** (`KeyGen_internal(ξ)`
FIPS 204 §6.1) — bu HD wallet, KES leaf, stealth recovery için kritik.

**Hibrit KEM — X25519 + Kyber.** Üç Kyber seviyesi (512/768/1024). `HybridKeyPair`
hem X25519 hem Kyber'i içerir; `encapsulate_hybrid` her ikisinden de shared secret
türetir, HKDF ile birleştirir. Bu yapı **klasik backdoor + PQC backdoor**'a
karşı sigorta — birinden biri kırılırsa diğeri tutar. Kullanım: stealth address
view key, gelecekteki noise handshake (NET-01 hâlâ açık).

**VRF — Ristretto255-VRF.** `schnorrkel` crate'i üzerinden. `VrfKeyPair::generate`
veya `::from_seed`, `evaluate(msg) -> (output, proof)`, `verify(pk, msg, proof) -> output`.
Output 32 byte, proof 96 byte (32 byte pre-out + 64 byte schnorr proof). ADR-004.

**KES — Sum-KES on Dilithium L3.** `kes_generate(master_seed) -> (KesPublicKey, KesSecretKey)`,
`kes_sign(sk, period, msg) -> KesSignature`, `kes_verify(pk, sig, msg) -> bool`,
`kes_evolve(sk) -> ()`. Master seed'den 2048 leaf seed pre-derive edilir (~2s on
commodity HW); her leaf bir Dilithium keypair'ine eşlenir; Merkle root = KesPublicKey
(32 byte). Period başına bir Dilithium sig + Merkle path. `evolve` mevcut leaf seed'i
zeroize eder — forward security. ADR-005.

**Threshold.** Shamir secret sharing, Pedersen DKG, Feldman VSS, ElGamal-style
threshold decryption. Encrypted mempool için (t-of-n committee). Pedersen + Feldman
testleri şu an `#[ignore]` (T-01 envanter: verification asymmetry).

**SecureBytes.** Zeroize-on-drop'lu `Vec<u8>` wrapper. Constant-time eşitlik,
debug'ta içerik sızdırmaz. Tüm PQC secret key'leri ve KES leaf seed'leri bunu kullanır.

---

## 5. UTXO Modeli (qv-core)

Account modeli **bilinçli olarak reddedildi** — paralellik ve gizlilik için UTXO daha uygun.

**Transaction.**
```
Transaction {
    inputs:  Vec<TxInput  { prev_outpoint, ... }>,
    outputs: Vec<TxOutput {
        value:        Amount,
        script:       Script,                     // locking script
        datum:        Option<Vec<u8>>,            // eUTXO datum
        stealth_info: Option<StealthOutputInfo>,  // gizlilik opsiyonel
    }>,
    validity_interval: ValidityInterval,         // from/to slot
}
```

Cardano eUTXO ile aynı: UTXO'yu harcamak için script'i çalıştırırsın, script
`OK` derse harcama geçerli. **Akıllı kontratlar L1'de yürütülmez** — Script VM
sadece "bu UTXO harcanabilir mi?" sorusuna evet/hayır verir. Tüm "off-chain
hesaplama → on-chain validation" pattern'i.

**TxId.** SHA3-256 of canonical bincode encoding. Deterministik, validity_interval
dahil her field'ı hash'ler.

**Block.**
```
BlockHeader {
    version, prev_hash, height, slot, timestamp,
    merkle_root: SHA3-256-binary-Merkle(txids),
    utxo_commitment: post-apply UTXO snapshot hash,  // K-03 hâlâ ZERO
    vrf_proof: 96 bytes,
    kes_sig: bincode(KesSignature),
    producer_key_hash: SHA3-256(producer_pk),
}
Block { header, transactions }
```

`utxo_commitment` placeholder (K-03/K-05 envanter); gerçek değer block apply
sonrası UTXO set'inin merkle/sparse-merkle hash'i olacak.

**ProtocolParams.** Mainnet/testnet/ephemeral preset'ler. Slot 2 saniye, epoch
12 saat (21600 slot), max block size 4 MiB, max tx size 1 MiB, finality k=50.
Toplam arz 21M (Bitcoin model), halving Bitcoin tarzı. TOML/JSON serileşir.

---

## 6. Script VM (qv-script)

Stack-based, deterministik, gas-limitli. Float yok, overflow=wrap, indexing
out-of-bounds → script fail.

**Opcode kategorileri:**
- **Stack ops** (0x10–0x17): Dup, Swap, Drop, Pick, Roll, Over, Rot, Dup2
- **Aritmetik** (0x20–0x2F): Add, Sub, Mul, Div, Mod, Neg, …
- **Karşılaştırma** (0x30–0x3F): Eq, Neq, Lt, Gt, Le, Ge
- **Mantık** (0x40–0x4F): If, Else, EndIf, Not
- **Kripto** (0x50–0x5F): Hash (SHA3), HashBlake3, CheckSigPqc
- **Introspection** (0x60–0x6F): Slot, InputCount, OutputCount, ReadInput, ReadOutput
- **Covenants** (0x70–0x7F): AssertScriptHash, AssertDatumHash, AssertValue
- **Data** (0x80–0x8F): Cat, Slice, Len
- **Meta**: Push1, Push2, Push4, PushInt, Nop, Return

Toplam **57 opcode**. `OpCode::COUNT = 57`.

**Templates (`templates.rs`).** Standart locking script'ler:
- `p2pkh_pqc(pubkey_hash)` — pay-to-public-key-hash, Dilithium imza doğrulama
- `multisig_pqc(threshold, pubkey_hashes)` — t-of-n PQC multisig
- `amm_swap_covenant(pool_hash)` — AMM swap script invariant
- `lending_repay_covenant(...)` — lending pozisyonu repay validation

**Gas modeli.** Her opcode'un sabit gas maliyeti. Default limit 100k. Crypto
opcode'ları (CheckSigPqc, Hash*) pahalı (1000+ gas). Limit aşıldığında script abort.

---

## 7. Konsensus — Ouroboros Praos (qv-consensus)

**Saf PoS, hibrit değil.** v1'de hibrit PoW+PoS denenmişti ama v2 pivotunda
bırakıldı. Tüm sistem stake-tabanlı.

**Slot.** Sabit 2 saniyelik aralıklar. Slot 0 = genesis. `Slot` newtype u64.

**Epoch.** 21600 slot = 12 saat. `EpochInfo` hangi slot hangi epoch'a
ait, epoch'un ilk/son slot'u nedir hesaplar.

**Epoch nonce evolution.** Her epoch'un başında `nonce_{e+1} = SHA3-256(nonce_e || VRF outputs from first 2/3 of epoch)`.
Adversary'nin VRF output'larını seçerek bir sonraki epoch'un nonce'unu manipule etmesini engeller (Ouroboros Praos
canonical design).

**Leader election.** Her slot için:
```
seed = nonce_e || slot
(output, proof) = VRF.evaluate(pool_sk, seed)
threshold = 1 − (1 − f)^stake_ratio    // f = active slot coefficient (mainnet: 0.05)
if output < threshold: pool is leader
```
Stake oranıyla orantılı olasılık. `f` mainnet'te ~0.05 (her slotun %5 olasılıkla aktif olması demek).

**Fork choice — density-weighted longest chain.** Cardano'nun varyantı: en çok
aktif slot içeren zincir kazanır. Yoğunluk = blok sayısı / slot aralığı.

**Finality.** k-deep, k=50 blok (~100 saniye). k blok altındaki bloklar pratik
olarak final.

**Stake pool model.**
```
StakePool {
    pool_id, vrf_key, kes_key, cold_key,
    pledge, margin_bps, fixed_cost,
    reward_account,
}
StakeDistribution = { pool_id → relative_stake }
```
Pool registration zincirde bir TX olarak yapılır (`qv-miner::registration::build_pool_registration_tx`).

**Reward distribution.** Block reward = `subsidy(epoch) + fee_sum`. Subsidy
Bitcoin tarzı halving. Operatör önce fixed_cost alır, sonra margin oranını alır,
kalan delegator'lara pledge+delegation oranıyla dağıtılır.

**`VrfEvaluator` ve `KesVerifier` trait'leri.** Konsensus katmanı bu trait'lerin
arkasında çalışır. Production impl'leri:
- `RistrettoVrfEvaluator` (qv_crypto::VrfKeyPair'i wrap eder)
- `DilithiumSumKesVerifier` (bincode-decoded KesSignature'ı doğrular)

Mock impl'leri (`TestVrf`, `TestKesVerifier`) hâlâ test için duruyor; production
kod her ikisini de kullanmıyor.

---

## 8. Storage (qv-storage)

**`KvStore` + `KvBatch` trait abstraction.** Üç backend:
- `MemoryKvStore` — `BTreeMap<Vec<u8>, Vec<u8>>` + `RwLock`. Test/simülasyon.
- `RocksKvStore` — RocksDB, üretim default.
- `RedbKvStore` — saf-Rust alternative, deneysel.

**`BlockStore`.** Block-by-hash, block-by-height, header-by-hash erişimi. Duplicate
hash reject, height collision reject. Bincode encoding.

**`UtxoStore`.** UTXO set'i + undo log. `apply_block(block)` her input'u
remove, her output'u insert eder; aynı zamanda undo entry'sini log'lar.
`revert_block(hash)` undo log'u oynar (genesis'e kadar gidebilir).
`snapshot()` + `restore(snapshot)` zincir state'i için checkpoint mekanizması.

**`StateStore`.** Chain entry (hash → height + parent), tip, ledger state
(epoch nonce, stake distribution snapshot, etc.). Atomic batched writes.

**Epoch snapshots.** Her epoch sınırında otomatik snapshot — rollback ve
finality için.

---

## 9. Mempool (qv-mempool)

İki ayrı pool — **clear pool** ve **encrypted pool**.

**Clear pool (`clear.rs`).** Standart TX havuzu. Fee-density sorting, capacity
eviction (en düşük fee'leri at), double-spend rejection (input outpoint tracking),
duplicate TX rejection. `get_batch(n)` en yüksek fee'li n TX'i döner.

**Encrypted pool (`encrypted.rs`).** TX'leri threshold-Kyber ile şifreli alır
(`EncryptedTransaction { ciphertext, epoch, ... }`). Her TX bir epoch'a bağlıdır;
o epoch boyunca decrypt edilemez. Epoch sonunda decryption committee
(t-of-n) decryption share'lerini birleştirir → batch decrypt → standart akışa
düşer. MEV koruması burada — sequencer içeriği göremez, sıralamayı manipule edemez.

**Deterministic ordering (`ordering.rs`).** Fee density (sat/byte) DESC, eşitse
FIFO arrival time, hâlâ eşitse SHA3(txid) tiebreak. Verifier `verify_order`
sıranın deterministik olduğunu doğrulayabilir (slashing evidence için).

**Batcher (`batcher.rs`).** AMM intent'lerini gruplar, her pool için tek bir
batch trade üretir (slippage hesaplaması ile). MEV protection devamı — sandwich
saldırısı yapan yok çünkü encrypted mempool sıralamayı tek seferde belirliyor.

**Slashing evidence.** Aynı slot için iki blok imzalama, deterministik sıralama
ihlali gibi malicious davranışlar için kanıt struct'ları (`SlashingEvidence`).
Verify edildiğinde stake pool slash'lanır.

---

## 10. DeFi (qv-defi)

eUTXO datum-validator pattern. Tüm DeFi şu yapıda çalışır:
1. Bir "pool UTXO" var — datum içinde state (reserves, ltv, vb.)
2. Pool UTXO'yu harcamak için "validator script" geçer
3. Script "yeni pool UTXO'sunun datum'u tutarlı mı?" der (örn. x·y=k)
4. State değişikliği = pool UTXO'nun yeni outpoint'i

**AMM (`amm.rs`).** Constant-product market maker (Uniswap V2 tarzı). `PoolDatum
{ reserves_a, reserves_b, fee_bps }`. `compute_swap(in, in_reserve, out_reserve, fee)`
ve `compute_add_liquidity / remove_liquidity` saf fonksiyonlar. `apply_swap`
yeni `PoolState` üretir.

**Lending (`lending.rs`).** Compound-tarzı. `PoolDatum { total_supply, total_borrow,
ltv_bps, liquidation_bonus_bps }`. `compute_max_borrow`, `compute_health_factor`,
`compute_liquidation_seize`. Position datum collateral + debt tutar.

**Oracle (`oracle.rs`).** TWAP + manipulation detection. `OracleWindow` son N
observation'ı tutar (FIFO eviction), median + TWAP üretir. `detect_manipulation`
%1 sapmadan büyük outlier'ları yakalar (gelecek versiyon — şu an D-12 ignored,
test premise tutarsız).

**Intents (`intents.rs`).** "Niyet bildirimi" — kullanıcı `SwapIntent { amount_in,
min_amount_out, pool_id, expiry }` yazar, mempool'da bekler, batcher tüm aynı
pool'a olan intent'leri bir blokta birlikte execute eder. Slippage protection
intent içinde; batcher minimum'u sağlayamıyorsa o intent'i skip eder.

---

## 11. Gizlilik (qv-privacy)

**Opt-in model.** Varsayılan = stealth address (her TX'te). Confidential amount
opsiyonel.

**Stealth Addresses (`stealth.rs`).** KEM-tabanlı:
- Alıcı sahibi: `view_keypair` (X25519 + Kyber hibrit) + `spend_keypair` (Dilithium)
- Stealth output üretimi: sender ephemeral X25519+Kyber → shared secret SS;
  one-time spend public key = `Dilithium.derive_from_seed(SS || spend_pk)`
- Alıcı tarama: kendi view_sk ile her stealth output'un SS'sini hesapla; eşleşme
  varsa stealth output bana ait
- Spend: SS + master spend_sk → one-time spend secret key türet, normal
  Dilithium imzasıyla harca

**Confidential Amounts (`confidential.rs`).** Bulletproofs-style range proof
+ Pedersen commitment. `Committer` trait + `MockCommitter` (gerçek bulletproofs
crate bağlanmadı henüz — P-01 envanter). Her output bir `Commitment { value: blinded,
range_proof }` taşır; sum constraint script-level doğrulanır.

**View Key + Selective Disclosure (`view_key.rs`).** Üçüncü taraf auditing.
Owner view_key'i compliance officer'a verir; o sadece **görme** yetkisi kazanır,
harcayamaz. `disclosure_proof(amount, commitment, viewer_pk)` üretir, üçüncü
parti `verify_disclosure_proof` ile değeri inanır.

**Ring Signatures REDDEDILDI** — block şişmesi (anonymity set boyu × signature
size). Monero pattern'ini kullanmadık.

---

## 12. Network (qv-net)

**libp2p 0.54** üzerinden. Şu an klasik Noise XX (X25519) — hibrit Kyber
handshake NET-01'de açık. Gelecek Faz 4 işi.

**Gossip topics.**
- `/qv/blocks/1` — yeni blok yayını
- `/qv/tx/1` — clear mempool TX'leri
- `/qv/vrf/1` — VRF proofs (slot leader announcements; rate-limit için)
- `/qv/votes/1` — finality votes (henüz aktif değil, NET-02)

**GossipSub config.** Mesh n=8 (mainnet) / 6 (testnet) / 3 (ephemeral),
heartbeat 700ms, max transmit 4 MiB. Message ID = SHA3-256(payload) → dedup.

**Request-response.** GetHeaders/Headers, GetBlocks/Blocks pattern'i — peer
sync için. Gossip değil unicast.

**Peer management (`peer.rs`).** `PeerStore` — peer info + reputation tracking +
address rotation + idle eviction. Reputation skoru misbehavior'da düşer, threshold
altında ban.

**Rate limiting (`node.rs`).** Token bucket per-peer; çok mesaj atan peer'lar
throttle edilir.

---

## 13. Node (qv-node)

Full node binary. **Tüm yukarıdaki katmanları birleştirir.**

**Bileşenleri:**
- **`cli.rs`** — `qv-node {init,run,reset,version}` komutları
- **`config.rs`** — mainnet/testnet/devnet preset'leri, TOML config
- **`genesis.rs`** — devnet için 10 hesaplı parametrik genesis (gerçek mainnet
  ceremony değil; ceremony modülü ayrı)
- **`ceremony.rs`** — mainnet trusted setup tooling (Coordinator + Participant
  + Registration + Contribution + Finalize) — modül dolu ama Node mainnet'te
  bağlamıyor
- **`rpc.rs`** — JSON-RPC server (jsonrpsee). Metodlar: `qv_getBlockHeight`,
  `qv_getBlockByHash`, `qv_getUtxo`, `qv_sendTransaction`, `qv_getMempoolStatus`,
  vs. `qv_scanStealth` + `qv_getBalanceFor` stub (N-01/N-02 açık)
- **`network_handler.rs`** — gossip event → NodeEvent decoder
- **`validation.rs`** — TX validation pipeline (well-formed + utxo lookup +
  script run + signature verify)
- **`slot_ticker.rs`** — slot tick generator, blok üretimi (block producer rolü
  burada); `with_kes_signing(kes_sk)` builder ile gerçek KES imzası bağlanır
- **`signals.rs`** — SIGTERM/Ctrl-C graceful shutdown
- **`metrics.rs`** — Prometheus metrics export
- **`node.rs`** — ana orchestration. `Node::new(config)` ile başlar; `Node::run()`
  event loop'unu yönetir; `Node::shutdown()` gossip kanalını kapatır + tip/mempool
  snapshot alır + log'lar

**Çalışan akışlar:**
- Devnet config + ephemeral storage ile başlat
- RPC ile TX al, validate et, mempool'a koy, gossip yay
- Block message'ı gossip'ten al, validate et, apply et, tip güncelle
- Tek-node block pipeline (`tests/transfer_e2e.rs` doğrular)

**Hâlâ açık:**
- `validate_well_formed_tx_with_available_utxo` test ignored (D-11)
- `cmd_run` daemon main loop büyük ölçüde mock — gerçek slot leader scheduling
  + production gossip henüz yok (Faz 3 sonu işi)
- Vote/finality akışı (N-03)

---

## 14. Wallet (qv-wallet)

CLI wallet binary. `qv-wallet {init, import-mnemonic, address, scan, balance, send}`.

**Çalışan:**
- **`init`** — BIP-39 mnemonic üret (24 kelime), passphrase prompt,
  master seed türet, Argon2id+AES-GCM ile keystore.enc dosyasına yaz, ilk
  hesabın stealth adresini bas
- **`import-mnemonic`** — varolan mnemonic'i import + keystore üret
- **`address`** — keystore aç, hesap N'nin stealth + spend public key'lerini bas
- **`send`** — `--to-pubkey <hex> --amount <sat> --input <txid:idx> --input-value
  <sat> [--account <n>] [--fee <sat>] [--broadcast]`. Pipeline:
  1. Keystore aç (mnemonic decrypt)
  2. Hesap N için Dilithium spend keypair türet (HD: SHA3 path-based)
  3. Coin selection — input + amount + 1000-unit fee/dust reserve
  4. `TxBuilder` ile unsigned TX kur (output: p2pkh_pqc(to_pubkey_hash))
  5. Dilithium sign (sign_pqc)
  6. Bincode + hex encode
  7. `--broadcast` ise RPC `qv_sendTransaction`

**Stub veya yarım:**
- `scan` — stealth output tarama (N-02 RPC bağımlı)
- `balance` — stealth scan üzerine kurulu (N-01 bağımlı)
- W-01: swap/lp-add/lp-remove/borrow/repay/pool-info/export-view-key/disclose
  komutları henüz yok (Faz 5/6)

---

## 15. Miner — Stake Pool Operator (qv-miner)

Önemli: **"miner" PoW miner değildir.** Cardano nomenklatürüyle "stake pool
operator". Hiç bir hash arama döngüsü yok. Daemon role'ü.

**Yapısı:**
- **`keys.rs`** — `VrfKeyPair`, `KesKeyPair`, `ColdKeyPair`, `OperatorKeys`
  bundle. `OperatorKeys::generate()` random 32-byte master seed üretir;
  `from_seed(master)` deterministik türetir: `vrf = sha3(master||"vrf")`,
  `kes = sha3(master||"kes")`, `cold = sha3(master||"cold")`. Master seed
  `self.master_seed` field'ında tutulur (keystore için)
- **`keystore.rs`** — Argon2id + AES-256-GCM. Tek dosyaya 32-byte master seed +
  KES current period yazar. M-04 olarak 2026-05-12'de kapatıldı. `OperatorKeys::
  save_encrypted(path, password)` ve `load_encrypted(path, password)`
- **`config.rs`** — `OperatorConfig { pool_id, pool_name, keystore_path, pledge,
  margin_bps, fixed_cost, reward_account, network, node_rpc_url, ... }`. TOML
- **`registration.rs`** — `build_pool_registration_tx(config, vrf_pk, kes_pk,
  keys)` pool registration TX'i kurar (datum içinde StakePool, output cold key'le
  imzalı). `submit_via_rpc` ile node'a gönderir (M-08 hâlâ stub — RPC çağrısı
  placeholder)
- **`committee.rs`** — Decryption committee sortition. VRF(`epoch_nonce ||
  pool_id || "committee"`) üzerinden rank hesapla, ilk t pool committee
- **`slot_loop.rs`** — Her slot tick:
  1. VRF leader check → `RistrettoVrfEvaluator.check_leadership(stake_ratio,
     epoch_nonce, slot)`
  2. Lider ise → `produce_block` çağır
  3. Aksi halde metriği güncelle
- **`block_producer.rs`** — Lider olduğunda:
  1. Clear mempool snapshot (RPC `qv_getMempoolStatus` — M-12 hâlâ sabit 0
     döner, gerçek RPC bağlanmadı)
  2. Encrypted mempool snapshot + committee üyesiyse decrypt (K-06 hâlâ yok)
  3. Deterministic ordering + AMM batch (K-07 hâlâ yok — `Vec::new()`)
  4. BlockHeader doldur (utxo_commitment K-03 hâlâ ZERO; producer_key_hash
     K-05 hâlâ ZERO)
  5. Unsigned header bincode → `KesKeyPair.sign(bytes)` → KesSignature
  6. Tam block bin → libp2p gossip
- **`dashboard.rs`** — TUI placeholder. Şu an sadece ASCII art skelet,
  ratatui gerçek implementation Faz 9 (M-10/M-11)

**Çalışan akışlar:**
- `qv-miner init` — master seed üret + keystore yaz + config TOML üret
- Keystore roundtrip (Argon2+AES-GCM) — ✅ M-04 ile tamam
- Pool registration TX'i imzala (cold key) — ✅ struct olarak doğru; submit_via_rpc
  yarım

**Henüz yapılmamış (Faz 3/4):**
- `cmd_run` daemon ana loop'u `sleep(u64::MAX)` ile uyutuyor (M-09)
- Encrypted mempool decrypt entegrasyonu (K-06)
- AMM batcher block producer'a bağlı değil (K-07)
- UTXO commitment hesaplama (K-03, K-05)
- Dashboard TUI (M-10, M-11)

---

## 16. Veri Akışları — Üç Yol

### 16.1 Basit Transfer (Alice → Bob)

```
1. Bob: qv-wallet address --account 0
   → Stealth address pq + view_pk basar.

2. Alice: qv-wallet send --to-pubkey <Bob_pq> --amount 100 \
            --input <txid:idx> --input-value 1000 --broadcast
   → Keystore aç, spend keypair türet
   → CoinSelector input'u kabul eder (1000 ≥ 100+1000 reserve)
   → TxBuilder unsigned TX: input + 2 output (Bob için 100, change Alice için ~899)
   → sign_pqc ile imzala
   → POST /rpc { method: "qv_sendTransaction", params: [hex] }

3. Node: rpc.rs → validate (well-formed + UTXO lookup + script run + sig verify)
   → ClearMempool'a ekle → gossip ("/qv/tx/1")

4. Network: tüm peer'lar gossip ile TX'i alır, kendi mempool'larına ekler

5. Bir sonraki slot lideri (qv-miner) → produce_block:
   → mempool snapshot (Alice TX dahil)
   → deterministic ordering → block_body
   → BlockHeader doldur (vrf_proof, kes_sig)
   → gossip ("/qv/blocks/1")

6. Tüm node'lar: block validate + apply_block(utxo_store)
   → Alice'in input UTXO'su silinir, Bob'un 100 outputu UTXO set'e girer
   → Tip güncellenir, height +1

7. Bob: bir sonraki scan'de Bob'un view_sk ile stealth output detect edilir,
   spend keypair türetilir, wallet balance +100 görünür.
```

Şu an **6. adıma kadar in-memory devnet'te çalışıyor**. 1. adımdaki balance display
(`scan`) ve 7. adım stealth scanning N-01/N-02'de yarım.

### 16.2 Pool Registration

```
1. Operatör: qv-miner init [--pool-name MyPool --pledge 1000000]
   → 32-byte master seed üret
   → OperatorKeys::from_seed(master) → vrf+kes+cold
   → keystore.save_encrypted(path, password) → JSON envelope
   → config TOML basar

2. Operatör: qv-miner register-pool --config operator.toml --node-rpc <url>
   → keystore.load_encrypted(path, password) → OperatorKeys
   → build_pool_registration_tx(config, vrf.pk, kes.pk, &keys)
   → datum: StakePool { pool_id, pledge, margin, vrf_pk, kes_pk, ... }
   → cold_key.sign(tx_unsigned) → cold sig
   → submit_via_rpc(tx, url) → POST qv_sendTransaction

3. Node validate + mempool + gossip → blockleştirilir

4. Bir sonraki epoch boundary'de:
   → Snapshot alınır (qv_consensus::stake)
   → Pool aktif hale gelir
   → Bir sonraki epoch'tan itibaren slot leader olabilir
```

**Çalışan**: 1, 2 (build kısmı). **Yarım**: submit_via_rpc (placeholder döner).

### 16.3 AMM Swap (Cardano eUTXO tarzı)

```
1. User: SwapIntent { pool_id, amount_in, min_amount_out, expiry } yazar.
   → Encrypted mempool'a şifreli koyar (threshold Kyber)

2. Epoch boundary: committee bütün epoch intent'lerini decrypt eder

3. Batcher (mempool veya block producer):
   → Pool için tüm intent'leri al
   → Toplam tradei hesapla (constant product preserve)
   → Tek bir AMM batch TX üret: 1 input (eski pool UTXO) + N output
     (yeni pool UTXO + her user için pay)

4. Block producer batch TX'i bloğa koyar

5. Validators script çalıştırır:
   → amm_swap_covenant: yeni pool UTXO datum tutarlı mı? (x·y=k)
   → Her user output min_amount_out karşılıyor mu?
   → Pool UTXO datum hash içeren script_hash assert
```

**Çalışan**: 1 (mempool), 2 (decrypt mock), 5 (script logic). **Yarım**: 3, 4 — batcher block producer'a bağlı değil (K-07).

---

## 17. Test Yığını

| Katman | Test sayısı | Notlar |
|---|---:|---|
| qv-consensus unit | 80 | VRF, KES, leader_schedule, epoch nonce, rewards, fork choice |
| qv-consensus integration | 12 | Tam epoch lifecycle, multi-pool simulation, minority attacker |
| qv-core unit | 72 | UTXO, Transaction, Block, Merkle, params |
| qv-core integration | 17 | End-to-end block apply, double-spend, commitment |
| qv-crypto unit | 77 | Hash, PQC, KEM, VRF, KES, threshold, secure_bytes |
| qv-crypto integration | 11 | from_seed HD pattern, KES leaf pattern, KEM roundtrip |
| qv-defi unit | 89 | AMM, lending, oracle, intents |
| qv-defi integration | 11 | AMM swap e2e, lending lifecycle, intent execution |
| qv-mempool unit | 31 | Clear, encrypted, ordering, batcher, slashing |
| qv-mempool integration | 12 | Full pipeline (encrypted → ordering → batch) |
| qv-miner unit | ~44 | Keys, keystore, committee, slot_loop, block_producer, dashboard |
| qv-miner integration | 9 | Block production mocked, committee diversity, config TOML |
| qv-net unit | 29 | Gossip, message envelope, peer store, transport |
| qv-net integration | 12 | Network node construction, peer lifecycle |
| qv-node unit | 52 | Ceremony, RPC, validation, slot_ticker, genesis |
| qv-node integration | 15 | Node creation per network, graceful shutdown |
| qv-node e2e | 1 | `transfer_e2e` — uçtan uca tek-node transfer |
| qv-privacy unit | 31 | Stealth, confidential, view_key |
| qv-privacy integration | 12 | Stealth + confidential combined, audit flow |
| qv-script unit | 58 | Opcode, interpreter, gas, templates |
| qv-script integration | 12 | Validate script, encode/decode roundtrip |
| qv-storage unit | 15 | KV (3 backends), block store, state store, utxo store |
| qv-storage integration | 12 | Multi-block apply/revert, snapshot lifecycle |
| qv-wallet unit | 5 | Keystore (4) + error display |
| qv-wallet integration | 13 | Mnemonic, coin select, tx build, CLI parse, RPC client |
| **Doc tests** | 10 | Compile-time API doğrulama |
| **TOPLAM** | **~741** | **0 failed, 32 ignored** |

**Ignored testler ne için?** Çoğu **yavaş** (KES leaf-tree gen ~2s), birkaçı
**bilinen mock gap'lere bağlı** (T-01 Pedersen DKG, D-07..D-12 DeFi yan vakaları,
B-03 Node !Send refactor). Her birinin bir envanter ID'si var; ROADMAP'te
tek tek takip edilebilir.

---

## 18. Açık Kalan Ana İşler

Detay `docs/ROADMAP.md`'de; burada yalın özet:

**Yakın vade (Faz 1–3 tamamlama):**
- **Devnet smoke test** — `qv-node` + `qv-wallet send --broadcast` + tip artışı.
  Tüm parçalar var; sadece elle koşturma.
- **N-03**: Vote/finality akışı — şu an gossip'te `Vote` mesajı placeholder, finality
  henüz BFT vote toplama yapmıyor (k-deep var ama oylama yok)
- **K-03/K-05**: UTXO commitment post-apply hash — block header'da ZERO yerine
  gerçek değer
- **K-06**: Encrypted mempool decrypt → block producer wiring
- **K-07**: AMM batcher → block producer wiring
- **M-08**: `submit_via_rpc` gerçek qv_sendTransaction çağrısı
- **M-09**: `qv-miner cmd_run` daemon ana loop'u

**Orta vade (Faz 4–7):**
- **NET-01**: Hibrit X25519+Kyber libp2p handshake
- **C-05**: View key seeded keygen (`pqcrypto-kyber` seeded API yok; alternatif crate)
- **N-01/N-02**: RPC `getBalanceFor` + `scanStealth`
- **MP-01**: Encrypted mempool decryptor wiring (`qv_crypto::threshold` gerçek
  primitif zaten var)
- **P-01**: Bulletproofs gerçek backend (şu an `MockCommitter`)

**Uzun vade (Faz 8–10):**
- **P-03**: STARK range proof migration (Bulletproofs PQC değil)
- **N-05**: Mainnet genesis ceremony tooling (modül var; orchestration eksik)
- **M-04 → tam keystore audit**: bağımsız audit + key rotation policies
- **Bağımsız güvenlik audit**
- Mainnet launch öncesi tüm parametre değerleri için fuzz coverage

---

## 19. Tasarım Felsefesi — Neden Bunlar?

**Neden saf PoS?** DeFi için **finalite** ve **latency** PoW'dan çok daha iyi. PoW
zincirlerinde "6 confirmation = 60 dakika" bekliyorsun; Ouroboros Praos'ta k=50
blok = 100 saniye. Ayrıca enerji tüketimi sıfır, geliştirici onboarding daha
basit.

**Neden FIPS 204 ML-DSA, hibrit değil?** İmza tarafında hibrit (klasik+PQC) yapmak
imza boyutunu **çift**'e çıkarırdı. Mainnet'te her TX'te o veriyi taşımak
verimsiz. Bunun yerine ECDSA'yı hiç koymadık — sadece PQC. KEM tarafında durum
farklı: KEM operasyonu daha küçük (Kyber-768 ciphertext ~1KB), klasik X25519 ile
hibrit yapmak makul → her ikisi shared secret'a katkı sağlar (HKDF birleştirir).

**Neden UTXO, account modeli değil?** Üç sebep:
1. **Paralellik**: UTXO'lar bağımsız; iki TX farklı UTXO'lara dokunuyorsa
   aynı anda valide edilebilir
2. **Gizlilik**: Account modeli adres reuse'u zorlar (sender adresi her TX'te);
   UTXO'da her output bağımsız bir stealth adres olabilir
3. **eUTXO determinizm**: Validator script "verilen input'lar ve datum ile
   gelecek output ne olmalı" sorusunu yanıtlar — Ethereum'daki gas estimation
   ve front-running problemleri ortadan kalkar

**Neden encrypted mempool?** MEV en büyük geliştirici/kullanıcı kaybı. Cardano
ve Ethereum'da çözüm henüz olgunlaşmadı (commit-reveal, MEV-Boost). QuantumVault
threshold encryption ile sequencer'ı **körleştirir** — TX içeriğini decrypt
edemediği için sandwich/front-run yapamaz. Decryption sadece epoch sonunda
committee'nin t-of-n share'iyle olur, sıralama bu noktada batch deterministic.

**Neden script L1'de yürütülmez?** Cardano deneyimi: smart contract'ları L1'de
çalıştırmak gas modelini ve fee piyasasını çok karmaşık yapıyor. eUTXO'da
script sadece "tutarlı mı?" der; ağır hesaplama off-chain (kullanıcı veya bir
DApp backend) yapılır. Bu Plutus tarzı.

**Neden 13 crate?** Tek crate olsa, qv-crypto'da yapılan bir değişiklik tüm
projeyi rebuild eder. 13 crate'lik separation hem build paralelizmi hem mental
clarity sağlıyor; ayrıca her crate'in `tests/` ve `benches/` dizini ayrı.

---

## 20. Nasıl Devam Edileceği

Eğer bu doküman okunduktan sonra projeye katkı yapacaksan, sıralı kontrol listesi:

1. `nix develop` ile devshell'e gir (veya manuel rustup 1.78+ + just kur)
2. `just build` — workspace derle (~2 dakika ilk seferinde)
3. `just test` — hızlı testleri koş (~40 saniye)
4. `cargo test -- --ignored` — yavaş KES testleri dahil tam koş (~5 dakika)
5. `docs/ROADMAP.md` → "Faz–Envanter Çapraz Tablosu" bölümü — sıradaki adımları
   gör
6. Bir envanter ID seç (örn. M-08), o crate'in kodunu oku, küçük bir PR yaz
7. PR'da: `just ci` lokal olarak yeşil + commit message açıklayıcı + ROADMAP'ten
   ilgili satırı sil/güncelle + MEMORY.md'ye 1 satır not

**Bilmek isteyeceğin başka dosyalar:**
- `docs/ABSTRACT.md` — projenin "neden var" yazısı
- `docs/MASTER_PLAN.md` — aşama bazlı tarihsel plan
- `MEMORY.md` — proje hafızası, kararlar, oturum özetleri
- `PROJECT_STATUS.md` — zaman serisi aşama dökümü
- `CLAUDE.md` — mimari kurallar (değiştirilemez ilkeler)
- `crates/<crate>/src/lib.rs` — her crate'in başında o crate'in açıklaması

---

## 21. Sözlük

| Terim | Açıklama |
|---|---|
| **UTXO** | Unspent Transaction Output. Bitcoin/Cardano modeli; harcanmamış çıktı |
| **eUTXO** | Extended UTXO. Cardano'nun "UTXO + datum + validator" varyantı |
| **Datum** | UTXO'ya bağlı ek veri; script'in görebileceği state |
| **Validator script** | UTXO'yu harcamak için geçilmesi gereken script |
| **VRF** | Verifiable Random Function. Deterministik ama doğrulanabilir random |
| **KES** | Key-Evolving Signature. Period başına evrilen, forward-secure imza |
| **Slot** | Konsensusun zaman birimi. QV'de 2 saniye |
| **Epoch** | 21600 slot = 12 saat. Stake snapshot ve nonce evolution sınırı |
| **Nonce (epoch nonce)** | Epoch'un VRF input seed'i; manipulation-resistant |
| **Stake pool** | Delegator'lardan stake toplayan operatör birimi |
| **Cold key** | Pool registration için Dilithium uzun-vadeli anahtar |
| **Hot key (KES)** | Blok imzalamak için kullanılan period-bound anahtar |
| **k-deep finality** | k blok altındaki bloklar pratik olarak final |
| **Stealth address** | Her TX'te yeni adres üreten gizlilik primitifi |
| **Threshold encryption** | t-of-n parties decrypt edebilir |
| **Bulletproofs** | Range proof primitifi (confidential amounts için) |
| **MEV** | Maximal Extractable Value. Sequencer'ın TX sıralamasını manipule edip kâr çıkarması |
| **PQC** | Post-Quantum Cryptography. Shor algoritmasına karşı dayanıklı |
| **FIPS 204** | NIST'in ML-DSA standardı (Aug 2024) — Dilithium'un resmi hali |
| **ML-DSA** | Module-Lattice Digital Signature Algorithm. FIPS 204 spec ismi |
| **ADR** | Architecture Decision Record. Mimari karar kalıcı kaydı |

---

**Bu doküman canlı.** Sistem değiştikçe güncellenmesi gerekir; özellikle
"Mevcut Olgunluk Seviyesi" (§1), "Açık Kalan İşler" (§18), ve "Test Yığını"
(§17) bölümleri her büyük PR sonrası güncel tutulmalıdır.
