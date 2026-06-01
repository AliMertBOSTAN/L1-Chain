//! Example: build a signed transaction and send it to a running devnet node.
//!
//! Demonstrates the full lifecycle with before/after state queries:
//!
//! 1. Query chain tip + mempool status (BEFORE)
//! 2. Query the genesis UTXO we're about to spend
//! 3. Build, sign, and submit a transfer transaction
//! 4. Query mempool status again (AFTER — should show +1 tx)
//! 5. Query the tip and look up the tx by ID
//!
//! # Prerequisites
//!
//! 1. Initialize the node:
//!    ```sh
//!    cargo run -p qv-node -- --init --network devnet
//!    ```
//!
//! 2. Start the node in another terminal:
//!    ```sh
//!    cargo run -p qv-node -- --network devnet --log-level debug
//!    ```
//!
//! 3. Run this example:
//!    ```sh
//!    cargo run -p qv-node --example send_tx
//!    ```

// Example/demo binary — panicking on parse/build failures is acceptable.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use qv_core::{Amount, OutPoint, Script, Transaction, TxInput, TxOutput, ValidityInterval};
use qv_crypto::{generate_pqc_keypair, DilithiumLevel, PqcSecretKey};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_script::ScriptBuilder;

/// Default RPC endpoint (override with first CLI arg, e.g. `send_tx http://127.0.0.1:8546`).
const DEFAULT_RPC_URL: &str = "http://127.0.0.1:8545";

/// Default genesis keys path (override with second CLI arg).
const DEFAULT_KEYS_PATH: &str = "data/genesis-keys.json";

// ============================================================================
// Helpers
// ============================================================================

thread_local! {
    static RPC_URL_CELL: std::cell::RefCell<String> = std::cell::RefCell::new(DEFAULT_RPC_URL.to_string());
}

fn current_rpc_url() -> String {
    RPC_URL_CELL.with(|c| c.borrow().clone())
}

/// Pretty-print a JSON-RPC response (or an error).
fn rpc_call(method: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    let rpc_url = current_rpc_url();
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    });

    match send_rpc_request(&rpc_url, &request.to_string()) {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(json) => {
                if let Some(err) = json.get("error") {
                    println!("  ERROR: {}", err);
                    None
                } else {
                    Some(json["result"].clone())
                }
            }
            Err(e) => {
                println!("  (parse error: {})", e);
                println!("  Raw: {}", &raw[..raw.len().min(300)]);
                None
            }
        },
        Err(e) => {
            println!("  (connection failed: {})", e);
            None
        }
    }
}

fn separator(title: &str) {
    println!("\n{}", "─".repeat(60));
    println!("  {}", title);
    println!("{}\n", "─".repeat(60));
}

// ============================================================================
// Main
// ============================================================================

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let rpc_url = args.get(1).map(|s| s.as_str()).unwrap_or(DEFAULT_RPC_URL);
    let keys_path_str = args.get(2).map(|s| s.as_str()).unwrap_or(DEFAULT_KEYS_PATH);

    // Store in a thread-local so rpc_call can access it.
    RPC_URL_CELL.with(|c| c.replace(rpc_url.to_string()));

    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║     QuantumVault Devnet — Transaction Demo              ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("  RPC: {}  Keys: {}\n", rpc_url, keys_path_str);

    // ── Pre-flight: check node connectivity ────────────────────────────────

    separator("1. PRE-FLIGHT: checking node connectivity");

    if rpc_call("qv_getTip", serde_json::json!([])).is_none() {
        println!("\nNode is not reachable at {}.", rpc_url);
        println!("Start the node first:\n");
        println!("  cargo run -p qv-node -- --init --network devnet");
        println!("  cargo run -p qv-node -- --network devnet --log-level debug\n");
        anyhow::bail!("node not reachable");
    }
    println!("  Node is alive!\n");

    // ── Query chain state BEFORE ───────────────────────────────────────────

    separator("2. CHAIN STATE (before tx)");

    // Tip
    print!("  Chain tip: ");
    if let Some(tip) = rpc_call("qv_getTip", serde_json::json!([])) {
        println!("{}", serde_json::to_string_pretty(&tip)?);
    }

    // Mempool
    print!("  Mempool: ");
    if let Some(status) = rpc_call("qv_getMempoolStatus", serde_json::json!([])) {
        println!("{}", serde_json::to_string_pretty(&status)?);
    }

    // Genesis block (height 0)
    print!("  Genesis block: ");
    if let Some(block) = rpc_call("qv_getBlockByHeight", serde_json::json!([0])) {
        if block.is_null() {
            println!("(not found — genesis may not be indexed by height yet)");
        } else {
            // Show tx count and first output value
            if let Some(txs) = block.get("transactions").and_then(|t| t.as_array()) {
                println!("{} transaction(s)", txs.len());
                if let Some(first_tx) = txs.first() {
                    if let Some(outputs) = first_tx.get("outputs").and_then(|o| o.as_array()) {
                        println!("  Genesis has {} outputs:", outputs.len());
                        for (i, out) in outputs.iter().enumerate().take(3) {
                            println!(
                                "    [{}] value: {}",
                                i,
                                out.get("value").unwrap_or(&serde_json::json!("?"))
                            );
                        }
                        if outputs.len() > 3 {
                            println!("    ... and {} more", outputs.len() - 3);
                        }
                    }
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&block)?);
            }
        }
    }

    // ── Load genesis keys ──────────────────────────────────────────────────

    separator("3. LOADING KEYS");

    let keys_path = PathBuf::from(keys_path_str);
    if !keys_path.exists() {
        anyhow::bail!(
            "Genesis keys not found at {}.\nRun: cargo run -p qv-node -- --init --network devnet",
            keys_path.display()
        );
    }

    let keys_json: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&keys_path)?)?;

    println!(
        "  Loaded {} genesis accounts from {}",
        keys_json.len(),
        keys_path.display()
    );

    let sender_hex = keys_json[0]["secret_key_hex"]
        .as_str()
        .expect("missing secret_key_hex for account 0");
    let sender_sk_bytes = hex::decode(sender_hex)?;
    let sender_sk = PqcSecretKey::from_bytes(DilithiumLevel::Level3, sender_sk_bytes)?;

    println!("  Sender:   account 0 (sk: {}...)", &sender_hex[..32]);

    // Fresh receiver keypair
    let receiver_kp = generate_pqc_keypair(DilithiumLevel::Level3)?;
    let receiver_pk = receiver_kp.public;
    let receiver_pk_hash = pubkey_hash(receiver_pk.as_bytes());
    println!(
        "  Receiver: fresh keypair (pk hash: {}...)",
        hex::encode(&receiver_pk_hash[..8])
    );

    // ── Rebuild genesis to get the tx ID ───────────────────────────────────

    separator("4. BUILDING TRANSACTION");

    let (genesis_block, _genesis_sks) = qv_node::genesis::devnet_genesis();
    let genesis_tx = &genesis_block.transactions[0];
    let genesis_tx_id = genesis_tx.id().expect("genesis tx must produce valid id");

    println!("  Genesis tx ID: {}", genesis_tx_id);
    println!(
        "  Spending output 0 (value: {} tokens)",
        genesis_tx.outputs[0].value
    );

    let outpoint = OutPoint::new(genesis_tx_id, 0);

    // Query that specific UTXO on the node. Use canonical `Display` impl
    // (`txid#idx`) — both `#` and `:` are accepted server-side post N-07 but
    // the canonical form matches the round-trip contract.
    let outpoint_str = outpoint.to_string();
    print!(
        "\n  UTXO lookup ({}): ",
        &outpoint_str[..outpoint_str.len().min(20)]
    );
    if let Some(utxo) = rpc_call("qv_getUtxo", serde_json::json!([outpoint_str])) {
        if utxo.is_null() {
            println!("not found (genesis UTXO may not be in store yet)");
        } else {
            println!("{}", serde_json::to_string_pretty(&utxo)?);
        }
    }

    // Amounts
    let send_amount = 500_000_000u64;
    let change_amount = 499_999_000u64;
    let fee = 1_000u64;

    // Receiver output
    let receiver_script = p2pkh_pqc(&receiver_pk_hash);
    let receiver_output = TxOutput::new(Amount::from(send_amount), Script::new(receiver_script));

    // Change output (sender gets change back)
    let sender_pk = {
        let kp = generate_pqc_keypair(DilithiumLevel::Level3)?;
        kp.public
    };
    let sender_pk_hash = pubkey_hash(sender_pk.as_bytes());
    let change_script = p2pkh_pqc(&sender_pk_hash);
    let change_output = TxOutput::new(Amount::from(change_amount), Script::new(change_script));

    // Assemble
    let input = TxInput::new(outpoint);
    let mut tx = Transaction::new(vec![input], vec![receiver_output, change_output]);
    tx.fee = Amount::from(fee);
    tx.validity_interval = ValidityInterval::UNBOUNDED;

    println!("\n  Transaction layout:");
    println!("    Input:    genesis:0 (1,000,000,000 tokens)");
    println!("    Output 0: {} tokens -> receiver", send_amount);
    println!("    Output 1: {} tokens -> sender (change)", change_amount);
    println!("    Fee:      {} tokens", fee);
    println!(
        "    Total:    {} = {} + {} + {} ✓",
        send_amount + change_amount + fee,
        send_amount,
        change_amount,
        fee
    );

    // ── Sign ───────────────────────────────────────────────────────────────

    separator("5. SIGNING");

    // ADR-012: sign the witness-excluded sighash; the witness carries only
    // <signature> <pubkey> and the locking script derives the message via
    // the SIG_HASH opcode.
    let sighash = tx.sighash().expect("sighash must compute");
    let signature = qv_crypto::sign_pqc(&sender_sk, &sighash)?;

    let witness_script = ScriptBuilder::new()
        .push_bytes(signature.as_bytes())
        .push_bytes(sender_pk.as_bytes())
        .build();

    tx.inputs[0].witness = qv_core::Witness::new(witness_script);

    let tx_id = tx.id().expect("signed tx must produce valid id");

    println!("  Algorithm:  Dilithium Level 3 (PQC)");
    println!("  Sighash:    {} bytes", sighash.len());
    println!("  Signature:  {} bytes", signature.as_bytes().len());
    println!("  Witness:    {} bytes", tx.inputs[0].witness.len());
    println!("  TX ID:      {}", tx_id);

    // ── Serialize & send ───────────────────────────────────────────────────

    separator("6. SUBMITTING TRANSACTION");

    let tx_bytes = bincode::serialize(&tx)?;
    let tx_hex = hex::encode(&tx_bytes);

    println!(
        "  Serialized: {} bytes ({} hex chars)",
        tx_bytes.len(),
        tx_hex.len()
    );
    println!("  Sending to {}...\n", current_rpc_url());

    if let Some(result) = rpc_call("qv_sendTransaction", serde_json::json!([tx_hex])) {
        println!("  ✔ Transaction accepted!");
        println!("  Returned TX ID: {}", result);
    } else {
        println!("  ✘ Transaction rejected (see error above).");
        println!("    This is expected if the genesis UTXO doesn't match — ");
        println!("    devnet_genesis() generates random keys each run.");
    }

    // ── Query state AFTER ──────────────────────────────────────────────────

    separator("7. CHAIN STATE (after tx)");

    // Mempool (should have +1 tx now)
    print!("  Mempool: ");
    if let Some(status) = rpc_call("qv_getMempoolStatus", serde_json::json!([])) {
        println!("{}", serde_json::to_string_pretty(&status)?);
    }

    // Look up our tx by ID
    let tx_id_hex = format!("{}", tx_id);
    print!(
        "\n  TX lookup ({}...): ",
        &tx_id_hex[..tx_id_hex.len().min(20)]
    );
    if let Some(found_tx) = rpc_call("qv_getTx", serde_json::json!([tx_id_hex])) {
        if found_tx.is_null() {
            println!("not found in mempool/blocks");
        } else {
            println!("FOUND in mempool!");
            if let Some(outputs) = found_tx.get("outputs").and_then(|o| o.as_array()) {
                for (i, out) in outputs.iter().enumerate() {
                    println!(
                        "    Output {}: {} tokens",
                        i,
                        out.get("value").unwrap_or(&serde_json::json!("?"))
                    );
                }
            }
        }
    }

    // Tip (unchanged until next block)
    print!("\n  Chain tip: ");
    if let Some(tip) = rpc_call("qv_getTip", serde_json::json!([])) {
        println!("{}", serde_json::to_string_pretty(&tip)?);
    }

    // ── Summary ────────────────────────────────────────────────────────────

    separator("SUMMARY");

    println!("  Sender (account 0):      1,000,000,000 -> spent");
    println!("  Receiver (fresh key):    +{} tokens", send_amount);
    println!("  Change (back to sender): +{} tokens", change_amount);
    println!("  Fee (to block producer):  {} tokens", fee);
    println!("\n  TX will be included in the next produced block.");
    println!("  Watch the node logs for: \"block produced\" with tx_count=1\n");

    println!("Done.\n");
    Ok(())
}

// ============================================================================
// Minimal HTTP client (no external dependency)
// ============================================================================

/// Minimal HTTP POST using std::net (no external HTTP client dependency).
fn send_rpc_request(url: &str, body: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    // Parse host:port from URL.
    let url = url.strip_prefix("http://").unwrap_or(url);
    let (host_port, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");

    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: {host_port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    stream.write_all(request.as_bytes())?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;

    // Extract body (after the blank line separating headers from body).
    if let Some(idx) = response.find("\r\n\r\n") {
        Ok(response[idx + 4..].to_string())
    } else {
        Ok(response)
    }
}
