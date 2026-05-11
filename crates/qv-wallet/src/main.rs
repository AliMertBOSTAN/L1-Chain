//! qv-wallet CLI entry point.
//!
//! Faz 1 / W-06 (Init) is wired against `WalletKeystore` save/load and the
//! HD spend-key derivation in `qv-wallet::hd` (which now uses
//! `qv_crypto::from_seed_pqc` per envanter C-04, deterministic).
//!
//! Send / Scan / Balance commands are still placeholders — see envanter
//! W-03/W-04/W-05/W-07 in `docs/ROADMAP.md`.

use std::path::PathBuf;

use clap::Parser;
use qv_core::{Amount, OutPoint, Script, TxId, TxInput, TxOutput, ValidityInterval};
use qv_crypto::{DilithiumLevel, PqcPublicKey};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_wallet::cli::{Cli, Commands};
use qv_wallet::hd::{DefaultSeedDeriver, SeedDeriver};
use qv_wallet::keystore::{WalletKeystore, WalletMetadata, WalletSecret};
use qv_wallet::rpc_client::RpcClient;
use qv_wallet::tx_builder::TxBuilder;
use qv_wallet::Mnemonic;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    tracing::info!("qv-wallet starting");

    match cli.command {
        Commands::Init { out } => {
            let path = out.unwrap_or_else(|| cli.keystore.clone());
            cmd_init(&path).await?;
        }
        Commands::ImportMnemonic { phrase, out } => {
            let path = out.unwrap_or_else(|| cli.keystore.clone());
            cmd_import(&path, &phrase).await?;
        }
        Commands::Address { account } => {
            cmd_address(&cli.keystore, account).await?;
        }
        Commands::Scan { from, to } => {
            tracing::warn!(
                from,
                to,
                "scan not yet wired (envanter W-03 + N-02 — qv-privacy stealth scan RPC)"
            );
        }
        Commands::Balance => {
            tracing::warn!(
                "balance not yet wired (envanter W-04 — needs scan_stealth + UTXO aggregation)"
            );
        }
        Commands::Send {
            to_pubkey,
            amount,
            input,
            input_value,
            account,
            fee,
            broadcast,
        } => {
            cmd_send(
                &cli.keystore,
                &cli.rpc,
                &to_pubkey,
                amount,
                &input,
                input_value,
                account,
                fee,
                broadcast,
            )
            .await?;
        }
    }

    Ok(())
}

/// Generate a fresh wallet: 24-word BIP-39 mnemonic, save encrypted, and
/// print the first account's stealth address.
async fn cmd_init(path: &PathBuf) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "keystore already exists at {} — refusing to overwrite",
            path.display()
        );
    }

    // 1. Generate mnemonic.
    let mnemonic = Mnemonic::generate()?;
    println!();
    println!("==============================================================");
    println!("  YENI WALLET — MNEMONIC (24 KELIME) — GUVENLI YERE YAZIN");
    println!("==============================================================");
    println!();
    println!("  {}", mnemonic.phrase());
    println!();
    println!("  Bu cumle CALINIRSA tum bakiyeniz transfer edilebilir.");
    println!("  Bu cumle KAYBOLURSA wallet'iniz geri donulmez sekilde gider.");
    println!("==============================================================");
    println!();

    // 2. Prompt for keystore password (Argon2id encryption key derives from this).
    let password = prompt_new_password()?;

    // 3. Build wallet secret.
    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
    };

    // 4. Save encrypted.
    WalletKeystore::save(path, &secret, &password)?;
    tracing::info!(path = %path.display(), "wallet keystore saved");

    // 5. Derive and print the first stealth address.
    let seed = mnemonic.to_seed("")?;
    let address = derive_address_string(&seed, 0)?;
    println!("  Account 0 stealth address:");
    println!("    {address}");
    println!();
    println!("  Keystore saved to: {}", path.display());

    Ok(())
}

async fn cmd_import(path: &PathBuf, phrase: &str) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "keystore already exists at {} — refusing to overwrite",
            path.display()
        );
    }

    let mnemonic = Mnemonic::from_phrase(phrase)?;
    let password = prompt_new_password()?;

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
    };

    WalletKeystore::save(path, &secret, &password)?;
    tracing::info!(path = %path.display(), "wallet imported and keystore saved");

    let seed = mnemonic.to_seed("")?;
    let address = derive_address_string(&seed, 0)?;
    println!("  Account 0 stealth address:");
    println!("    {address}");

    Ok(())
}

async fn cmd_address(keystore_path: &PathBuf, account: u32) -> anyhow::Result<()> {
    let password = prompt_password("Wallet password")?;
    let secret = WalletKeystore::load(keystore_path, &password)?;
    let seed = secret.mnemonic.to_seed("")?;
    let address = derive_address_string(&seed, account)?;
    println!("Account {account} stealth address:");
    println!("  {address}");
    Ok(())
}

/// Build, sign, and (optionally) broadcast a transfer transaction.
///
/// The signing flow follows the `transfer_e2e` integration test pattern:
/// `p2pkh_pqc` locking script over `pubkey_hash(pk)`, witness encoded as
/// script bytecode pushing `(message, signature, pubkey)`.
#[allow(clippy::too_many_arguments)]
async fn cmd_send(
    keystore_path: &PathBuf,
    rpc_url: &str,
    to_pubkey_hex: &str,
    amount: u64,
    input: &str,
    input_value: u64,
    account: u32,
    fee: u64,
    broadcast: bool,
) -> anyhow::Result<()> {
    // 1. Validate amounts.
    if amount.checked_add(fee).is_none() {
        anyhow::bail!("amount + fee overflow");
    }
    let outflow = amount + fee;
    if outflow > input_value {
        anyhow::bail!(
            "amount ({amount}) + fee ({fee}) = {outflow} exceeds input_value ({input_value})"
        );
    }
    let change = input_value - outflow;

    // 2. Load keystore.
    let password = prompt_password("Wallet password")?;
    let secret = WalletKeystore::load(keystore_path, &password)?;

    // 3. Derive sender keys for `account`.
    let seed = secret.mnemonic.to_seed("")?;
    let stealth = DefaultSeedDeriver::default_levels().derive_account(&seed, account)?;
    let sender_sk = stealth.spend_kp.secret;
    let sender_pk = stealth.spend_kp.public;

    // 4. Parse `--input` as `<txid_hex>:<idx>`.
    let (txid_hex, idx_str) = input
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("--input must be 'txid_hex:idx', got {input:?}"))?;
    let txid_bytes = hex::decode(txid_hex)
        .map_err(|e| anyhow::anyhow!("invalid txid hex: {e}"))?;
    let txid_arr: [u8; 32] = txid_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("txid must be exactly 32 bytes hex (64 chars)"))?;
    let txid = TxId::from_bytes(txid_arr);
    let idx: u32 = idx_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid output index: {e}"))?;
    let outpoint = OutPoint::new(txid, idx);

    // 5. Parse recipient public key.
    let to_pk_bytes = hex::decode(to_pubkey_hex)
        .map_err(|e| anyhow::anyhow!("invalid --to-pubkey hex: {e}"))?;
    let to_pk = PqcPublicKey::from_bytes(DilithiumLevel::Level3, to_pk_bytes)
        .map_err(|e| anyhow::anyhow!("recipient pk parse failed: {e}"))?;

    // 6. Build locking scripts (p2pkh_pqc).
    let to_pk_hash = pubkey_hash(to_pk.as_bytes());
    let to_script = Script::new(p2pkh_pqc(&to_pk_hash));

    let change_pk_hash = pubkey_hash(sender_pk.as_bytes());
    let change_script = Script::new(p2pkh_pqc(&change_pk_hash));

    // 7. Build the transaction.
    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    builder.add_input(TxInput::new(outpoint));
    builder.add_output(TxOutput::new(Amount::from(amount), to_script));
    if change > 0 {
        builder.add_output(TxOutput::new(Amount::from(change), change_script));
    }

    // 8. Sign with the spend key (witness gets attached to input 0).
    builder.sign_with(&sender_sk, &sender_pk)?;
    let tx = builder.build_unsigned()?;

    // 9. Compute the local tx_id and serialize.
    let tx_id = tx
        .id()
        .map_err(|e| anyhow::anyhow!("tx id compute failed: {e}"))?;
    let tx_bytes = bincode::serialize(&tx)
        .map_err(|e| anyhow::anyhow!("bincode serialize failed: {e}"))?;
    let tx_hex = hex::encode(&tx_bytes);

    println!();
    println!("Signed transaction built");
    println!("  account:      {account}");
    println!("  input:        {txid_hex}:{idx}  (value {input_value})");
    println!("  to amount:    {amount}");
    println!("  fee:          {fee}");
    println!("  change:       {change}");
    println!("  local tx_id:  {}", tx_id.to_hex());
    println!("  tx size:      {} bytes ({} hex)", tx_bytes.len(), tx_hex.len());
    println!();

    // 10. Broadcast or print.
    if broadcast {
        let rpc = RpcClient::new(rpc_url);
        let result = rpc
            .call(
                "qv_sendTransaction",
                vec![serde_json::Value::String(tx_hex.clone())],
            )
            .await
            .map_err(|e| anyhow::anyhow!("rpc send failed: {e}"))?;
        println!("Broadcast OK. RPC result:");
        println!("  {result}");
    } else {
        println!("Hex-encoded transaction (paste to RPC qv_sendTransaction):");
        println!("  {tx_hex}");
        println!();
        println!("Add --broadcast to submit via {rpc_url} automatically.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive the account's stealth keys (spend Dilithium + view Hybrid KEM) and
/// produce a printable address string.
///
/// The stealth address binds both public keys via SHA3-256 and is encoded as
/// `qv1` + hex (40 hex chars + checksum). This is a temporary encoding for
/// devnet; bech32m is on the wallet roadmap (Faz 5 / DOC item).
fn derive_address_string(seed: &[u8; 64], account: u32) -> anyhow::Result<String> {
    let deriver = DefaultSeedDeriver::default_levels();
    let stealth = deriver.derive_account(seed, account)?;

    // Hash all public-key material into a single 32-byte address binding.
    // `HybridPublicKey` is split into `x25519` (32 B) + `kyber` (Vec<u8>);
    // include both pieces so changing either changes the address.
    use qv_crypto::sha3_256;
    let mut input = Vec::new();
    input.extend_from_slice(b"QuantumVault-StealthAddr-v1");
    input.extend_from_slice(stealth.spend_kp.public.as_bytes());
    input.extend_from_slice(&stealth.view_kp.public.x25519);
    input.extend_from_slice(&stealth.view_kp.public.kyber);
    let digest = sha3_256(&input);

    // Devnet encoding: "qv1" prefix + first 20 bytes of digest as hex.
    Ok(format!("qv1{}", hex::encode(&digest[..20])))
}

/// Prompt for a new password twice (with confirmation).
fn prompt_new_password() -> anyhow::Result<String> {
    let pw1 = rpassword::prompt_password("New keystore password: ")?;
    if pw1.len() < 8 {
        anyhow::bail!("password must be at least 8 characters");
    }
    let pw2 = rpassword::prompt_password("Confirm password:       ")?;
    if pw1 != pw2 {
        anyhow::bail!("passwords do not match");
    }
    Ok(pw1)
}

fn prompt_password(label: &str) -> anyhow::Result<String> {
    Ok(rpassword::prompt_password(format!("{label}: "))?)
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
