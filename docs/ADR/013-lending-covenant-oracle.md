# ADR-013: Lending Kovenantı + İmzalı Oracle Fiyat Doğrulama

**Durum:** Kabul edildi — uygulandı (`qv-script::templates::lending_pool_lock`,
`qv-defi::lending` kanonik encoding, `qv-defi::tx_helpers` lending builder'ları)
**Tarih:** 2026-06-10
**Yazarlar:** QuantumVault Team
**Yer:** `crates/qv-script` (`lending_pool_lock` template'i, layout sabitleri,
oracle domain tag), `crates/qv-defi` (`LendingPoolDatum::to/from_canonical_bytes`,
`build_lending_{deposit,borrow,repay,withdraw}_tx`, `OracleSignedPrice`)

---

## Bağlam

Faz 6 D-2..D-5, AMM havuzunu zincir üstüne taşıdı: kanonik sabit-genişlik
datum + introspection tabanlı kovenant (`amm_pool_lock`) + cüzdan tarafı
`build_*_tx` helper'ları. D-6 aynı deseni **lending havuzuna** uygular.

Lending'in AMM'den temel farkı: **borrow** ve **withdraw** yolları bir
*teminat kontrolü* gerektirir ve bu kontrol bir **fiyata** ihtiyaç duyar
(teminat token'ı ile borç token'ı farklı varlıklardır). AMM'de invariant
(`x·y ≥ k`) tamamen içsel veriyle doğrulanabiliyordu; lending'de dışsal
bir fiyat girdisi kaçınılmazdır.

Kısıtlar:

- Script VM'de **referans input** yok — bir oracle UTXO'sunu "harcamadan
  okumak" mümkün değil.
- VM'de u128 için yalnızca `MUL_U128` (u64×u64→u128) ve `GE_U128`
  (16-bayt LE karşılaştırma) var; **u128 bölme, u128 toplama ve
  u128×u128 çarpma yok**.
- `MAX_SCRIPT_SIZE` 16 KB (witness + kilit script'i birlikte decode
  edilir), `DEFAULT_GAS_LIMIT` 100K, `CHECKSIG_PQC` 500 gas.

## Karar

### 1. Kanonik `LendingPoolDatum` encoding'i (146 bayt, sabit genişlik)

AMM `PoolDatum` deseni: layout sabitleri **qv-script'te** tanımlanır
(layout'un tüketicisi script'tir), `qv-defi` import eder — drift imkânsız.
Tüm tamsayılar little-endian:

| Bayt      | Alan                        | Tip          |
|-----------|-----------------------------|--------------|
| 0..32     | `pool_id`                   | 32 ham bayt  |
| 32..64    | `collateral_token_id`       | 32 ham bayt  |
| 64..96    | `debt_token_id`             | 32 ham bayt  |
| 96..104   | `total_collateral`          | `u64` LE     |
| 104..112  | `total_debt`                | `u64` LE     |
| 112..114  | `base_rate_bps`             | `u16` LE     |
| 114..116  | `slope_bps`                 | `u16` LE     |
| 116..118  | `ltv_max_bps`               | `u16` LE     |
| 118..120  | `liquidation_threshold_bps` | `u16` LE     |
| 120..122  | `liquidation_bonus_bps`     | `u16` LE     |
| 122..138  | `interest_multiplier_q64`   | `u128` LE    |
| 138..146  | `last_accrual_slot`         | `u64` LE     |

Script açısından üç bölge vardır:

- **Kimlik bölgesi** `0..96` (id'ler) ve **parametre bölgesi** `112..122`
  (5 × u16): script'e gömülü sabitlere **pinlenir** (hem eski hem yeni
  datum'da) — havuz kimliği ve risk parametreleri değiştirilemez.
- **Mutasyon bölgesi** `96..112` (`total_collateral`, `total_debt`):
  harcama yolu invariantlarınca yönetilir.
- **Faiz bölgesi** `122..146` (çarpan + son tahakkuk slotu): v1'de
  **dondurulur** (eski slice == yeni slice; bkz. §4).

### 2. Harcama yolları — witness'tan branch seçimi (IF/ELSE)

Havuz UTXO'su işlemde **input #0**, ardıl havuz **output #0**
(AMM konvansiyonuyla aynı). Witness'ın en üstüne bir branch selector
itilir; script ortak kontrolleri çalıştırdıktan sonra `IF`/`ELSE` ile
yolu seçer.

**Ortak kontroller (her iki yol):**

1. Eski ve yeni datum tam 146 bayt.
2. `0..96` ve `112..122` bölgeleri eski VE yeni datum'da script'e gömülü
   baytlara eşit.
3. Faiz bölgesi `122..146` eski == yeni (dondurulmuş).
4. `output0.value ≥ input0.value` — havuz UTXO'sunun yerel değeri
   azaltılamaz (AMM kovenantında olmayan bir sıkılaştırma; havuz
   değerinin "datum geçerli ama value 0" hilesiyle boşaltılmasını kapatır).
5. Script sürekliliği: `SELF_SCRIPT_HASH` + `ASSERT_OUTPUT_SCRIPT_HASH 0`.

**Yol 0 — deposit / repay (fiyatsız).** Witness: `OP_0`.
Script-doğrulanan invariant:

```text
total_collateral_new ≥ total_collateral_old   (deposit: artar, repay: sabit)
total_debt_new       ≤ total_debt_old         (repay: azalır, deposit: sabit)
```

Bu iki koşulu sağlayan her geçiş havuz sağlığını **monotonik iyileştirir**
ya da değiştirmez; fiyat bilgisine gerek yoktur. u64 alanlar 16 bayta
sıfır-uzatılıp (`SLICE` + 8 sıfır bayt `CAT`) `GE_U128` ile karşılaştırılır —
tam u64 aralığında, işaret yorumundan bağımsız, doğru.

**Yol 1 — borrow / withdraw (imzalı fiyat + teminat kontrolü).**
Witness (alttan üste): `<oracle_sig> <oracle_pubkey> <price_le8> <slot_le8> OP_1`.
Script sırasıyla:

1. **Tazelik**: `price_slot ≤ current_slot` ve
   `current_slot − price_slot ≤ max_price_age_slots` (`SLOT_NUMBER`).
   Slot baytları imzalı mesajın parçası olduğundan saldırgan sahte slot
   üretemez; yalnızca *gerçekten imzalanmış* eski bir fiyatı tekrar
   oynatabilir, onu da pencere reddeder.
2. **Teminat kontrolü** (yeni durum üzerinde):

   ```text
   total_debt_new · K  ≤  total_collateral_new · price_scaled
   K = ceil(10_000 · PRICE_SCALE / ltv_max_bps)   (script üretiminde hesaplanır)
   PRICE_SCALE = 10^6
   ```

   `price_scaled` = "1 teminat birimi başına borç birimi × 10⁶" (u64).
   Bu, `debt ≤ collateral · price · ltv` eşitsizliğinin bölmesiz,
   iki-çarpanlı formudur: her taraf tam olarak bir `MUL_U128`
   (u64×u64→u128, taşması imkânsız) + tek `GE_U128`. `K`'nin yukarı
   yuvarlanması kontrolü **borçlu aleyhine** sıkılaştırır (havuz lehine
   güvenli). Taşma analizi: `debt ≤ 2⁶⁴−1`, `K ≤ 10¹⁰ < 2³⁴` →
   `debt·K < 2⁹⁸ < 2¹²⁸`; `collateral·price < 2¹²⁸`. `ltv_max_bps = 0`
   script üretiminde reddedilir (`TemplateError::InvalidLtv`).
3. **Oracle imza doğrulama** (gerçek ML-DSA, mock değil): mesaj
   `LENDING_ORACLE_DOMAIN_TAG ‖ pool_id ‖ price_le8 ‖ slot_le8` script
   içinde `CAT` ile yeniden kurulur (`tag‖pool_id` script'e gömülü —
   havuzlar-arası replay imkânsız); witness'taki pubkey'in SHA3-256'sı
   script'e gömülü `oracle_pk_hash` ile eşleşmeli; sonra
   `CHECKSIG_PQC(pubkey, sig, msg)` + `VERIFY`.

Yol 1, yol 0'ın izin verdiği geçişleri de kapsar (daha talepkârdır);
bu kasıtlıdır — borrow+deposit gibi bileşik geçişler tek yoldan geçer.

### 3. Oracle tasarımı: witness'ta taşınan imzalı fiyat

Referans input olmadığından fiyat **witness ile taşınır**; script imzayı
doğrular. v1'de **tek oracle anahtarı** (`oracle_pk_hash` script'e gömülü).

**Dürüst merkeziyet trade-off'u:** tek anahtar tek hata/yolsuzluk
noktasıdır — oracle operatörü yanlış fiyat imzalayarak havuz aleyhine
borç verdirebilir. Kabul gerekçesi: (a) script başına 1 × `CHECKSIG_PQC`
(500 gas) ile bütçeye rahat sığar; t-of-n medyan, n × 500 gas + n × ~5,3 KB
witness demektir (3-of-5'te ~26 KB → 16 KB decode sınırını **aşar**);
(b) `qv-defi::oracle` modülündeki medyan/TWAP/manipülasyon altyapısı
zaten off-chain agregasyon için mevcut — operatör imzaladığı fiyatı
oradan üretebilir; (c) v2'de eşik-imza (tek doğrulamayla t-of-n) veya
witness boyut sınıflarının yeniden düzenlenmesi planlıdır. Fiyatın
`max_price_age_slots` penceresi içinde herkesçe yeniden kullanılabilir
olması tasarım gereğidir (fiyat gizli değildir, imza onu işleme değil
*(havuz, slot)* çiftine bağlar).

### 4. Faiz tahakkuku — script'te doğrulanabilirlik analizi

`accrue_interest` formülü:

```text
factor          = 2^64 + rate_per_slot_q64 · slots_elapsed
rate_per_slot   = (rate_bps << 64) / 10^4 / slots_per_year
multiplier_new  = multiplier_old · factor / 2^64
```

Mevcut opcode setiyle değerlendirilen seçenekler:

| Seçenek | Değerlendirme |
|---|---|
| (a) `DivU128` opcode'u ekle | **Yetmez.** Bölme tek eksik değil: `multiplier_old · factor` u128×u128 çarpımdır → 256-bit ara değer ister. Tam doğrulama için `MUL_U128_WIDE` + `ADD_U128` + `SHR_U128` ailesi gerekir — D-6 kapsamını aşan bir VM genişlemesi. |
| (b) Çarpma-tabanlı eşitsizlik (`multiplier_new · 2^64 ≥ multiplier_old · factor`) | **Aynı engel:** her iki taraf u128×u128. `MUL_U128` yalnızca u64×u64 alır. |
| (c1) Monotonluk (`multiplier_new ≥ multiplier_old`, üst sınırsız) | Script'te ifade edilebilir (`GE_U128`) ama **istismar edilebilir**: üst sınır olmadan herhangi bir harcayan çarpanı `u128::MAX`'e şişirip off-chain borç ölçeklemesini bozar. Üst sınır (`old + rate·Δslot`) u128 toplama ister — yok. Reddedildi. |
| (c2) **Dondurma (seçilen)**: `interest_multiplier_q64` ve `last_accrual_slot` v1 kovenantında **bayt-bayt sabit** (`eski_slice == yeni_slice`, 24 bayt tek `EQ`) | Gerçek ve zorlanır bir kural: faiz alanları **kurcalanamaz** — ne şişirme ne geri sarma mümkün. Bedeli: v1 havuzu zincir üstünde faiz tahakkuk ettirmez; `accrue_interest` off-chain kotasyon/muhasebe için kalır. |

**Karar: (c2).** Mevcut VM'de u128 alanı üzerinde zorlanabilir tek sağlam
kural eşitlik/sıralamadır; sıralama tek başına (c1) saldırı yüzeyi açtığından
en güçlü *güvenli* kural sabitliktir. Zincir üstü tahakkuk v2 işi: ya
128-bit aritmetik opcode ailesi (`ADD_U128`, geniş çarpma/kaydırma) ya da
ayrı bir "crank" harcama yolu ile.

### 5. Likidasyon — v2'ye ertelendi (gerekçe)

v1 datum'u yalnızca **agregat** tutar (`total_collateral`, `total_debt`);
pozisyonlar (`LendingPosition`) zincir üstünde değildir. Likidasyon ise
doğası gereği **pozisyon-bazlıdır**: "şu borçlunun sağlık faktörü < 1"
koşulu, borçlu başına teminat/borç bilgisi olmadan script'te ifade
edilemez. Agregat seviyede bir "likidasyon" tanımlamak (havuz toplamı
LTV'yi aşınca herkese ceza) ekonomik olarak anlamsızdır. Doğru tasarım,
borçlu başına **pozisyon UTXO'ları** (kendi datum + kovenantı, sahibinin
anahtarına + havuz script'ine bağlı) ister — bu D-6'nın "tek havuz
UTXO'su" kapsamını aşan ayrı bir dilimdir. Ayrıca likidasyon koşulu
*ters yönlü* bir eşitsizliktir (`debt·K' > collateral·price`) ve bonus
hesabı ek çarpım katmanı getirir. v2'de pozisyon UTXO'larıyla birlikte
gelir.

### 6. Script boyutu / gas analizi

Gömülü sabitler: 2×96 B kimlik + 2×10 B parametre + ~52 B oracle önek
(tag+pool_id) + 32 B pk hash + 2×8 B sıfır-uzatma + ~45 adet `PUSH_INT`
(9'ar bayt) + ~90 tek-bayt opcode → **script ≈ 1,1 KB** (≪ 16 KB).
Witness (yol 1): Dilithium3 imza 3 309 B + pubkey 1 952 B + 16 B fiyat/slot
→ ≈ 5,3 KB; witness+script ≈ **6,4 KB < 16 KB** decode sınırı ✓
(p2pkh_pqc ile aynı imza/pubkey sınıfı).

Gas (yol 1, en pahalı): ortak kontroller ≈ 550, dönüşüm+tazelik ≈ 90,
teminat kontrolü ≈ 200 (2×`MUL_U128`+`GE_U128` = 45 + introspection),
mesaj kurma+hash ≈ 100, `CHECKSIG_PQC` 500 + sarmalama ≈ 510, süreklilik
+ value kontrolü ≈ 75 → **toplam ≈ 1 500 gas ≪ 100 000** ✓ (testte
assert edilir). Yol 0 ≈ 800 gas.

### 7. Off-chain yüzey (`qv-defi::tx_helpers`)

- `build_lending_deposit_tx` / `build_lending_repay_tx` — yol 0 witness'ı
  (`OP_0`), datum geçişini uygular, kovenant ön-kontrolünü yerelde yapar.
- `build_lending_borrow_tx` / `build_lending_withdraw_tx` — `OracleSignedPrice`
  alır, teminat kontrolünü helper'da da yapar (erken hata), yol 1
  witness'ını kurar.
- `sign_oracle_price` — oracle operatörünün üretim fonksiyonu: mesajı
  kurar, `qv_crypto::pqc_sign::sign` ile ML-DSA imzalar.
- İşlem şekli AMM ile aynı: input #0 havuz (witness'lı kovenant),
  input #1 kullanıcı (cüzdan imzalar), output #0 ardıl havuz, output #1
  kullanıcı parası. Sighash witness-dışlayan olduğundan (ADR-012) havuz
  witness'ı kullanıcı imzasını bozmaz.

## Sonuçlar

### Olumlu

- Lending havuzu durum geçişleri zincir üstünde **gerçekten** doğrulanır;
  deposit/repay fiyatsız, borrow/withdraw imzalı-fiyat + bölmesiz teminat
  kontrolüyle. Yeni opcode **gerekmedi**.
- Oracle fiyatı gerçek ML-DSA imzasıyla, domain-ayrımlı ve havuza bağlı
  mesajla doğrulanır; tazelik penceresi bayat fiyatı reddeder.
- Faiz alanları kurcalamaya kapalı (dondurma); risk parametreleri ve
  kimlikler script'e pinli.
- Havuz UTXO'sunun yerel değeri kovenantça korunur (`out ≥ in`).

### Olumsuz / dürüst sınırlar

- **Tek oracle anahtarı** (v1): merkeziyet; t-of-n medyan v2 (eşik imza
  veya witness bütçesi yeniden düzenlemesi ister, bkz. §3).
- **Zincir üstü faiz tahakkuku yok** (v1): faiz bölgesi donduruldu;
  tahakkuk off-chain kotasyondur. v2: 128-bit aritmetik opcode'ları
  veya crank yolu.
- **Pozisyonlar zincir üstünde değil**: kovenant *havuz-agregat* LTV'yi
  zorlar, kullanıcı-başına LTV'yi değil. Pozisyon UTXO'ları + likidasyon
  v2 (bkz. §5).
- **Token yerleşimi datum-seviyesinde**: AMM D-3/D-5 ile aynı sınır —
  teminat/borç hareketleri datum muhasebesidir; yerel çoklu-varlık
  çıktıları ayrı Faz 6 dilimi.
- **CLI / RPC yüzeyi yok** (D-6 kapsamı dışı): bu katman `tx_helpers`
  seviyesindedir; `qv-wallet lend ...` CLI'ı ve RPC entegrasyonu sonraki
  iş (D-4'teki `swap` CLI deseni izlenir).
- Slot karşılaştırmaları i64 yorumuyla yapılır; slot değerleri < 2⁶³
  (protokol ömrü boyunca garanti — 2 sn/slot'ta ~5,8×10¹¹ yıl) ve slot
  baytları imza kapsamında olduğundan istismar edilemez.

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| Oracle UTXO'sunu referans input ile okumak | VM'de referans input yok; eklemek konsensüs+ledger değişikliği — D-6 kapsamı dışı |
| Fiyatı datum'a gömüp güncel tutmak | Her fiyat güncellemesi havuz UTXO'sunu harcar → yarış/contention; tazelik yine imza ister |
| `DivU128` opcode'u ekleyip LTV'yi script'te bölmek | Bölme gereksiz: sabitler script üretiminde katlanır (`K`); tahakkuk için ise bölme tek başına yetmez (§4a) |
| t-of-n oracle medyanı (v1'de) | n × 5,3 KB witness 16 KB decode sınırını aşar; eşik-imza altyapısı (threshold ML-DSA) henüz yok |
| Ring/aggregate imza ile oracle seti | PQC agregat imza standardı olgunlaşmadı; blok şişmesi |

## Doğrulama / Test

- `qv-script::templates` testleri: geçerli deposit/repay (yol 0) ✅,
  geçerli borrow (gerçek Dilithium3 imzalı fiyat) ✅, teminatsız borrow ❌,
  bayat fiyat ❌, gelecek-slotlu fiyat ❌, yanlış oracle anahtarı ❌,
  yanlış mesaja imza ❌, parametre/faiz-bölgesi kurcalama ❌, script
  değişimi ❌ (`CovenantFailed`), havuz değeri düşürme ❌, gas < limit ✓,
  script boyutu sınırı ✓.
- `qv-defi::tx_helpers` testleri: dört builder'ın ürettiği işlemler
  `validate_script` ile uçtan uca havuz kovenantını geçer; datum
  kurcalama / script değişimi / yetersiz teminat reddedilir.
- `qv-defi::lending` kanonik encoding round-trip + offset uyum testleri.

## Bağlantılı

- ADR-002 — DeFi mimarisi (Shared UTXO Pattern)
- ADR-012 — işlem sighash'i (havuz witness'ı kullanıcı imzasını etkilemez)
- `crates/qv-script/src/templates.rs` — `amm_pool_lock` (D-2 deseni)
- `crates/qv-defi/src/oracle.rs` — off-chain medyan/TWAP (operatör girdisi)
- `docs/ROADMAP.md` — D-07/D-08 faiz Q.64 düzeltmesi (off-chain taraf)
