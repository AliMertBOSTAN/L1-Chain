# QuantumVault L1 — Kaldığımız yerden devam (Claude'a yapıştır)

QuantumVault L1 blokzincir projesinde (Rust workspace, kuantum-korumalı,
UTXO tabanlı Katman-1) birlikte çalışıyoruz. Bu prompt önceki oturumda
nereye kadar geldiğimizi, hangi sırada ilerleyeceğimizi ve çalışma
kurallarını özetliyor.

## Proje

- Konum: `C:\Users\mbostan\Desktop\L1\L1 Blockchain`
- Rust cargo workspace, 13 crate (`crates/qv-*`). Mimari kararlar `CLAUDE.md`'de.
- **Önemli:** Kullanıcının ortamında Rust derleyici lokal'de (Windows
  PowerShell). Sen kodu yaz; **kullanıcı** `cargo build` / `cargo nextest`
  / `cargo clippy --all-targets -- -D warnings` çalıştırıp çıktıyı sana
  yapıştırır. Sandbox'ta derleme yapma.

## Şimdiye kadar bitenler (özet)

Tarihler ROADMAP.md "Mevcut Durum" tablosundan / PROJECT_STATUS.md
bölümlerinden alınmıştır.

1. **Consensus fork/finalite denetimi (2026 erken)** — KRİTİK çift-finalite
   hatası kapandı, ADR-008/009/010 (genesis maxvalid-bg, deterministik
   lider, bootstrap sync `SyncManager` iskeleti).
2. **ADR-012 — sighash (2026-05-22)** — `Transaction::sighash()` + `SigHash`
   opcode (0x69). In-flight witness yeniden-oynatma açığı kapandı.
3. **ADR-011 — stealth (Faz 1-5, 2026-05-22)** — stealth_p2pkh template,
   `qv_scanStealth` + `qv_scanP2pkh`, cüzdan UI (axum + gömülü HTML),
   qvst1/qvfp1 adres formatı, QR + `.qvaddr`.
4. **Devnet bootstrap köprüsü (2026-05-22)** — `devnet_genesis()` artık
   `DEVNET_TEST_MNEMONIC`'ten türetiliyor; `qv-wallet devnet-import` ile
   anında genesis pre-fund'ı görünür. `run-single.{ps1,sh}` + `run-all.{ps1,sh}`.
5. **C-05 kapandı (2026-05-22)** — keystore v2 + per-account `view_keypairs`
   (view key persist edilir, restart'a dayanır).
6. **Audit-mode view-key export + Encrypted address book + Per-output
   selective disclosure (2026-05-22)** — `.qvview`, `wallet.json.contacts`,
   `.qvdisclose` formatları + ilgili CLI/HTTP/UI.
7. **Multi-account UI + Tx history + Devnet faucet + QR scanner (2026-06-02)** —
   `/api/wallet/{accounts,switch-account}`, `qv-wallet::history` modülü
   (`HistoryLog` + `merge_with_received`), `qv-wallet::faucet::drip` +
   `/api/devnet/faucet`, BarcodeDetector tabanlı kamera QR okuma.
8. **Multi-tenant cüzdan + LAN bind + monitör tx geçmişi + lag-watchdog
   (2026-06-03)** — `qv-wallet::session` (SessionStore + Bearer extractor),
   `Backend::{Single, Multi}` refactor, register/login/logout endpoint'leri,
   per-user `wallets/<username>/wallet.json`, UI auth paneli, HTTP'de
   clipboard + QR auth header fix'i, node-monitor tx geçmişi 60→600 default
   ve 10000 entry cap, `devnet/watchdog.ps1` + `QV_LAG_RESTART` env'i.
9. **Binary dağıtımı C-1..C-5 (2026-06-05)** — `.github/workflows/release.yml`
   (tag-tabanlı Windows x64 + macOS arm64 release), üç binary crate'te
   `build.rs` + `VERSION_STRING` sürüm damgası, `docs/INSTALL.md` kullanıcı
   kılavuzu, `docs/PUBLIC-DEVNET-RPC.md` operatör kılavuzu (Caddy + Let's
   Encrypt + rate limit + systemd), `qv-wallet --network devnet|local`
   alias'ı (`effective_rpc_url()`).
10. **Faz 6 D-1 — ReadInputDatum (2026-06-05)** — `OpCode::ReadInputDatum
    (0x6A)` + interpreter + gas (10) + 3 unit test. **Henüz lokalde
    derlenmedi** — sıradaki oturumda `cargo build -p qv-script` +
    `cargo nextest run -p qv-script` doğrulaması gerekli.

Detay için: ROADMAP.md "Mevcut Durum" tablosu ve PROJECT_STATUS.md.

## Çalışan akış (mevcut demo)

```powershell
.\devnet\run-all.ps1 stop

$env:QV_WALLET_MULTI = "1"      # multi-tenant mod
$env:QV_WALLET_BIND  = "0.0.0.0:7777"   # LAN'a aç
$env:QV_LAG_RESTART  = "1000"   # 1000+ blok geride kalan node auto-restart

.\devnet\run-all.ps1 start
```

→ 4 node + cüzdan UI (multi-tenant) + node-monitor + watchdog. Telefon
LAN'dan `http://<bilgisayar-LAN-IP>:7777` ile register edebilir.

## Yol Haritası (A→C→D→E→B sırasında ilerliyoruz)

Kullanıcı bu sırayı tercih etti. A ✅ ve C ✅ bitti; sıradaki = **D-2
(`amm_pool_lock` template)**, ardından D-3..D-6 → E → B.

### ✅ A — Dokümantasyon hizalama (BİTTİ — 2026-06-03)

ROADMAP.md "Mevcut Durum" tablosuna 2026-06-03 satırı eklendi,
PROJECT_STATUS.md'ye yeni bölüm eklendi, bu prompt yeniden yazıldı.

### ✅ C — Binary dağıtımı (BİTTİ — 2026-06-05)

Altı alt-görev tamam:

- **C-1** `.github/workflows/release.yml` — tag-tabanlı multi-platform
  (Windows x64 + macOS arm64) release pipeline. `softprops/action-gh-release@v2`
  ile GitHub Release'e zip/tar.gz + sha256 + BUILD-INFO upload.
- **C-2** Üç binary crate'te `build.rs` + `VERSION_STRING` const
  (cli.rs'lerde). `--version` çıktısı `<pkg> (<tag>, git <short_sha>)`.
- **C-3** `docs/INSTALL.md` — Windows + macOS kullanıcı kurulum kılavuzu.
- **C-4** `docs/PUBLIC-DEVNET-RPC.md` — Caddy + Let's Encrypt + rate
  limit + fail2ban + Prometheus + systemd sertleştirilmiş unit.
- **C-5** `qv-wallet --network devnet|local` alias'ı, `effective_rpc_url()`.
- **C-6** PROJECT_STATUS.md + bu prompt güncellendi (bu satır).

Sıradaki oturumda doğrulama: kullanıcı `cargo build -p qv-wallet -p
qv-node -p qv-miner` çalıştırmalı; build.rs yol çözümlemesi muhtemel
küçük hata kaynağı (`../../.git/HEAD` cargo working-dir'ine göre).

### 🚧 D — Faz 6: Script VM + DeFi temelleri (devam ediyor)

**D-1 ✅ (2026-06-05)** — `OpCode::ReadInputDatum (0x6A)` + interpreter
+ gas + 3 unit test. Stack: pop index `i` → push resolved input #i'nin
datum baytları. `ReadOutputDatum`'a paralel. AMM/lending kovenant'larının
old-state'i okumak için temel yetenek. Henüz lokal build doğrulaması
yapılmadı.

**D-2 sıradaki — AMM kovenant locking script.** Plan:

```text
amm_pool_lock(token_a_id, token_b_id, fee_bps):
  Sabitler:
    SELF_SCRIPT_HASH   = SHA3-256(this script bytes)  // recursive — D-2.1
    POOL_DATUM_LAYOUT  = | token_a_id (32) | token_b_id (32) | reserve_a (8 LE) | reserve_b (8 LE) | lp_total (8 LE) | fee_bps (2 LE) |

  Adımlar:
    1. ReadInputDatum 0  → old_datum_bytes
    2. ReadOutputDatum 0 → new_datum_bytes
    3. Slice old_datum 0  32 → old_a_id ; Slice new_datum 0 32 → new_a_id ; Eq Verify
    4. Slice old_datum 32 32 → old_b_id ; Slice new_datum 32 32 → new_b_id ; Eq Verify
    5. Slice old_datum 64 8 → old_a (u64 LE)
    6. Slice old_datum 72 8 → old_b
    7. Slice new_datum 64 8 → new_a
    8. Slice new_datum 72 8 → new_b
    9. old_a * old_b → k_old (u128)
    10. new_a * new_b → k_new (u128)
    11. k_new >= k_old → Verify  // invariant
    12. AssertOutputScriptHash 0 SELF_SCRIPT_HASH  // pool stays AMM
    13. Op1  // success
```

Bilinmesi gerekenler:
- **Self-script-hash döngüsü** (D-2.1): Locking script kendi hash'ini
  bilemez. İki çözüm: (a) hash'i witness'tan al + AssertOutputScriptHash
  ile match'le; (b) "script hash placeholder" desenli template — script
  derleyici sonradan kendi hash'iyle yamayıp commit eder. Cardano (b)
  yolunu kullanır.
- **Slice + Mul u128 desteği**: Şu an `Mul` opcode'u i64'te wrapping
  yapıyor. `Mul` u128 versiyonu (`MulU128`?) ya da çift-genişlik trick'i
  gerekebilir. İncele.
- **u64 LE bayt → integer dönüşümü**: `Slice` bayt verisi döndürür;
  bunu `Int` value'ya çevirmek için yeni bir opcode `BytesToInt` ya da
  benzeri lazım olabilir.

D-2 başlamadan önce bu üç tasarım kararını netleştir, gerekirse mini-ADR
yaz.

**D-3..D-6 sırası (D-2'den sonra):**

- **D-3** — `qv-defi::tx_helpers::build_swap_tx`: cüzdan tarafı off-chain
  helper (pool UTXO'sunu bul, yeni PoolDatum hesapla, swap tx'i kur).
- **D-4** — CLI: `qv-wallet swap` (`qv-defi::amm::compute_swap_output`
  matematiği zaten yazılı; sadece pipe'lamak gerek).
- **D-5** — HTTP + UI: `/api/defi/swap`, `/api/defi/pools`, cüzdan UI'da
  Send panelinin yanına Swap paneli.
- **D-6** — Lending (`qv-defi::lending` matematik tarafı tamam) + oracle
  entegrasyonu.

#### D ana hatları

**Hedef:** Programlanabilir UTXO'lar, AMM ve lending.

**Görevler (öncelik sırası):**

1. **W-01 CLI komutları** — `swap`, `lp-add`, `lp-remove`, `borrow`,
   `repay`, `pool-info`. UI'a da Send panelinin yanına "Swap" sekmesi.
2. **`qv-defi::amm`** — constant-product `x·y=k` gerçek state. **Shared UTXO
   Pattern**: tek bir AMM UTXO + script invariant. Datum'da rezervler.
3. **`qv-defi::lending`** — collateralized lending temel akış (deposit /
   borrow / repay / liquidate).
4. **`qv-defi::oracle`** — fiyat oracle UTXO'su (zincir-içi multi-sig oracle).
5. **`qv-script::templates`** — `amm_swap_template`, `lending_template`
   covenant'ları.

**Bağımlılık:** Faz 6 ADR-002 (DeFi mimarisi)'nde belirtilen. Yeni ADR
gerekirse açılır.

### E — Faz 7: Encrypted mempool + MEV koruması (tam wiring)

**Hedef:** MP-01 tam uçtan uca + T-01 Pedersen DKG.

**Görevler:**

1. **T-01 — Pedersen DKG / Feldman VSS** implementasyonu. `qv-crypto::threshold`
   modülünde. Validator komitesinin t-of-n threshold anahtarı oluşturması.
2. **`DkgEnvelopeDecryptor`'a gerçek köprü** — şu an stub var. DKG çıktısı
   komite üyeleri arasında dağıtılıyor; lider blok kabulünde komiteden
   t pay topluyor → mempool zarflarını çözüyor.
3. **Validator komitesi seçimi** — slot başına stake'e orantılı n-üyeli
   komite (VRF tabanlı).
4. **Mempool decrypt entegrasyonu** — `qv-node::slot_ticker` lider olduğunda
   önce komiteden pay topla, sonra zarfları aç, sonra batch sırala.
5. **Deterministik batch sıralama** — canonical hash sırası (MEV adres
   önceliği yok). Spec ve test.

**Bağımlılık:** Faz 7 — ADR-003 (MEV stratejisi).

### B — Demo sağlamlaştırma (HTTPS + rate limit + TTL sweep)

**Hedef:** Mevcut multi-tenant LAN demo'sunu üretim-seviyesine yakınlaştır.

**Görevler:**

1. **HTTPS** — axum'a `tokio-rustls` ekle; `--tls-cert <pem> --tls-key <pem>`
   flag'leri. Self-signed sertifika üretme script'i (`devnet/gen-cert.ps1`).
   Telefonda BarcodeDetector + clipboard HTTPS'te çalışır → QR scanner
   gerçekten kullanılır olur.
2. **Faucet rate limit** — `qv-wallet::faucet`'a kullanıcı başına / IP başına
   throttle ekle. Per-user counter `wallets/<user>/.faucet-state`'te,
   24 saatte max 1.000.000 birim.
3. **Session TTL background sweep** — `qv-wallet::session::SessionStore::gc()`
   zaten var; 60sn'de bir çalışan tokio task ile sürdür.
4. **Watchdog crash-loop koruması** — `watchdog.ps1`'e "son 5 dakikada >3
   restart" kuralı; aşılırsa o node için restart'ı askıya al ve loga
   kalın bir uyarı yaz.

## Çalışma kuralları

- Bir dosyayı **Edit etmeden önce mutlaka `Read`** et.
- Kod kuralları `CLAUDE.md`'de: `unwrap`/`expect`/`panic`/`indexing`/`integer_division`/`float_arithmetic` yasak (clippy deny), `thiserror`, her kripto primitifi hibrit (klasik+PQC), deterministik script VM.
- ADR'ler Türkçe, commit mesajları Türkçe.
- Her yeni özellik için en az 3 unit + (mümkünse) 1 integration test.
- Workspace `forbid(unsafe_code)`; `unsafe` zorunlu olursa `// SAFETY:` yorumu.
- **Asla** sandbox'ta `cargo build` çalıştırma — kullanıcı lokalde yapar.
- **AskUserQuestion**'ı mimari karar gerektiğinde kullan (örn. HTTPS için
  rustls vs native-tls, sertifika dağıtım stratejisi).

## Sıradaki ilk adım

1. **Bekleyen build doğrulamaları** (2026-06-05 seansının ekleri henüz
   lokalde derlenmedi — kullanıcıdan çalıştırıp çıktıyı yapıştırmasını iste):
   - `cargo build -p qv-script` + `cargo nextest run -p qv-script`
     (D-1 ReadInputDatum)
   - `cargo build -p qv-wallet -p qv-node -p qv-miner` (C-2 build.rs sürüm
     damgası; `../../.git/HEAD` yol çözümlemesi muhtemel küçük hata kaynağı)
2. **D-2 — `amm_pool_lock` template** ile devam et. Başlamadan önce üç
   tasarım kararını netleştir (self-script-hash döngüsü, u128 çarpma /
   `Mul` genişliği, `BytesToInt` ihtiyacı) — gerekirse mini-ADR yaz.
3. D-2 bitince sıra: **D-3 → D-4 → D-5 → D-6** (yukarıdaki plan), sonra E.
4. Araya sıkıştırılabilir küçük işler — açık **B-grubu**: HTTPS
   (tokio-rustls + `--tls-cert/--tls-key`), faucet rate limit, session TTL
   background sweep (tokio task), watchdog crash-loop koruması.
