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
use qv_wallet::cli::{Cli, Commands, ContactsCmd, SwapDirectionArg};
use qv_wallet::hd::{DefaultSeedDeriver, DEVNET_TEST_MNEMONIC};
use qv_wallet::keystore::{
    PersistedViewKey, WalletKeystore, WalletMetadata, WalletSecret,
};
use qv_wallet::qvaddr::{address_to_qr_parts, render_qr_unicode, Qvaddr};
use qv_wallet::rpc_client::{P2pkhMatch, RpcClient, StealthMatch};
use qv_wallet::server::{serve as serve_ui, AppState};
use qv_wallet::swap::{
    direction_from_arg, direction_label, execute_create_pool, execute_swap,
    CreatePoolParams, SwapParams,
};
use qv_wallet::tx_builder::TxBuilder;
use qv_wallet::disclose::{create_disclosure, DisclosureFile};
use qv_wallet::view_export::ViewKeyExport;
use qv_wallet::Mnemonic;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    tracing::info!("qv-wallet starting");

    // `--network devnet` ya da `--network local` iyi-bilinen URL'lere
    // çözülür; yoksa `--rpc` parametresinin değeri kullanılır.
    let rpc_url = cli.effective_rpc_url();

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
            cmd_scan(&cli.keystore, &rpc_url, from, to, account).await?;
        }
        Commands::Balance { account } => {
            cmd_balance(&cli.keystore, &rpc_url, account).await?;
        }
        Commands::SendStealth {
            to_address,
            to_qvaddr,
            to_contact,
            amount,
            fee,
            account,
        } => {
            let recipient = resolve_recipient_address(
                &cli.keystore,
                to_address,
                to_qvaddr.as_deref(),
                to_contact.as_deref(),
            )?;
            cmd_send_stealth(&cli.keystore, &rpc_url, &recipient, amount, fee, account).await?;
        }
        Commands::Contacts(cmd) => {
            cmd_contacts(&cli.keystore, cmd).await?;
        }
        Commands::ExportViewKey { out, account, label } => {
            cmd_export_view_key(&cli.keystore, &out, account, label).await?;
        }
        Commands::AuditScan { view_key, from, to } => {
            cmd_audit_scan(&view_key, &rpc_url, from, to).await?;
        }
        Commands::Disclose {
            utxo,
            out,
            label,
            account,
            amount,
        } => {
            cmd_disclose(&cli.keystore, &rpc_url, &utxo, &out, label, account, amount).await?;
        }
        Commands::VerifyDisclosure { proof } => {
            cmd_verify_disclosure(&proof).await?;
        }
        Commands::Serve {
            bind,
            wallets_dir,
            session_ttl_secs,
        } => {
            cmd_serve(
                &cli.keystore,
                &rpc_url,
                &bind,
                wallets_dir.as_deref(),
                session_ttl_secs,
            )
            .await?;
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
                &rpc_url,
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
        Commands::Swap {
            pool,
            direction,
            amount,
            min_receive,
            input,
            input_value,
            account,
            fee,
            broadcast,
        } => {
            cmd_swap(
                &cli.keystore,
                &rpc_url,
                &pool,
                direction,
                amount,
                min_receive,
                input.as_deref(),
                input_value,
                account,
                fee,
                broadcast,
            )
            .await?;
        }
        Commands::CreatePool {
            token_a,
            token_b,
            fee_bps,
            reserve_a,
            reserve_b,
            pool_value,
            input,
            input_value,
            account,
            fee,
            broadcast,
        } => {
            cmd_create_pool(
                &cli.keystore,
                &rpc_url,
                CreatePoolParams {
                    token_a_hex: token_a,
                    token_b_hex: token_b,
                    fee_bps,
                    reserve_a,
                    reserve_b,
                    pool_value,
                    input,
                    input_value,
                    fee,
                },
                account,
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
/// CLI's `(to_address, to_qvaddr, to_contact)` triple. Clap already
/// rejects supplying more than one at once; this only enforces that
/// *at least one* was provided.
fn resolve_recipient_address(
    keystore_path: &Path,
    to_address: Option<String>,
    to_qvaddr: Option<&Path>,
    to_contact: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(s) = to_address {
        return Ok(s);
    }
    if let Some(path) = to_qvaddr {
        return Ok(Qvaddr::load(path)?.address);
    }
    if let Some(label) = to_contact {
        let password = prompt_password("Wallet password")?;
        let book_path = qv_wallet::address_book::contacts_path_for(keystore_path);
        let book = qv_wallet::address_book::load_or_empty(&book_path, &password)?;
        let contact = book.get(label).ok_or_else(|| {
            anyhow::anyhow!("no contact named `{label}` — check `qv-wallet contacts list`")
        })?;
        return Ok(contact.address.clone());
    }
    anyhow::bail!(
        "send-stealth: one of --to-address / --to-qvaddr / --to-contact is required"
    )
}

async fn cmd_contacts(keystore_path: &Path, cmd: ContactsCmd) -> anyhow::Result<()> {
    use qv_wallet::address_book::{contacts_path_for, load_or_empty, save};

    let book_path = contacts_path_for(keystore_path);
    let password = prompt_password("Wallet password")?;
    let mut book = load_or_empty(&book_path, &password)?;

    match cmd {
        ContactsCmd::Add { label, address, notes } => {
            book.add(&label, &address, notes)?;
            save(&book_path, &book, &password)?;
            println!("Contact `{label}` added.");
            if let Some(c) = book.get(&label) {
                print_contact(&label, c);
            }
        }
        ContactsCmd::List => {
            if book.is_empty() {
                println!("No contacts yet. Add one with:");
                println!("  qv-wallet contacts add --label alice --address qvst1...");
                return Ok(());
            }
            println!("Contacts ({}):", book.len());
            println!();
            for (label, c) in book.iter() {
                print_contact(label, c);
                println!();
            }
        }
        ContactsCmd::Remove { label } => {
            let removed = book.remove(&label)?;
            save(&book_path, &book, &password)?;
            println!("Removed contact `{label}` (fingerprint {}).", removed.fingerprint);
        }
        ContactsCmd::Show { label } => {
            let c = book.get(&label).ok_or_else(|| {
                anyhow::anyhow!("no contact named `{label}`")
            })?;
            print_contact(&label, c);
        }
    }
    Ok(())
}

fn print_contact(label: &str, c: &qv_wallet::address_book::Contact) {
    let _: &qv_wallet::address_book::Contact = c; // keep type explicit
    println!("  label       : {label}");
    println!("  fingerprint : {}", c.fingerprint);
    if let Some(notes) = &c.notes {
        println!("  notes       : {notes}");
    }
    println!("  added_at    : {}", c.added_at);
    let short = if c.address.len() > 24 {
        format!("{}…{}", &c.address[..16], &c.address[c.address.len() - 8..])
    } else {
        c.address.clone()
    };
    println!("  address     : {short}  ({} chars)", c.address.len());
}

async fn cmd_balance(
    keystore_path: &Path,
    rpc_url: &str,
    account: u32,
) -> anyhow::Result<()> {
    let stealth = unlock_and_derive(keystore_path, account)?;
    let rpc = RpcClient::new(rpc_url);
    let stealth_bal = rpc.get_balance_for(&stealth).await?;
    let pk_hash = pubkey_hash(stealth.spend_kp.public.as_bytes());
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
    let pk_hash = pubkey_hash(stealth.spend_kp.public.as_bytes());
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
    let pk_hash = pubkey_hash(stealth.spend_kp.public.as_bytes());
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

async fn cmd_export_view_key(
    keystore_path: &Path,
    out: &Path,
    account: u32,
    label: Option<String>,
) -> anyhow::Result<()> {
    if out.exists() {
        anyhow::bail!(
            "view-key export already exists at {} — refusing to overwrite",
            out.display()
        );
    }
    let stealth = unlock_and_derive(keystore_path, account)?;
    let export = ViewKeyExport::from_keys(&stealth.view_kp, &stealth.spend_kp.public, label);
    export.save(out)?;

    println!();
    println!("==============================================================");
    println!("  VIEW KEY EXPORT — AUDIT-ONLY (no spend authority)");
    println!("==============================================================");
    println!("  account     : {account}");
    println!("  written to  : {}", out.display());
    println!("  fingerprint : {}", fingerprint(&stealth.address()));
    println!();
    println!("  Bu dosya alici tarafa GELEN tum stealth odemeleri tarayabilir.");
    println!("  Harcama yetkisi VERMEZ — spend secret bu dosyada yok.");
    println!("==============================================================");
    Ok(())
}

async fn cmd_audit_scan(
    view_key_path: &Path,
    rpc_url: &str,
    from: u64,
    to: u64,
) -> anyhow::Result<()> {
    let export = ViewKeyExport::load(view_key_path)?;
    let label = export.label.clone();
    let (view_kp, spend_pk) = export.into_keys()?;
    let rpc = RpcClient::new(rpc_url);
    let matches = rpc
        .scan_stealth_with_view_key(&view_kp, &spend_pk, from, to)
        .await?;

    if matches.is_empty() {
        println!("Audit scan: no stealth UTXOs detected.");
        return Ok(());
    }

    let total: u64 = matches.iter().map(|m| m.value).sum();
    println!();
    if let Some(label) = label {
        println!("Audit scan for `{label}`");
    } else {
        println!("Audit scan");
    }
    println!(
        "  {} stealth UTXO(s) detected, total {} units (audit-only — cannot spend):",
        matches.len(),
        total
    );
    for m in &matches {
        println!("  {}:{}  value={}", m.tx_id, m.output_index, m.value);
    }
    Ok(())
}

async fn cmd_disclose(
    keystore_path: &Path,
    rpc_url: &str,
    utxo: &str,
    out: &Path,
    label: Option<String>,
    account: u32,
    amount: Option<u64>,
) -> anyhow::Result<()> {
    use qv_crypto::{DilithiumLevel, KyberLevel, SharedSecret};

    if out.exists() {
        anyhow::bail!(
            "disclosure file already exists at {} — refusing to overwrite",
            out.display()
        );
    }
    let (tx_id_hex, idx_str) = utxo
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("--utxo must be `<tx_id_hex>:<idx>`, got {utxo:?}"))?;
    let target_idx: u32 = idx_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid output index: {e}"))?;

    // Unlock the wallet and ask the node for the stealth UTXO set — we
    // need the per-output `shared_secret`, `kem_ciphertext`, `view_tag`,
    // and `onetime_pk_hash` that only `scan_stealth` returns. Pure
    // RPC: we don't need any local stealth scanning logic here.
    let stealth = unlock_and_derive(keystore_path, account)?;
    let rpc = RpcClient::new(rpc_url);
    let matches = rpc.scan_stealth(&stealth, 0, u64::MAX).await?;
    let m = matches
        .iter()
        .find(|m| m.tx_id == tx_id_hex && m.output_index == target_idx)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "outpoint {tx_id_hex}:{target_idx} is not in this wallet's stealth UTXO set"
            )
        })?;

    // The on-chain `StealthInfo` (kem_ciphertext + view_tag + kyber_level)
    // is now embedded directly in `StealthMatch` so we don't need a
    // second RPC round-trip.
    let kyber_level = match m.kyber_level {
        1 => KyberLevel::Level1,
        3 => KyberLevel::Level3,
        5 => KyberLevel::Level5,
        other => anyhow::bail!("unknown Kyber level on chain: {other}"),
    };
    let view_tag_bytes = hex::decode(&m.view_tag_hex)?;
    if view_tag_bytes.len() != 1 {
        anyhow::bail!("view_tag must be 1 byte, got {}", view_tag_bytes.len());
    }
    let view_tag = view_tag_bytes[0];
    let ephemeral_pubkey = hex::decode(&m.kem_ciphertext_hex)?;

    // Decode shared_secret + onetime_pk_hash from the scan response.
    let ss_bytes = hex::decode(&m.shared_secret_hex)?;
    let ss_arr: [u8; 32] = ss_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("shared_secret must be 32 bytes"))?;
    let shared_secret = SharedSecret(ss_arr);
    let opk_bytes = hex::decode(&m.onetime_pk_hash_hex)?;
    let opk: [u8; 32] = opk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("onetime_pk_hash must be 32 bytes"))?;

    let file = create_disclosure(
        tx_id_hex,
        target_idx,
        kyber_level,
        DilithiumLevel::Level3,
        &hex::encode(&ephemeral_pubkey),
        view_tag,
        &opk,
        &stealth.spend_kp.public,
        &shared_secret,
        amount,
        label,
    )?;
    file.save(out)?;

    println!();
    println!("==============================================================");
    println!("  SELECTIVE DISCLOSURE");
    println!("==============================================================");
    println!("  outpoint    : {tx_id_hex}:{target_idx}");
    println!("  on-chain    : {} units", m.value);
    match amount {
        Some(a) => println!("  disclosed   : {a} units (amount revealed)"),
        None => println!("  disclosed   : amount kept private (binding-hash-only)"),
    }
    println!("  written to  : {}", out.display());
    println!();
    println!("  Verifier needs ONLY this file + their crypto code:");
    println!("    qv-wallet verify-disclosure --proof {}", out.display());
    println!("==============================================================");
    Ok(())
}

async fn cmd_verify_disclosure(proof: &Path) -> anyhow::Result<()> {
    let file = DisclosureFile::load(proof)?;
    let ok = file.verify_self_contained()?;
    println!();
    println!("Disclosure file       : {}", proof.display());
    if let Some(label) = &file.label {
        println!("Label                 : {label}");
    }
    println!("Outpoint              : {}:{}", file.tx_id_hex, file.output_index);
    println!("Spend pk fingerprint  : sha3-{}", short_hex(&file.spend_pk_hex));
    match file.disclosed_amount {
        Some(a) => println!("Disclosed amount      : {a} units"),
        None => println!("Disclosed amount      : (none — only proves ownership)"),
    }
    println!();
    if ok {
        println!("✅ VALID — every check passed (view tag, one-time PK hash, binding hash).");
        println!("           The owner of {} truly received this output.", short_hex(&file.spend_pk_hex));
    } else {
        println!("❌ INVALID — the file does not pass self-contained verification.");
    }
    println!();
    println!("Note: this verifier only proves the proof is consistent with itself.");
    println!("To confirm the outpoint is on-chain, query the node:");
    println!("  qv_getUtxo {}:{}", file.tx_id_hex, file.output_index);
    Ok(())
}

fn short_hex(s: &str) -> String {
    if s.len() <= 16 {
        s.to_string()
    } else {
        format!("{}…{}", &s[..8], &s[s.len() - 8..])
    }
}

async fn cmd_serve(
    keystore_path: &Path,
    rpc_url: &str,
    bind: &str,
    wallets_dir: Option<&Path>,
    session_ttl_secs: u64,
) -> anyhow::Result<()> {
    let addr = std::net::SocketAddr::from_str(bind)
        .map_err(|e| anyhow::anyhow!("invalid bind address {bind}: {e}"))?;
    let loopback = addr.ip().is_loopback();
    if !loopback {
        match wallets_dir {
            None => {
                // Single-user + non-loopback bind = unlocked view key
                // would be exposed to LAN. Refuse.
                anyhow::bail!(
                    "refusing to bind single-user mode to {addr} — pass --wallets-dir to enable multi-tenant + per-user session tokens, or bind to 127.0.0.1 / ::1"
                );
            }
            Some(_) => {
                eprintln!(
                    "WARN: binding multi-tenant wallet to {addr}. Custodial mode is acceptable for devnet/demo only — every logged-in user's spend secret lives in this process's RAM."
                );
            }
        }
    }

    let state = if let Some(dir) = wallets_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("create wallets dir {}: {e}", dir.display()))?;
        AppState::new_multi(
            dir.to_path_buf(),
            rpc_url.to_string(),
            std::time::Duration::from_secs(session_ttl_secs),
        )
    } else {
        AppState::new(keystore_path.to_path_buf(), rpc_url.to_string())
    };

    println!("qv-wallet UI listening at http://{addr}");
    if let Some(dir) = wallets_dir {
        println!(
            "  mode      : multi-tenant (CUSTODIAL — devnet/demo only)"
        );
        println!("  wallets   : {}", dir.display());
        println!("  ttl       : {session_ttl_secs}s per session");
    } else {
        println!("  mode      : single-user");
        println!("  keystore  : {}", keystore_path.display());
    }
    println!("  node RPC  : {rpc_url}");
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

/// Build, sign, and (optionally) broadcast an AMM swap against a pool UTXO
/// locked by `amm_pool_lock` (Faz 6 / D-4).
///
/// The whole flow (pool fetch + verification, funding selection,
/// `qv_defi::build_swap_tx`, ADR-012 signing) lives in the shared
/// [`qv_wallet::swap::execute_swap`] — the same code path the HTTP
/// `/api/defi/swap` endpoint uses. This function only unlocks the
/// keystore, prints the summary, and handles `--broadcast`.
///
/// Token accounting is datum-level for now: the funding input only covers
/// the network fee, and the change + (datum-tracked) proceeds go to the
/// account's own plain p2pkh output.
#[allow(clippy::too_many_arguments)]
async fn cmd_swap(
    keystore_path: &Path,
    rpc_url: &str,
    pool: &str,
    direction_arg: SwapDirectionArg,
    amount: u64,
    min_receive: u64,
    input: Option<&str>,
    input_value: Option<u64>,
    account: u32,
    fee: u64,
    broadcast: bool,
) -> anyhow::Result<()> {
    if amount == 0 {
        anyhow::bail!("--amount must be positive");
    }
    let direction = direction_from_arg(direction_arg);
    let rpc = RpcClient::new(rpc_url);
    let stealth = unlock_and_derive(keystore_path, account)?;

    let params = SwapParams {
        pool: pool.to_string(),
        direction,
        amount_in: amount,
        min_receive,
        input: input.map(str::to_string),
        input_value,
        fee,
    };
    let outcome = execute_swap(
        &rpc,
        &stealth.spend_kp.secret,
        &stealth.spend_kp.public,
        &params,
    )
    .await?;

    // ----- Summary + broadcast (send command pattern). -----
    println!();
    println!("AMM swap built");
    println!("  pool:          {}", outcome.pool_outpoint);
    println!("  direction:     {}", direction_label(direction));
    println!("  amount in:     {amount}");
    println!(
        "  amount out:    {}  (min-receive floor {min_receive})",
        outcome.amount_out
    );
    println!(
        "  pool fee:      {} (stays in the pool reserves)",
        outcome.pool_fee_paid
    );
    println!("  network fee:   {fee}");
    println!(
        "  funding utxo:  {}  (value {}, change {})",
        outcome.user_outpoint, outcome.user_input_value, outcome.change
    );
    println!(
        "  new reserves:  A={}  B={}  (lp_total {})",
        outcome.new_pool_datum.reserve_a,
        outcome.new_pool_datum.reserve_b,
        outcome.new_pool_datum.lp_total
    );
    println!("  local tx_id:   {}", outcome.tx_id_hex);
    println!(
        "  tx size:       {} bytes ({} hex)",
        outcome.tx_size,
        outcome.tx_hex.len()
    );

    if broadcast {
        let res = rpc.send_transaction(&outcome.tx_hex).await?;
        println!("Broadcast OK. RPC result: {res}");
    } else {
        println!();
        println!("Hex-encoded transaction (paste to RPC qv_sendTransaction):");
        println!("  {}", outcome.tx_hex);
        println!();
        println!("Add --broadcast to submit via {rpc_url} automatically.");
    }

    Ok(())
}

/// Build, sign, and (optionally) broadcast the genesis transaction of a
/// brand-new AMM pool UTXO (Faz 6 / D-5).
///
/// The shared flow ([`qv_wallet::swap::execute_create_pool`], also behind
/// `/api/defi/create-pool`) assembles output #0 as the pool UTXO
/// (`amm_pool_lock` script + canonical `PoolDatum`,
/// `lp_total = ⌊sqrt(reserve_a · reserve_b)⌋` via the empty-pool
/// add-liquidity path) and output #1 as change.
async fn cmd_create_pool(
    keystore_path: &Path,
    rpc_url: &str,
    params: CreatePoolParams,
    account: u32,
    broadcast: bool,
) -> anyhow::Result<()> {
    let rpc = RpcClient::new(rpc_url);
    let stealth = unlock_and_derive(keystore_path, account)?;

    let outcome = execute_create_pool(
        &rpc,
        &stealth.spend_kp.secret,
        &stealth.spend_kp.public,
        &params,
    )
    .await?;

    println!();
    println!("AMM pool genesis built");
    println!("  pool outpoint: {}   <- pass this to `swap --pool`", outcome.pool_outpoint);
    println!(
        "  token A:       {}",
        hex::encode(outcome.pool_datum.token_a_id.as_bytes())
    );
    println!(
        "  token B:       {}",
        hex::encode(outcome.pool_datum.token_b_id.as_bytes())
    );
    println!(
        "  reserves:      A={}  B={}",
        outcome.pool_datum.reserve_a, outcome.pool_datum.reserve_b
    );
    println!(
        "  swap fee:      {} bps  |  pool native value: {}",
        outcome.pool_datum.fee_bps, params.pool_value
    );
    println!(
        "  lp_total:      {}  (datum-level LP accounting — see note below)",
        outcome.lp_total
    );
    println!("  network fee:   {}", params.fee);
    println!(
        "  funding utxo:  {}  (value {}, change {})",
        outcome.user_outpoint, outcome.user_input_value, outcome.change
    );
    println!("  local tx_id:   {}", outcome.tx_id_hex);
    println!(
        "  tx size:       {} bytes ({} hex)",
        outcome.tx_size,
        outcome.tx_hex.len()
    );
    println!();
    println!("  NOTE (D-5 scope): LP shares exist only as the datum's lp_total field —");
    println!("  there is NO on-chain LP token. Add/remove-liquidity spend paths are D-6+;");
    println!("  the pool covenant (x*y >= k) would pass add-liquidity-shaped transitions");
    println!("  but blocks remove-liquidity, so locked reserves cannot be withdrawn yet.");

    if broadcast {
        let res = rpc.send_transaction(&outcome.tx_hex).await?;
        println!();
        println!("Broadcast OK. RPC result: {res}");
    } else {
        println!();
        println!("Hex-encoded transaction (paste to RPC qv_sendTransaction):");
        println!("  {}", outcome.tx_hex);
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
