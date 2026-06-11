# QuantumVault L1 — Kurulum Kılavuzu

Bu kılavuz hazır binary'leri indirip QuantumVault L1 testnet'inde
**non-custodial** bir cüzdan açmanı anlatır. Geliştirici aracı gerekmez —
Rust toolchain'i yüklemene gerek yok.

> **Güvenlik notu.** İndirdiğin binary'lerin git revizyonu ve build
> kaynağı `BUILD-INFO.txt` ile arşivin içinde gelir. Her dosya
> `<filename>.sha256` checksum'unun yanı sıra GitHub Release sayfasında
> da yayınlanır. Resmi olmayan kaynaklardan indirme.

## İçerik

- [Hızlı başlangıç (Windows)](#hizli-baslangic--windows)
- [Hızlı başlangıç (macOS)](#hizli-baslangic--macos)
- [İlk cüzdan oluşturma](#ilk-cuzdan-olusturma)
- [Devnet'e bağlanma](#devnete-baglanma)
- [Test parası çekme (faucet)](#test-parasi-cekme--faucet)
- [Kendi node'unu çalıştırma](#kendi-nodeunu-calistirma-opsiyonel)
- [Sürüm doğrulama](#surum-dogrulama)
- [Sık takılma noktaları](#sik-takilma-noktalari)

## Hızlı başlangıç — Windows

**Sistem gereksinimi:** Windows 10 (1809+) ya da Windows 11, x64.

1. <https://github.com/anthropics/quantumvault-l1/releases/latest> adresine git.
2. `quantumvault-v<sürüm>-windows-x64.zip` dosyasını indir.
3. **İndirilen `.zip`'i sağ tıkla → Properties → Unblock** kutucuğunu işaretle, Apply de.
   (Windows internetten gelen dosyaları varsayılan engeller; bu adım atlanırsa
   exe'ler "publisher unknown" uyarısı verir.)
4. Zip'i açtığın bir yere çıkar (örn. `C:\QuantumVault\`).
5. **Windows Defender exclusion** (opsiyonel, performans için):

   Yönetici PowerShell'de:
   ```powershell
   Add-MpPreference -ExclusionPath 'C:\QuantumVault'
   ```

6. PowerShell'i bu klasöre yönlendirerek aç:
   ```powershell
   cd C:\QuantumVault\bin
   .\qv-wallet.exe --version
   ```

   Doğru çalıştıysa şuna benzer bir satır basacak:
   ```
   qv-wallet 0.1.0 (v0.1.0, git a1b2c3d4e5f6)
   ```

7. Cüzdan UI'sını başlat:
   ```powershell
   .\qv-wallet.exe serve --bind 127.0.0.1:7777
   ```

8. Tarayıcıdan `http://127.0.0.1:7777` adresine git. Cüzdan UI açılır.

## Hızlı başlangıç — macOS

**Sistem gereksinimi:** macOS 12 Monterey ya da üstü, **Apple Silicon (M1/M2/M3+)**.

> Intel Mac kullanıcıları: bu seansta x86_64 macOS binary'si dağıtmıyoruz.
> Rosetta 2 üzerinden çalıştırma testi yapılmadı; Rust toolchain'i kurup
> kaynaktan derleme öneriliyor.

1. <https://github.com/anthropics/quantumvault-l1/releases/latest> adresine git.
2. `quantumvault-v<sürüm>-macos-arm64.tar.gz` dosyasını indir.
3. Terminal aç, indirme klasörüne git ve aç:

   ```bash
   cd ~/Downloads
   mkdir -p ~/QuantumVault && tar -xzf quantumvault-v*-macos-arm64.tar.gz -C ~/QuantumVault
   cd ~/QuantumVault
   ```

4. **Gatekeeper quarantine kaldır** (Apple Developer ile imzalanmamış binary):

   ```bash
   xattr -dr com.apple.quarantine ~/QuantumVault
   ```

   Bu adımı atlatırsan macOS "qv-wallet hasarlı, çöp kutusuna at" diyecek.

5. Sürümü doğrula:

   ```bash
   ./bin/qv-wallet --version
   ```

6. Cüzdan UI'sını başlat:

   ```bash
   ./bin/qv-wallet serve --bind 127.0.0.1:7777
   ```

7. Tarayıcıdan `http://127.0.0.1:7777` adresine git.

## İlk cüzdan oluşturma

İlk açılışta UI seni iki seçenekle karşılar:

- **Create new wallet** — Sıfırdan üretilen bir BIP-39 mnemonic + parola.
- **I have a mnemonic** — Mevcut 24-kelimelik recovery phrase ile içe aktar.

**Yeni cüzdan akışı:**

1. Parola seç (en az 8 karakter). Bu parola yerel makinende **Argon2id +
   AES-256-GCM** ile keystore dosyasını şifreler. Asla bir sunucuya
   gönderilmez.
2. "Create new wallet" tıkla.
3. Ekranda 24 kelimelik **mnemonic** belirir — **bir kerelik**.
4. **Yaz** (kâğıda, fiziksel olarak). Bilgisayara kaydetme. Bu cümleyi
   kaybedersen cüzdana ulaşmanın başka yolu yok.
5. "I have written it down" tıkla. Cüzdan kullanıma hazır.

## Devnet'e bağlanma

Varsayılan olarak `--rpc http://localhost:8080` bekler — bu sende node
olmadığı için bağlanamaz. İki seçenek:

### 1) Public devnet RPC'sine bağlan (önerilen)

Cüzdanın `--network devnet` kısa-yolu bilinen resmi public testnet
RPC URL'sini otomatik kullanır — uzun URL'yi yazmana gerek yok:

**Windows:**
```powershell
.\qv-wallet.exe --network devnet serve --bind 127.0.0.1:7777
```

**macOS:**
```bash
./bin/qv-wallet --network devnet serve --bind 127.0.0.1:7777
```

Hangi URL'ye bağlandığını görmek için cüzdan UI'nin sağ üstündeki
durum çubuğuna bak — "node: https://..." şeklinde görünür. Mevcut
varsayılan: `https://rpc.testnet.quantumvault.example`. Kendi
endpoint'inle değiştirmek istersen `--rpc https://...` flag'ini
kullan, bu `--network`'i geçersiz kılar.

### 2) Kendi node'una bağlan

Aşağıda [Kendi node'unu çalıştırma](#kendi-nodeunu-calistirma-opsiyonel)
bölümüne bak.

## Test parası çekme (faucet)

Devnet'te yeni bir cüzdan başlangıçta **0 bakiyeli**dir. Test parası
almak için UI'nin **Balance** panelinde **"Get devnet test funds"**
butonuna bas. Cüzdan otomatik olarak devnet faucet'inden stealth bir
transfer alır; bir-iki blok sonra (~5 saniye) bakiyene yansır.

## Kendi node'unu çalıştırma (opsiyonel)

Bir public RPC'ye bağlanmak yerine kendi `qv-node`'unu da çalıştırabilirsin.
Aynı zip/tar.gz arşivinde `bin/qv-node` mevcut.

> **Not:** Genesis ve peer keşfi için `--network devnet` parametresi
> gerekir. Single-node bir devnet yerine **public testnet peer
> listesi**ne katılmak istiyorsan release notes'a bak — peer adresleri
> orada yayınlanır.

**Single-node lokal devnet (sadece tek başına):**

```powershell
# Windows
.\qv-node.exe --init --network devnet --data-dir ./my-node
.\qv-node.exe --network devnet --data-dir ./my-node --rpc-addr 127.0.0.1:8545
```

```bash
# macOS
./bin/qv-node --init --network devnet --data-dir ./my-node
./bin/qv-node --network devnet --data-dir ./my-node --rpc-addr 127.0.0.1:8545
```

Sonra cüzdanı `--rpc http://127.0.0.1:8545` ile başlat.

**Public testnet peer'larına katılma:** Release notes'taki bootstrap
adreslerini config dosyana ekleyip `seed_nodes` listesine koy.

## Sürüm doğrulama

İndirdiğin binary'nin doğru build olduğunu doğrulamak için:

```bash
# Linux/macOS — sha256 indirilen yanındaki .sha256 ile karşılaştır
shasum -a 256 quantumvault-v0.1.0-macos-arm64.tar.gz
cat quantumvault-v0.1.0-macos-arm64.tar.gz.sha256

# Windows
Get-FileHash quantumvault-v0.1.0-windows-x64.zip -Algorithm SHA256
Get-Content quantumvault-v0.1.0-windows-x64.zip.sha256
```

İki çıktı **birebir aynı** olmalı (sadece hash dizesi — `.sha256` dosyası
zaten saf hash içerir).

Çalışan binary'nin damgasını da kontrol edebilirsin:

```bash
./bin/qv-wallet --version
./bin/qv-node --version
./bin/qv-miner --version
```

Çıktı `<cargo_version> (<release_tag>, git <commit_hash>)` formatındadır;
release tag arşiv dosya adındakiyle, git commit hash GitHub Release
sayfasındakiyle eşleşmeli.

## Sık takılma noktaları

| Belirti | Nedeni | Çözüm |
|---|---|---|
| Windows'ta "Windows protected your PC" | SmartScreen imzasız binary'lere uyarır | "More info" → "Run anyway" |
| macOS "qv-wallet is damaged and can't be opened" | Gatekeeper quarantine | `xattr -dr com.apple.quarantine ~/QuantumVault` |
| `connection refused` cüzdan açılışta | RPC URL'i hatalı veya node kapalı | `--rpc` parametresini doğru ver, public URL kullanıyorsan internet bağlantısını kontrol et |
| `wrong password or corrupted keystore` | Parolayı yanlış girdin | Doğru parolayla tekrar dene. Parolayı kaybettiysen mnemonic'le yeniden oluştur. |
| Cüzdan UI açılmıyor (tarayıcıda DNS hatası) | URL'de `https://` yerine `http://` yaz | `http://127.0.0.1:7777` |
| Balance 0 ve faucet butonu hata veriyor | Public devnet faucet havuzu boşalmış olabilir | Birkaç dakika sonra dene; veya kendi devnet node'unu kur |
| `--version` "git unknown" basıyor | Tag'lenmemiş bir build (geliştirici sürümü) | Resmi release sayfasından indir |

## Mnemonic kaybı

**Mnemonic'i kaybedersen ve keystore parolanı bilmiyorsan cüzdanın
geri getirilemez.** Backup yapmaman halinde içindeki tüm test parası
erişilemez kalır. Mainnet sürümünden önce hardware wallet entegrasyonu
gelecek; o noktaya kadar mnemonic backup tek korumadır.

## Yardım

- Daha çok bilgi: <https://github.com/anthropics/quantumvault-l1/blob/main/README.md>
- Hata raporu: <https://github.com/anthropics/quantumvault-l1/issues>
- Sunucu kurmak istersen: [`PUBLIC-DEVNET-RPC.md`](PUBLIC-DEVNET-RPC.md)
