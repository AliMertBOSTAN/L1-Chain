# QuantumVault JSON-RPC API Reference

Node A: `http://127.0.0.1:8545` | Node B: `http://127.0.0.1:8546`

---

## qv_getTip

Zincirin en son blok bilgisini (tip) döndürür: hash, yükseklik ve zaman damgası.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
```

---

## qv_getMempoolStatus

Mempool durumunu gösterir: clear pool boyutu, encrypted pool boyutu, minimum fee rate ve toplam değer.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getMempoolStatus","params":[]}'
```

---

## qv_getBlockByHeight

Belirtilen yükseklikteki bloğu döndürür. Genesis blok için `0` kullanılır.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getBlockByHeight","params":[0]}'
```

---

## qv_getBlockByHash

Hex formatında blok hash'i ile bloğu döndürür.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getBlockByHash","params":["abc123..."]}'
```

Blok hash'ini önce `qv_getTip` ile öğrenebilirsiniz.

---

## qv_getTx

Hex formatında TX ID ile işlemi arar. Önce mempool'a, sonra son 50 bloğa bakar.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getTx","params":["tx_id_hex_buraya"]}'
```

---

## qv_getUtxo

`txid:index` formatında bir outpoint ile UTXO bilgisini döndürür: değer, script hash, datum ve stealth bilgisi.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getUtxo","params":["tx_id_hex:0"]}'
```

---

## qv_sendTransaction

Hex-encoded bincode formatında imzalanmış bir işlemi mempool'a gönderir. Başarılı olursa TX ID döner.

```powershell
$txHex = "deadbeef..."
$body = '{"jsonrpc":"2.0","id":1,"method":"qv_sendTransaction","params":["' + $txHex + '"]}'
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body $body
```

Programatik olarak göndermek için: `cargo run -p qv-node --example send_tx`

---

## qv_getBalanceFor (stub)

Stealth view key ile bakiye sorgular. Henüz implement edilmedi, hata döner.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_getBalanceFor","params":["view_key_hex"]}'
```

---

## qv_scanStealth (stub)

Belirtilen yükseklik aralığında stealth output'ları tarar. Henüz implement edilmedi, hata döner.

```powershell
Invoke-RestMethod http://127.0.0.1:8545 -Method POST -ContentType "application/json" -Body '{"jsonrpc":"2.0","id":1,"method":"qv_scanStealth","params":["view_key_hex", 0, 100]}'
```

---

## Notlar

- Tüm hash ve ID parametreleri hex string formatındadır.
- İki node çalışırken Node A porta `8545`, Node B porta `8546` dinler.
- `send_tx` örneği her çalışmada farklı key üretir; aynı TX iki kez gönderilirse `DuplicateTx` hatası alınır.
- GossipSub ile bir node'a gönderilen TX otomatik olarak diğer peer'lara yayılır.
