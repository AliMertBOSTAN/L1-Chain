# ADR-011: Stealth Adres Entegrasyonu

**Durum:** Tamamı uygulandı — Faz 1-5 tamam.

- Faz 1: `qv-core::StealthInfo` formatı + `qv-script::stealth_p2pkh` template.
- Faz 2: `qv-privacy::scan_output` uyarlama (+ yeni `scan_output_view`).
- Faz 3: `qv-wallet::tx_builder` — `add_stealth_output` / `sign_stealth_input`.
- Faz 4: `qv-node` RPC — `qv_getBalanceFor` / `qv_scanStealth` gerçek
  implementasyon (`StealthViewKey` wire payload, `StealthScan` zenginleştirildi).
- Faz 5: `qv-wallet` cüzdan uygulaması — axum HTTP API + gömülü HTML/JS UI,
  CLI'da `serve`, `balance`, `scan`, `send-stealth` komutları; `qvst1…` /
  `qvfp1…` adres encoding'i.

**Tarih:** 2026-05-22
**Yazarlar:** QuantumVault Team
**Yer (planlanan):** `crates/qv-core` (`StealthInfo`), `crates/qv-privacy` (`stealth`), `crates/qv-script` (kilit template), `crates/qv-wallet` (tx-builder), `crates/qv-node` (tarama RPC)

---

## Bağlam

`CLAUDE.md` stealth adresleri **varsayılan gizlilik modeli** olarak belirtiyor.
Ancak inceleme (2026-05-22, cüzdan uygulaması çalışması sırasında) şunu ortaya
koydu: stealth özelliği **uçtan uca entegre değil** — büyük ölçüde placeholder.

Mevcut durum:

- `qv-privacy::stealth` kripto primitiflerine sahip: `StealthAddress`,
  `StealthKeys`, `StealthOutput`, `create_stealth_output`, `scan_output`,
  `compute_view_tag`, `compute_onetime_pk_hash`, `compute_spend_derivation_seed`.
- **Zincir-üstü format uyumsuz.** `qv-core::StealthInfo` (serileştirilen
  `TxOutput`'un parçası) yalnızca `ephemeral_pubkey` + `view_tag` taşıyor;
  `scan_output`'un istediği `StealthOutput` ise `kem_ciphertext` + `kyber_level`
  + `view_tag` + `onetime_pk_hash` istiyor. `kyber_level` zincirde yok.
- **Üretim bağlı değil.** `create_stealth_output` işlem-kurma yoluna hiç
  bağlanmamış; hiçbir işlem gerçekte stealth çıktı üretmiyor.
- **Harcama anahtarı kurtarılamıyor.** Yalnızca `MockSpendKeyDeriver` var ve
  o paylaşılan sırdan türetmeden **rastgele** bir anahtar çifti üretiyor.
- **Tarama RPC'leri stub.** `qv_getBalanceFor` ve `qv_scanStealth` "not yet
  implemented" hatası döndürüyor.

### Kök tasarım zorluğu

Klasik (Monero tarzı) stealth adresler EC noktalarının toplamsal
homomorfizmini kullanır: gönderen, alıcının statik açık anahtarından bir
**tek-seferlik açık anahtar** `P = spend_pub + H(s)·G` türetir; alıcı eşleşen
gizli anahtarı türetir. **PQC/kafes imzalarda (Dilithium) bu mümkün değil** —
bir Dilithium açık anahtarını gizli anahtarı bilmeden "ayarlayamazsın".

Mevcut `compute_onetime_pk_hash = SHA3(tag || shared_secret || spend_pk)` bir
yer-tutucu: statik `spend_pk`'in bir hash-bağlaması. Bir çıktı
`p2pkh_pqc(onetime_pk_hash)` ile kilitlenirse, harcamak için
`pubkey_hash(pk) == onetime_pk_hash` olan bir `pk` gerekir — hiçbir dürüst
Dilithium anahtar çifti bunu sağlayamaz. Yani **şema şu hâliyle harcanabilir
stealth çıktı üretemez.** Entegrasyon, bu PQC-stealth harcama mekanizmasını
çözmeyi gerektiriyor.

## Karar

### 1. PQC stealth harcama mekanizması — stealth'e özel kilit template

Stealth çıktılar düz `p2pkh_pqc` ile değil, **yeni bir `stealth_p2pkh` script
template'i** ile kilitlenir. Kilit, `onetime_pk_hash` taahhüdünü tutar.
Harcama sırasında alıcı witness'ta `(shared_secret, spend_pk, signature)`
sunar; script şunları doğrular:

1. `onetime_pk_hash == SHA3(STEALTH_KDF_TAG || shared_secret || spend_pk)`
2. `CHECKSIG_PQC(spend_pk, signature, message)`

Alıcı `shared_secret`'i yalnızca `scan_output` ile türetebildiğinden,
harcayabilen tek taraf odur. Açık tek-seferlik anahtar türetmeye gerek kalmaz.

**Bilinçli ödünleşim:** Harcama anında statik `spend_pk` açığa çıkar; yani bir
alıcının kendi harcamaları birbiriyle ilişkilendirilebilir. Tespit anında
çıktılar yine ilişkilendirilemez (üçüncü taraflar view key olmadan bağlayamaz).
Bu, PQC ortamında bilinen bir ödünleşimdir — EC stealth'in harcama-anında
ilişkilendirilemezliği kafes imzalarla ucuz değildir. (`MockSpendKeyDeriver` /
tek-seferlik anahtar yaklaşımı bu yüzden terk edilir.)

### 2. Zincir-üstü `StealthInfo` formatı

`qv-core::StealthInfo` şu alanları taşıyacak şekilde genişletilir:

```text
StealthInfo {
    kem_ciphertext: Vec<u8>,   // (mevcut ephemeral_pubkey — yeniden adlandır)
    kyber_level:    u8,        // YENİ — decapsulation için gerekli
    view_tag:       u8,        // mevcut
}
```

`onetime_pk_hash` ayrı bir alan **değil** — çıktının `locking_script`'i
(`stealth_p2pkh` template'i) zaten onu taşır. Tarama bu ikisinden bir
`StealthOutput` yeniden kurar.

Bu bir **konsensüs serileştirme değişikliğidir**; mainnet ya da kalıcı bir
zincir başlamadan önce yapılmalı (veya blok/işlem versiyon artışıyla).

### 3. Üretim — tx-builder entegrasyonu

`qv-wallet::tx_builder`, alıcı bir `StealthAddress` olduğunda:
`create_stealth_output(addr)` → `(StealthOutput, shared_secret)` →
çıktıyı `locking_script = stealth_p2pkh(onetime_pk_hash)` ve
`stealth_info = StealthInfo { kem_ciphertext, kyber_level, view_tag }` ile kurar.

### 4. Tarama RPC'leri

`qv_getBalanceFor` ve `qv_scanStealth` gerçek implementasyona kavuşur:
`utxo_store.entries()` ile tüm UTXO seti gezilir; `stealth_info`'su olan her
çıktı için `StealthInfo` + locking-script hash'inden bir `StealthOutput`
kurulup `scan_output(view_keys, ...)` çağrılır; eşleşenler toplanır.

Not: `scan_stealth`'in `from/to` yükseklik aralığı UTXO seti yükseklik
indeksli olmadığından best-effort kalır (canlı UTXO setini tarar). Cüzdan
için bu yeterli — bakiye + harcanabilir outpoint'ler döner.

### 5. Gerçek `SpendKeyDeriver`

`MockSpendKeyDeriver` kaldırılır. Harcama (Karar 1) tek-seferlik anahtar
türetmeye dayanmadığından ayrı bir deriver gerekmez — alıcı statik spend
anahtar çiftini kullanır. (`compute_spend_derivation_seed` ve
`from_seed_pqc` altyapısı, ileride gerçek tek-seferlik anahtar şeması
istenirse korunur.)

### Fazlar

1. `qv-core::StealthInfo` formatı + `qv-script` `stealth_p2pkh` template'i.
2. `qv-privacy` — `scan_output`'u yeni `StealthInfo`↔`StealthOutput`
   dönüşümüne göre uyarla; harcama doğrulama yardımcıları.
3. `qv-wallet::tx_builder` — stealth çıktı üretimi + stealth girdi harcama.
4. `qv-node` — `getBalanceFor` / `scanStealth` RPC implementasyonu.
5. Cüzdan uygulaması (ADR yok — UI işi).

## Sonuçlar

### Olumlu

- Stealth, `CLAUDE.md`'nin belirttiği gibi gerçekten varsayılan ve işlevsel
  gizlilik modeli olur.
- PQC-uyumlu: ne tespit ne harcama klasik kripto gerektirir.
- Cüzdan uygulaması gerçek bakiye/transfer için sağlam bir zemine oturur.

### Olumsuz

- Konsensüs serileştirme değişikliği (`StealthInfo`) — zincir başlamadan
  yapılmalı.
- Harcama anında statik `spend_pk` açığa çıkar → bir alıcının kendi
  harcamaları ilişkilendirilebilir (PQC ödünleşimi).
- Yeni script template'i + işlem-kurma karmaşıklığı.

### Nötr / İleride

- Harcama-anında ilişkilendirilemezlik istenirse, kafes-uyumlu gerçek
  tek-seferlik anahtar şeması (ör. hash-tabanlı tek kullanımlık anahtarlar)
  ayrı bir araştırma konusudur.
- `scan_stealth` için yükseklik-indeksli tarama, blok yürüyüşü ya da bir
  yükseklik indeksi gerektirir — gelecekteki iş.

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| EC-tarzı tek-seferlik açık anahtar | Dilithium/kafes anahtarları toplamsal homomorfizme sahip değil |
| Klasik (X25519) harcama anahtarı | Harcama imzası PQC olmalı (`CLAUDE.md`) |
| Mevcut `MockSpendKeyDeriver`'ı koru | Rastgele anahtar üretiyor — harcanabilir çıktı vermez |
| Stealth'i tamamen bırak, düz p2pkh | `CLAUDE.md` varsayılan gizliliği stealth diyor; mimari karar |

## Doğrulama / Test (planlanan)

- `stealth_p2pkh` template'i için script VM birim testleri (doğru/yanlış
  `shared_secret`, `spend_pk`, imza).
- `create_stealth_output` → `scan_output` → harcama uçtan uca testi.
- `getBalanceFor`/`scanStealth` RPC'leri için UTXO seti üzerinde entegrasyon
  testi.

## Bağlantılı

- `docs/security/qv-consensus-fork-finality-audit.md` — denetim disiplini.
- `crates/qv-privacy/src/stealth.rs` — mevcut primitifler.
- `crates/qv-core/src/transaction.rs` — `StealthInfo` (değişecek).
- ADR-002 — DeFi/eUTXO mimarisi (datum/validator deseni).
