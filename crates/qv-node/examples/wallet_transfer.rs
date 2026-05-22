//! ============================================================================
//! wallet_transfer.rs — Aciklamali, TEKRAR calistirabilir cuzdan transferi
//! ============================================================================
//!
//! `transfer_demo`'dan farki: devnet'i YENIDEN BASLATMADAN defalarca
//! calistirilabilir. Her calismada Cuzdan A, Cuzdan B'ye yeni bir transfer yapar.
//!
//! Nasil tekrar calisabiliyor?
//!   UTXO modelinde "bakiye" = harcanmamis ciktilar (UTXO) kumesidir. Ilk
//!   calismada A genesis UTXO'sunu harcar; geri kalani ("ust para"/change)
//!   yeni bir UTXO olarak A'ya doner. Bu script o "ust" UTXO'yu
//!   `transfer-state.json`'a yazar; sonraki calismada onu harcar.
//!
//! Calistirma:  cargo run -p qv-node --example wallet_transfer
//!
//! Bu dosyayi OKUYARAK sunlari gorebilirsin:
//!   - bir "cuzdan"in aslinda ne oldugu          (ADIM 1)
//!   - cuzdanin node'a nasil "baglandigi"        (ADIM 2)
//!   - bir islemin nasil imzalandigi/dogrulandigi (ADIM 5)
//! ============================================================================

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::FromStr;
use std::time::Duration;

use qv_core::{
    Amount, OutPoint, Script, Transaction, TxId, TxInput, TxOutput, ValidityInterval, Witness,
};
use qv_crypto::{from_seed_pqc, sha3_256, sign_pqc, DilithiumLevel, PqcPublicKey, PqcSecretKey};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_script::ScriptBuilder;

/// Devnet node'larinin JSON-RPC adresleri.
const DEFAULT_RPCS: &[&str] = &[
    "http://127.0.0.1:8545",
    "http://127.0.0.1:8546",
    "http://127.0.0.1:8547",
    "http://127.0.0.1:8548",
];
/// Onceki transferin "ust" UTXO'sunu hatirladigimiz dosya.
const STATE_FILE: &str = "transfer-state.json";
/// Her transferde B'ye gonderilen miktar (en kucuk birim).
const SEND_AMOUNT: u64 = 50_000_000;
/// Islem ucreti — blogu ureten node'a gider.
const FEE: u64 = 1_000;

fn main() -> anyhow::Result<()> {
    println!("\n{}", "=".repeat(70));
    println!("  QuantumVault — Aciklamali Cuzdan Transferi (tekrar calistirilabilir)");
    println!("{}", "=".repeat(70));

    // ========================================================================
    // ADIM 1 — CUZDANLARI TURET
    // ------------------------------------------------------------------------
    // Bir "cuzdan" aslinda bir ANAHTAR CIFTIDIR:
    //   - gizli anahtar (secret key) : harcama yetkisi. Imza uretir. GIZLI.
    //   - acik anahtar  (public key) : kimligin. Ondan "adres" turetilir.
    // QuantumVault kuantum-guvenli imza kullanir: Dilithium / ML-DSA (Level 3).
    //
    // "Adres" = acik anahtarin SHA3-256 ozeti (pubkey hash). Bir UTXO bir
    // adrese kilitlendiginde, sadece o acik anahtara ait gizli anahtarin
    // urettigi imza onu acabilir.
    //
    // Burada A ve B, devnet genesis hesaplari 0 ve 1. (Gercek bir cuzdanda
    // anahtar bir mnemonic'ten turetilir; burada ornek tekrarlanabilir olsun
    // diye deterministik devnet tohumundan turetiyoruz.)
    // ========================================================================
    let (a_pk, a_sk) = devnet_account(0);
    let (b_pk, _b_sk) = devnet_account(1);
    let a_hash = pubkey_hash(a_pk.as_bytes()); // Cuzdan A'nin "adresi"
    let b_hash = pubkey_hash(b_pk.as_bytes()); // Cuzdan B'nin "adresi"

    println!("\n[ADIM 1] Cuzdanlar turetildi (Dilithium/ML-DSA L3 anahtar cifti)");
    println!("  Cuzdan A  adres(pubkey-hash): {}", hex::encode(a_hash));
    println!("  Cuzdan A gizli anahtar     : {} ", hex::encode(a_sk.expose_secret()));
    println!("  Cuzdan B  adres(pubkey-hash): {}", hex::encode(b_hash));
    println!("  Cuzdan B gizli anahtar     : {} ", hex::encode(_b_sk.expose_secret()));

    // ========================================================================
    // ADIM 2 — NODE'A "BAGLAN"
    // ------------------------------------------------------------------------
    // Cuzdan bir node CALISTIRMAZ, p2p agina KATILMAZ. Cuzdanin node ile tek
    // temasi, node'un JSON-RPC sunucusuna yaptigi siradan bir HTTP istegidir.
    // "Baglanmak" = node'un RPC portuna (8545 vb.) TCP acip JSON-RPC cagrisi
    // yapmaktir. Asagidaki `http_post` fonksiyonu bu istegin ta kendisidir —
    // harici bir kutuphane bile kullanmaz, std::net::TcpStream ile yazilmistir.
    // ========================================================================
    let rpc_urls: Vec<String> = DEFAULT_RPCS.iter().map(|s| s.to_string()).collect();
    let live: Vec<String> = rpc_urls
        .iter()
        .filter(|u| rpc(u, "qv_getTip", serde_json::json!([])).is_some())
        .cloned()
        .collect();
    if live.is_empty() {
        anyhow::bail!("Hicbir devnet node'una ulasilamadi — once devnet'i baslat");
    }
    let node = live[0].clone();
    println!(
        "\n[ADIM 2] Node'a baglanildi — {} / {} node yanit veriyor",
        live.len(),
        rpc_urls.len()
    );
    println!("  Baglanti turu: JSON-RPC over HTTP (cuzdan = basit bir HTTP istemcisi)");

    // ========================================================================
    // ADIM 3 — CUZDAN A'NIN HARCANABILIR UTXO'SUNU BUL
    // ------------------------------------------------------------------------
    // UTXO modeli: "bakiyen" diye tek bir sayi yoktur; sahip oldugun
    // harcanmamis ciktilarin (UTXO) toplamidir. Transfer icin harcayacagin
    // SOMUT bir UTXO secersin.
    //   - Ilk calisma : A'nin genesis UTXO'su (genesis tx, cikti 0).
    //   - Sonrakiler  : onceki transferin A'ya donen "ust" UTXO'su
    //                   (transfer-state.json'a yazilmisti).
    // `qv_getUtxo` RPC'si bir UTXO'nun hala harcanmamis olup olmadigini ve
    // degerini soyler; harcanmissa `null` doner.
    // ========================================================================
    let (genesis_block, _keys) = qv_node::genesis::devnet_genesis();
    let genesis_txid = genesis_block.transactions[0].id().expect("genesis tx id");

    let (input_op, input_value) = resolve_spendable(&node, genesis_txid)?;
    println!("\n[ADIM 3] Cuzdan A'nin harcanabilir UTXO'su bulundu");
    println!("  UTXO   : {}", input_op);
    println!("  degeri : {} birim", input_value);

    if input_value < SEND_AMOUNT + FEE {
        anyhow::bail!(
            "Cuzdan A'da yeterli bakiye yok ({} < {}). Devnet'i yeniden baslat.",
            input_value,
            SEND_AMOUNT + FEE
        );
    }

    // ========================================================================
    // ADIM 4 — ISLEMI (TRANSACTION) KUR
    // ------------------------------------------------------------------------
    // Bir islem = GIRDILER (harcanan UTXO'lar) + CIKTILAR (yeni UTXO'lar).
    // Kural: girdi toplami = cikti toplami + ucret. Burada:
    //   girdi   : A'nin yukarida bulunan UTXO'su (input_value birim)
    //   cikti 0 : B'ye SEND_AMOUNT birim  (B'nin adresine kilitli)
    //   cikti 1 : A'ya geri kalan "ust"   (A'nin adresine kilitli)
    //   ucret   : FEE birim
    // Her cikti bir "kilitleme script'i" (p2pkh_pqc) ile bir adrese kilitlenir.
    // ========================================================================
    let change = input_value - SEND_AMOUNT - FEE;
    let out_to_b = TxOutput::new(Amount::from(SEND_AMOUNT), Script::new(p2pkh_pqc(&b_hash)));
    let out_change = TxOutput::new(Amount::from(change), Script::new(p2pkh_pqc(&a_hash)));
    let mut tx = Transaction::new(vec![TxInput::new(input_op)], vec![out_to_b, out_change]);
    tx.fee = Amount::from(FEE);
    tx.validity_interval = ValidityInterval::UNBOUNDED;

    println!("\n[ADIM 4] Islem kuruldu");
    println!("  girdi   : {}  ({} birim)", input_op, input_value);
    println!("  cikti 0 : {:>11} birim  -> Cuzdan B", SEND_AMOUNT);
    println!("  cikti 1 : {:>11} birim  -> Cuzdan A (ust)", change);
    println!("  ucret   : {:>11} birim  -> blogu ureten node", FEE);

    // ========================================================================
    // ADIM 5 — ISLEMI IMZALA   (en onemli kisim)
    // ------------------------------------------------------------------------
    // A'nin bu UTXO'yu harcamaya YETKILI oldugunu kanitlamamiz gerekir.
    // Kanit = A'nin gizli anahtariyla atilmis bir imza.
    //
    //  (a) IMZALANACAK MESAJ:
    //      `tx.canonical_bytes()` — islemin tanik(witness)-icermeyen kanonik
    //      bayt dizilimi. Imza islemin ICERIGINE baglanir; tek bayt degisse
    //      imza gecersiz olur. (Imzayi imzanin icine koyamayiz; o yuzden
    //      mesaj witness'siz hesaplanir.)
    //
    //  (b) IMZA:
    //      `sign_pqc(&a_sk, &mesaj)` — A'nin GIZLI anahtariyla Dilithium /
    //      ML-DSA (kuantum-guvenli) imzasi uretir (~3.3 KB).
    //
    //  (c) TANIK (witness):
    //      Mesaji, imzayi ve acik anahtari girdiye ilistiririz. Bunu kucuk bir
    //      script olarak paketleriz: [mesaj, imza, acik_anahtar] yigina birakilir.
    //
    //  (d) NODE NASIL DOGRULAR:
    //      Harcadigimiz UTXO `p2pkh_pqc(A'nin adresi)` ile kilitliydi. Bir node
    //      blogu islerken script VM'ini calistirir:
    //        1) witness'taki acik anahtari alir, SHA3-256'sini hesaplar, kilitteki
    //           adresle (pubkey-hash) eslesiyor mu bakar,
    //        2) CHECKSIG_PQC: imzayi, mesaji ve acik anahtari dogrular.
    //      Ikisi de gecerse UTXO harcanabilir; degilse islem reddedilir. Yani
    //      "sahiplik" = gecerli imza uretebilen gizli anahtara sahip olmaktir.
    // ========================================================================
    let message = tx.canonical_bytes().expect("kanonik bayt dizilimi");
    let signature = sign_pqc(&a_sk, &message)?;
    let witness_script = ScriptBuilder::new()
        .push_bytes(&message) // (a) imzalanan mesaj
        .push_bytes(signature.as_bytes()) // (b) imzanin kendisi
        .push_bytes(a_pk.as_bytes()) // (c) A'nin acik anahtari
        .build();
    tx.inputs[0].witness = Witness::new(witness_script);

    let txid = tx.id().expect("imzali islem id'si");
    println!("\n[ADIM 5] Islem imzalandi");
    println!("  algoritma : Dilithium / ML-DSA Level 3 (post-quantum)");
    println!("  mesaj     : {} bayt (witness'siz kanonik islem)", message.len());
    println!("  imza      : {} bayt", signature.as_bytes().len());
    println!("  islem id  : {}", txid.to_hex());

    // ========================================================================
    // ADIM 6 — ISLEMI GONDER
    // ------------------------------------------------------------------------
    // Imzali islemi bincode ile serilestirip hex'e ceviririz ve
    // `qv_sendTransaction` RPC'siyle her node'a gondeririz (her birinin
    // mempool'una dussun ki siradaki lider hangisiyse hemen bloga alabilsin).
    // ========================================================================
    let tx_hex = hex::encode(bincode::serialize(&tx)?);
    println!("\n[ADIM 6] Islem gonderiliyor (her node'un mempool'una)");
    let mut accepted = 0;
    for url in &live {
        match rpc_checked(url, "qv_sendTransaction", serde_json::json!([tx_hex])) {
            Ok(_) => {
                accepted += 1;
                println!("  {url}  -> mempool'a kabul edildi");
            }
            Err(e) => println!("  {url}  -> reddedildi: {e}"),
        }
    }
    if accepted == 0 {
        anyhow::bail!("Islem hicbir node tarafindan kabul edilmedi");
    }

    // ========================================================================
    // ADIM 7 — ONAY BEKLE
    // ------------------------------------------------------------------------
    // Islem bir bloga girince harcadigimiz girdi UTXO'su artik "harcanmis"
    // olur — `qv_getUtxo` onun icin `null` doner. Bunu bekliyoruz.
    // ========================================================================
    println!("\n[ADIM 7] Islemin bloga alinmasi bekleniyor...");
    let mut mined = false;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(1000));
        if utxo_value(&node, &input_op).is_none() {
            mined = true;
            break;
        }
    }
    if !mined {
        anyhow::bail!("Islem 60 saniyede onaylanmadi");
    }
    let tip = rpc(&node, "qv_getTip", serde_json::json!([]))
        .and_then(|t| t.get("height").and_then(|h| h.as_u64()))
        .unwrap_or(0);
    println!("  Islem bloga alindi. Zincir ucu artik height {}.", tip);

    // ========================================================================
    // ADIM 8 — DURUMU KAYDET (sonraki calisma icin)
    // ------------------------------------------------------------------------
    // A'nin bu transferden donen "ust" UTXO'su = yeni islemin 1 numarali
    // ciktisi (txid:1). Onu state dosyasina yaziyoruz ki bu script tekrar
    // calistiginda devnet'i yeniden baslatmadan oradan devam etsin.
    // ========================================================================
    let next_op = OutPoint::new(txid, 1);
    write_state(&next_op.to_string())?;

    let b_recv = utxo_value(&node, &OutPoint::new(txid, 0));
    let a_left = utxo_value(&node, &next_op);
    println!("\n[ADIM 8] Sonuc");
    println!("  Cuzdan B aldi : {} birim  (yeni UTXO {}:0)", show(b_recv), txid.to_hex());
    println!("  Cuzdan A ust  : {} birim  (yeni UTXO {}:1)", show(a_left), txid.to_hex());
    println!("  Durum kaydedildi -> {}", STATE_FILE);
    println!("  Bu script'i TEKRAR calistirabilirsin; A'nin ustunden devam eder.");

    println!("\n{}", "=".repeat(70));
    println!("  Transfer tamamlandi.");
    println!("{}\n", "=".repeat(70));
    Ok(())
}

// ============================================================================
// Yardimci fonksiyonlar
// ============================================================================

/// Devnet genesis hesabi `i`'nin anahtar ciftini deterministik turetir
/// (genesis ile birebir ayni: seed = SHA3-256("qv-devnet-account-" || i)).
fn devnet_account(i: u8) -> (PqcPublicKey, PqcSecretKey) {
    let mut preimage = b"qv-devnet-account-".to_vec();
    preimage.push(i);
    let seed = sha3_256(&preimage);
    let kp = from_seed_pqc(DilithiumLevel::Level3, &seed).expect("anahtar turetme");
    (kp.public, kp.secret)
}

/// Cuzdan A'nin harcayabilecegi UTXO'yu cozer: once onceki transferin "ust"
/// UTXO'su (state dosyasi), o yoksa genesis UTXO'su.
fn resolve_spendable(node: &str, genesis_txid: TxId) -> anyhow::Result<(OutPoint, u64)> {
    if let Some(saved) = read_state() {
        if let Ok(op) = OutPoint::from_str(&saved) {
            if let Some(v) = utxo_value(node, &op) {
                println!("  (kaynak: onceki transferin ust UTXO'su)");
                return Ok((op, v));
            }
        }
        println!("  (kayitli UTXO yok — devnet sifirlanmis olabilir, genesis'e donuluyor)");
    }
    let gen = OutPoint::new(genesis_txid, 0);
    if let Some(v) = utxo_value(node, &gen) {
        println!("  (kaynak: Cuzdan A'nin genesis UTXO'su — ilk transfer)");
        return Ok((gen, v));
    }
    anyhow::bail!("Cuzdan A'nin harcanabilir UTXO'su yok — devnet'i yeniden baslat")
}

/// Bir UTXO'nun degerini dondurur; harcanmissa `None`.
fn utxo_value(node: &str, op: &OutPoint) -> Option<u64> {
    let res = rpc(node, "qv_getUtxo", serde_json::json!([op.to_string()]))?;
    if res.is_null() {
        return None;
    }
    res.get("value").and_then(|v| v.as_u64())
}

/// State dosyasindan kayitli A-UTXO outpoint string'ini okur.
fn read_state() -> Option<String> {
    let txt = std::fs::read_to_string(STATE_FILE).ok()?;
    let j: serde_json::Value = serde_json::from_str(&txt).ok()?;
    j.get("a_outpoint")?.as_str().map(|s| s.to_string())
}

/// A'nin yeni "ust" UTXO outpoint'ini state dosyasina yazar.
fn write_state(outpoint: &str) -> anyhow::Result<()> {
    let j = serde_json::json!({ "a_outpoint": outpoint });
    std::fs::write(STATE_FILE, serde_json::to_string_pretty(&j)?)?;
    Ok(())
}

fn show(v: Option<u64>) -> String {
    v.map(|x| x.to_string()).unwrap_or_else(|| "0".to_string())
}

/// JSON-RPC cagrisi; hata olursa `None`.
fn rpc(url: &str, method: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    rpc_checked(url, method, params).ok()
}

/// JSON-RPC cagrisi; RPC hatasini metin olarak dondurur.
fn rpc_checked(
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let raw = http_post(url, &req.to_string()).map_err(|e| e.to_string())?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("bozuk yanit: {e}"))?;
    if let Some(err) = json.get("error") {
        return Err(err.to_string());
    }
    Ok(json.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Cuzdanin node'a "baglanmasi" iste budur: duz bir HTTP POST. Harici bir HTTP
/// kutuphanesi bile yok — std::net::TcpStream ile elle yazilmistir.
fn http_post(url: &str, body: &str) -> Result<String, Box<dyn std::error::Error>> {
    let url = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");
    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    match response.find("\r\n\r\n") {
        Some(idx) => Ok(response[idx + 4..].to_string()),
        None => Ok(response),
    }
}
