# QuantumVault JSON-RPC API Reference

**Son güncelleme:** 2026-06-10

Kaynak: `crates/qv-node/src/rpc.rs` (`QvNodeApi` trait'i). Bu dosya node'un
JSON-RPC 2.0 yüzeyini belgeler — aşağıdaki 16 metodun tamamı kodda mevcuttur
ve `jsonrpsee` ile HTTP üzerinden servis edilir.

Devnet portları (`devnet/run-devnet`): node0 `8545`, node1 `8546`,
node2 `8547`, node3 `8548`. Örnekler node0'ı kullanır.

> **Kapsam dışı:** Cüzdan HTTP API'si (`qv-wallet serve` üzerindeki `/api/...`
> uç noktaları) bu dosyanın kapsamında DEĞİLDİR — o ayrı bir REST yüzeyidir.

Genel notlar:

- Tüm hash / ID / byte parametreleri lower-case, prefix'siz hex string'dir.
- İşlem ve blok payload'ları **hex-encoded bincode** formatındadır.
- Hata kodları: `-32602` geçersiz parametre, `-32603` sunucu/storage hatası.

---

## Zincir Sorguları

### qv_getTip

Zincirin ucunu (tip) döndürür: blok hash'i ve yükseklik.

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
```

**Yanıt (`TipInfo`):**

```json
{ "block_hash": "9f2c…", "height": 42, "timestamp": 0 }
```

Not: `timestamp` şu an her zaman `0` döner (`ChainEntry` zaman damgası tutmaz).

---

### qv_getBlockByHeight

Belirtilen yükseklikteki bloğu döndürür (yükseklik indekslidir). Genesis için `0`.

**Parametreler:** `[height: u64]`

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getBlockByHeight","params":[0]}'
```

**Yanıt:** tam `Block` nesnesi (header + transactions) veya `null`.

---

### qv_getBlockByHash

Hex blok hash'i ile bloğu döndürür. Hash'i `qv_getTip` ile öğrenebilirsiniz.

**Parametreler:** `[block_hash: hex string]`

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getBlockByHash","params":["9f2c…"]}'
```

**Yanıt:** tam `Block` nesnesi veya `null`.

---

### qv_getTx

TX ID ile işlemi arar. Önce clear mempool'a, sonra tip'ten geriye doğru son
50 bloğa (k-finalite penceresi) bakar.

**Parametreler:** `[tx_id: hex string]`

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTx","params":["tx_id_hex_buraya"]}'
```

**Yanıt:** `Transaction` nesnesi veya `null`. Not: 50 bloktan daha derindeki
işlemler bu metodla bulunamaz.

---

## UTXO Sorguları

### qv_getUtxo

`txid:index` formatında bir outpoint ile canlı UTXO bilgisini döndürür.

**Parametreler:** `["<tx_id_hex>:<index>"]`

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getUtxo","params":["tx_id_hex:0"]}'
```

**Yanıt (`UtxoInfo`):**

```json
{
  "value": 100000,
  "script_hash": "ab12…",
  "has_datum": false,
  "has_stealth": true,
  "script_hex": "51ab…",
  "datum_hex": null
}
```

- `script_hex` — ham kilitleme script baytları (hex). Faz 6 (D-4, cüzdan
  swap akışı) ile eklendi; DeFi istemcileri covenant script'ini bayt-bayt
  doğrulamak için kullanır.
- `datum_hex` — UTXO datum taşıyorsa ham datum baytları (hex), yoksa
  `null`. AMM havuz UTXO'larında bu, kanonik `PoolDatum` kodlamasıdır
  (90 bayt).

**Geriye uyumluluk:** iki alan da opsiyoneldir (`Option` +
`serde(default)`). Eski istemciler fazladan JSON alanlarını yok sayar;
yeni istemciler eski node yanıtlarını (`script_hex`/`datum_hex` yok)
`None` olarak çözümler. `qv-wallet swap` bu alanlara ihtiyaç duyar —
eski bir node'a karşı açık bir hata mesajıyla durur.

`null` dönerse UTXO yoktur (hiç olmadı ya da harcandı).

---

### qv_scanP2pkh

UTXO setini düz `p2pkh_pqc(pubkey_hash)` çıktılar için tarar. Cüzdanlar
"stealth olmayan" fonları (özellikle genesis allokasyonları) keşfetmek için
kullanır. Harcama, normal Dilithium spend anahtarıyla yapılır
(`shared_secret` gerekmez).

**Parametreler:** `[pubkey_hash_hex: 32-byte hex]`

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_scanP2pkh","params":["<32_byte_pubkey_hash_hex>"]}'
```

**Yanıt:** `P2pkhMatch` dizisi (tx_id, output_index'e göre sıralı):

```json
[ { "tx_id": "ab…", "output_index": 0, "value": 10000000000 } ]
```

---

### qv_listPools

Canlı UTXO setindeki tüm AMM havuzlarını döndürür (Faz 6 / D-5). Bir çıktı
**ancak ve ancak** şu iki koşulu sağlıyorsa havuz sayılır:

1. Datum'u tam 90 baytlık kanonik `PoolDatum` kodlamasıdır, **ve**
2. Kilitleme script'i, datum'daki `(token_a_id, token_b_id, fee_bps)`
   üçlüsünden yeniden üretilen `amm_pool_lock` script'iyle **bayt-bayt
   aynıdır** (sahte-havuz filtresi — D-4 cüzdan tarafı doğrulamayla aynı
   ilke).

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_listPools","params":[]}'
```

**Yanıt:** `PoolInfo` dizisi (outpoint'e göre deterministik sıralı):

```json
[
  {
    "outpoint": "ab…#0",
    "value": 1000,
    "token_a_id": "a1…",
    "token_b_id": "b2…",
    "reserve_a": 1000000,
    "reserve_b": 1000000,
    "lp_total": 1000000,
    "fee_bps": 30
  }
]
```

- `outpoint` kanonik `<txid_hex>#<idx>` formundadır — doğrudan
  `qv-wallet swap --pool` argümanına verilebilir.
- `lp_total` **datum-seviyesi LP pay muhasebesidir** — zincir üstü LP
  token YOKTUR (D-6+ kapsamı).
- Maliyet: çağrı başına O(UTXO seti) tarama — devnet ölçeği için kabul
  edilebilir; havuz sayısı arttığında kalıcı bir havuz indeksi planlanan
  iyileştirmedir.

---

## Stealth Tarama (ADR-011)

Her iki metod da `StealthViewKey` nesnesi alır. Spend **secret** anahtarı
asla gönderilmez — yalnızca view keypair (sırlarıyla) ve spend **public** key:

```json
{
  "kyber_level": 3,
  "dilithium_level": 3,
  "x25519_pk_hex": "…",  "x25519_sk_hex": "…",
  "kyber_pk_hex": "…",   "kyber_sk_hex": "…",
  "spend_pk_hex": "…"
}
```

`kyber_level` ∈ {1,3,5}, `dilithium_level` ∈ {2,3,5}; byte uzunlukları
sunucuda doğrulanır.

### qv_getBalanceFor

Verilen view key'in çözebildiği (decapsulate) tüm stealth UTXO'ların toplam
değerini döndürür. Tespit mantığı `qv_scanStealth` ile birebir aynıdır.

**Parametreler:** `[view_key: StealthViewKey]`

```powershell
$viewKey = '{"kyber_level":3,"dilithium_level":3,"x25519_pk_hex":"…","x25519_sk_hex":"…","kyber_pk_hex":"…","kyber_sk_hex":"…","spend_pk_hex":"…"}'
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_getBalanceFor","params":[' + $viewKey + ']}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

**Yanıt:** `u64` toplam bakiye (en küçük birim).

---

### qv_scanStealth

UTXO setini, verilen view key'in tespit edebildiği stealth çıktılar için
tarar. Harcama için gereken `shared_secret` ve `onetime_pk_hash` değerlerini
de döndürür.

**Parametreler:** `[view_key: StealthViewKey, from_height: u64, to_height: u64]`

> `from_height` / `to_height` şimdilik **yok sayılır** (UTXO seti yükseklik
> indeksli değil; tüm canlı set taranır). Parametreler ileride kırıcı
> değişiklik olmadan kullanılabilsin diye wire'da korunuyor.

```powershell
$viewKey = '{"kyber_level":3,"dilithium_level":3,"x25519_pk_hex":"…","x25519_sk_hex":"…","kyber_pk_hex":"…","kyber_sk_hex":"…","spend_pk_hex":"…"}'
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_scanStealth","params":[' + $viewKey + ', 0, 100]}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

**Yanıt:** `StealthScan` dizisi (deterministik sıralı):

```json
[
  {
    "height": 0,
    "tx_id": "ab…",
    "output_index": 1,
    "value": 50000,
    "shared_secret_hex": "cd…",
    "onetime_pk_hash_hex": "ef…",
    "kem_ciphertext_hex": "01…",
    "view_tag_hex": "a7",
    "kyber_level": 3
  }
]
```

- `height` şimdilik daima `0` (rezerve alan).
- `shared_secret_hex` + `onetime_pk_hash_hex` **hassastır** — spend secret ile
  birlikte UTXO'yu harcamaya yeter.
- `kem_ciphertext_hex` / `view_tag_hex`, `.qvdisclose` seçici ifşa dosyaları
  için ikinci bir RPC turu gerekmesin diye eklenmiştir.

---

## Mempool ve İşlem Gönderimi

### qv_sendTransaction

Hex-encoded bincode formatında imzalanmış bir işlemi clear mempool'a gönderir.
Yapısal doğrulamadan geçerse TX ID döner. (Tam UTXO/fee doğrulaması blok
işleme yolunda yapılır; RPC ekleme hızlı yoldur.)

**Parametreler:** `[tx_bytes: hex string]`

```powershell
$txHex = "deadbeef..."
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_sendTransaction","params":["' + $txHex + '"]}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

**Yanıt:** `TxId` (hex). Aynı TX iki kez gönderilirse `DuplicateTx` hatası.

Programatik örnek: `cargo run -p qv-node --example wallet_transfer`

---

### qv_getMempoolStatus

Mempool durumunu döndürür.

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getMempoolStatus","params":[]}'
```

**Yanıt (`MempoolStatus`):**

```json
{ "clear_pool_size": 3, "encrypted_pool_size": 0, "min_fee_rate": 1, "total_value": 150000 }
```

`total_value` clear pool'daki işlemlerin output toplamıdır (yaklaşık değer).

---

### qv_getPendingTransactions

Clear mempool snapshot'ındaki tüm bekleyen işlemleri deterministik sırada
(fee-density azalan, sonra tx-id artan) döndürür. Mempool **mutasyona
uğramaz** — snapshot okumadır. `qv-miner` slot kazandıktan sonra blok
gövdesini doldurmak için kullanır.

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getPendingTransactions","params":[]}'
```

**Yanıt:** hex-encoded bincode `Transaction` string dizisi:

```json
[ "0a1b2c…", "3d4e5f…" ]
```

---

## Blok Üretimi (qv-miner arayüzü)

### qv_submitBlock

Tam imzalı bir bloğu node'a teslim eder. Payload hex-encoded bincode
`Block`'tur. Node yapısal doğrulama yapar, bloğu ağ-kaynaklı bloklarla aynı
pipeline'a (zincir bağlantı kontrolü, UTXO apply, gossip relay) gönderir.

**Parametreler:** `[block_bytes: hex string]`

```powershell
$blockHex = "0011aabb..."
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_submitBlock","params":["' + $blockHex + '"]}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

**Yanıt:** kabul edilen bloğun kanonik hash'i (hex). Not: hash, asıl
pipeline'daki zincir-bağlantı kontrolünden **önce** döner — blok daha sonra
reddedilirse hash log araması için kullanılabilir.

---

### qv_getPostApplyCommitment

Aday blok için uygulama-sonrası UTXO commitment'ını hesaplar. Her giriş
hex-encoded bincode `Transaction`'dır; node bunları canlı UTXO setinin bir
snapshot'ına spekülatif uygular ve commitment kökünü döndürür. Kalıcı set
**değişmez**. Harici blok üreticileri (`qv-miner`) header'ı imzalamadan önce
doğru değeri basmak için kullanır (envanter K-05).

**Parametreler:** `[tx_bytes_hex: string[]]`

```powershell
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_getPostApplyCommitment","params":[["<tx1_hex>","<tx2_hex>"]]}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

**Yanıt:** 32-byte commitment kökü (hex string). Tek bir bozuk girişte tüm
istek reddedilir.

---

## Konsensüs Sorguları

### qv_getStakeDistribution

Aktif (epoch başında dondurulmuş) stake dağılımı snapshot'ını döndürür. VRF
lider seçiminde kullanılır; `qv-miner` başlangıçta ve her epoch sınırında
sorgular.

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getStakeDistribution","params":[]}'
```

**Yanıt (`StakeDistributionSnapshot`):**

```json
{
  "epoch": 7,
  "total_stake": 1000000,
  "pools": [ { "pool_id": "aa…", "stake": 250000 } ]
}
```

`pool_id` = operatörün VRF public key'inin SHA3-256'sı; `pools` deterministik
olarak `pool_id`'ye göre sıralıdır.

---

### qv_getEpochNonce

Güncel epoch nonce'unu döndürür — VRF lider seçimini parametrize eden 32-byte
seed. Her epoch sınırında `η_e = SHA3-256(η_{e-1} || extra_entropy ||
boundary_block_hash)` olarak evrilir.

**Parametreler:** yok.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getEpochNonce","params":[]}'
```

**Yanıt (`EpochNonceInfo`):**

```json
{ "nonce_hex": "ff…(64 hex karakter)", "epoch": 12 }
```

`epoch`, tip slot'undan türetilen *güncel* epoch numarasıdır.

---

## Notlar

- **WebSocket abonelikleri** (`qv_subscribeNewBlocks`, `qv_subscribeNewTx`)
  henüz YOK — kodda yorum satırı olarak ertelenmiş durumda; node'a gerçek bir
  event-source bağlandığında eklenecek.
- GossipSub sayesinde bir node'a gönderilen TX/blok otomatik olarak diğer
  peer'lara yayılır; herhangi bir node'a sorgu atabilirsiniz.
- Devnet'i ayağa kaldırmak için: `docs/DEVNET.md` ve `devnet/SCRIPTS.md`.
