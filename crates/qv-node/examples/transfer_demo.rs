//! Devnet transfer demo — "Wallet A" sends QV to "Wallet B", end to end.
//!
//! Wallet A and Wallet B are the first two deterministic devnet genesis
//! accounts (account 0 and account 1). Each owns a 1_000_000_000-unit
//! genesis UTXO. This program builds + Dilithium-signs a transaction that
//! spends A's genesis UTXO (output 0 -> B, output 1 -> A change), submits
//! it to every reachable node, and waits until it is mined into a block.
//!
//!   cargo run -p qv-node --example transfer_demo
//!   cargo run -p qv-node --example transfer_demo -- http://127.0.0.1:8545

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use qv_core::{Amount, OutPoint, Script, Transaction, TxInput, TxOutput, ValidityInterval, Witness};
use qv_crypto::{from_seed_pqc, sha3_256, sign_pqc, DilithiumLevel, PqcPublicKey, PqcSecretKey};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_script::ScriptBuilder;

const DEFAULT_RPCS: &[&str] = &[
    "http://127.0.0.1:8545",
    "http://127.0.0.1:8546",
    "http://127.0.0.1:8547",
    "http://127.0.0.1:8548",
];

const GENESIS_VALUE: u64 = 1_000_000_000;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rpc_urls: Vec<String> = if args.is_empty() {
        DEFAULT_RPCS.iter().map(|s| s.to_string()).collect()
    } else {
        args
    };

    banner("QuantumVault Devnet — Wallet A -> Wallet B Transfer");

    let (a_pk, a_sk) = devnet_account(0);
    let (b_pk, _b_sk) = devnet_account(1);
    let a_hash = pubkey_hash(a_pk.as_bytes());
    let b_hash = pubkey_hash(b_pk.as_bytes());

    let (genesis_block, _keys) = qv_node::genesis::devnet_genesis();
    let genesis_txid = genesis_block.transactions[0]
        .id()
        .expect("genesis tx id is infallible");

    let a_outpoint = OutPoint::new(genesis_txid, 0);
    let b_outpoint = OutPoint::new(genesis_txid, 1);

    println!("  Wallet A  (genesis account 0)");
    println!("    pubkey hash : {}", hex::encode(a_hash));
    println!("    genesis UTXO: {}", a_outpoint);
    println!("  Wallet B  (genesis account 1)");
    println!("    pubkey hash : {}", hex::encode(b_hash));
    println!("    genesis UTXO: {}", b_outpoint);
    println!();

    let wallets = serde_json::json!({
        "wallet_a": { "account": 0, "pubkey_hash": hex::encode(a_hash),
                      "genesis_outpoint": a_outpoint.to_string() },
        "wallet_b": { "account": 1, "pubkey_hash": hex::encode(b_hash),
                      "genesis_outpoint": b_outpoint.to_string() },
        "genesis_txid": genesis_txid.to_hex(),
    });
    let _ = std::fs::write("wallets.json", serde_json::to_string_pretty(&wallets)?);

    let live: Vec<String> = rpc_urls
        .iter()
        .filter(|u| rpc(u, "qv_getTip", serde_json::json!([])).is_some())
        .cloned()
        .collect();
    if live.is_empty() {
        anyhow::bail!("no devnet node reachable on {:?}", rpc_urls);
    }
    println!("  Reachable nodes: {} / {}", live.len(), rpc_urls.len());

    section("BEFORE — wallet balances");
    let a_before = utxo_value(&live[0], &a_outpoint);
    let b_before = utxo_value(&live[0], &b_outpoint);
    println!("  Wallet A: {} units  (UTXO {})", show(a_before), a_outpoint);
    println!("  Wallet B: {} units  (UTXO {})", show(b_before), b_outpoint);
    if a_before.is_none() {
        anyhow::bail!("Wallet A's genesis UTXO is already spent");
    }

    section("BUILD — transaction A -> B");
    let send_amount: u64 = 250_000_000;
    let fee: u64 = 1_000;
    let change: u64 = GENESIS_VALUE - send_amount - fee;

    let out_to_b = TxOutput::new(Amount::from(send_amount), Script::new(p2pkh_pqc(&b_hash)));
    let out_change = TxOutput::new(Amount::from(change), Script::new(p2pkh_pqc(&a_hash)));

    let mut tx = Transaction::new(vec![TxInput::new(a_outpoint)], vec![out_to_b, out_change]);
    tx.fee = Amount::from(fee);
    tx.validity_interval = ValidityInterval::UNBOUNDED;

    let payload = tx.canonical_bytes().expect("canonical encoding");
    let signature = sign_pqc(&a_sk, &payload)?;
    let witness = ScriptBuilder::new()
        .push_bytes(&payload)
        .push_bytes(signature.as_bytes())
        .push_bytes(a_pk.as_bytes())
        .build();
    tx.inputs[0].witness = Witness::new(witness);

    let txid = tx.id().expect("signed tx id");
    let tx_hex = hex::encode(bincode::serialize(&tx)?);

    println!("    input    : {} ({} units)", a_outpoint, GENESIS_VALUE);
    println!("    output 0 : {:>12} units  -> Wallet B", send_amount);
    println!("    output 1 : {:>12} units  -> Wallet A (change)", change);
    println!("    fee      : {:>12} units  -> block producer", fee);
    println!("    signature: Dilithium / ML-DSA Level 3 ({} bytes)", signature.as_bytes().len());
    println!("    tx id    : {}", txid.to_hex());

    section("SUBMIT — broadcasting to every node's mempool");
    let mut accepted = 0;
    for url in &live {
        match rpc_checked(url, "qv_sendTransaction", serde_json::json!([tx_hex])) {
            Ok(_) => {
                accepted += 1;
                println!("    {url}  -> accepted into mempool");
            }
            Err(e) => println!("    {url}  -> rejected: {e}"),
        }
    }
    if accepted == 0 {
        anyhow::bail!("transaction rejected by every node");
    }

    section("CONFIRM — waiting for the transaction to be mined");
    let mut confirmed_height: Option<u64> = None;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(1000));
        if utxo_value(&live[0], &a_outpoint).is_none() {
            confirmed_height = tip_height(&live[0]);
            break;
        }
        if let Some(h) = tip_height(&live[0]) {
            print!("\r  tip height: {h}   mempool: {} tx ", mempool_size(&live[0]));
            let _ = std::io::stdout().flush();
        }
    }
    println!();
    match confirmed_height {
        Some(h) => println!("  Transaction mined — chain tip is now height {h}."),
        None => anyhow::bail!("transaction not confirmed within 60s"),
    }

    section("AFTER — wallet balances and convergence");
    let a_after = utxo_value(&live[0], &OutPoint::new(txid, 1));
    let b_received = utxo_value(&live[0], &OutPoint::new(txid, 0));
    println!("  Wallet A: change UTXO {}:1 = {} units", txid.to_hex(), show(a_after));
    println!("  Wallet B: received UTXO {}:0 = {} units", txid.to_hex(), show(b_received));
    println!();
    println!("  Per-node chain tip (all should match):");
    for url in &rpc_urls {
        match tip_height(url) {
            Some(h) => println!("    {url}  height {h}"),
            None => println!("    {url}  (unreachable)"),
        }
    }

    banner("Transfer complete");
    Ok(())
}

fn devnet_account(i: u8) -> (PqcPublicKey, PqcSecretKey) {
    let mut preimage = b"qv-devnet-account-".to_vec();
    preimage.push(i);
    let seed = sha3_256(&preimage);
    let kp = from_seed_pqc(DilithiumLevel::Level3, &seed)
        .expect("deterministic devnet keypair derivation");
    (kp.public, kp.secret)
}

fn utxo_value(url: &str, op: &OutPoint) -> Option<u64> {
    let res = rpc(url, "qv_getUtxo", serde_json::json!([op.to_string()]))?;
    if res.is_null() {
        return None;
    }
    res.get("value").and_then(|v| v.as_u64())
}

fn tip_height(url: &str) -> Option<u64> {
    rpc(url, "qv_getTip", serde_json::json!([]))?
        .get("height")
        .and_then(|v| v.as_u64())
}

fn mempool_size(url: &str) -> u64 {
    rpc(url, "qv_getMempoolStatus", serde_json::json!([]))
        .and_then(|v| v.get("clear_pool_size").and_then(|x| x.as_u64()))
        .unwrap_or(0)
}

fn rpc(url: &str, method: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    rpc_checked(url, method, params).ok()
}

fn rpc_checked(
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let raw = http_post(url, &req.to_string()).map_err(|e| e.to_string())?;
    let json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("bad response: {e}"))?;
    if let Some(err) = json.get("error") {
        return Err(err.to_string());
    }
    Ok(json.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

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

fn show(v: Option<u64>) -> String {
    match v {
        Some(x) => x.to_string(),
        None => "0 (none)".to_string(),
    }
}

fn banner(title: &str) {
    println!("\n{}", "=".repeat(64));
    println!("  {title}");
    println!("{}", "=".repeat(64));
}

fn section(title: &str) {
    println!("\n-- {title} {}", "-".repeat(58usize.saturating_sub(title.len())));
}
