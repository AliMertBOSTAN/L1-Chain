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

Kullanıcı bu sırayı tercih etti. Sıradaki = **C**.

### ✅ A — Dokümantasyon hizalama (BİTTİ — 2026-06-03 bu prompt)

ROADMAP.md "Mevcut Durum" tablosuna 2026-06-03 satırı eklendi,
PROJECT_STATUS.md'ye yeni bölüm eklendi, bu prompt yeniden yazıldı.

### 🔜 C — Binary dağıtımı (Seçenek A — non-custodial — sıradaki)

**Hedef:** Kullanıcılar `qv-wallet.exe` (Windows), `qv-wallet` (Linux/macOS)
binary'sini indirip kendi makinelerinde non-custodial çalıştırsın. Sunucu
sadece node + (opsiyonel) faucet sayfası.

**Görevler:**

1. **Cross-compile setup.** Workspace'e `.cargo/config.toml` ekle —
   `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
   `aarch64-apple-darwin`, `x86_64-apple-darwin` target'ları.
2. **GitHub Actions release workflow.** Tag'lendiğinde her platform için
   binary build, strip, zip, GitHub Release'e attach. (`actions/upload-release-asset` veya `softprops/action-gh-release`.)
3. **Binary sürümleme.** `cargo metadata`'dan version, `git describe` ile
   commit hash; `qv-wallet --version` çıktısı bunları bassın.
4. **Kullanıcı dağıtım kılavuzu.** `docs/INSTALL.md` — Windows için
   "indir → yönetici PowerShell'de Defender exclusion → çalıştır", macOS
   için Gatekeeper notu, Linux için chmod+x. Kendi node'a vs. public devnet
   node'a nasıl bağlanacak.
5. **Public devnet RPC** (opsiyonel — bu seansın kapsamı dışı olabilir):
   `https://devnet.quantumvault.example/rpc` gibi bir endpoint kurmak için
   notlar; reverse proxy + rate limit. Geçici çözüm: kullanıcının kendi
   `qv-node`'unu çalıştırması.
6. **Cüzdan tarafı: bootstrap mode.** İlk açılışta "Devnet'e bağlan" / "Kendi
   node'una bağlan" seçimi. Devnet seçilirse otomatik RPC URL'i embed.

**Başarı kriteri:** Boş bir Windows makineye binary indir → çift tıkla →
cüzdan UI lokalhost'ta açıldı → register + balance + send çalıştı; **hiçbir
geliştirici aracı yüklemeden**.

### D — Faz 6: Script VM + DeFi temelleri

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

**C — Binary dağıtımı** ile başla. İlk soru kullanıcıya: "Hangi platformlara
build hedefliyoruz başlangıçta? (Windows-x64 mutlaka var; Linux-x64?
macOS arm64 + x64?)". Ardından `.cargo/config.toml` + GitHub Actions
workflow ile başla.
