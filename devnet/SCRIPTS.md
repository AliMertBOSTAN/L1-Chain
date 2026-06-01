# QuantumVault — Devnet Çalıştırma Script'leri

Local geliştirme yığınını tek komutla ayağa kaldıran script'ler. Hepsi
hem **PowerShell** (`.ps1`) hem **bash** (`.sh`) sürümünde mevcut.

| Script | Ne kaldırır? | Tarayıcıda |
|---|---|---|
| `run-single.{ps1,sh}` | 1 node + cüzdan UI | http://127.0.0.1:7777 |
| `run-devnet.{ps1,sh}` | 4 node (konsensüs için) | — (CLI / RPC üzerinden izlenir) |
| `run-all.{ps1,sh}` | 4 node + cüzdan UI + node-monitor | http://127.0.0.1:7777 ve http://127.0.0.1:7070 |

Hepsi `start | stop | status | clean` alt komutlarını destekler.

## Hızlı başlangıç — tek node

En basit deneyim, "yeni cüzdan oluştur → bakiyeyi gör → transfer at":

**PowerShell:**

```powershell
cd 'C:\Users\mbostan\Desktop\L1\L1 Blockchain\devnet'
.\run-single.ps1 start
```

**bash:**

```bash
cd "C:/Users/mbostan/Desktop/L1/L1 Blockchain/devnet"
./run-single.sh start
```

Script şunları yapar:

1. `qv-node` ve `qv-wallet` binarylerini derler.
2. `qv-node --init` ile devnet config'i ve genesis'i kurar.
3. Tek bir node'u `127.0.0.1:8545` RPC'de arka planda başlatır.
4. Cüzdan keystore'unu `DEVNET_TEST_MNEMONIC` ile import eder
   (parola: `devnetpw`, `$env:QV_WALLET_PW` ile değiştirilebilir).
5. Cüzdan UI'sini `127.0.0.1:7777`'de başlatır.
6. Varsayılan tarayıcıyı açar.

Bakiyeyi tarayıcıdan görmelisin (~10 milyar küçük birim = 10 QV). Daha
sonra "Send" formuna başka bir `qvst1…` adresi yapıştırıp transfer
atabilirsin.

## Tam paket — 4 node + cüzdan + monitör

```powershell
.\run-all.ps1 start
```

```bash
./run-all.sh start
```

İki tarayıcı sekmesi açılır:

- **Cüzdan UI** — http://127.0.0.1:7777
- **Node Monitor** — http://127.0.0.1:7070 (4 node'un yükseklik, eş sayısı, mempool durumu)

Cüzdan node0'a (port 8545) bağlanır; node-monitor 4'ünü de takip eder.

> `run-all.sh start` Node.js gerektirir (node-monitor için). Node.js
> yüklü değilse cüzdan + 4 node'lu kısım çalışır, monitör atlanır.

## Durdurma

```powershell
.\run-single.ps1 stop
.\run-all.ps1 stop
```

PID'ler `work-single\pids` (tek node) ve `work4\pids` + `work4\extras-pids`
(tam paket) dosyalarında tutulur. Script bunları kullanarak hepsini öldürür.

## Durum kontrolü

```powershell
.\run-single.ps1 status
.\run-all.ps1 status
```

Her node'un RPC'sini sorar, cüzdan ve monitör HTTP uç noktalarını ping
atar. Beklenen çıktı:

```
  node0 rpc=8545  height=42
  node1 rpc=8546  height=42
  ...
  wallet   http=7777  unlocked=False  keystore_exists=True
  monitor  http=7070  up
```

## Temiz başlangıç

Tüm state'i (blok deposu, UTXO seti, cüzdan keystore) silmek için:

```powershell
.\run-single.ps1 clean
.\run-all.ps1 clean
```

## Çevre değişkenleri

| Değişken | Hangi script | Varsayılan | Açıklama |
|---|---|---|---|
| `QV_SINGLE_WORK` | run-single | `devnet\work-single` | Tek node iş klasörü |
| `QV_DEVNET_WORK` | run-devnet, run-all | `devnet\work4` | 4 node iş klasörü |
| `QV_WALLET_PW` | run-single, run-all | `devnetpw` | Cüzdan keystore parolası |
| `QV_MONITOR_PORT` | run-all | `7070` | node-monitor port'u |
| `QV_NODE_BIN` | run-devnet | `target/debug/qv-node[.exe]` | Hazır binary yolu (build atla) |
| `QV_WARMUP` | run-devnet | `12` | Node ayağa kalkma süresi (sn) |
| `QV_STAGGER` | run-devnet | `1` | Node'lar arası başlatma gecikmesi (sn) |

## Logları okuma

Her process kendi log dosyasına yazıyor:

```
work-single\
  node.log          # qv-node stdout
  node.err          # qv-node stderr
  wallet.log        # qv-wallet stdout
  wallet.err        # qv-wallet stderr
  init.log          # qv-node --init çıktısı (devnet test mnemonic burada)

work4\
  node0.log, node0.err, …, node3.log, node3.err
  wallet.log, wallet.err
  monitor.log, monitor.err
```

`init.log`'da basılan `DEVNET_TEST_MNEMONIC` (`abandon …×23 art`) hep aynı —
public test vektörü, mainnet için ASLA kullanma.

## Test akışı (manuel duman testi)

1. `.\run-single.ps1 start` çalıştır, tarayıcıyı bekle.
2. Cüzdan unlock formuna `devnetpw` yaz, Unlock.
3. Balance panelinde ~10 milyar (10 QV) görmen lazım — devnet
   genesis'in plain p2pkh allokasyonu.
4. Adres panelinde "Download .qvaddr" → dosyayı kaydet.
5. Aynı node'a ikinci bir cüzdan ile bağlan (manuel):
   ```powershell
   $env:QV_SINGLE_WORK = "devnet\work-single-2"
   # Aynı node ile farklı bir wallet directory'sinden farklı bir port'ta:
   cargo run -p qv-wallet -- --keystore .\wallet2.json --rpc http://127.0.0.1:8545 init
   cargo run -p qv-wallet -- --keystore .\wallet2.json --rpc http://127.0.0.1:8545 serve --bind 127.0.0.1:7778
   ```
6. wallet2'nin "Address" panelinden tam adresi (`qvst1…`) kopyala.
7. İlk cüzdanın "Send" formuna yapıştır, miktar gir, gönder.
8. Birkaç saniye sonra wallet2'nin UTXO tablosunda yeni bir **stealth**
   satırı görünmeli; bakiye artmış olmalı.

İlk cüzdanın UTXO listesinde kalan girdiler **plain**'den **stealth**'e
geçmiş olur (transfer change'i otomatik stealth çıktısı yapar — düz
genesis fonu bir kerelik gizli havuza akar, sonraki transferler tamamen
gizli).
