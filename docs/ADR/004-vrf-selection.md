# ADR-004: VRF Seçimi — Slot Lideri için Doğrulanabilir Rastgele Fonksiyon

**Durum:** Approved + Uygulandı (2026-05-06)
**Tarih:** 2026-05-06 (yazıldı + impl)
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-crypto/src/vrf.rs` (üretimde — Ristretto255-VRF via `schnorrkel = 0.11`), `crates/qv-consensus/src/leader_schedule.rs` (`RistrettoVrfEvaluator` üretimde; `TestVrf` mock sadece test için duruyor)

---

## Bağlam

Ouroboros Praos slot lideri seçimi, her slot için bir **Verifiable Random Function**
(VRF) üretimine dayanır. Stake oranıyla ağırlıklandırılmış eşik (`T = 1 − (1−f)^σ`)
karşısında VRF çıktısı kontrol edilir; eşiğin altındaysa stake havuzu o slotun
lideridir. Süreç şöyle:

1. **`evaluate(sk, msg) → (output, proof)`** — Stake havuzu özel anahtarıyla
   her slot için bir VRF değeri ve onun ispatını üretir.
2. **`verify(pk, msg, proof) → output`** — Diğer düğümler havuzun gerçekten
   o slotun lideri olduğunu kanıtla doğrular.
3. **Determinizm + manipüle edilemezlik:** Aynı `(sk, msg)` her zaman aynı
   `output`'u verir; saldırgan `output`'u beğenmediği için tekrar deneyemez.
4. **`output` rasgeleliği:** İstatistiksel olarak uniform; saldırgan bir
   sonraki epoch için lider olma şansını arttıramaz.

`qv-crypto/src/vrf.rs` şu an sadece doc-comment içeren boş bir dosya
(envanter ID **C-01**). `qv-consensus` katmanı `VrfEvaluator` trait + `TestVrf`
mock üzerinden çalışıyor (envanter **K-01**). Bu ADR, gerçek primitif seçimini
ve API yüzeyini sabitler.

CLAUDE.md mimari kararı VRF için **hibrit (klasik + PQC)** yaklaşım söyler.
Bu ADR o ilkeye uyarken bugün shippable olanı seçer.

---

## Kabul Kriterleri

Seçilecek VRF:

1. **Determinizm + benzersiz ispat** (uniqueness): Bir `(sk, msg)` çifti tek
   bir geçerli `(output, proof)` üretmeli — saldırgan farklı ispatlar
   denemekle çıktı dağılımını eğemez.
2. **IRTF VRF güvenlik tanımları:** *pseudorandomness*, *uniqueness*,
   *collision-resistance* — tümü kanıtlanmış olmalı.
3. **Hızlı doğrulama:** Slot başına yüzlerce node `verify` koşacak;
   gecikme < 1 ms hedefi.
4. **Küçük ispat boyutu:** Slot başı zincire yazılır (`BlockHeader.vrf_proof`).
   ≤ 1 KB tercih edilir.
5. **Audit edilmiş referans implementasyon:** Kendi yazmayacağız; iyi test
   edilmiş bir crate seçeceğiz.
6. **PQC roadmap'i:** Bugün PQC zorunlu değil ama gelecekte hibrit/swap
   yapabilmek için API trait arkasında olmalı.

---

## Aday Yaklaşımlar

### Aday 1 — Ristretto255-VRF (IETF `draft-irtf-cfrg-vrf-15`, ECVRF-RISTRETTO255-SHA512)

**Nasıl çalışır:** Ristretto255 grubu üzerinde Schnorr-benzeri ispat. Curve25519
prime-order subgroup'undan türetilir; cofactor ve malleability problemleri yok.
Cardano, Polkadot ve Solana production'da bu aileyi kullanıyor.

**Avantajlar:**
- ✅ Production-tested; on yıllarca akademik analiz, milyarlarca dolar değer korur
- ✅ ~96 byte proof, < 0.1 ms verify
- ✅ Audit edilmiş Rust crate'leri var (`vrf-r255`, `schnorrkel`, `vrf` crate'i)
- ✅ IRTF draft-15 finalize aşamasında — wire format stabil
- ✅ Constant-time implementation'lar mevcut (sub-channel resistance)

**Dezavantajlar:**
- ❌ **PQC değil**: Discrete log Curve25519 üzerinde — Shor ile kırılabilir
- ❌ Mainnet'e mainnet sonrası lattice-VRF'ye geçiş **hard fork** gerektirir

**Olgunluk:** Production. Cardano (2020+), Polkadot (2020+), Aleo, Mina kullanıyor.

---

### Aday 2 — LB-VRF (Esgin et al. 2021, Lattice-Based VRF)

**Nasıl çalışır:** Module-LWE ve module-SIS varsayımları üzerinde kurulmuş; ispat
Fiat-Shamir dönüşümlü 5-round identification şemasından türetilir. Akademik
referans: *"Practical Post-Quantum Few-Time Verifiable Random Function with
Applications to Algorand"* (FC '21).

**Avantajlar:**
- ✅ Post-quantum güvenli (Shor'a dirençli)
- ✅ Algorand'ın referans tasarımı

**Dezavantajlar:**
- ❌ Proof boyutu ~3-4 KB (Ristretto'nun ~30 katı) — `BlockHeader` şişer
- ❌ "Few-time" — aynı anahtar sınırlı sayıda kullanılabilir; rotation gerek
- ❌ Production-grade Rust crate yok; akademik referans implementasyonu var
- ❌ Verify ~10 ms (Ristretto'nun ~100 katı) — 2 saniye slot'ta darboğaz olabilir
- ❌ Henüz audit edilmemiş

**Olgunluk:** Akademik prototype; Algorand bile henüz production'a almadı.

---

### Aday 3 — VRF-AD (Aggregable Designated-VRF, lattice-based, Boneh-Eskandarian)

**Nasıl çalışır:** Daha yeni (2024 EUROCRYPT) lattice tabanlı ispat sistemi;
çıktıları aggregate edilebilir.

**Avantajlar:**
- ✅ PQC, daha küçük proof (~1 KB)
- ✅ Aggregation epoch nonce evolution için ilginç

**Dezavantajlar:**
- ❌ 2024 sonu yayını — production implementasyonu yok
- ❌ Akademik analiz devam ediyor

**Verdict:** Çok erken; takip listesinde tut.

---

### Aday 4 — Hibrit Ristretto+LB-VRF (Senin VRF + benim VRF tipi)

**Nasıl çalışır:** Aynı `msg` her iki şemayla VRF'lenir; `output` SHA3-256
ile karıştırılır; `proof` her iki ispatın birleşimi.

**Avantajlar:**
- ✅ Klasik OR PQC kırılırsa diğeri korur
- ✅ CLAUDE.md hibrit ilkesinin doğal uzantısı

**Dezavantajlar:**
- ❌ Proof boyutu = sum(her iki); ~4 KB
- ❌ Verify maliyeti iki katı
- ❌ LB-VRF üretim için yeterince olgun değilken ona bağımlı olmak

**Verdict:** İdeal hedef ama bugün ship edilemez.

---

## Karar

**MVP (mainnet v1):** **Aday 1 — Ristretto255-VRF.**

**Mainnet v2 / 2027 hedefi:** **Aday 4 — Hibrit Ristretto + LB-VRF**, LB-VRF
production-grade Rust crate çıkınca.

### Gerekçe

1. **Bugün ship edilebilir olmak şart:** Aday 2/3/4 ya akademik prototype ya da
   audit edilmemiş. Devnet/testnet/mainnet'i bekletecek lüks yok.
2. **Cardano/Polkadot kanıtlanmış pattern:** Aynı VRF'i dünyanın en büyük
   PoS zincirlerinden ikisi 2020'den beri çalıştırıyor; sıfır kritik VRF
   açığı.
3. **Hibrit'e geçiş zaten trait arkasında planlandı:** `VrfEvaluator` trait
   yerinde olduğu sürece, swap = bir crate eklemek + trait impl. Hard fork
   değil, soft upgrade.
4. **PQC felsefesi ile çelişki yönetilebilir:** İmza (Dilithium) + KEM (Kyber)
   PQC. VRF sadece slot lideri seçiminde kullanılır; saldırgan VRF'i kırarsa
   "haksız yere lider seçilebilir" — bu **canlılık** problemi, **güvenlik**
   problemi değil. UTXO bütünlüğü/imza/encryption hâlâ PQC.
5. **Performans gereksinimi karşılıyor:** 2 saniye slot'ta < 0.1 ms verify
   ile her node binlerce VRF doğrular.

---

## Implementation Planı

### Crate seçimi

Kullanılacak crate: **`schnorrkel`** (Web3 Foundation, Polkadot tarafından
maintain edilir; `vrf` modülü IRTF draft uyumlu).

```toml
# crates/qv-crypto/Cargo.toml
[dependencies]
schnorrkel = { version = "0.11", default-features = false, features = ["std"] }
```

Yedek: `vrf-r255` (Cardano referansı, daha basit ama daha az aktif maintenance).

### Public API (qv-crypto/src/vrf.rs)

```rust
//! Ristretto255-VRF (IETF draft-irtf-cfrg-vrf-15).
//!
//! Per ADR-004 ekstra hibrit lattice-VRF mainnet v2'de eklenecek;
//! bu modülün API'si o ekleme ile geriye uyumlu kalacak şekilde tasarlandı.

use crate::{CryptoError, Result};
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;

/// VRF özel anahtarı (stake havuzu operatörü tutar).
#[derive(Clone, ZeroizeOnDrop)]
pub struct VrfSecretKey(/* schnorrkel::Keypair */);

/// VRF açık anahtarı (`StakePool.vrf_key` olarak zincire yazılır).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfPublicKey(pub [u8; 32]);

/// VRF anahtar çifti.
pub struct VrfKeyPair {
    pub secret: VrfSecretKey,
    pub public: VrfPublicKey,
}

/// VRF rastgele çıktısı (32 byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfOutput(pub [u8; 32]);

/// VRF doğrulanabilir ispatı.
///
/// Ristretto255-VRF için ~96 byte. Hibrit'e geçişte boyut artabilir;
/// callers `as_bytes().len()`'e güvenmemeli.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfProof(pub Vec<u8>);

impl VrfKeyPair {
    /// Yeni anahtar çifti üret. CSPRNG (`OsRng`) kullanır.
    pub fn generate() -> Result<Self> { todo!("schnorrkel::Keypair::generate") }

    /// 32-byte seed'den deterministik anahtar çifti türet.
    /// Wallet HD derivation tarafından kullanılır.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self> { todo!() }
}

/// Mesajı imzala: `(output, proof)` döndürür.
pub fn evaluate(sk: &VrfSecretKey, msg: &[u8]) -> Result<(VrfOutput, VrfProof)> {
    todo!("schnorrkel vrf_sign")
}

/// İspatı doğrula. Geçerliyse `output`'u döndürür; aksi takdirde Err.
pub fn verify(pk: &VrfPublicKey, msg: &[u8], proof: &VrfProof) -> Result<VrfOutput> {
    todo!("schnorrkel vrf_verify")
}
```

### qv-consensus entegrasyonu

`qv-consensus::leader_schedule::VrfEvaluator` trait zaten doğru yerde:

```rust
// Mevcut (mock):
pub trait VrfEvaluator {
    fn evaluate(&self, msg: &[u8]) -> (VrfOutput, VrfProof);
    fn verify(&self, pk: &[u8], msg: &[u8], proof: &VrfProof) -> Result<VrfOutput, ...>;
}
```

ADR-004 sonrası bir `RistrettoVrfEvaluator` impl'i eklenir; `TestVrf` test'ler
için kalır. Mevcut tüm consensus testleri trait üzerinden çalıştığından
implementasyon swap mekaniktir.

### qv-miner entegrasyonu

`crates/qv-miner/src/keys.rs:VrfKeyPair` (envanter **M-01**) bu modülün
`VrfKeyPair`'ına yönlendirilir. Sahte 32-byte secret/public placeholder'ları
silinir.

`OperatorKeys::generate()` → `qv_crypto::vrf::VrfKeyPair::generate()`.

### Wire format ve domain separation

VRF mesajı her zaman:

```text
msg = "QuantumVault-Praos-VRF-v1" || epoch_nonce (32B) || slot (LE u64) || pool_id (32B)
```

Bu prefix domain separation sağlar; başka bir bağlamda üretilmiş bir
VRF burada geçerli olamaz. `qv-consensus::leader_schedule::vrf_input()`
zaten bu pattern'i kuruyor — sadece üst tag'i `v1`'e sabitliyoruz.

### Test stratejisi

1. **Unit test (qv-crypto):** Roundtrip, determinism, wrong-key rejection,
   tampered-proof rejection, edge cases (empty msg, large msg).
2. **KAT test:** IRTF draft-15 Appendix A test vektörlerini doğrula.
3. **Property test (proptest):** `verify(evaluate(sk, msg)) == output` ∀ msg.
4. **qv-consensus integration:** Mevcut `leader_schedule_fairness_50k_slots`
   testini `RistrettoVrfEvaluator` ile çalıştır; sonuç istatistiksel olarak
   `TestVrf` ile aynı (uniform output).
5. **Benchmark (criterion):** keygen, evaluate, verify — hedef: verify < 100 µs.

---

## Geri Çevrilebilirlik (Hibrit Geçiş Planı)

Mainnet v2'de hibrit moda geçiş:

1. Yeni crate: `qv-crypto::vrf::lattice` (LB-VRF wrapper)
2. Yeni `HybridVrfEvaluator { ristretto, lattice }` impl'i
3. `BlockHeader.vrf_proof` artık `(ristretto_proof, lattice_proof)` çifti
   olarak bincode-encoded
4. Hard fork yerine **dual-VRF dönemi**: N epoch boyunca her iki ispat
   doğrulanır; sonra eski ispat soft-deprecate edilir
5. Eski blok header'ları geçerli kalır (audit trail)

---

## Sonuçlar (Consequences)

**Olumlu:**
- Yarın derleyebileceğimiz, audit edilmiş bir VRF
- Mevcut `VrfEvaluator` trait kontratını bozmuyor → diff küçük
- Slot lideri performansı production-grade

**Olumsuz:**
- Bir Curve25519 bağımlılığı eklemiş oluyoruz (klasik kripto, mainnet-day-1
  PQC değil)
- 2027 hibrit geçişi ek mühendislik

**Kabul edilen risk:** VRF kuantum ile kırılırsa saldırgan haksız yere lider
seçilebilir → **canlılık degradasyonu**. UTXO/imza/KEM PQC olduğundan değer
kaybı yok.

---

## Onay ve Implementasyon Takvimi

- [ ] Bu ADR review (1 oturum)
- [ ] `qv-crypto/Cargo.toml` → `schnorrkel` ekle
- [ ] `qv-crypto/src/vrf.rs` → API + impl + 5+ unit test
- [ ] KAT test fixture'larını ekle
- [ ] `qv-consensus::leader_schedule::RistrettoVrfEvaluator` impl
- [ ] `qv-miner::keys::VrfKeyPair` gerçek bağlama (envanter M-01)
- [ ] `qv-node::slot_ticker` ve `qv-miner::block_producer`'da `TestVrf`
      yerine `RistrettoVrfEvaluator` kullan
- [ ] Benchmark + performans doğrulama
- [ ] ROADMAP envanteri C-01, K-01, M-01, N-06, DOC-05 (kısmi) kapat

**Tahmini efor:** 1.5–2 oturum (kod + test). ADR-005 ile birlikte alınırsa
toplamda 2.5–3 oturum.

---

## Referanslar

- IRTF VRF Draft 15: <https://datatracker.ietf.org/doc/draft-irtf-cfrg-vrf/>
- David et al., *Ouroboros Praos: An adaptively-secure, semi-synchronous
  proof-of-stake blockchain* (EUROCRYPT 2018)
- Esgin et al., *Practical Post-Quantum Few-Time Verifiable Random Function
  with Applications to Algorand* (FC 2021) — Aday 2 referansı
- `schnorrkel` crate: <https://github.com/w3f/schnorrkel>
- ADR-005 (KES): bu ADR ile birlikte okunmalı; ikisi Praos'un iki ayağı

---

## Sign-Off

- **Mimari Review:** ⬜ bekleniyor
- **Güvenlik Review:** ⬜ bekleniyor
- **Implementasyon Review:** ⬜ bekleniyor
