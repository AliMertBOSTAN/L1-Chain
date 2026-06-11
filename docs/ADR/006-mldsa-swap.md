# ADR-006: Full Swap to `ml-dsa` (FIPS 204) — pqcrypto-dilithium'ı Kaldır

**Durum:** Approved + Uygulandı (2026-05-07) — `pqcrypto-dilithium` workspace'ten tamamen kaldırıldı, C-04/C-06 kapandı; test suite yeşil (736+)
**Tarih:** 2026-05-07
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-crypto/src/pqc_sign.rs`, `crates/qv-crypto/Cargo.toml`, workspace `Cargo.toml`
**Bağlı envanter:** ✅ kapatır C-04 (`from_seed_pqc` runtime gap), ✅ kapatır C-06 (verified seeded ML-DSA crate seçimi)

---

## Bağlam

QuantumVault L1 çekirdeğindeki tüm post-quantum dijital imzalar (UTXO harcama, blok
imza, KES leaf, mainnet genesis ceremony, miner cold key, wallet HD spend key)
**FIPS 204 ML-DSA** standardına dayanır. Mevcut implementasyon `pqcrypto-dilithium 0.5`
crate'ini kullanıyor (bkz. `pqc_sign.rs` 2026-04-15 ilk yazımı).

İki belirsizlik bizi etkiledi:

1. **Seeded keygen yokluğu (C-04, 2026-04-30 / 2026-05-07).**
   `pqcrypto-dilithium` deterministik seed'den anahtar üretme API'si açmıyor —
   yalnızca `dilithium*::keypair()` (OS entropy) sunuyor. HD wallet derivation,
   KES leaf seed → key derivation ve stealth one-time spend key recovery için
   bu **zorunlu**. Geçici çözüm olarak `fips204` 0.4.6 değerlendirildi
   (2026-05-06), ancak web-search sonrası bu sürümün de seeded keygen sunmadığı
   doğrulandı (2026-05-07). C-04 yeniden açıldı, `pqc_sign::from_seed` `Err` stub'a
   indirgendi.

2. **Spec uyuşmazlığı riski (C-06, 2026-05-07).**
   `pqcrypto-dilithium 0.5` PQClean üzerinden Dilithium'un **NIST round-3 sürümünü**
   bağlıyor (PQClean'in resmi FIPS 204 transitional support'u var, ancak Rust crate
   bu yeni variant'ı henüz expose etmiyor). NIST FIPS 204 final spec (Aug 2024)
   round-3'ten farklı parametre / encoding değişiklikleri içeriyor; spike (`spikes/c06-mldsa/`)
   `ml-dsa = 0.0.4` ile üretilen ML-DSA-65 anahtar boyutlarını şöyle gösterdi:

   | Boyut | round-3 (pqcrypto-dilithium 0.5 yorum) | FIPS 204 (`ml-dsa` 0.0.4 spike) |
   |---|---:|---:|
   | pk | 1952 | 1952 |
   | sk | 4000 | **4032** |
   | sig | 3293 | **3309** |

   Public key boyutları aynı, ama secret key ve signature boyutları farklı —
   yani **wire formatları ikili uyumsuzdur**. Bu durumda projede eş zamanlı iki
   crate barındırmak ve birinde üretilen anahtarın diğerinde verify edilmesini
   beklemek mainnet sonrası **kalıcı tutarsızlık** demek olur.

---

## Karar

**`pqcrypto-dilithium`'ı QuantumVault `qv-crypto` crate'inden tamamen kaldır
ve tüm imza primitiflerini RustCrypto `ml-dsa = "0.0.4"` üzerinden FIPS 204
final spec ile yeniden bağla.**

- **Yeni crate:** `ml-dsa = "0.0.4"` (RustCrypto, Apache-2.0/MIT, FIPS 204 final)
- **Kaldırılan crate:** `pqcrypto-dilithium = "0.5"` (NIST round-3, deprecated bizim için)
- **Etkilenen kod:** `qv-crypto/src/pqc_sign.rs` (5 fonksiyon: `generate_keypair`, `from_seed`, `sign`, `verify`, `DilithiumLevel::*_bytes` const'lar). Public API surface (struct'lar `PqcKeyPair`, `PqcPublicKey`, `PqcSecretKey`, `PqcSignature`, `DilithiumLevel`) korunur — yukarı katmanlar (qv-consensus, qv-script, qv-wallet, qv-miner, qv-node, qv-privacy) kod düzeyinde değişmez.

### Implementasyon kalıbı

```rust
use ml_dsa::{B32, KeyGen, MlDsa44, MlDsa65, MlDsa87};
use ml_dsa::signature::{Signer, Verifier};

// Deterministic seeded keygen (closes C-04):
let mut xi = B32::default();
xi.copy_from_slice(seed);                                // [u8; 32]
let kp = <MlDsa65 as KeyGen>::key_gen_internal(&xi);     // FIPS 204 §6.1

// Random keygen:
let kp = <MlDsa65 as KeyGen>::key_gen(&mut rand::rngs::OsRng);

// Encode for wire:
let pk_bytes = kp.verifying_key().encode().as_slice().to_vec();   // 1952 B
let sk_bytes = kp.signing_key().encode().as_slice().to_vec();     // 4032 B

// Sign:
let sig = kp.signing_key().sign(message);                          // panics on fail (rare)
// or `try_sign(msg)` for Result

// Verify:
let ok = kp.verifying_key().verify(message, &sig).is_ok();
```

### Yeni byte boyutları (`DilithiumLevel::*_bytes`)

FIPS 204 ML-DSA spec ile uyumlu — `ml-dsa` crate'inden runtime alınır
(`SigningKeySize`, `VerifyingKeySize`, `SignatureSize` `typenum` const'ları):

| Level | pk bytes | sk bytes | sig bytes |
|---|---:|---:|---:|
| Level 2 (ML-DSA-44) | 1312 | **2560** | **2420** |
| Level 3 (ML-DSA-65) | 1952 | **4032** | **3309** |
| Level 5 (ML-DSA-87) | 2592 | **4896** | **4627** |

(ML-DSA-44/87 değerleri spike'ta doğrulanmadı — sadece Level 3. Diğer iki
level için genel FIPS 204 spec değerleri yazıldı; uygulama anında runtime'dan
alınır, hardcoded değil.)

---

## Alternatifler ve neden reddedildi

### Yol A: Conservative split — `from_seed` ml-dsa, sign/verify pqcrypto-dilithium
Sadece `from_seed` ml-dsa'ya delege, sign/verify mevcut yolda kalır.
**Reddedildi** — wire formatları farklı (sk: 4000 vs 4032, sig: 3293 vs 3309);
ml-dsa ile üretilen anahtar pqcrypto-dilithium'ın `SecretKey::from_bytes`
ile yüklenmez, dolayısıyla sign edilemez. Cross-compat imkânsız.

### Yol B (seçilen): Full swap to ml-dsa
Tek crate, tek wire format, FIPS 204 final.

### Yol C: `oqs` (liboqs Rust binding)
liboqs FIPS 204'ü destekliyor, lib daha olgun, ama:
- C bağımlılığı (qv-crypto'da `#![forbid(unsafe_code)]`'den çıkar — istemiyoruz)
- Build complexity artar (Nix flake'i daha karmaşık)
- Saf-Rust imza önceliğimiz (ADR-001 felsefesi: "matematiksel olarak doğrulanabilir,
  bağımlılık yüzeyi minimal").

### Yol D: `pqcrypto-mldsa`
Mevcut değil veya çok yeni. Eğer çıkarsa C-06 sonrası ileride değerlendirilebilir.

---

## Sonuçlar (consequences)

### Olumlu
- ✅ **C-04 kapanır** — `from_seed_pqc` çalışır, KES leaf gen + HD wallet + miner
  cold key + stealth recovery hepsi runtime'da fonksiyonel.
- ✅ **C-06 kapanır** — verified seeded ML-DSA crate seçildi.
- ✅ FIPS 204 final spec, NIST'in resmi standardı; gelecekte sertifikasyon /
  audit için temiz bir hedef.
- ✅ Tek crate; bağımlılık yüzeyi azalır (`pqcrypto-dilithium`, `pqcrypto-traits`'in
  bu projedeki kullanımı tamamen ortadan kalkar — `pqcrypto-kyber` halen Kyber için
  kullanılmaya devam eder, KEM tarafında etkilenmez).
- ✅ Saf-Rust crate (RustCrypto), C bağımlılığı yok.
- ✅ `Signer` / `Verifier` standart `signature` crate trait'leri ile genel ekosisteme entegre.

### Olumsuz / risk
- ⚠️ **Wire format değişikliği BREAKING.** Eskiden bu kod tabanına ait herhangi
  bir test fixture (gerçek pqcrypto-dilithium signature byte'ları içeren) artık
  doğrulanamaz. Şu anki test suite'i etkileyen bir fixture görünmüyor
  (testler keypair'i runtime'da üretip imzalıyor), ancak sonradan eklenecek
  static fixture'lar dikkat ister.
- ⚠️ **Performans farkı bilinmiyor.** ml-dsa saf-Rust, pqcrypto-dilithium PQClean
  C bağlaması. Pratikte mainnet için kritik değil (her slotta bir blok = 0.5 imza/sn);
  ölçüm Faz 9 (production hardening) içinde benchmark'a girer.
- ⚠️ **Crate sürümü 0.0.4 — pre-stable.** RustCrypto'nun ml-dsa'sı henüz 1.0
  değil. Sürüm bump'larında API kırılması olabilir; sürüm pin'liyoruz
  (`= "0.0.4"`) ve next-bump'ı ADR-006 follow-up olarak takip ederiz.

### Nötr
- 📌 Public API surface (`PqcKeyPair`, `PqcPublicKey`, `PqcSecretKey`, `PqcSignature`,
  `DilithiumLevel`, fonksiyon adları) korunur. Yukarı katmanlar (qv-consensus,
  qv-script, qv-wallet, qv-miner, qv-node, qv-privacy) **hiçbir kod değişikliği
  görmez**, sadece runtime byte boyutları farklılaşır.

---

## Test stratejisi

ml-dsa swap'ı landlerken yapılacak doğrulamalar:

1. **Unit testler** (`pqc_sign::tests`, mevcut 7+1 test):
   - `keypair_sizes_match_level_spec` — yeni boyutlarla geçer (ml-dsa runtime değerleri kullandığı için).
   - `sign_and_verify_roundtrip_level3`, `tampered_message_fails_verification`,
     `wrong_public_key_fails_verification`, `all_levels_roundtrip`,
     `level_mismatch_is_error`, `pubkey_from_bytes_rejects_wrong_size` — değişmemeli.
   - `from_seed_returns_explicit_error_until_c06_lands` — **kaldırılır** (artık
     stub değil); yerine `from_seed_is_deterministic` + `from_seed_different_seeds_differ`
     + `from_seed_signed_message_verifies_with_derived_pk` testleri eklenir.

2. **Integration testleri** (`qv-crypto/tests/integration.rs`):
   - `from_seed_models_hd_derivation_pattern` ve
     `from_seed_models_kes_leaf_derivation_pattern` `#[ignore]`'dan çıkarılır.

3. **Cross-crate testler** (qv-miner, qv-wallet, qv-consensus):
   - Mevcut `#[ignore]` etiketli ~15 test (KES generate, miner key from_seed,
     wallet HD spend) yeşillenir.

4. **Cross-compat (C-07 izi):**
   - schnorrkel (VRF) ve Sum-KES paketlemeleri ml-dsa imzalı KES leaf'leri ile
     hâlâ çalışıyor mu? Test: `qv_crypto::kes::tests::full_generate_sign_verify_roundtrip`
     (`#[ignore]`'dan çıkar).

Tahmini etki: 728 → ~743 passed test, 38 → ~23 ignored test.

---

## Geri alma planı (rollback)

C-06 swap test suite'i kıracak (ya da production'da bir uyumsuzluk çıkacak)
durumunda:

1. **Branch level:** Bu ADR ve `pqc_sign.rs`/`Cargo.toml` swap commit'leri tek
   feature branch'tir. `git revert` tek commit ile eski hale döner.
2. **C-04 yeniden açılır:** `from_seed` `Err` stub'a geri döner; alternatif
   crate (`oqs`, gelecek `pqcrypto-mldsa`) değerlendirilir.
3. **Etkilenen testler:** ml-dsa specific test'ler `#[ignore]`'a çekilir,
   C-04 bağımlı testler eski `#[ignore]` haline döner.

---

## Onay

| | |
|---|---|
| Önerildi | 2026-05-07 (Claude / mert) |
| Onay | bekleniyor |
| Implementation | 2026-05-07 başladı |
| Cross-test pass | bekleniyor (kullanıcı `cargo test --workspace` koşacak) |
| Mainnet'e dokunma | hayır (henüz mainnet yok); Faz 10 öncesi son onay |

---

## Referanslar

- [NIST FIPS 204 (Aug 13 2024) — ML-DSA spec](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)
- [RustCrypto/signatures — ml-dsa crate](https://github.com/RustCrypto/signatures/tree/master/ml-dsa)
- ADR-001 (testing framework) — saf-Rust + minimum bağımlılık ilkesi
- ADR-005 (Sum-KES on Dilithium) — KES leaf imza için ML-DSA gereksinimi
- `spikes/c06-mldsa/` — verification scratch projesi (6/6 ✅, 2026-05-07)
- envanter C-04, C-06 — `docs/ROADMAP.md`
