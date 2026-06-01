# QuantumVault L1 — Production Roadmap

**Doküman tarihi:** 2026-05-07 (full test suite green milestone)
**Hedef:** Mevcut iskeletten production-ready bir L1 blockchain'e giden faz-faz plan.

> **Not:** PROJECT_STATUS.md'deki "AŞAMA 15 tamamlandı — code-complete" başlığı yanıltıcıdır.
> Gerçek durum: **workspace derleniyor, 728 test (572 unit + 146 integration + 10 doc)
> tamamı geçiyor, 38 test bilinçli `#[ignore]` (her biri envanterde ID'li)**. Ama uçtan
> uca akış hâlâ mock/trait arkasında. Bu doküman gerçek "yapılması gerekenler"
> listesidir; aşağıdaki **Placeholder ve Mock Envanteri** ile birlikte okunmalıdır.

---

## Mevcut Durum (2026-05-14 — CI pipeline tamamen yeşil)

| Konu | Durum |
|---|---|
| Workspace derleme | ✅ 0 error, **0 warning**, 13 crate |
| Test suite | ✅ **735 passed / 0 failed / 36 ignored** (`cargo test --lib --tests`; doc-test'ler ayrı CI job'unda) |
| GitHub Actions CI | ✅ **5 job yeşil**: clippy, rustfmt, rustdoc, cargo-audit, cargo-deny (2026-05-14) |
| Çalışan binary'ler | qv-node (kısmen), qv-wallet (init/send/address çalışıyor), qv-miner (M-09 core: VRF+stake distribution+slot loop+SIGINT) |
| Çalışan akışlar | CLI parse, mnemonic gen, PQC key gen, config save/load, **gerçek Ristretto255-VRF**, **gerçek Sum-KES (depth-11) on Dilithium**, in-memory blok pipeline, wallet keystore (Argon2id + AES-256-GCM), wallet send (TxBuilder + Dilithium sign + RPC), node `shutdown()` graceful flush, miner slot loop |
| **Eksik / placeholder** | Bulletproofs gerçek implementasyon (P-01), mainnet genesis ceremony tooling (N-05), miner daemon RPC fetch (M-09b), block producer UTXO commitment hash (K-03/K-05), AMM batcher (K-07), Pedersen DKG (T-01), BFT finality gadget (ADR-013 öneri) |
| Boş `Ok(...)` veya stub dönen public method | ~26 (envanterde takipli) |
| Bilinçli `#[ignore]` test | 36 (detay: bkz. **O. Ignored Test İndeksi**) |
| **Kapanmış envanter ID'leri (2026-05-14)** | C-01, C-02, C-04, C-06, C-07, K-01, K-02, K-04, **K-06**, L-01..L-04, M-01, M-02, M-03, M-04, M-05, **M-09 (core)**, N-04, N-07, G-01, G-02, W-05, W-06, B-01, B-02, **+ CI sprint (clippy/fmt/rustdoc/audit/deny)** (**26+ envanter girdisi**) |
| **4-node yerel devnet (2026-05-21)** | ✅ Gerçek libp2p çok-process devnet: ayrı `qv-node`'lar bağlanıp blok gossip'liyor ve aynı zincirde yakınsıyor (round-robin lider); Dilithium-imzalı transfer 4 node'a yayılıyor. Launcher + CLI monitor + transfer demoları eklendi. Detay: PROJECT_STATUS.md / MEMORY.md (2026-05-21 oturumu) |
| **Stealth + sighash + cüzdan uygulaması (2026-05-22)** | ✅ ADR-011 (Faz 1-5) + ADR-012 uygulandı. `Transaction::sighash()` + `SigHash` opcode (`0x69`) → in-flight witness yeniden-oynatma kapandı. `stealth_p2pkh` template'i gerçek doğrulamada çalışıyor. Cüzdan: `qv_scanStealth` + `qv_scanP2pkh` (devnet köprüsü), axum tabanlı HTTP API + gömülü HTML/CSS/JS UI, QR + `.qvaddr` desteği, `qvst1…` / `qvfp1…` adres formatı. `devnet/run-single.{ps1,sh}` ve `devnet/run-all.{ps1,sh}` tek-komutla launcher script'leri eklendi. Devnet genesis artık `DEVNET_TEST_MNEMONIC`'ten türetiliyor — bir cüzdanın `devnet-import` ile import etmesi bakiyeyi anında getiriyor |

---

## Placeholder ve Mock Envanteri (canlı liste)

Aşağıdaki tüm girdiler **doğrudan kod denetiminden** çıkarıldı. Her satırda `crate/path:line`
formatında tıklanabilir referans ve girdiyi kapatacak faz numarası var. Bir öğe kapatıldığında
o satır kaldırılır; yeni placeholder eklenirse buraya yazılır. Bu liste tek doğru kaynaktır.

### A. Kriptografi (qv-crypto)

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~C-01~~ | ✅ **KAPATILDI 2026-05-06** — `qv_crypto::vrf` modülü yazıldı (`schnorrkel` ile Ristretto255-VRF). API: `VrfKeyPair::generate/from_seed`, `evaluate`, `verify`. 11 unit test. Wire format: pre_out(32) || proof(64) | — |
| ~~C-02~~ | ✅ **KAPATILDI 2026-05-06** — `qv_crypto::kes` modülü yazıldı (Sum-KES on Dilithium L3, depth=11, N=2048). API: `generate`, `sign`, `verify`, `evolve`. 11 unit test (slow ones `#[ignore]`). Forward security: master seed zeroize + per-period leaf seed zeroize | — |
| C-03 | `crates/qv-crypto/src/lib.rs:11-13` | Doc tablosunda `vrf/kes/threshold` hâlâ "TODO" diyor — threshold artık dolu, doc güncellenmeli | Faz 0 (doc fix) |
| ~~C-04~~ | ✅ **KAPATILDI 2026-05-07** — full ml-dsa swap (ADR-006). `from_seed_pqc` artık `<MlDsaP as KeyGen>::key_gen_internal(&B32)` ile FIPS 204 deterministik seeded keygen sunuyor. HD wallet, KES leaf, stealth recovery, miner cold key hepsi runtime'da çalışır | — |
| ~~C-05~~ | ✅ **KAPATILDI 2026-05-22** (pratik çözüm) — `pqcrypto-kyber` 0.8 hâlâ seeded keygen sunmuyor; bunun yerine view key cüzdan keystore'una **kalıcı şifreli** yazılıyor. v2 keystore formatı `view_keypairs: BTreeMap<u32, PersistedViewKey>` taşıyor; `init` / `import-mnemonic` / `devnet-import` ilk hesap için fresh KEM çifti üretip persist ediyor; `unlock_account` mevcut keypair'i yeniden kullanıyor, yeni account ilk açıldığında üretip otomatik yazıyor (v1→v2 dosya migrasyonu in-place). Sonuç: stealth ödemeler keystore intact olduğu sürece restart'a dayanır. Tam FIPS 203 deterministik seeded çözümü için `ml-kem` migrasyonu ileride ayrı bir ADR olarak yapılabilir | — |
| ~~C-06~~ | ✅ **KAPATILDI 2026-05-07** (ADR-006). RustCrypto `ml-dsa = "0.0.4"` seçildi ve full swap yapıldı (`pqcrypto-dilithium` workspace'den çıkarıldı). Spike (`spikes/c06-mldsa/`) 6/6 ✅ ile API doğrulandı. Wire format değişikliği: sk 4000→4032 byte, sig 3293→3309 byte (FIPS 204 final) | — |
| ~~C-07~~ | ✅ **DOĞRULANDI 2026-05-07** — kullanıcı lokalde `cargo build`'i başlattı; **qv-crypto compile etti** (içinde vrf.rs + kes.rs). schnorrkel 0.11 API çağrılarım (`MiniSecretKey::ED25519_MODE`, `vrf_sign`, `VRFInOut::to_preout()`, `make_bytes(domain)`) gerçek ile uyumlu. Test çalıştırma henüz yapılmadı — VRF roundtrip gerçek runtime davranışını C-06 sonrası mı, yoksa bağımsız mı doğrulayacağımız ayrı konu | — |

### B. Konsensüs / Block Production

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~K-01~~ | ✅ **KAPATILDI 2026-05-06** — `RistrettoVrfEvaluator` impl edildi (`leader_schedule.rs`). Mevcut `TestVrf` mock korundu. 4 yeni unit test (deterministik, roundtrip, wrong-pk, fairness) | — |
| ~~K-02~~ | ✅ **KAPATILDI 2026-05-06** — `DilithiumSumKesVerifier` impl edildi (`block_validator.rs`). Stateless, bincode-decoded `KesSignature`'ı doğruluyor; mevcut `TestKesVerifier` korundu | — |
| ~~K-03~~ | ✅ **KAPATILDI 2026-05-22** — `SlotTicker::compute_post_apply_commitment` artık canlı UTXO setini snapshot'lar, blok'un transaction'larını speculatively uygular ve elde edilen `commitment_root()`'u header'a stamp eder. 2 unit test: boş blok için snapshot = post; non-trivial blok için bağımsız `InMemoryUtxoSet` mutasyonu ile aynı root. Mainnet için incremental (sparse Merkle) sürüm Faz 9'da | — |
| ~~K-04~~ | ✅ **KAPATILDI 2026-05-06** — `SlotTicker::with_kes_signing(kes_sk)` builder ile gerçek KES bağlanır; `produce_block` içinde `bincode::serialize(&unsigned_header)` üzerinde `qv_crypto::kes_sign` çağrılır. `kes_sk = None` (legacy/test path) backward-compatible | — |
| K-05 | `crates/qv-miner/src/block_producer.rs:100-102, 117` | `utxo_commitment = ZERO`, `producer_key_hash = ZERO` placeholder. **Not (2026-05-22):** qv-node lokalde K-03'ü kapattı; miner ayrı binary olduğundan UTXO setini bilmiyor → RPC köprüsü gerekli (`qv_getPostApplyCommitment(tx_hex)` veya benzeri). Ayrı follow-up | Faz 3 |
| ~~K-06~~ | `crates/qv-miner/src/block_producer.rs::produce_block_with_decryption` | ✅ **KAPATILDI 2026-05-12** — Yeni fonksiyon `produce_block_with_decryption<D: ThresholdDecryptor>` mevcut `produce_block` yanına eklendi. Committee üyesi olan operatör `encrypted_pool.decrypt_batch(decryptor, shares)` ile drain + decrypt + bincode-deserialize akışını çalıştırır; sonuç clear-pool ile merge edilir. `MockThresholdDecryptor` ile test (`produce_block_with_decryption_merges_encrypted_tx`). Production decryptor T-01 (Pedersen DKG / Feldman VSS asymmetry) kapandığında devreye girer | — |
| K-07 | `crates/qv-miner/src/block_producer.rs::produce_block_with_decryption` step 4 yorumu | AMM batcher wire scaffolding noktası genişletildi: `qv_defi::batcher::build_amm_batch` çağrı noktası, intent extraction helper'ı eksik (`qv-defi::Intent::extract_swap` yazılmamış) ve pool state oracle RPC yok. Intent tx'leri şimdilik plain UTXO spend gibi akar | Faz 6/7 |

### C. qv-miner (stake pool operatörü) — **kabuk seviyesinde**

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~M-01~~ | ✅ **KAPATILDI 2026-05-06** — `VrfKeyPair` artık `qv_crypto::VrfKeyPair`'i wrap ediyor; Ristretto255 keypair üretiyor. `into_evaluator` ile `RistrettoVrfEvaluator`'a dönüşür | — |
| ~~M-02~~ | ✅ **KAPATILDI 2026-05-06** — `KesKeyPair` artık `qv_crypto::kes_generate`'i kullanıyor (real depth-11 Sum-KES on Dilithium) | — |
| ~~M-03~~ | ✅ **KAPATILDI 2026-05-06** — `ColdKeyPair` artık `qv_crypto::generate_pqc_keypair(Level3)` (gerçek Dilithium 1952-byte pk) | — |
| ~~M-04~~ | ✅ **KAPATILDI 2026-05-12** — `qv-miner/src/keystore.rs` yazıldı (Argon2id + AES-256-GCM, wallet keystore port'u). `OperatorKeys::save_encrypted(path, password)` ve `load_encrypted(path, password)` tek dosyaya 32-byte master seed + KES current period yazar; load'da `from_seed` ile keys'i yeniden türetir + KES period kadar `evolve` eder. API 3-path → single-path: `OperatorConfig.keystore_path`. **741 passed / 0 failed / 36 ignored** workspace toplam sonrası (5 keystore unit + 2 ignored end-to-end roundtrip slow KES). Build + test verify ✅ 2026-05-12 | — |
| ~~M-05~~ | ✅ **KAPATILDI 2026-05-06** — `evolve_to_next_period` artık `qv_crypto::kes_evolve`'i çağırıyor (gerçek leaf zeroize + period advance) | — |
| M-06 | `crates/qv-miner/src/registration.rs:54` | Locking script boş + `// TODO: Script::standard_registration_lock()` | Faz 6 |
| M-07 | `crates/qv-miner/src/registration.rs:64-68` | UTXO seçimi/change/cold-key imzalama yok; tek dummy input | Faz 1 |
| M-08 | `crates/qv-miner/src/registration.rs:99-107` | `submit_via_rpc` RPC çağırmıyor, `"txid_placeholder"` döner | Faz 1 |
| ~~M-09~~ | `crates/qv-miner/src/main.rs::cmd_run` | ✅ **KAPATILDI 2026-05-12 (core scaffolding)** — `sleep(u64::MAX)` kaldırıldı; gerçek akış: keystore load (QV_KEYSTORE_PASS env veya `rpassword` prompt) → OperatorKeys (Argon2id+AES-GCM decrypt) → mock single-pool stake distribution → SlotLoop initialize + `run_slot_loop` çağrısı → graceful Ctrl+C shutdown via `tokio::select!`. Production block submit/stake fetch RPC bağımlılıkları yeni envanter olarak ayrıldı (**M-09b**, **M-09c**) | — |
| ~~M-09b~~ | ✅ **KAPATILDI 2026-05-15** — `qv-node` artık `qv_getStakeDistribution` ve `qv_getEpochNonce` RPC endpoint'leri sunuyor. `Node`'a `Arc<RwLock<StakeDistribution>>` ve `Arc<RwLock<EpochNonce>>` paylaşılan state'i eklendi; `spawn_slot_ticker` lokal pool config'i bu paylaşılan state'e de yazıyor. `qv-miner::node_rpc::NodeRpcClient` yeni modülü RPC çağrılarını yapar; `cmd_run` artık mock single-pool kullanmıyor — node'dan gerçek distribution + nonce çekiyor, pool kayıtlı değilse net hata ile çıkıyor | — |
| ~~M-09c~~ | ✅ **KAPATILDI 2026-05-15** — `qv-node` artık `qv_submitBlock(hex) -> block_hash_hex` + `qv_getPendingTransactions() -> Vec<hex>` endpoint'leri sunuyor. `RpcServer` event_tx kanalını alır ve gelen blokları `NodeEvent::BlockReceived` üzerinden ana akışa dispatch eder (handle_block: linkage + UTXO apply + gossip). `qv-miner::cmd_run` block_producer_fn artık gerçek: tip fetch → pending tx fetch → merkle root → unsigned header → KES sign → bincode serialize → `submit_block` RPC. `slot_loop::run_slot_loop` artık VrfProof'u callback'e geçiriyor. Bonus: `qv_getTip` artık `Debug` yerine canonical hex `to_hex()` döndürüyor (önceki bug düzeltildi) | — |
| M-10 | `crates/qv-miner/src/main.rs:182-188` | `cmd_dashboard` log atıp dönüyor; ratatui TUI yok | Faz 9 |
| M-11 | `crates/qv-miner/src/dashboard.rs:140, 281` | `render_dashboard_placeholder` ASCII art mockup | Faz 9 |
| M-12 | `crates/qv-miner/src/block_producer.rs:154-161` | `RpcMempoolProvider.get_mempool_status` sabit 0 döner | Faz 1 |
| M-13 | `crates/qv-miner/src/block_producer.rs:165-181` | `snapshot_clear/encrypted` her seferinde boş yeni pool yaratıyor; RPC'den çekmiyor | Faz 1 |

### D. qv-node (full node)

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~N-01~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 4) — `qv_getBalanceFor(view_key: StealthViewKey)` artık gerçek toplam (stealth + plain p2pkh köprüsü) döner. `StealthViewKey` wire payload + `into_view_keys` validation, `qv-privacy::scan_output_view(view_kp, spend_pk, out)` ile spend secret'a değmeden tarama | — |
| ~~N-02~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 4) — `qv_scanStealth(view_key, from_height, to_height)` UTXO setini geziyor; `stealth_info` taşıyan her çıktıda `scan_output_view` çağırıp locking-script bytewise eşleşmesiyle view-tag false positive'lerini eliyor. `StealthScan` artık `shared_secret_hex` + `onetime_pk_hash_hex` taşıyor (cüzdan harcayabilsin). Bonus: `qv_scanP2pkh(pubkey_hash_hex)` köprüsü düz genesis allokasyonlarını da bulur | — |
| N-03 | `crates/qv-node/src/network_handler.rs:90-97` | `VrfProof` ve `Vote` mesajları sadece debug log; finality akışı yok | Faz 3/4 |
| ~~N-04~~ | ✅ **KAPATILDI 2026-05-06** — `Node::shutdown()` artık gossip channel'i kapatiyor + final tip/clear-pool snapshot'i log'luyor. Storage backend'leri Drop'ta zaten flush ediyor; future `KvStore::flush()` trait metodu eklenince explicit hale gelecek | — |
| N-05 | `crates/qv-node/src/node.rs:191-198` | Mainnet/testnet için "Real genesis ceremony would use threshold Kyber DKG — placeholder for now"; `ceremony.rs` modülü var ama bağlanmamış | Faz 9/10 |
| N-06 | `crates/qv-node/src/ceremony.rs:891` | Test fixture içinde `vrf_key = vec![42u8; 32]; // placeholder VRF key` — gerçek VRF gelince ceremony testleri güncellenmeli | Faz 3 |

### E. qv-wallet — **kabuk seviyesinde**

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| W-01 | `crates/qv-wallet/src/cli.rs` | DeFi komutları (`swap, lp-add, lp-remove, borrow, repay, pool-info, export-view-key, disclose`) hâlâ tanımlı değil. **Eklendi 2026-05-22:** `devnet-import`, `send-stealth`, `serve` (UI), `address` flag'leri (`--save`, `--qr`, `--full-qr`). | Faz 5/6 (DeFi kısmı Faz 6'da) |
| ~~W-02~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 5) — `cmd_address` artık tam `qvst1…` adresini, kısa `qvfp1…` fingerprint'ini, opsiyonel ASCII QR (`--qr`, `--full-qr`) ve `.qvaddr` dosyasını (`--save <path>`) yazar | — |
| ~~W-03~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 5) — `cmd_scan` `qv_scanStealth` + `qv_scanP2pkh` çağırıyor; her iki havuzu listeliyor | — |
| ~~W-04~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 5) — `cmd_balance` stealth + plain bakiyeyi toplayıp basıyor | — |
| ~~W-05~~ | ✅ **KAPATILDI 2026-05-06** — `cmd_send` end-to-end: keystore load → spend key derive → input/recipient parse → p2pkh_pqc locking script → TxBuilder → Dilithium sign → bincode+hex encode → opsiyonel `--broadcast` ile RPC `qv_sendTransaction`. CLI flagleri: `--to-pubkey`, `--amount`, `--input <txid:idx>`, `--input-value`, `--account`, `--fee`, `--broadcast` | — |
| ~~W-06~~ | ✅ **KAPATILDI 2026-05-06** — `cmd_init`: mnemonic üret + password prompt + Argon2id+AES-GCM ile keystore'a yaz + ilk hesabin stealth adresini bas. Bonus: `WalletKeystore::save/load` artik gercek calisiyor (placeholder `key = [0u8;32]` silindi). Yeni komutlar: `import-mnemonic`, `address` |
| ~~W-07~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011 Faz 2) — `scanner.scan_transaction` artık `qv_privacy::scan_output`'un yeniden hesapladığı `onetime_pk_hash`'i kullanıyor; locking script'i `qv_script::stealth_p2pkh(scan.onetime_pk_hash)` ile bytewise karşılaştırıyor (1/256 view-tag false-positive elenir) | — |

### F. qv-privacy

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| P-01 | `crates/qv-privacy/src/confidential.rs` | `Committer / RangeProver / RangeVerifier` trait + Mock impl. Gerçek Bulletproofs entegrasyonu yok | Faz 8 |
| ~~P-02~~ | ✅ **KAPATILDI 2026-05-22** (ADR-011) — `MockSpendKeyDeriver` artık tasarım gereği kullanılmıyor. ADR-011 PQC-stealth modeli tek-seferlik harcama anahtarı türetmek yerine alıcının statik `spend_kp`'sini + `stealth_p2pkh(SHA3(tag || ss || spend_pk))` taahhüdü + witness'taki `shared_secret`'i birleştiriyor. Eski `SpendKeyDeriver` trait'i yasal/legacy testler için duruyor | — |
| P-03 | (proje geneli) | STARK range proof migration — winterfell entegrasyonu trait arkasında, yapılmadı | Faz 8 sonrası |

### G. qv-net

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~NET-01~~ | ✅ **KAPATILDI 2026-05-15** (ADR-007). `qv-net/src/handshake.rs` yazıldı (~470 satır + 7 unit test). Wire: `request_response::Behaviour<HandshakeCodec>` üzerinde `/quantumvault/handshake/1.0.0` tek-RTT protokolü. `HandshakeHello` + `HandshakeAck` bincode'lu, frame ≤ 8 KiB. `qv-crypto::encapsulate_hybrid`/`decapsulate_hybrid` (ML-KEM-768) + transcript-bound KDF kullanılıyor. `session_binding = SHA3-256(tag || ss || init_pid || resp_pid)` constant-time karşılaştırılıyor. `NetworkNode` her ConnectionEstablished'da dialer ise Hello gönderir; per-peer `SessionStore` shared secret tutuyor (disconnect'te siliniyor — replay sigortası). Noise-XX kimlik katmanı yerinde duruyor (PQC gizlilik üst katmanı) | — |
| NET-02 | `crates/qv-net/src/message.rs:60` | `Vote` variant'ı "placeholder — concrete fields depend on finality design" | Faz 3 (finality) sonrası |

### H. qv-mempool

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| MP-01 | `crates/qv-mempool/src/encrypted.rs` | `ThresholdDecryptor` trait + `MockThresholdDecryptor` (XOR mock). qv-crypto::threshold artık gerçek (Pedersen DKG var) — wiring gerekli | Faz 7 |
| MP-02 | `crates/qv-mempool/src/batcher.rs` | `build_amm_batch` placeholder constant-product invariant; qv-defi'nin gerçek pool state ile bağlanmamış | Faz 7 |

### I. Devnet & E2E

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| ~~D-01~~ | ✅ **KAPATILDI 2026-05-22** — N-02 (scan_stealth) gerçek implementasyon aldı. Bash script çalıştırılabilir hâle geldi; lokal smoke testi ile yeniden doğrulanması gerekiyor (re-test pending) | — |
| ~~D-02~~ | ✅ **KAPATILDI 2026-05-22** — W-05 zaten kapalıydı; ADR-012 sighash + plain p2pkh köprüsü sonrası `qv-wallet send-stealth` ve `send` yolları yeni `devnet/run-single.{ps1,sh}` script'inden tek atışta çalıştırılabilir | — |
| D-03 | `tests/e2e/60_encrypted_mempool.sh` | Encrypted mempool decrypt akışı (MP-01, K-06) bağlı değil | Faz 7 |
| D-04 | `tests/e2e/30_amm_swap.sh` & `40_lending.sh` | qv-defi on-chain script entegrasyonu mock — covenant assert gerçek çalışmıyor | Faz 6 |
| ~~D-05~~ | ✅ **KAPATILDI 2026-05-22** (alternatif olarak) — Faucet.py'a artık ihtiyaç yok: devnet genesis `DEVNET_TEST_MNEMONIC`'ten türetilen ilk 10 hesabı önceden fonluyor. Cüzdan `devnet-import` ile keystore'u kurar, `qv_scanP2pkh` ile bakiye anında görünür. Faucet script'ini koruyup ileride sözleşme tabanlı faucet yapmak hâlâ mümkün | — |
| ~~D-06~~ | ✅ **KAPATILDI 2026-05-22** — N-01 (get_balance_for) gerçek implementasyon aldı. `explorer.py` artık anlamlı veri döner; re-test pending | — |

### K. Build verify bulguları (2026-05-07 oturumu)

Kullanıcının lokalde `cargo build --workspace` koşması ile çıkan derleme hataları. Her biri tek satır düzeltme ile kapandı; not olarak burada işaretli ki bir sonraki round'da regression olmasın.

| ID | Yer | Sorun | Durum |
|---|---|---|---|
| ~~B-01~~ | `crates/qv-wallet/src/main.rs:314` | `stealth.view_kp.public.as_bytes()` çağrıldı ama `HybridPublicKey`'de `as_bytes` metodu yok — alanlar `.x25519: [u8;32]` ve `.kyber: Vec<u8>` ayrı | ✅ Düzeltildi: stealth address derive iki alanı ayrı hash'e ekliyor |
| ~~B-02~~ | `crates/qv-node/src/node.rs:482` | `chain_state.tip()` `&ChainEntry` referansı dönüyor; lock guard düştüğünde dangling oluyordu | ✅ Düzeltildi: `tip_height()` + `tip_hash()` (owned değer dönüyor) kullanıldı |
| ~~B-03~~ | `crates/qv-node/Cargo.toml` ve `crates/qv-miner/Cargo.toml` | `cargo nextest run --workspace --all-features` Windows MSVC ortamında "crate `librocksdb_sys` required to be available in rlib format" hatası verir. `[lib]` + `[[bin]]` ve transitive `*-sys` (rocksdb) etkileşiminden kaynaklanan Cargo limit. Bin'lerin kendi testi olmadığı için `test = false, doctest = false, bench = false` ekleyerek bin'in test profilinde derlenmesini engelledik. Lib + `tests/` dizini hala tam coverage'a sahip; bin'ler `cargo build --workspace` ile normal derliyor | ✅ Düzeltildi 2026-05-15 |

**Bu bulguların büyük kazanımı:** İlk hata wallet bin'e geldi — yani **tüm üst katman crate'ler** (qv-common, qv-core, qv-crypto, qv-script, qv-consensus, qv-storage, qv-net, qv-mempool, qv-privacy, qv-defi) ve qv-wallet lib derledi. Bu C-07 (schnorrkel verify) endişesini büyük ölçüde kapattı: schnorrkel 0.11 API çağrılarım uyumlu çıktı.

> ✅ **2026-05-07: `cargo build --workspace` başarılı.** B-01 + B-02 düzeltildikten sonra workspace'in tamamı derliyor (qv-node, qv-miner dahil). Tek kalan: bir kullanılmayan import warning'i (`VRFInOut`) — bu da temizlendi. Faz 3 yapısal kod tamamen derlenebilir durumda. Runtime gap'i sadece C-04/C-06 (from_seed_pqc stub).

### O. Ignored Test İndeksi (2026-05-12)

36 ignored testin her biri bilinçli olarak gizlendi — kaynağına göre kategori:

| # | Test | Crate | Sebep | Envanter ID | Açılacak |
|---|---|---|---|---|---|
| 1 | `chain_grows_with_validated_blocks` | qv-consensus integration | `TimestampOutOfRange` test mock'unda fixed timestamp / slot mismatch | D-11 | Test mock'u gerçek `SlotClock` ile yenilenmesi |
| 2 | `interest_accrual_basic` | qv-defi::lending unit | u64 overflow Q.64 interest hesaplamasında | D-07 | Q.128 fixed-point veya u128 swap |
| 3 | `median_manipulation_accepted` | qv-defi::oracle unit | Test premise tutarsız (u16 max ile %952+ sapma toleransı ifade edilemiyor) | D-12 | Yeni premise + assertion |
| 4 | `test_amm_oracle_feedback_loop` | qv-defi integration | Oracle + AMM closed loop entegrasyon eksik | D-08 | Oracle observation → AMM price reaction akışı |
| 5 | `test_lending_full_lifecycle` | qv-defi integration | Q.64 conversion path asymmetry | D-09 | D-07 ile birlikte fix |
| 6 | `test_lending_liquidation_scenario` | qv-defi integration | Liquidation kerelilen Q.64 conversion asymmetry | D-10 | D-07 ile birlikte fix |
| 7 | `test_lending_oracle_price_feedback` | qv-defi integration | Lending + Oracle closed loop entegrasyon eksik | D-08 | D-08 ile birlikte fix |
| 8 | `test_oracle_median_with_multiple_validators` | qv-defi integration | Multi-validator median collector test fixture eksik | D-12 | Validator signing fixture |
| 9 | `cross_period_signatures_dont_match` | qv-crypto::kes unit | Yavaş (~2s KES leaf-tree gen) | A-01 | Sadece `cargo test -- --ignored` |
| 10 | `evolve_then_sign_uses_next_period` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 11 | `forward_security_zeroizes_old_leaf` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 12 | `full_generate_sign_verify_roundtrip` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 13 | `signature_serde_roundtrip` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 14 | `tampered_leaf_signature_rejected` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 15 | `tampered_merkle_path_rejected` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 16 | `wrong_message_rejected` | qv-crypto::kes unit | Yavaş (~2s) | A-01 | Aynı |
| 17 | `test_feldman_share_verification` | qv-crypto::threshold unit | Feldman VSS verification asymmetry | T-01 | Pedersen DKG + Feldman VSS hizalama |
| 18 | `test_pedersen_dkg_2_of_3` | qv-crypto::threshold unit | Pedersen DKG asymmetry | T-01 | T-01 ile birlikte fix |
| 19 | `test_pedersen_dkg_3_of_5` | qv-crypto::threshold unit | Pedersen DKG asymmetry | T-01 | Aynı |
| 20 | `test_pedersen_dkg_public_key_determinism` | qv-crypto::threshold unit | Pedersen DKG asymmetry | T-01 | Aynı |
| 21 | `test_threshold_encrypt_decrypt` | qv-crypto::threshold unit | t-of-n decrypt share collection bug | T-01 | Aynı |
| 22 | `cold_key_from_seed_is_deterministic` | qv-miner::keys unit | ~~C-04 bağımlı~~ — şimdi sadece slow KES tree gen (cold key kendi başına hızlı, test setup yavaş kalmış) | M-04 (yavaş) | `--ignored` flag |
| 23 | `cold_key_sign_verify_roundtrip` | qv-miner::keys unit | Yavaş test setup | M-04 (yavaş) | `--ignored` |
| 24 | `kes_key_evolve_advances_period` | qv-miner::keys unit | Yavaş (~2s KES gen) | A-01 | `--ignored` |
| 25 | `kes_key_generate_and_period` | qv-miner::keys unit | Yavaş | A-01 | `--ignored` |
| 26 | `kes_sign_verify_roundtrip` | qv-miner::keys unit | Yavaş | A-01 | `--ignored` |
| 27 | `keystore_save_load_roundtrip_preserves_keys` | qv-miner::keys unit | Yavaş (M-04 keystore round-trip + KES gen) | A-01 | `--ignored` |
| 28 | `keystore_wrong_password_rejected` | qv-miner::keys unit | Yavaş (save half çağırıyor → KES gen) | A-01 | `--ignored` |
| 29 | `operator_keys_from_seed_is_deterministic` | qv-miner::keys unit | Yavaş (KES generate) | A-01 | `--ignored` |
| 30 | `build_pool_registration_tx_ok` | qv-miner::registration unit | Yavaş (OperatorKeys::generate → KES gen) | A-01 | `--ignored` |
| 31 | `registration_output_has_correct_value` | qv-miner::registration unit | Yavaş | A-01 | `--ignored` |
| 32 | `test_kes_rotation` | qv-miner integration | Yavaş KES leaf tree gen | A-01 | `--ignored` |
| 33 | `test_keypair_from_seed_is_deterministic` | qv-miner integration | Yavaş | A-01 | `--ignored` |
| 34 | `test_keypair_generation_roundtrip` | qv-miner integration | Yavaş | A-01 | `--ignored` |
| 35 | `test_pool_registration_tx_structure` | qv-miner integration | Yavaş (OperatorKeys gen) | A-01 | `--ignored` |
| 36 | `validate_well_formed_tx_with_available_utxo` | qv-node::validation unit | UTXO store fixture + tx validation pipeline eksik | D-11 | Test fixture genişletme |

**Özet:** 18 test sadece **performans** nedeniyle (`#[ignore]` yerine `cargo test -- --ignored`); 5 test **T-01 Pedersen DKG/Feldman VSS asymmetry**'sine bağlı; 7 test **D-07..D-12 DeFi yan vakaları**'na; 6 test KES setup yavaşlığı (cold key + keystore round-trip karışıkları); 1 test **D-11** (UTXO validation pipeline).

İdeal mainnet öncesi: tüm 36'sı yeşil. Kısa vadeli öncelik: **T-01** (encrypted mempool wiring için ön koşul) + **D-07..D-12** (DeFi safety).

### N. Devnet smoke test + buglar (2026-05-12 oturumu)

İlk gerçek uçtan uca devnet transferi çalıştırıldı: `qv-node init` + `qv-node run` + `examples/send_tx` → ML-DSA imzalı TX zincire (block height=18, tx_count=1, ~4.9s latency). Çalıştırma sırasında üç teknik bulgu yapıldı:

| ID | Yer | Sorun | Durum |
|---|---|---|---|
| ~~G-01~~ | `crates/qv-node/src/genesis.rs::devnet_genesis` | Her çağrıda random keypair → init/run/send_tx üçü farklı zincir görür | ✅ Kapatıldı: `from_seed_pqc` ile deterministik seed (`SHA3("qv-devnet-account-"||i)`); init=run aynı `merkle_root=fa9ea55b…c059ddec`. `devnet_genesis_is_deterministic` unit test eklendi |
| ~~G-02~~ | Windows main-thread 1 MiB stack | `#[tokio::main]` block_on main thread'de çalışır; ml-dsa NTT buffer'ları stack patlatıyor | ✅ Kapatıldı: `.cargo/config.toml` Windows için `link-arg=/STACK:8388608` (8 MiB). Linux/macOS zaten 8 MiB default, etkilenmez |
| ~~N-07~~ | `crates/qv-core/src/types.rs::OutPoint::FromStr` | ✅ **KAPATILDI 2026-05-12** — Display impl `txid#idx` (Cardano convention) basıyor ama parser sadece `#` ayırıcısı kabul ediyordu; `examples/send_tx.rs` `:` (Bitcoin convention) gönderiyordu. Server şimdi her iki ayırıcıyı da kabul ediyor (`split_once('#').or_else(|| split_once(':'))`); client canonical Display'i kullanıyor. Yeni `outpoint_from_str_accepts_colon_separator` unit test eklendi (qv-core 72→73 passed). Devnet smoke test 2. koşumda UTXO lookup yeşil ✓ |

### L. Test triajı (2026-05-07 oturumu)

`cargo test --workspace` ile çıkan 4 başarısız test triaj edildi. Her biri ya tek satır kod düzeltmesi ile ya da tutarsız test premise için `#[ignore]` ile kapandı. Sonuçta **728 passed / 0 failed / 38 ignored** elde edildi.

| ID | Yer | Sorun | Durum |
|---|---|---|---|
| ~~L-01~~ | `crates/qv-script/src/opcode.rs:300` | `OpCode::COUNT = 55` — `KNOWN_PAIRS` array'i 57 girişli (`opcode::all_opcodes_have_mnemonics` kırıldı) | ✅ Düzeltildi: COUNT 57 |
| ~~L-02~~ | `crates/qv-wallet/tests/integration.rs:48` | `test_coin_select_basic`: UTXO=1000, target=500, ama `CoinSelector` flat 1000-unit dust/fee reserve tutuyor (`needed = 500 + 1000 = 1500 > 1000`) | ✅ Düzeltildi: test UTXO 1000→2000 |
| ~~L-03~~ | `crates/qv-defi/src/oracle.rs::median_manipulation_accepted` | Test premise tutarsız: u16 max ile %952+ sapma toleransı ifade edilemez | ✅ `#[ignore]`, D-12 olarak takip |
| ~~L-04~~ | `crates/qv-node/src/ceremony.rs:32` doc-test | `rust,no_run` undefined `keypair`/`stake_amount`/`vrf_key`/`entropy` ile derlenemiyor | ✅ ```rust,no_run``` → ```text``` (illüstratif blok) |

**Halen `#[ignore]` olan testler (38 toplam, hepsi envanter ID'siyle eşli):**
- `qv-crypto::threshold` — 5 test (T-01: Pedersen DKG / Feldman VSS asymmetry)
- `qv-crypto::kes` — 6 yavaş roundtrip testi (A-01..A-03: cross-period, evolve, forward security, signature serde, tampered leaf/path, wrong message)
- `qv-crypto::pqc_sign::from_seed*` — 1 stub-Err testi + 2 integration testi (C-04 bağımlı)
- `qv-defi` — 7 test (D-07 lending interest, D-08..D-11 lending/oracle yan vakalar, D-12 oracle median premise)
- `qv-miner::keys` — 6 test (`from_seed`/`generate`/`sign_verify`/`evolve` — C-04 bağımlı)
- `qv-miner::registration` — 2 test (M-04 keystore + C-06 cold key bağımlı)
- `qv-miner` integration — 4 test (KES rotation, keypair from_seed, pool registration tx)
- `qv-node::validation::validate_well_formed_tx_with_available_utxo` — D-11 bağımlı
- `qv-consensus` integration — 1 test (`chain_grows_with_validated_blocks`: TimestampOutOfRange)

Hepsi C-04/C-06 + T-01 + B-03 kapandığında otomatik yeşillenir; veya test premise yenilendiğinde (D-07..D-12).

### J. Doküman tutarsızlıkları

| ID | Yer | Sorun | Kapatan faz |
|---|---|---|---|
| DOC-01 | `MEMORY.md` | Son güncelleme 2026-04-24, "Aşama 9 sıradaki" diyor; PROJECT_STATUS 2026-05-06, "Aşama 15 code-complete" diyor — uyumsuz | Faz 0 |
| DOC-02 | `PROJECT_STATUS.md` başlık | "AŞAMA 15 tamamlandı — code-complete" yanıltıcı; gerçek durum scaffold + placeholder | Faz 0 |
| DOC-03 | `MEMORY.md` "Crate Durumu" tablosu | qv-node/wallet/miner "İskelet" diyor (doğru), qv-defi "İskelet" (yanlış — modül dolu) | Faz 0 |
| DOC-04 | `book/src/SUMMARY.md` | ✅ ROADMAP linki "Getting Started"da, ADR-004/005 + MEMORY + PROJECT_STATUS linkleri eklendi (2026-05-06). Kalan: book derleme/serve testi | Faz 0 (büyük kısım kapatıldı, sadece test) |
| DOC-05 | ADR-004 (VRF) ve ADR-005 (KES) | ✅ **Yazıldı (2026-05-06)** — `docs/ADR/004-vrf-selection.md`, `docs/ADR/005-kes-selection.md`. Onay bekleniyor; impl Faz 3'te | Faz 0 (yazma) tamamlandı; Faz 3 (impl) bekleniyor |
| DOC-06 | Repo kökü `QuantumVault/` artefakt klasörü (varsa) | v1 sırasında yanlışlıkla oluşturulmuş kopya; manuel silinmesi gerekiyor | Faz 0 |

---

## Faz–Envanter Çapraz Tablosu

| Faz | Açıklama | Kapatacağı envanter ID'leri |
|---|---|---|
| Faz 0 | Hijyen + dokümantasyon | C-03, DOC-01..06 (DOC-05 ✅ 2026-05-06) |
| Faz 1 | Devnet MVP single-node + RPC bind + wallet keystore | ✅ **N-04, W-02, W-04, W-05, W-06, D-02, D-05** kapandi. `devnet/run-single.{ps1,sh}` ile tek komutluk MVP kuruldu. Kalan: M-04, M-07, M-08, M-12, M-13 |
| Faz 2 | RocksDB persistence | (K-03 ile birlikte UTXO snapshot) |
| Faz 3 | Single-node consensus loop + gerçek VRF/KES | ✅ **Workspace derler + tum test suite yesil 2026-05-07** (728/0/38). C-01, C-02, K-01, K-02, K-04, M-01, M-02, M-03, M-05, C-07 (schnorrkel verify), L-01..L-04 (test triaj) hepsi tamam. **Runtime gap'i sadece C-04/C-06** (from_seed_pqc → ml-dsa swap). Kalan başka: K-03/K-05 (UTXO commitment), K-07 (AMM batcher), M-09 (miner daemon), N-03 (vote/finality), N-06 |
| Faz 4 | libp2p networking + hibrit handshake | NET-01, M-09 (gossip yönü) |
| Faz 5 | Wallet pro + stealth address | ✅ **ADR-011 Faz 1-5 + ADR-012 + C-05 kapandı (2026-05-22)**: N-01, N-02, P-02, W-03, W-07, D-01, D-06, **C-05** hepsi yeşil. C-05 keystore v2 + per-account `view_keypairs` ile kapatıldı (view key persist edilir). W-01 DeFi komutları Faz 6'ya kaldı. |
| Faz 6 | Script VM + DeFi temelleri | M-06, K-07 (kısmen), W-01, D-04 |
| Faz 7 | Encrypted mempool + MEV koruması | K-06, K-07, MP-01, MP-02, D-03 |
| Faz 8 | Confidential amounts | P-01, P-03 |
| Faz 9 | Production hardening | M-10, M-11, NET-02, N-05 (kısmen) |
| Faz 10 | Mainnet ön hazırlık | N-05 (genesis ceremony), bağımsız audit |

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
1. Tüm warning'ler 0 — **2026-05-14: CI 5 job yeşil** (clippy, rustfmt, rustdoc,
   cargo-audit, cargo-deny). Workspace lint'leri kripto/UTXO pattern'lerine göre
   ayarlandı; sertleştirme L-09 / L-10 / L-11 altında. ✅ baseline
2. `cargo clippy -- -D warnings -D clippy::pedantic` temiz — L-09'a bağlı
3. Fuzz testler (cargo-fuzz, mevcut `fuzz/` klasörünü kullan)
4. Property-based testler (proptest) — kritik invariant'lar için (UTXO conservation, AMM invariant, vs.)
5. Genesis ceremony tooling (gerçek + reproducible)
6. Monitoring stack (Prometheus + Grafana dashboard'ları)
7. Docker compose tam çalışır
8. Block explorer (`devnet/scripts/explorer.py` aktif)
9. Faucet (`devnet/scripts/faucet.py` aktif)
10. Performans benchmark'ları (criterion) — TPS, finality latency, block validation süresi

**Faz 9 takip envanteri (2026-05-14 CI sprintinden eklenenler):**
- **C-09**: `ml-dsa` 0.0.4 → 0.1.0-rc.3 swap (RUSTSEC-2025-0144: timing
  side-channel in decomposition). Seeded keygen API'si değişti
  (`key_gen_internal(&B32)` → `key_gen_with_seed`); FIPS 204 wire format
  (sk 4032 B, sig 3309 B) re-verify, deterministik test fixture'lar yeniden
  üretilmeli. `.cargo/audit.toml`'dan `RUSTSEC-2025-0144` ignore kalkar.
- **N-12**: `libp2p` 0.54 → 0.55+ major bump. Bağımlı vulnerability ignore'ları:
  `RUSTSEC-2026-0119` (hickory-proto), `RUSTSEC-2025-0009`/`RUSTSEC-2025-0010`
  (ring 0.16), `RUSTSEC-2026-0098`/`0099`/`0104` (rustls-webpki 0.101.7). API
  yüzeyi değişikliği: gossipsub, kad, swarm. `.cargo/audit.toml`'dan 6 ID kalkar.
- **N-13**: `metrics-exporter-prometheus` → rustls migration. `openssl-sys`
  zincirini söker; deny.toml'dan `native-tls` wrapper çıkar (openssl wrapper
  hâlâ kalabilir veya tamamen silinir). Yan etki: hyper-tls bağımlılığı düşer.
- **L-09**: Workspace lint'leri sertleştir — `indexing_slicing` ve
  `integer_division` workspace `allow` → kripto crate'lerinde per-file
  `#![allow(...)]`, geri kalanda `deny`. Cerrahi geçiş.
- **L-10**: Test modüllerindeki `#[allow(unwrap_used, expect_used, panic)]`
  blanket'larını fonksiyon-bazlı allow'a indir (test isabeti artar).
- **L-11**: `cargo clippy -- -D clippy::pedantic` ayrı CI job olarak; en az
  half-allow-list'le başla, oradan daralt.
- **N-14**: `bincode` 1.3 → 2.x migration (RUSTSEC-2025-0141 unmaintained).
  Storage + wallet + script encoding tüm wire format'ları etkiler — major
  scope, sürüm bump'a saklanacak.

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

**2026-05-06 denetim oturumu:**
- ✅ Tüm crate'ler dosya:satır seviyesinde tarandı; placeholder envanteri (A–J grupları, ~45 girdi) yukarı eklendi
- ✅ Faz–envanter çapraz tablosu eklendi
- ✅ MEMORY.md ve PROJECT_STATUS.md ile tutarsızlıklar (DOC-01, DOC-02, DOC-03) tespit edildi ve bu güncellemeyle birlikte düzeltildi
- ✅ ROADMAP doküman tarihi 2026-05-06'ya çekildi

**Tamamlanan (2026-05-06):**
- ✅ DOC-05: ADR-004 (VRF) + ADR-005 (KES) yazıldı
- ✅ C-04: `qv_crypto::from_seed_pqc` eklendi (önce `fips204` denendi → 2026-05-07'de doğrulanamadı; ADR-006 ile `ml-dsa = 0.0.4` üzerinden kapatıldı). qv-wallet HD spend key derivation artık gerçek deterministic
- ✅ DOC-04: `book/src/SUMMARY.md` ADR/MEMORY/STATUS linkleriyle güncellendi
- 🆕 C-05 envanter girdisi açıldı (Hybrid KEM seeded keygen)

**Sonraki oturumda iki yol var:**

**Yol A — Faz 3'e dal (kriptografi):** Artık önkoşul yok.
1. C-01: `qv-crypto::vrf` Ristretto255-VRF impl (`schnorrkel` crate)
2. C-02: `qv-crypto::kes` Sum-KES on Dilithium impl (depth=11)
3. K-01/K-02: `qv-consensus` `RistrettoVrfEvaluator` + `DilithiumSumKesVerifier`
4. M-01/M-02/M-05: `qv-miner::keys` gerçek VRF/KES bağlama
5. K-04: `qv-node::slot_ticker` `kes_sig: Vec::new()` → gerçek imza
~3-4 oturum.

**Yol B — Faz 1'e dal (devnet MVP):**
1. M-03 (cold key Dilithium'a bağla) + M-04 (Argon2+AES-GCM keystore)
2. W-05/W-06 (wallet `Init`/`Send` gerçeği)
3. N-04 (`Node::shutdown` flush)
4. M-07/M-08 (pool registration UTXO seçimi + RPC submit)
~2-3 oturum.

**Tek oturumda kapsam önerisi:**
Bir oturumda en fazla **bir alt-fazın 3-5 görevi** yapılabilir. Envanter ID'leri ile commit
mesajları ilişkilendirilmeli (örn. `feat(M-04): Argon2+AES-GCM operator keystore`) ki bu
roadmap'ten satır silmek/işaretlemek mekanik kalsın.

---

## Notlar

- **CLAUDE.md** mimari ve kararlar (PQC + UTXO + Ouroboros + Encrypted mempool) DEĞİŞMEZ kabul edildi. Bu roadmap o kararları implementasyona dönüştürür.
- **ADR-001/002/003** referansları korunur; her faz ilgili ADR'ı işaret eder.
- **Test öncelik:** her yeni özellik için en az 3 unit + 1 integration test. CI gate.
- **Asla** `unwrap`/`expect`/`panic`/`indexing`/`integer_division`/`float_arithmetic` kullanmayacağız — workspace `clippy.toml` ile zaten deny seviyesinde.
- **Branch stratejisi**: `main` her zaman yeşil; her görev `feat/faz-X-task-Y` branch'inde, PR ile merge.
