//! qv-wallet CLI entry point.
//!
//! Wires the same building blocks the library exposes (`Mnemonic`,
//! `WalletKeystore`, `TxBuilder`, `RpcClient`, the new `server` UI)
//! into a single binary.

use std::path::Path;
use std::str::FromStr;

use clap::Parser;
use qv_core::{Amount, OutPoint, Script, TxId, TxInput, TxOutput, ValidityInterval};
use qv_crypto::{DilithiumLevel, PqcPublicKey, SharedSecret};
use qv_privacy::StealthKeys;
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_wallet::address::{decode_address, encode_address, fingerprint};
use qv_wallet::cli::{Cli, Commands};
use qv_wallet::hd::{DefaultSeedDeriver, SeedDeriver, DEVNET_TEST_MNEMONIC};
use qv_wallet::keystore::{
    PersistedViewKey, WalletKeystore, WalletMetadata, WalletSecret,
};
use qv_wallet::qvaddr::{address_to_qr_parts, render_qr_unicode, Qvaddr};
use qv_wallet::rpc_client::{P2pkhMatch, RpcClient, StealthMatch};
use qv_wallet::server::{serve as serve_ui, AppState};
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
        Commands::DevnetImport { password, out } => {
            let path = out.unwrap_or_else(|| cli.keystore.clone());
            cmd_devnet_import(&path, password.as_deref()).await?;
        }
        Commands::Address {
            account,
            save,
            qr,
            full_qr,
            qr_parts,
        } => {
            cmd_address(
                &cli.keystore,
                account,
                save.as_deref(),
                qr,
                full_qr,
                qr_parts,
            )
            .await?;
        }
        Commands::Scan { from, to, account } => {
            cmd_scan(&cli.keystore, &cli.rpc, from, to, account).await?;
        }
        Commands::Balance { account } => {
            cmd_balance(&cli.keystore, &cli.rpc, account).await?;
        }
        Commands::SendStealth {
            to_address,
            to_qvaddr,
            amount,
            fee,
            account,
        } => {
            let recipient = resolve_recipient_address(to_address, to_qvaddr.as_deref())?;
            cmd_send_stealth(&cli.keystore, &cli.rpc, &recipient, amount, fee, account).await?;
        }
        Commands::Serve { bind } => {
            cmd_serve(&cli.keystore, &cli.rpc, &bind).await?;
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
            cmd_send_plain(
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
/// print the first account's full stealth address.
async fn cmd_init(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "keystore already exists at {} — refusing to overwrite",
            path.display()
        );
    }

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

    let password = prompt_new_password()?;

    // Pre-bake the account-0 view keypair so the address we print is
    // exactly what the next unlock will derive (envanter C-05).
    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver.generate_fresh_view_keypair()?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };

    WalletKeystore::save(path, &secret, &password)?;
    tracing::info!(path = %path.display(), "wallet keystore saved");

    let seed = mnemonic.to_seed("")?;
    let stealth = deriver.derive_account_with_view(&seed, 0, view_kp)?;
    print_address_block(&stealth, 0)?;
    println!("  Keystore saved to: {}", path.display());

    Ok(())
}

async fn cmd_import(path: &Path, phrase: &str) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "keystore already exists at {} — refusing to overwrite",
            path.display()
        );
    }

    let mnemonic = Mnemonic::from_phrase(phrase)?;
    let password = prompt_new_password()?;

    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver.generate_fresh_view_keypair()?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };

    WalletKeystore::save(path, &secret, &password)?;
    tracing::info!(path = %path.display(), "wallet imported and keystore saved");

    let seed = mnemonic.to_seed("")?;
    let stealth = deriver.derive_account_with_view(&seed, 0, view_kp)?;
    print_address_block(&stealth, 0)?;
    Ok(())
}

async fn cmd_devnet_import(path: &Path, password: Option<&str>) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "keystore already exists at {} — refusing to overwrite",
            path.display()
        );
    }
    println!();
    println!("==============================================================");
    println!("  DEVNET TEST WALLET IMPORT — PUBLIC MNEMONIC, DO NOT USE ON MAINNET");
    println!("==============================================================");
    println!();

    let mnemonic = Mnemonic::from_phrase(DEVNET_TEST_MNEMONIC)?;
    let pw = match password {
        Some(p) if p.len() >= 8 => p.to_string(),
        Some(_) => anyhow::bail!("password must be at least 8 characters"),
        None => prompt_new_password()?,
    };

    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver.generate_fresh_view_keypair()?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };
    WalletKeystore::save(path, &secret, &pw)?;
    tracing::info!(path = %path.display(), "devnet wallet keystore saved");

    let seed = mnemonic.to_seed("")?;
    let stealth = deriver.derive_account_with_view(&seed, 0, view_kp)?;
    print_address_block(&stealth, 0)?;
    println!();
    println!("  Keystore saved to: {}", path.display());
    println!(
        "  Run `qv-wallet balance` to confirm — devnet genesis should pre-fund this account."
    );
    Ok(())
}

async fn cmd_address(
    keystore_path: &Path,
    account: u32,
    save: Option<&Path>,
    show_qr: bool,
    full_qr: bool,
    qr_parts: usize,
) -> anyhow::Result<()> {
    let stealth = unlock_and_derive(keystore_path, account)?;
    let addr = stealth.address();
    print_address_block(&stealth, account)?;

    if show_qr {
        let fp = fingerprint(&addr);
        println!();
        println!("Fingerprint QR ({fp}):");
        println!("{}", render_qr_unicode(&fp)?);
    }

    if full_qr {
        let full = encode_address(&addr)?;
        let parts = address_to_qr_parts(&full, qr_parts.max(1))?;
        for (i, p) in parts.iter().enumerate() {
            println!();
            println!("Full address QR — part {} / {}:", i + 1, parts.len());
            println!("{}", render_qr_unicode(p)?);
        }
        println!();
        println!(
            "Scan all {} codes with a QuantumVault wallet to reassemble the address.",
            parts.len()
        );
    }

    if let Some(path) = save {
        let qv = Qvaddr::from_address(&addr, None)?;
        qv.save(path)?;
        println!();
        println!("Saved .qvaddr to {}", path.display());
    }
    Ok(())
}

/// Decide which recipient address `send-stealth` should use, given the
/// CLI's `(to_address, to_qvaddr)` pair. Clap already rejects supplying
/// both at once; this only enforces that *at least one* was provided.
fn resolve_recipient_address(
    to_address: Option<String>,
    to_qvaddr: Option<&Path>,
) -> anyhow::Result<String> {
    match (to_address, to_qvaddr) {
        (Some(s), _) => Ok(s),
        (None, Some(path)) => {
            let q = Qvaddr::load(path)?;
            Ok(q.address)
        }
        (None, None) => {
            anyhow::bail!("send-stealth: either --to-address or --to-qvaddr is required")
        }
    }
}

async fn cmd_balance(
    keystore_path: &Path,
    rpc_url: &str,
    account: u32,
) -> anyhow::Result<()> {
    let stealth = unlock_and_derive(keystore_path, account)?;
    let rpc = RpcClient::new(rpc_url);
    let stealth_bal = rpc.get_balance_for(&stealth).await?;
    let pk_hash = qv_script::templates::pubkey_hash(stealth.spend_kp.public.as_bytes());
    let plain_utxos = rpc.scan_p2pkh(&pk_hash).await?;
    let plain_bal: u64 = plain_utxos.iter().map(|u| u.value).sum();
    println!("Account {account} balance:");
    println!("  stealth : {stealth_bal} units");
    println!("  plain   : {plain_bal} units  ({} utxo)", plain_utxos.len());
    println!(
        "  total   : {} units",
        stealth_bal.saturating_add(plain_bal)
    );
    Ok(())
}

async fn cmd_scan(
    keystore_path: &Path,
    rpc_url: &str,
    from: u64,
    to: u64,
    account: u32,
) -> anyhow::Result<()> {
    let stealth = unlock_and_derive(keystore_path, account)?;
    let rpc = RpcClient::new(rpc_url);
    let stealth_utxos = rpc.scan_stealth(&stealth, from, to).await?;
    let pk_hash = qv_script::templates::pubkey_hash(stealth.spend_kp.public.as_bytes());
    let plain_utxos = rpc.scan_p2pkh(&pk_hash).await?;

    if stealth_utxos.is_empty() && plain_utxos.is_empty() {
        println!("No UTXOs found (neither stealth nor plain).");
        return Ok(());
    }
    if !stealth_utxos.is_empty() {
        println!("Stealth UTXOs ({}):", stealth_utxos.len());
        for m in &stealth_utxos {
            println!("  {}:{}  value={}", m.tx_id, m.output_index, m.value);
        }
    }
    if !plain_utxos.is_empty() {
        println!("Plain p2pkh_pqc UTXOs ({}):", plain_utxos.len());
        for m in &plain_utxos {
            println!("  {}:{}  value={}", m.tx_id, m.output_index, m.value);
        }
    }
    Ok(())
}

async fn cmd_send_stealth(
    keystore_path: &Path,
    rpc_url: &str,
    to_address: &str,
    amount: u64,
    fee: u64,
    account: u32,
) -> anyhow::Result<()> {
    if amount == 0 {
        anyhow::bail!("amount must be positive");
    }
    let outflow = amount
        .checked_add(fee)
        .ok_or_else(|| anyhow::anyhow!("amount + fee overflow"))?;

    let stealth = unlock_and_derive(keystore_path, account)?;
    let recipient = decode_address(to_address)?;

    let rpc = RpcClient::new(rpc_url);

    // Fetch both pools: stealth + plain (p2pkh_pqc to our spend pk hash).
    let stealth_utxos = rpc.scan_stealth(&stealth, 0, u64::MAX).await?;
    let pk_hash = qv_script::templates::pubkey_hash(stealth.spend_kp.public.as_bytes());
    let plain_utxos = rpc.scan_p2pkh(&pk_hash).await?;

    let available: u64 = stealth_utxos.iter().map(|u| u.value).sum::<u64>()
        + plain_utxos.iter().map(|u| u.value).sum::<u64>();
    if available < outflow {
        anyhow::bail!(
            "insufficient balance: need {outflow}, have {available} (stealth + plain)"
        );
    }

    enum Pick<'a> {
        Stealth(&'a StealthMatch),
        Plain(&'a P2pkhMatch),
    }
    let mut stealth_sorted: Vec<&StealthMatch> = stealth_utxos.iter().collect();
    stealth_sorted.sort_by(|a, b| b.value.cmp(&a.value));
    let mut plain_sorted: Vec<&P2pkhMatch> = plain_utxos.iter().collect();
    plain_sorted.sort_by(|a, b| b.value.cmp(&a.value));

    let mut picks: Vec<Pick<'_>> = Vec::new();
    let mut total: u64 = 0;
    for u in &stealth_sorted {
        if total >= outflow {
            break;
        }
        total = total.saturating_add(u.value);
        picks.push(Pick::Stealth(u));
    }
    for u in &plain_sorted {
        if total >= outflow {
            break;
        }
        total = total.saturating_add(u.value);
        picks.push(Pick::Plain(u));
    }
    if total < outflow {
        anyhow::bail!(
            "insufficient balance after selection: need {outflow}, picked {total}"
        );
    }
    let change = total - outflow;

    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    for pick in &picks {
        let (tx_id_hex, out_idx) = match pick {
            Pick::Stealth(u) => (&u.tx_id, u.output_index),
            Pick::Plain(u) => (&u.tx_id, u.output_index),
        };
        let tx_id_bytes = hex::decode(tx_id_hex)?;
        let tx_id_arr: [u8; 32] = tx_id_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("server tx_id must be 32 bytes"))?;
        let op = OutPoint::new(TxId::from_bytes(tx_id_arr), out_idx);
        builder.add_input(TxInput::new(op));
    }

    builder.add_stealth_output(Amount::from(amount), &recipient)?;
    if change > 0 {
        builder.add_stealth_output(Amount::from(change), &stealth.address())?;
    }

    for (idx, pick) in picks.iter().enumerate() {
        match pick {
            Pick::Stealth(u) => {
                let ss_bytes = hex::decode(&u.shared_secret_hex)?;
                let ss_arr: [u8; 32] = ss_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("shared_secret must be 32 bytes"))?;
                let shared = SharedSecret(ss_arr);
                builder.sign_stealth_input(
                    idx,
                    &stealth.spend_kp.secret,
                    &stealth.spend_kp.public,
                    &shared,
                )?;
            }
            Pick::Plain(_) => {
                builder.sign_plain_input(
                    idx,
                    &stealth.spend_kp.secret,
                    &stealth.spend_kp.public,
                )?;
            }
        }
    }

    let tx = builder.build_unsigned()?;
    let tx_id = tx
        .id()
        .map_err(|e| anyhow::anyhow!("tx id compute failed: {e}"))?;
    let tx_bytes = bincode::serialize(&tx)?;
    let tx_hex = hex::encode(&tx_bytes);

    println!();
    println!("Stealth transfer built");
    let n_stealth = picks
        .iter()
        .filter(|p| matches!(p, Pick::Stealth(_)))
        .count();
    let n_plain = picks.len().saturating_sub(n_stealth);
    println!(
        "  inputs:     {} utxos ({} stealth + {} plain), total {} units",
        picks.len(),
        n_stealth,
        n_plain,
        total
    );
    println!("  recipient:  {to_address}");
    println!("  amount:     {amount}");
    println!("  change:     {change} (returned to your own stealth address)");
    println!("  fee:        {fee}");
    println!("  local tx_id: {}", tx_id.to_hex());
    println!("  tx size:    {} bytes", tx_bytes.len());

    let res = rpc.send_transaction(&tx_hex).await?;
    println!();
    println!("Broadcast OK. RPC result: {res}");
    Ok(())
}

async fn cmd_serve(keystore_path: &Path, rpc_url: &str, bind: &str) -> anyhow::Result<()> {
    let addr = std::net::SocketAddr::from_str(bind)
        .map_err(|e| anyhow::anyhow!("invalid bind address {bind}: {e}"))?;
    if !addr.ip().is_loopback() {
        // Refuse to expose the unlocked view key on any non-loopback iface.
        anyhow::bail!(
            "refusing to bind to {addr} — only 127.0.0.1 / ::1 are permitted (the unlocked view key would otherwise be exposed)"
        );
    }
    let state = AppState::new(keystore_path.to_path_buf(), rpc_url.to_string());
    println!("qv-wallet UI listening at http://{addr}");
    println!("  keystore: {}", keystore_path.display());
    println!("  node RPC: {rpc_url}");
    serve_ui(state, addr).await
}

/// Build, sign, and (optionally) broadcast a plain `p2pkh_pqc` transfer.
///
/// Kept for compatibility with the existing devnet flow. New flows should
/// use [`cmd_send_stealth`].
#[allow(clippy::too_many_arguments)]
async fn cmd_send_plain(
    keystore_path: &Path,
    rpc_url: &str,
    to_pubkey_hex: &str,
    amount: u64,
    input: &str,
    input_value: u64,
    account: u32,
    fee: u64,
    broadcast: bool,
) -> anyhow::Result<()> {
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

    let stealth = unlock_and_derive(keystore_path, account)?;
    let sender_sk = stealth.spend_kp.secret.clone();
    let sender_pk = stealth.spend_kp.public.clone();

    let (txid_hex, idx_str) = input
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("--input must be 'txid_hex:idx', got {input:?}"))?;
    let txid_bytes =
        hex::decode(txid_hex).map_err(|e| anyhow::anyhow!("invalid txid hex: {e}"))?;
    let txid_arr: [u8; 32] = txid_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("txid must be exactly 32 bytes hex (64 chars)"))?;
    let txid = TxId::from_bytes(txid_arr);
    let idx: u32 = idx_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid output index: {e}"))?;
    let outpoint = OutPoint::new(txid, idx);

    let to_pk_bytes = hex::decode(to_pubkey_hex)
        .map_err(|e| anyhow::anyhow!("invalid --to-pubkey hex: {e}"))?;
    let to_pk = PqcPublicKey::from_bytes(DilithiumLevel::Level3, to_pk_bytes)
        .map_err(|e| anyhow::anyhow!("recipient pk parse failed: {e}"))?;

    let to_pk_hash = pubkey_hash(to_pk.as_bytes());
    let to_script = Script::new(p2pkh_pqc(&to_pk_hash));
    let change_pk_hash = pubkey_hash(sender_pk.as_bytes());
    let change_script = Script::new(p2pkh_pqc(&change_pk_hash));

    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    builder.add_input(TxInput::new(outpoint));
    builder.add_output(TxOutput::new(Amount::from(amount), to_script));
    if change > 0 {
        builder.add_output(TxOutput::new(Amount::from(change), change_script));
    }

    builder.sign_with(&sender_sk, &sender_pk)?;
    let tx = builder.build_unsigned()?;
    let tx_id = tx
        .id()
        .map_err(|e| anyhow::anyhow!("tx id compute failed: {e}"))?;
    let tx_bytes = bincode::serialize(&tx)?;
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

    if broadcast {
        let rpc = RpcClient::new(rpc_url);
        let res = rpc.send_transaction(&tx_hex).await?;
        println!("Broadcast OK. RPC result: {res}");
    } else {
        println!();
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

fn unlock_and_derive(keystore_path: &Path, account: u32) -> anyhow::Result<StealthKeys> {
    let password = prompt_password("Wallet password")?;
    let deriver = DefaultSeedDeriver::default_levels();
    // `unlock_account` reuses a persisted view keypair if present, else
    // generates one and re-saves the keystore (v1→v2 auto-upgrade,
    // envanter C-05). The spend key is always deterministic from the
    // mnemonic seed.
    let stealth = WalletKeystore::unlock_account(keystore_path, &password, account, &deriver)?;
    Ok(stealth)
}

fn print_address_block(stealth: &StealthKeys, account: u32) -> anyhow::Result<()> {
    let addr = stealth.address();
    println!("Account {account} stealth address:");
    println!("  fingerprint : {}", fingerprint(&addr));
    println!("  full        : {}", encode_address(&addr)?);
    Ok(())
}

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
