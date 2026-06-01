# ADR-012: İşlem Sighash'i — İmzayı İşleme Bağlama

**Durum:** Kabul edildi — uygulandı (`qv-core`, `qv-script`, `qv-wallet`;
örnekler ve `transfer_e2e` testi güncellendi)
**Tarih:** 2026-05-22
**Yazarlar:** QuantumVault Team
**Yer (planlanan):** `crates/qv-core` (`Transaction::sighash`), `crates/qv-script` (`SigHash` opcode, `p2pkh_pqc` / `stealth_p2pkh` template'leri), `crates/qv-wallet` (`tx_builder` imzalama)

---

## Bağlam

ADR-011 Faz 3 hazırlığında bulundu. `qv-node::validation::validate_script`
her işlem girdisi için `witness ++ locking_script`'i, **gerçek işlemi içeren
bir `Context` ile** çalıştırır. Yani `Context` (ve `TxHash` opcode'u) script'e
erişilebilir.

Ancak `p2pkh_pqc` kilit template'i imza mesajını `Context`'ten **değil**,
witness'tan alır: cüzdan imzasız işlemin `canonical_bytes`'ını imzalar ve bu
baytları witness'a `msg` olarak koyar; script `CHECKSIG_PQC(pubkey, sig, msg)`'i
witness'tan gelen `msg` ile çalıştırır. **Hiçbir yer — ne script, ne
`validate_script`, ne `validation.rs` — bu `msg`'in gerçek işleme karşılık
geldiğini doğrulamaz.**

### Güvenlik açığı (KRİTİK)

`p2pkh_pqc` imzayı işleme bağlamadığı için, mempool'daki bir harcamanın
witness'ı (`<msg, sig, pubkey>`) **yeniden oynatılabilir**:

1. Alice, U UTXO'sunu (`p2pkh_pqc(pkh_alice)` ile kilitli) Bob'a gönderen T1
   işlemini yayınlar. T1 mempool'da.
2. Saldırgan Eve, T1'in girdi witness'ı `W = <msg, sig, pubkey>`'i çıkarır.
3. Eve, **aynı U'yu kendine** yönlendiren T2 işlemini kurar ve girdi
   witness'ını `W` yapar.
4. Doğrulayıcı T2'yi denetler: `pubkey_hash(pubkey) == pkh_alice` ✓,
   `CHECKSIG_PQC(pubkey, sig, msg)` ✓ (imza `msg` üzerinde gerçekten geçerli).
   → T2 geçerli → Eve U'yu çalar (hangi işlem önce onaylanır yarışı).

### Kök neden

Zincirde witness'ları **dışlayan** bir hash yok. `canonical_bytes()`,
`tx.id()` ve `TxHash` opcode'u (`SHA3-256(canonical_bytes)`) — hepsi
`TxInput.witness` alanını içerir. Bu yüzden `TxHash`'i sighash olarak
kullanmak döngüseldir (imza → witness → hash → imza). Bu döngüsellik aynı
zamanda ADR-011'in `stealth_p2pkh` template'inin de neden gerçek doğrulamada
çalışmadığının sebebidir.

## Karar

İmzanın imzaladığı şeyi tanımlayan, witness'ları dışlayan bir **sighash**
ekleriz.

### 1. `Transaction::sighash()`

`qv-core`'a, witness'lardan bağımsız kanonik bir hash:

```text
sighash = SHA3-256( canonical_bytes( tx, tüm input.witness alanları boş ) )
```

İşlemi klonla, her `input.witness`'i `Witness::default()` yap, `canonical_bytes`
al, `SHA3-256`'la. Deterministik ve imzadan bağımsız. İşlem-geneli tek değer —
tüm girdiler aynı `sighash`'i imzalar.

`tx.id()` (witness-dahil) **değişmez**; o ayrı bir konudur (txid
malleability — bu ADR'in dışı). `sighash` yalnızca imzalama içindir.

### 2. `SigHash` opcode

`qv-script`'e yeni introspection opcode'u `SigHash` (`0x69`); `ctx.sighash`'i
yığına basar. `Context`'e `sighash` alanı eklenir, `Context::new` içinde
`tx.sighash()` ile hesaplanır.

### 3. Template'ler `SigHash` kullanır

- **`p2pkh_pqc`**: witness artık `<signature> <pubkey>` (mesaj kaldırıldı).
  Script `pubkey_hash` doğrulamasından sonra mesajı `SigHash` ile alır,
  `CHECKSIG_PQC` çalıştırır. İmza artık işleme bağlı → yeniden-oynatma kapanır.
- **`stealth_p2pkh`** (ADR-011): `TxHash` yerine `SigHash`. `SigHash`
  witness-dışlayan olduğundan döngüsellik biter; template harcanabilir hâle
  gelir. Witness `<signature> <spend_pk> <shared_secret>`.

### 4. Cüzdan imzalama

`qv-wallet::tx_builder` (`sign_with`, `sign_inputs`) `tx.canonical_bytes()`
yerine `tx.sighash()`'i imzalar; witness'a artık `msg` koymaz — yalnızca
`<sig> <pubkey>` (stealth için `<sig> <spend_pk> <shared_secret>`).

### Konsensüs etkisi

Bu, imzalanan baytları ve kilit template'lerini değiştirir — bir konsensüs
değişikliğidir. Mainnet ya da kalıcı bir zincir başlamadan yapılmalı.
`config/genesis.toml` henüz yazılmadığından (Aşama 4) şu an güvenli.

## Sonuçlar

### Olumlu

- İmza işleme bağlanır; uçuştaki işlem hırsızlığı (KRİTİK açık) kapanır.
- Tek düzeltme hem `p2pkh_pqc`'yi onarır hem `stealth_p2pkh`'yi mümkün kılar.
- Witness artık `msg` taşımadığı için işlemler biraz küçülür.

### Olumsuz

- Konsensüs serileştirme + template değişikliği; `p2pkh_pqc` harcayan tüm
  testler/örnekler (`transfer_e2e`, `wallet_transfer`, `send_tx` …) güncellenmeli.
- Witness formatı değişir — eski format işlemler geçersiz olur (pre-mainnet,
  sorun değil).

### Nötr / İleride

- `tx.id()` hâlâ witness-dahil → txid malleability ayrı bir konudur; bir
  takip işi olarak değerlendirilmeli.
- BIP143-tarzı per-input / SIGHASH-flag'li sighash şu an gereksiz; tek
  işlem-geneli sighash yeterli.

## Alternatifler (kısa)

| Alternatif | Neden seçilmedi |
|---|---|
| Witness-msg modelini koru (status quo) | İmza işleme bağlanmaz — fon hırsızlığı açık kalır |
| `canonical_bytes`'ı witness'sız yap | `tx.id()` ve depolama anahtarlarını etkiler; daha geniş kırılma |
| BIP143-tarzı per-input sighash | Daha karmaşık; mevcut "tüm işlemi imzala" modeli için gereksiz |

## Doğrulama / Test (planlanan)

- `Transaction::sighash` birim testi: witness değişse de sighash sabit.
- `SigHash` opcode interpreter testi.
- `p2pkh_pqc` / `stealth_p2pkh` uçtan uca: doğru witness ile harcanır.
- **Yeniden-oynatma regresyon testi**: bir işlemin witness'ı, çıktıları
  farklı ikinci bir işleme kopyalanınca `validate_script` reddetmeli.

## Bağlantılı

- ADR-011 — stealth adres entegrasyonu (`stealth_p2pkh` bu sighash'e bağlı).
- `docs/security/qv-consensus-fork-finality-audit.md` — denetim; bu açık
  oraya da işlendi.
- `crates/qv-node/src/validation.rs` — `validate_script` (açığın görüldüğü yer).
