# QuantumVault L1 — Kaldığımız yerden devam (yarın bu mesajı Claude'a yapıştır)

QuantumVault L1 blokzincir projesinde (Rust workspace, kuantum-korumalı,
UTXO tabanlı Katman-1) birlikte çalışıyoruz. Bu mesaj önceki oturumda nerede
kaldığımızı anlatıyor — lütfen oku, durumu anla. **Ben hâlâ derlemedim;
ilk işin doğrulama olmalı**, sonra varsa hataları düzelt.

## Proje
- Konum: `C:\Users\mbostan\Desktop\L1\L1 Blockchain`
- Rust cargo workspace, 13 crate (`crates/qv-*`). Mimari kararlar `CLAUDE.md`'de.
- **Önemli:** Senin ortamında Rust derleyici yok. Sen kodu yazıyorsun; **ben
  lokalde** (Windows PowerShell) `cargo test` / `cargo clippy` çalıştırıp
  çıktıyı sana yapıştırıyorum.

## Şimdiye kadar bitenler

1. **Consensus fork/finalite denetimi** — `docs/security/qv-consensus-fork-finality-audit.md`.
   KRİTİK çift-finalite hatası kapatıldı; 7 bulgu + ek bulgular giderildi.
2. **ADR-008 / 009 / 010** — Genesis maxvalid-bg (çekirdek), sabit-nokta lider
   kontrolü, bootstrap sync (`SyncManager` iskeleti).
3. **ADR-012 (sighash)** — `Transaction::sighash()`, `SigHash` opcode (0x69),
   `p2pkh_pqc` ve `stealth_p2pkh` template'leri sighash kullanıyor; in-flight
   witness yeniden-oynatma açığı kapandı. Derlendi + tüm testler + clippy YEŞİL.
4. **ADR-011 (stealth) — TÜM FAZLAR uygulandı.**
   - **Faz 1-3:** Daha önce derlendi + test edildi, YEŞİL.
   - **Faz 4 — RPC (DERLENMEDİ):**
     - `qv-crypto`: `HybridKeyPair::from_raw_parts` + `x25519/kyber_secret_bytes`.
     - `qv-privacy::stealth::scan_output_view(view_kp, spend_pk, output)` —
       spend gizli anahtarı gerektirmeyen tarama; mevcut `scan_output`
       artık ince bir wrapper.
     - `qv-node` artık `qv-privacy`'ye bağlanıyor (`Cargo.toml`).
     - `qv-node::rpc`:
       - `StealthViewKey` (wire payload — kyber/dilithium seviyeleri + 4 hex
         anahtar + spend_pk hex; `into_view_keys()` ile doğrulanır).
       - `StealthScan` genişledi: `shared_secret_hex`,
         `onetime_pk_hash_hex` eklendi (cüzdanın daha sonra harcayabilmesi için).
       - Trait imzaları `view_key_hex: String` yerine `view_key: StealthViewKey`.
       - `scan_stealth` gerçek implementasyon: UTXO setini gez, her
         `stealth_info`'lu çıktıda `scan_output_view` çağır, locking script
         doğrulamasıyla view-tag false positive'lerini ele.
       - `get_balance_for` artık `scan_stealth`'i çağırıp toplamı dönüyor.
   - **Faz 5 — Cüzdan uygulaması + QR / .qvaddr UX (DERLENMEDİ):**
     - Yeni modüller: `qv-wallet/src/{address.rs, server.rs, server_ui.rs}`.
     - `qv-wallet::address`: `qvst1…` (tam payable adres = bincode(view_pk+spend_pk
       payload), hex) + `qvfp1…` (kısa fingerprint).
     - `qv-wallet::rpc_client`: `get_balance_for` ve `scan_stealth` metodları;
       `StealthMatch` DTO; `view_key_payload` yardımcısı.
     - `qv-wallet::server`: axum HTTP API.
       - `POST /api/wallet/{create,import,unlock,lock}`,
         `GET /api/{status,wallet/address,balance,utxos}`, `POST /api/send`.
       - `AppState` ile kilitli/açık durum; spend gizli anahtarı sadece
         RAM'de.
     - `qv-wallet::server_ui`: gömülü tek-dosya HTML/CSS/JS UI
       (oluştur/import/unlock/balance/utxos/send).
     - CLI yeni komutlar: `balance`, `scan` (gerçek RPC çağrısı), `send-stealth`
       (otomatik UTXO seçimi + stealth çıktı), `serve` (UI'i 127.0.0.1'de açar
       — non-loopback IP reddedilir).
     - `cmd_address` artık hem `qvst1…` tam adres hem `qvfp1…` fingerprint basıyor.
     - Workspace `Cargo.toml`'a axum 0.7 (`json + tokio + http1 + query`) ve
       `qrcode 0.14` (sadece `svg` feature) eklendi.
     - **K-03 kapandı — post-apply UTXO commitment (qv-node lokal):**
       - `SlotTicker::compute_post_apply_commitment(&[Transaction])`:
         canlı `UtxoStore.entries()` snapshot'lanır → `InMemoryUtxoSet`
         kurulur → blok'un her tx'i sırayla (inputs remove → outputs
         insert) uygulanır → `commitment_root()` döndürülür. Chained
         tx'ler doğal olarak çalışır (önceki tx'in çıktısı sonraki için
         girdi olabilir).
       - `BlockHeader.utxo_commitment` artık `ZERO` değil; gerçek
         post-state root.
       - `_utxo_store` field underscore-prefix'i kaldırıldı (artık
         kullanılıyor).
       - 2 yeni `tokio::test`: empty block için commitment = snapshot;
         non-trivial block için bağımsız `InMemoryUtxoSet` mutasyonu
         ile aynı root.
       - K-05 (qv-miner aynı boşluk) ayrı follow-up — miner ayrı binary,
         RPC köprüsü gerekli (`qv_getPostApplyCommitment` veya benzeri).
     - **C-05 kapandı — view key kalıcılığı (keystore v2):**
       - Yeni `qv-wallet::keystore::PersistedViewKey` struct: kyber_level
         + raw bytes (x25519_pk/sk + kyber_pk/sk). Bayt seviyesinde
         `HybridKeyPair::from_raw_parts` ile yeniden kurulabiliyor.
       - `WalletSecret` artık `view_keypairs: BTreeMap<u32, PersistedViewKey>`
         taşıyor (per-account view key map). `#[serde(default)]` v1
         keystore'larıyla geri uyumlu (`view_keypairs` eksikse boş map).
       - Envelope versiyonu 1 → 2'ye bumplandı; v1 dosyalar hâlâ okunur,
         ilk `save` ile in-place v2'ye yükselir.
       - Yeni `WalletKeystore::unlock_account(path, password, account,
         deriver)` helper: keystore'u açar, account için view key varsa
         yeniden kullanır, yoksa fresh üretip aynı parolayla kaydeder ve
         deterministik spend key ile birleştirip `StealthKeys` döner.
       - `init` / `import-mnemonic` / `devnet-import` artık account-0 view
         key'ini ÖNCEDEN üretip keystore'a yazıyor — kullanıcının ilk
         gördüğü adres bir sonraki `unlock` ile birebir aynı olacak.
       - Yeni testler: `persisted_view_keypair_roundtrip` (encap/decap
         ile fonksiyonel doğrulama), `v1_keystore_reads_with_empty_view_map`.
     - **Tek-komutla launcher script'leri:**
       - `devnet/run-single.{ps1,sh}` — 1 node + cüzdan UI tek bir komutta:
         build → `qv-node --init --network devnet` → arka planda node →
         `qv-wallet devnet-import` (parola: `devnetpw`) → arka planda
         `qv-wallet serve` → tarayıcı aç. `{start|stop|status|clean}`.
       - `devnet/run-all.{ps1,sh}` — TAM PAKET: mevcut `run-devnet.{ps1,sh}`'ı
         (4 node) çağırıp üzerine cüzdan UI ve `node-monitor/` Node.js
         panelini açar. `{start|stop|status|clean}`.
       - `devnet/SCRIPTS.md` — Türkçe kılavuz; her script'in ne yaptığı,
         ortam değişkenleri, manuel duman-test akışı (iki cüzdan, transfer)
         bu dosyada.
     - **Devnet bootstrap köprüsü (uçtan uca akış kapandı):**
       - `qv-wallet::hd::DEVNET_TEST_MNEMONIC` — sabit BIP-39 24-kelime
         test vektörü ("abandon …×23 art"); `DefaultSeedDeriver::derive_spend_key`
         artık `pub`.
       - `qv-node::genesis::devnet_genesis()` artık bu mnemonic'ten cüzdanın
         HD yolunda türettiği ilk 10 spend public key'e fon dağıtıyor (eski
         `sha3("qv-devnet-account-"||i)` yolu kaldırıldı). Belirleyici;
         iki çağrı bayt-bayt aynı bloğu üretir.
       - `qv-node` `init` çıktısında mnemonic'i ekrana basıyor, kullanıcı
         doğrudan `qv-wallet import-mnemonic "…"` ile içeri alabilir.
       - `qv-wallet devnet-import [--password <pw>]` — tek atışta köprü
         (mnemonic'i kopyalamadan): keystore'u test mnemonic'inden kurar,
         ilk hesabın adresini basar; cüzdan `balance` komutu artık
         genesis fonunu görmeli.
       - Yeni invariant testi `devnet_genesis_matches_wallet_test_mnemonic`:
         genesis çıktılarındaki kilit script'ler, cüzdanın türeteceği
         spend-pk-hash ile **bayt-bayt** eşleşmek zorunda — köprü
         sözleşmesi.
     - **Düz `p2pkh_pqc` köprüsü (devnet ilk-para deneyimi):**
       - Cüzdan artık `qv_scanP2pkh(pubkey_hash_hex)` RPC'si ile kendi
         spend-pk-hash'ine kilitli düz UTXO'ları da tarıyor.
       - `TxBuilder::sign_plain_input(idx, sk, pk)` — per-input düz
         `p2pkh_pqc` imzalama (sighash, witness `<sig> <pubkey>`).
       - `handle_balance` / `handle_utxos` her iki havuzu (stealth + plain)
         birleştiriyor; `UtxosResponse` artık `{ stealth: [...], plain: [...] }`.
       - `handle_send` ve `cmd_send_stealth`: önce stealth, kalanı plain'den
         büyükten küçüğe seç; her girdiyi kendi witness formatıyla imzala.
         Çıktılar daima stealth (düz genesis fonu, ilk transferde
         otomatik olarak gizliye dönüşür).
       - CLI: `balance` ve `scan` artık iki havuzu da gösteriyor (etiketli);
         UI'daki UTXO tablosuna "Kind" sütunu eklendi (stealth/plain renk
         kodlamalı).
       - Audit'te bulunan küçük temizlikler: `UnlockedWallet`'tan kullanılmayan
         `mnemonic` alanı kaldırıldı (`dead_code`/`-D warnings` için), eski
         `test_cli_parse_address` desen-eşlemesine `..` rest-pattern eklendi.
     - **QR + `.qvaddr` desteği:**
       - `qv-wallet::qvaddr` modülü: `Qvaddr` JSON dosya yapısı (load/save +
         fingerprint doğrulama), `address_to_qr_parts` / `address_from_qr_parts`
         (3 KB'lık adresi `QVADDR1:k/N:<HEX>` ön ekiyle çok-parçalı QR'a böler),
         `render_qr_svg` / `render_qr_unicode`.
       - HTTP endpoint'leri: `GET /api/wallet/address.qvaddr` (dosya
         indirme), `GET /api/wallet/fingerprint.svg`, `GET
         /api/wallet/address-qr?parts=N`, `POST
         /api/wallet/{import-qvaddr,qr-reassemble}`.
       - UI: adres panelinde küçük fingerprint QR'ı, "Download .qvaddr" ve
         "Show full QR (2 codes)" düğmeleri; gönderme panelinde
         `<input type="file" .qvaddr>` ile alıcı adresini otomatik doldurma.
       - CLI: `address --qr` (terminale fingerprint için ASCII QR),
         `--full-qr --qr-parts N` (tam adres için çok-parçalı ASCII QR),
         `--save <path>` (`.qvaddr` yaz); `send-stealth --to-qvaddr <path>`
         ile dosyadan alıcı oku.

## Şu an: doğrulama
Henüz hiçbir Faz 4/5 kodu derlenmedi (önceki oturumda kullanıcı testi
ertelemişti). İlk işin:

```
cargo test -p qv-crypto
cargo test -p qv-privacy
cargo test -p qv-node
cargo test -p qv-wallet
cargo clippy --workspace --all-targets -- -D warnings
```

Olası küçük dert kaynakları (önden tahmin):
- axum 0.7 feature seti / hyper sürüm çakışmaları — workspace'te
  `axum = { version = "0.7", default-features = false, features = ["json","tokio","http1"] }`.
- `tokio::test` makro feature'ı (workspace'te `features = ["full"]` var — sorun
  beklenmiyor).
- Belki `unused_imports` veya `unused_qualifications` uyarıları.

Derleme yeşil olunca sıradaki polish:

1. **Wallet UI'i lokalde dene:** Bir devnet node çalışırken
   `cargo run -p qv-wallet -- --keystore ./test-wallet.json --rpc http://127.0.0.1:8080 serve`
   ile 127.0.0.1:7777 aç, cüzdan oluştur, başka bir cüzdana transfer at,
   bakiyenin değiştiğini gör.
2. **Entegrasyon testi:** `crates/qv-wallet/tests/` altına axum'u
   `tower::ServiceExt::oneshot` ile sürerek end-to-end UI testi ekle (opsiyonel).
3. **Genesis stealth fonlama:** Devnet genesis'i kullanıcının cüzdan
   adresine fon dağıtacak şekilde nasıl başlatacağına dair kısa bir
   "ilk para" rehberi (docs/) — şu an cüzdanlar boş başlıyor; bir
   `p2pkh_pqc` UTXO'sundan `send-stealth`'e geçiş için bir köprü
   notu mantıklı olur.
4. **Konsensüs / DA / DeFi** alanlarındaki "ileride" işler hâlâ açık —
   `docs/MASTER_PLAN.md` ve `docs/ROADMAP.md` referans.

## Çalışma kuralları

- Bir dosyayı Edit etmeden önce mutlaka `Read` et.
- Kod kuralları `CLAUDE.md`'de: `unwrap/expect/panic/indexing` yasak (clippy
  deny), `thiserror`, her kripto primitifi hibrit, deterministik script VM.
- ADR'ler ve commit mesajları Türkçe.

**Başla — önce derle, hataları gider, sonra polish'e geç.**
