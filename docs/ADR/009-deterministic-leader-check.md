# ADR-009: Deterministik Lider Kontrolü (Sabit-Nokta)

**Durum:** Approved + Uygulandı (2026-05-22)
**Tarih:** 2026-05-22
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-consensus/src/leader_schedule.rs` (`is_slot_leader`, `leader_threshold_fixed`, `exp_neg_fixed`)

---

## Bağlam

Praos lider testi: stake oranı `σ` olan bir havuz, VRF çıktısı `p` için
`p < 1 − (1−f)^σ` ise o slotun lideridir.

Önceki kod `(1−f)^σ`'yi `f64` `exp()` / `ln()` ile hesaplıyordu. IEEE 754
`+ − × ÷` platformlar arası bit-aynıdır, **ama transandantal fonksiyonlar
(`exp`, `ln`) öyle değildir** — sonuç, libm sürümüne/derleyiciye/platforma
göre son bitte farklılaşabilir. Mutabakat-kritik bir yolda bu, iki dürüst
düğümün eşiği farklı hesaplayıp aynı slot için farklı lider kararı vermesine
— yani **mutabakat ayrışmasına** — yol açabilir.

Bu, 2026-05-22 fork/finalite denetimi (`docs/security/qv-consensus-fork-finality-audit.md`)
sırasında DÜŞÜK-1 incelemesinde fark edildi.

## Karar

Lider kontrolünü **tamamen sabit-nokta tamsayı aritmetiğine** taşırız.
Yeni fonksiyon: `is_slot_leader(stake_num, stake_den, vrf_output) -> bool`.

### Algoritma

`p < 1 − (1−f)^σ` ⟺ `exp(σ · ln(1−f)) < 1−p`. Adımlar:

1. **`f` bir protokol sabiti** (`ACTIVE_SLOT_COEFF = 0.05`) olduğundan
   `ln(1−f)` de sabittir. Çevrimdışı, yüksek hassasiyetle hesaplanıp
   `2^64` ölçeğinde bir tamsayı sabiti olarak gömülür:
   `LN_ONE_MINUS_F_MAG = 946_194_274_264_587_207`. Çalışma anında **asla
   yeniden hesaplanmaz** — determinizmin anahtarı budur.
2. `m = |σ · ln(1−f)|`, `2^64` ölçeğinde: `m = stake_num · LN_ONE_MINUS_F_MAG / stake_den`.
3. `exp(−m)` sınırlı bir Taylor serisiyle: `Σ (−m)^k / k!`, `k = 0..K`.
   `|x| ≤ |ln(0.95)| ≈ 0.0513` olduğu için seri çok hızlı yakınsar.
   **K = 9 terim** tüm `σ ∈ (0,1]` için sub-ulp hata verir.
4. Eşik = `2^64 − exp(−m)`. Lider ⟺ `vrf_top64 < eşik`.

Tüm ara değerler negatif olmayan `u128` tamsayılar; her bölme kesen
(truncating) bölmedir. Sonuç **her düğümde bit-aynıdır**.

### Parametreler

| Parametre | Değer | Not |
|---|---|---|
| Sabit-nokta ölçeği | `2^64` | `u128` içinde |
| `LN_ONE_MINUS_F_MAG` | `946 194 274 264 587 207` | `\|ln(0.95)\| · 2^64` |
| Taylor terim sayısı `K` | 9 | sub-ulp hata |
| VRF hassasiyeti | en üst 64 bit | lider seçimi için fazlasıyla yeterli |

### Taşma bütçesi

En büyük ara değer `term · m`: `term ≤ 2^64`, `m < 2^60` ⇒ çarpım `< 2^124`.
`u128` tavanı `2^128` — yaklaşık **16× pay**. `u256` ya da bignum gerekmez.
(`σ ≤ 1` gereklidir; `relative_stake` bunu garanti eder.)

### f64 fonksiyonların durumu

`leader_threshold(σ) -> f64` ve `VrfOutput::to_unit_interval() -> f64`
**silinmedi** ama mutabakat yolundan çıkarıldı; dokümantasyonları "yalnızca
teşhis/gösterim" olarak işaretlendi. Yeni mutabakat yolu yalnızca
`is_slot_leader`.

## Sonuçlar

### Olumlu

- Lider kontrolü artık platformdan bağımsız bit-aynı — mutabakat ayrışması
  riski kapandı.
- Blok üretimi (`check_leadership`) ve doğrulama (`verify_leadership`) artık
  **aynı** deterministik kriteri kullanıyor.
- Yeni bağımlılık yok; `u128` yeterli.

### Olumsuz

- `LN_ONE_MINUS_F_MAG`, `ACTIVE_SLOT_COEFF`'e bağlı. `f` değişirse sabit
  yeniden hesaplanmalı — `ln_constant_matches_active_slot_coeff` testi bunu
  yakalar.
- İki lider-matematiği yüzeyi (deterministik tamsayı + teşhis amaçlı f64).
  Kafa karışıklığını önlemek için f64 sürümler açıkça etiketlendi.

### Nötr / İleride

- Sabit-nokta sonucu ideal reel-değerli eşikten sub-ulp sapar; bu sapma her
  düğümde **aynı** olduğundan ayrışma değildir.
- Lider eşiği 64-bit hassasiyette; gerekirse tasarım daha fazla VRF bitine
  genişletilebilir (256-bit tamsayı uzayı).

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| f64 `exp`/`ln`'yi koru (status quo) | Transandantaller platformlar arası bit-aynı değil — ayrışma riski |
| `u256` / bignum aritmetiği | Gereksiz — `u128` 16× payla yetiyor; yeni bağımlılık |
| Cardano `taylorExpCmp` (ABOVE/BELOW/UNKNOWN) | Gereksiz karmaşıklık — saf sabit-nokta zaten her zaman kesin sonuç verir |
| log-uzayında karşılaştırma | Yine `ln` gerektirir; sabit-nokta exp daha basit ve kesin |

## Doğrulama

- Bağımsız referans modeli `docs/security/leader_check_reference.py`: `ln(0.95)` sabitini
  80-haneli hassasiyetle hesapladı, sabit-nokta Taylor exp'i uyguladı,
  K = 9'u seçti, taşma bütçesini doğruladı (`< 2^124`), yüksek-hassasiyetli
  bir oracle ile karşılaştırdı (sapmalar yalnızca sub-ulp bandında) ve Rust
  testleri için vektörler üretti.
- Rust birim testleri referans vektörleriyle: `leader_threshold_fixed_matches_reference`,
  `exp_neg_fixed_of_zero_is_one`, `sigma_one_threshold_approximates_f`,
  `is_slot_leader_boundaries`, `ln_constant_matches_active_slot_coeff`.

## Bağlantılı

- `docs/security/qv-consensus-fork-finality-audit.md` — bu ADR'i tetikleyen
  denetim (DÜŞÜK-1 / float determinizmi gözlemi).
- `crates/qv-consensus/src/leader_schedule.rs` — uygulama.
