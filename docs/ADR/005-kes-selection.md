# ADR-005: KES Seçimi — Forward-Secure Block İmzası

**Durum:** Önerildi — onay bekleniyor
**Tarih:** 2026-05-06
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-crypto/src/kes.rs` (şu an boş skeleton),
`crates/qv-consensus/src/block_validator.rs` (`KesVerifier` trait + `TestKesVerifier` mock),
`crates/qv-miner/src/keys.rs::KesKeyPair` (32-byte placeholder)

---

## Bağlam

**KES** (Key Evolving Signature), block producer'ın özel anahtarını her
periyotta evrimleyerek **forward security** sağlayan bir imza şemasıdır:
"Bugünkü anahtar dünkü imzaları üretemez." Klasik Nakamoto PoS'un can
yakıcı zayıflığı **uzun menzilli (long-range) saldırı**dır: eski validatörler
özel anahtarlarını sızdırır → saldırgan "alternatif tarih" yazar → light
client'lar yanlış zinciri kabul eder. KES bu pencereyi epoch granülerinde
kapatır.

Praos KES'in iki temel özelliği:

1. **Periodik evrim:** Anahtar `evolve(sk_t) → sk_{t+1}` operasyonuyla
   ilerler; eski `sk_t` **silinmek zorundadır** (forward security).
2. **Public key sabit:** `pk` bir kez `StakePool`'a yazılır; tüm periyot
   imzaları aynı `pk` ile doğrulanır.

`qv-crypto/src/kes.rs` şu an sadece doc-comment içeren boş bir dosya
(envanter ID **C-02**). `qv-consensus::block_validator::KesVerifier`
trait + `TestKesVerifier` mock kullanılıyor (envanter **K-02**).
`qv-miner::keys::KesKeyPair` ise sahte 32-byte placeholder
(envanter **M-02**, **M-05**) ve `evolve_to_next_period` sadece sayaç
artırıyor — gerçek forward security yok.

CLAUDE.md mimari kararı tüm imza primitiflerini **PQC** olarak gerektirir
(Dilithium tercihli). Bu ADR o ilkeye uyan KES'i sabitler.

---

## Kabul Kriterleri

Seçilecek KES:

1. **Forward security:** `sk_t` sızarsa, `t' < t` periyotlarındaki imzalar
   forge edilememeli.
2. **PQC güvenli:** Hem temel imza hem evrim primitifi kuantum dirençli olmalı.
3. **Sabit `pk`:** Tüm KES periyotları için tek `pk` (zincire bir kez
   yazılır).
4. **Verifiable evolution:** `verify(pk, period_t, sig, msg)` belirli bir
   periyodun imzasını doğrulayabilmeli.
5. **Makul anahtar boyutu:** N periyot için `O(log N)` ya da `O(√N)` —
   asimptotik olarak ölçeklenebilir.
6. **Audit edilmiş referans:** Production'da kullanılmış (Cardano), iyi
   bilinen pattern.
7. **`qv-consensus::block_validator::KesVerifier` trait kontratını koru:**
   API yüzeyi `verify(pk, period, sig, msg) → bool` etrafında.

### Boyutlandırma

QuantumVault parametreleri:
- 1 epoch = 12 saat = 21 600 slot
- 1 KES periyot = 1 epoch (Cardano modeli)
- Bir validator'ın çalışma ömrü ≈ 1-3 yıl ≈ 730-2190 epoch

→ KES tree depth `N = 2^11 = 2048` epoch yeterli (≈ 2.8 yıl). Ötesi için
**cold key ile yeniden registration** gerekir.

---

## Aday Yaklaşımlar

### Aday 1 — Sum-KES on Dilithium (PQC, "Dilithium-sum")

**Nasıl çalışır:** `MMM` (Malkin-Micciancio-Miner 2002) sum-composition
inşası, **N periyot** için **log₂(N) seviyeli ikili ağaç** oluşturur. Her
yaprakta taze bir Dilithium anahtar çifti vardır.

```
Period 0:    ┌── pk0 (Dilithium kp)   ── sk0 (live, sign with this)
             │
Period 1:    ├── pk1                  ── sk1 (will be derived)
             │
            pk_root = H(pk0_left || pk1_right)
```

`evolve()` yaprağı tüketildiğinde sol alt-ağaç bellekten silinir; saldırgan
elindeki `sk_t` ile yalnızca `≥ t` periyotlarına imza atabilir.

**İmza formatı:** Yaprak Dilithium imzası + ağaç boyunca **kardeş hash'leri**
(Merkle yolu) → ağaç kökü `pk_root` ile doğrulanır.

**Avantajlar:**
- ✅ **PQC** — Dilithium üzerinde inşa edildiği için kuantum dirençli
- ✅ Cardano modelini bire bir taklit eder (orada Ed25519-sum); olgun pattern
- ✅ `O(log N)` aktif anahtar bellek + `O(log N)` imza kardeş hash'leri
- ✅ Public key tek Merkle root (32 byte) — `pk_root = SHA3-256(...)`
- ✅ `qv-crypto::pqc_sign` zaten Dilithium kullanıyor → ek bağımlılık yok

**Dezavantajlar:**
- ❌ İmza boyutu: yaprak Dilithium L3 imzası ~3.3 KB + 11 hash (≈ 350 byte) → ~3.7 KB
  - Karşılaştırma: Cardano Ed25519-sum imzası ~700 byte
- ❌ Verify: bir Dilithium verify (~1 ms) + `log₂(N)` SHA3 (~50 µs) ≈ 1 ms
- ❌ Keygen: `N = 2048` için 2048 Dilithium kp × ~1 ms ≈ 2 s tek seferlik
  (operatör onboarding zamanı; sorun değil)

**Olgunluk:** Akademik kanıtlı (MMM 2002). Cardano sum-KES'i Ed25519
üzerinde production'da; Dilithium üzerine port etmek mekaniktir — yapraktaki
imza primitifi swap edilir.

---

### Aday 2 — Cardano-style Ed25519-sum KES (klasik)

**Nasıl çalışır:** Aday 1 ile aynı sum-composition, ama yapraklarda Ed25519.
Cardano'nun gerçek production KES'i.

**Avantajlar:**
- ✅ Production'da kanıtlanmış (Cardano 2020+)
- ✅ İmza boyutu küçük (~700 byte)
- ✅ Verify çok hızlı (~50 µs)
- ✅ `ed25519-dalek` zaten workspace'de

**Dezavantajlar:**
- ❌ **PQC değil** — Shor ile kırılır
- ❌ CLAUDE.md "tüm kriptografik primitif hibrit (klasik + PQC)" ilkesine
  doğrudan aykırı

**Verdict:** PQC ilkesini ihlal ettiği için **reddedildi**.

---

### Aday 3 — Forward-Secure Lattice Signature (FS-Dilithium variantları)

**Nasıl çalışır:** Dilithium'un kendisini forward-secure yapan lattice-spesifik
şemalar (e.g., Boyen-Eskandarian forward-secure puncturable signatures, 2024).

**Avantajlar:**
- ✅ Dilithium üzerinde direkt; ek ağaç yapısı yok
- ✅ Daha küçük state

**Dezavantajlar:**
- ❌ 2023+ akademik şemalar; production-grade Rust crate yok
- ❌ Audit yapılmamış
- ❌ Spec finalize edilmemiş

**Verdict:** Erken; takip listesinde.

---

### Aday 4 — Hibrit Sum-KES (Ed25519 + Dilithium yapraklarda paralel)

**Nasıl çalışır:** Her yaprakta hem Ed25519 hem Dilithium kp; iki imza
yapılır, birinin doğrulanması yetmez (and-composition).

**Avantajlar:**
- ✅ Klasik veya PQC'den biri kırılırsa diğeri korur
- ✅ CLAUDE.md hibrit ilkesinin doğal uzantısı

**Dezavantajlar:**
- ❌ İmza boyutu = sum(Ed25519 + Dilithium L3) + Merkle yolu ≈ 4 KB
- ❌ Verify maliyeti iki katı
- ❌ Sum-KES inşasında yaprak değişimi (pk hash değişir) zaten bir karmaşıklık;
  hibridi 2x karmaşık yapar

**Verdict:** İdeal hedef ama bugün ship edilebilir Aday 1'i tercih ediyoruz;
v2 hibrit geçişi açık tutulur.

---

## Karar

**MVP (mainnet v1):** **Aday 1 — Sum-KES on Dilithium ("Dilithium-sum KES").**

**Mainnet v2 / 2027 hedefi:** **Aday 4 — Hibrit Sum-KES**, gerçek production
ihtiyacı belirginleşince (hibrit imzanın maliyeti, gerçek tehdit modeline
karşı tartılacak).

### Gerekçe

1. **PQC ilkesi koruma:** Aday 2 reddedildi çünkü CLAUDE.md "tüm imza
   primitiflerini PQC" zorunluluğu var. Aday 1 doğrudan Dilithium üzerinde
   inşa ediliyor.
2. **Olgun pattern:** Sum-KES inşasının kendisi (ağaç yapısı + evolve)
   Cardano'da production'da; sadece yapraktaki imza primitifini swap
   ediyoruz.
3. **Mevcut kod tabanı uyumlu:** `qv-crypto::pqc_sign` Dilithium-3 + 5 + 2
   destekli; sum-KES yapraklarında **Dilithium Level 3** kullanacağız
   (CLAUDE.md varsayılanı).
4. **Boyut/performans makul:** ~3.7 KB KES imzası, ~1 ms verify. 2 saniye
   slot'ta darboğaz değil.
5. **Hibrit'e geçiş trait arkasında:** `KesVerifier` trait yerinde olduğu
   sürece v2'de Aday 4'e geçmek mekanik.

---

## Tasarım Detayları (Sum-KES on Dilithium)

### Parametreler

| Parametre | Değer | Gerekçe |
|---|---|---|
| Yaprak imza primitifi | Dilithium Level 3 (ML-DSA-65) | CLAUDE.md varsayılanı |
| Ağaç derinliği `d` | 11 | `2^11 = 2048` periyot |
| Maks periyot `N` | 2048 | ≈ 2.8 yıl @ 12 saat/epoch |
| Hash fonksiyonu | SHA3-256 | Workspace varsayılanı |
| Periyot süresi | 1 epoch (43 200 saniye) | Cardano modeli |

### Anahtar yapısı

```
KesPublicKey: [u8; 32]   // pk_root = SHA3-256(...) — sabit, on-chain

KesSecretKey {
    period: u32,                          // şu anki periyot
    leaf_kp: PqcKeyPair,                  // şu anki yaprak Dilithium kp
    sibling_hashes: [[u8; 32]; d],        // Merkle yolu kardeşleri
    // ... evolve için gereken minimum state
}
```

`KesSecretKey` **diskte şifrelenmiş tutulmalı** — Argon2id + AES-GCM
(envanter M-04 ile birlikte hayata geçer).

### `evolve(sk: KesSecretKey) → KesSecretKey`

Pseudocode:

```rust
fn evolve(sk: KesSecretKey) -> Result<KesSecretKey> {
    if sk.period + 1 >= N {
        return Err(KesError::Exhausted);
    }
    // 1. Eski yaprak kp'yi zeroize et (forward security)
    sk.leaf_kp.zeroize();

    // 2. Bir sonraki yaprak kp'yi türet (deterministic from seed?)
    //    — sum-KES standartı: yaprak kp seed'den türetilir; seed evrim
    //      operasyonuyla "ilerler" ve eski tohum silinir.
    let next_seed = derive_next_leaf_seed(&sk.evolve_state);
    let next_kp = PqcKeyPair::from_seed(&next_seed)?; // ⚠️ envanter C-04

    // 3. Merkle yolu kardeşlerini güncelle
    let new_siblings = update_sibling_hashes(&sk.sibling_hashes, sk.period + 1);

    Ok(KesSecretKey {
        period: sk.period + 1,
        leaf_kp: next_kp,
        sibling_hashes: new_siblings,
        ...
    })
}
```

**Bağımlılık (kritik):** `evolve` deterministik yaprak türetimi gerektirir —
**`PqcKeyPair::from_seed(seed: &[u8; 32])`** API'si. `pqcrypto-dilithium`
şu an seeded keygen desteklemiyor (envanter **C-04**).

→ ADR-005 implementasyonundan **önce** veya **eşzamanlı** olarak
`qv-crypto::pqc_sign::PqcKeyPair::from_seed` eklenmeli. İki seçenek:

1. `pqcrypto-dilithium`'a fork ile seed parametresi geç → upstream PR
2. Kendi Dilithium implementasyonuna geçiş (kapsam fazla)
3. **Önerilen:** `liboqs`'un C API'si seed kabul ediyor; `pqcrypto-dilithium`
   wrapper'a doğrudan FFI çağrısı ekle (tek fonksiyon)

### `sign(sk: &KesSecretKey, msg: &[u8]) → KesSignature`

```rust
pub struct KesSignature {
    pub period: u32,
    pub leaf_signature: PqcSignature,        // ~3.3 KB Dilithium L3
    pub sibling_hashes: Vec<[u8; 32]>,       // d=11 → 352 B
}
```

```rust
fn sign(sk: &KesSecretKey, msg: &[u8]) -> Result<KesSignature> {
    // Mesajla period bind: replay across periods imkansız
    let bound_msg = sha3_256(b"QuantumVault-KES-v1" || sk.period.to_le_bytes() || msg);
    let leaf_sig = qv_crypto::sign_pqc(&sk.leaf_kp.secret, &bound_msg)?;
    Ok(KesSignature {
        period: sk.period,
        leaf_signature: leaf_sig,
        sibling_hashes: sk.sibling_hashes.clone(),
    })
}
```

### `verify(pk_root: &KesPublicKey, sig: &KesSignature, msg: &[u8]) → bool`

```rust
fn verify(pk_root: &KesPublicKey, sig: &KesSignature, msg: &[u8]) -> Result<bool> {
    // 1. Yaprak Dilithium imzasını domain-bound mesajla doğrula
    let bound_msg = sha3_256(b"QuantumVault-KES-v1" || sig.period.to_le_bytes() || msg);
    let leaf_pk = derive_leaf_pk_from_signature(&sig.leaf_signature)?;
    if !qv_crypto::verify_pqc(&leaf_pk, &bound_msg, &sig.leaf_signature)? {
        return Ok(false);
    }
    // 2. Merkle yolu üzerinden pk_root'a yürü
    let computed_root = walk_merkle(
        leaf_pk_hash(&leaf_pk),
        sig.period,
        &sig.sibling_hashes,
    );
    Ok(computed_root == pk_root.0)
}
```

### Wire format (BlockHeader.kes_sig)

`Block.header.kes_sig` artık opak `Vec<u8>` değil; `bincode::serialize(&KesSignature)`
çıktısı. `qv-core::block::BlockHeader` boyut limiti zaten >= 4 KB destekliyor.

---

## Implementasyon Planı

### Crate seçimi

**Tercih:** Kendi sum-KES inşamızı yazmak; yaprak primitif `qv-crypto::pqc_sign`.
Sum-KES yapısı basit (Merkle ağacı + state machine) — dış crate eklemek yerine
~300 satır Rust ile inşa edilir.

Yedek: Cardano'nun Haskell `cardano-base` referansını okuyup port et.

### Public API (qv-crypto/src/kes.rs)

```rust
//! Sum-KES on Dilithium — forward-secure block signing.
//!
//! Per ADR-005. Yaprak imza primitifi Dilithium Level 3.

use crate::{CryptoError, Result, sign_pqc, verify_pqc, sha3_256};
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;

pub const KES_TREE_DEPTH: u32 = 11;
pub const KES_MAX_PERIODS: u32 = 1 << KES_TREE_DEPTH; // 2048

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KesPublicKey(pub [u8; 32]);

#[derive(Clone, ZeroizeOnDrop)]
pub struct KesSecretKey { /* period, leaf_kp, evolve_state, sibling_hashes */ }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KesSignature {
    pub period: u32,
    pub leaf_signature: Vec<u8>,       // bincode'd PqcSignature
    pub sibling_hashes: Vec<[u8; 32]>, // d=11
}

pub fn generate(seed: &[u8; 32]) -> Result<(KesPublicKey, KesSecretKey)>;
pub fn sign(sk: &KesSecretKey, msg: &[u8]) -> Result<KesSignature>;
pub fn verify(pk: &KesPublicKey, sig: &KesSignature, msg: &[u8]) -> Result<bool>;
pub fn evolve(sk: &mut KesSecretKey) -> Result<()>;
pub fn current_period(sk: &KesSecretKey) -> u32;
```

### qv-consensus entegrasyonu

`KesVerifier` trait zaten doğru yerde:

```rust
// Mevcut (mock):
pub trait KesVerifier {
    fn verify(&self, pk: &[u8], period: u64, sig: &[u8], msg: &[u8]) -> bool;
}
```

ADR-005 sonrası `DilithiumSumKesVerifier` impl'i eklenir; `TestKesVerifier`
test'ler için kalır.

### qv-miner entegrasyonu

`crates/qv-miner/src/keys.rs::KesKeyPair` (envanter **M-02**) bu modülün
`KesPublicKey + KesSecretKey` çiftine yönlendirilir. Sahte 32-byte placeholder
silinir.

`evolve_to_next_period` (envanter **M-05**): sayaç artırma yerine gerçek
`qv_crypto::kes::evolve(&mut sk)` çağrısı.

### qv-node entegrasyonu

`crates/qv-node/src/slot_ticker.rs:236`'daki `kes_sig: Vec::new()`
(envanter **K-04**) gerçek `kes::sign(&operator_kes_sk, &header_bytes_to_sign)`
çağrısına dönüşür.

### Test stratejisi

1. **Unit test (qv-crypto):**
   - Roundtrip: gen → sign → verify
   - Period binding: aynı msg, farklı period → farklı sig, çapraz verify fail
   - Forward security: `evolve` sonrası eski periyod için sign denemesi `Err`
   - Tampered Merkle path → verify false
   - Exhaustion: `period == N` sonrası sign `KesError::Exhausted`
2. **Integration test:** N=8 (küçük) ağaç ile tüm periyotları gez; her birini
   sign/verify; sonra evolve sonrası eski periyod sign denemesini test et
3. **Property test (proptest):** ∀ msg, period ∈ [0, N): verify(sign) == true
4. **qv-consensus integration:** `block_validator` zaten KES doğruluyor;
   `DilithiumSumKesVerifier` ile aynı integration'lar yeşil olmalı
5. **Benchmark:** keygen, sign, verify, evolve — hedefler:
   - keygen N=2048: < 5 s (one-time)
   - sign: < 5 ms
   - verify: < 5 ms
   - evolve: < 5 ms

---

## Geri Çevrilebilirlik (Hibrit Geçiş Planı)

Mainnet v2'de Aday 4 (hibrit Ed25519 + Dilithium):

1. Yaprak `PqcKeyPair` yerine `(Ed25519KeyPair, PqcKeyPair)` çift
2. `KesSignature.leaf_signature` artık `(ed_sig, dil_sig)` çifti
3. `verify` her ikisini de doğrular (and-composition)
4. Wire format dağıtık olarak değişir; **dual-KES dönemi** N epoch
5. Eski blok header'ları geçerli kalır

---

## Sonuçlar (Consequences)

**Olumlu:**
- Forward security: long-range saldırı penceresi 1 epoch'a iner
- PQC ilke korumalı (Dilithium yapraklar)
- Cardano kanıtlanmış sum-composition pattern'i
- `KesVerifier` trait kontratı bozulmuyor

**Olumsuz:**
- ~3.7 KB imza boyutu (Cardano'nun ~5 katı; ama PQC değiş tokuş)
- `PqcKeyPair::from_seed` API'sini açma zorunluluğu (envanter C-04 önkoşul)
- One-time keygen 2-5 saniye (operator onboarding'de yapılır, sorun değil)

**Kabul edilen risk:** N=2048 periyot tükendiğinde (≈ 2.8 yıl) operator
**cold key ile yeniden registration** yapmak zorunda. Bu Cardano'da da var;
operatif prosedür `docs/security/key-management.md`'a eklenir.

---

## Onay ve Implementasyon Takvimi

- [ ] Bu ADR review (1 oturum)
- [ ] **Önkoşul:** `qv-crypto::pqc_sign::PqcKeyPair::from_seed` ekle
      (envanter C-04) — ya `pqcrypto-dilithium` PR ya `liboqs` doğrudan FFI
- [ ] `qv-crypto/src/kes.rs` → API + sum-KES inşası + 6+ unit test
- [ ] `qv-consensus::block_validator::DilithiumSumKesVerifier` impl
- [ ] `qv-miner::keys::KesKeyPair` gerçek bağlama (envanter M-02, M-05)
- [ ] `qv-node::slot_ticker.rs` → `kes_sig: Vec::new()` yerine gerçek imza
      (envanter K-04)
- [ ] Benchmark + performans doğrulama
- [ ] Operator key rotation runbook → `docs/security/key-management.md`
- [ ] ROADMAP envanteri C-02, K-02, K-04, M-02, M-05, DOC-05 (kısmi) kapat

**Tahmini efor:** 2.5–3 oturum (kod + test + key-management runbook).
ADR-004 ile birlikte alınırsa toplamda 3.5–4 oturum.

---

## Referanslar

- Malkin, Micciancio, Miner, *Efficient Generic Forward-Secure Signatures
  with an Unbounded Number of Time Periods* (EUROCRYPT 2002)
- Cardano `cardano-base` KES referansı:
  <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class/src/Cardano/Crypto/KES>
- David et al., *Ouroboros Praos* (EUROCRYPT 2018) — KES'in Praos güvenlik
  modelindeki rolü
- ML-DSA / Dilithium FIPS 204
- ADR-004 (VRF): bu ADR ile birlikte okunmalı; ikisi Praos'un iki ayağı

---

## Sign-Off

- **Mimari Review:** ⬜ bekleniyor
- **Güvenlik Review:** ⬜ bekleniyor
- **Implementasyon Review:** ⬜ bekleniyor
