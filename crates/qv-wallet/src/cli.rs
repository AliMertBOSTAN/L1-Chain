//! CLI using clap.
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "QuantumVault CLI wallet")]
pub struct Cli {
    #[arg(long, default_value = "wallet.json", global = true)]
    pub keystore: PathBuf,
    #[arg(long, default_value = "http://localhost:8080", global = true)]
    pub rpc: String,
    #[command(subcommand)]
    pub command: Commands,
}

/// Subcommands for the `contacts` group (address book).
#[derive(Debug, Subcommand)]
pub enum ContactsCmd {
    /// Add a contact. Rejects duplicate labels and unparseable addresses.
    Add {
        /// Short label (e.g. "alice", "Vendor Q2").
        #[arg(long)]
        label: String,
        /// Full `qvst1…` stealth address.
        #[arg(long)]
        address: String,
        /// Optional free-text note.
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all contacts in sorted order.
    List,
    /// Remove a contact by label.
    Remove {
        #[arg(long)]
        label: String,
    },
    /// Print one contact's full details (address + fingerprint + notes).
    Show {
        #[arg(long)]
        label: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    ImportMnemonic {
        phrase: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// One-shot devnet bootstrap: import the well-known
    /// `DEVNET_TEST_MNEMONIC` and save it as the wallet keystore.
    ///
    /// The matching `qv-node --network devnet` genesis pre-funds the first
    /// 10 accounts of this mnemonic, so after running this the wallet
    /// immediately sees a non-zero balance via `qv_scanP2pkh`.
    ///
    /// **Never use on mainnet** — the mnemonic is public.
    DevnetImport {
        /// Keystore password (≥ 8 characters). If omitted, the user is
        /// prompted interactively (recommended).
        #[arg(long)]
        password: Option<String>,
        /// Alternate keystore path (defaults to the global `--keystore`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    Address {
        #[arg(default_value = "0")]
        account: u32,
        /// Also write a `.qvaddr` file (JSON sidecar) to this path.
        #[arg(long)]
        save: Option<PathBuf>,
        /// Render an ASCII QR of the address fingerprint (small).
        #[arg(long, default_value_t = false)]
        qr: bool,
        /// Render an ASCII multi-part QR of the FULL stealth address
        /// (large — terminal must be wide). Splits across `qr_parts` codes.
        #[arg(long, default_value_t = false)]
        full_qr: bool,
        /// Number of parts to use when `--full-qr` is on.
        #[arg(long, default_value_t = 2)]
        qr_parts: usize,
    },
    /// Scan the live UTXO set on the node for stealth outputs that the
    /// current wallet's view key can detect (ADR-011 Faz 4 RPC).
    Scan {
        #[arg(long, default_value_t = 0)]
        from: u64,
        #[arg(long, default_value_t = u64::MAX)]
        to: u64,
        #[arg(long, default_value_t = 0)]
        account: u32,
    },
    /// Sum the value of every stealth UTXO detectable by the current wallet.
    Balance {
        #[arg(long, default_value_t = 0)]
        account: u32,
    },
    /// Send to a `qvst1…` stealth address — automatically scans the node
    /// for usable UTXOs and produces a stealth-locked output (ADR-011 Faz 5).
    SendStealth {
        /// Recipient's `qvst1…` stealth address. Mutually exclusive with
        /// `--to-qvaddr` and `--to-contact`.
        #[arg(long)]
        to_address: Option<String>,
        /// Path to a `.qvaddr` file describing the recipient. Mutually
        /// exclusive with the other `--to-*` options.
        #[arg(long, conflicts_with_all = ["to_address", "to_contact"])]
        to_qvaddr: Option<PathBuf>,
        /// Contact label from the encrypted address book (saved via
        /// `qv-wallet contacts add`). Mutually exclusive with the other
        /// `--to-*` options.
        #[arg(long, conflicts_with_all = ["to_address", "to_qvaddr"])]
        to_contact: Option<String>,
        /// Amount to send (smallest units).
        #[arg(long)]
        amount: u64,
        /// Transaction fee (smallest units).
        #[arg(long, default_value_t = 1000)]
        fee: u64,
        /// Account index to spend from.
        #[arg(long, default_value_t = 0)]
        account: u32,
    },
    /// Export the wallet's view keypair + spend public key to a `.qvview`
    /// file (ADR-011 audit mode). The auditor who holds this file can run
    /// `audit-scan` against the node and see every incoming stealth
    /// payment — but cannot spend.
    ///
    /// **Never** include the mnemonic or spend secret in such a file.
    ExportViewKey {
        /// Output path (must not exist).
        #[arg(long)]
        out: PathBuf,
        /// Account to export.
        #[arg(long, default_value_t = 0)]
        account: u32,
        /// Optional human-readable label baked into the file.
        #[arg(long)]
        label: Option<String>,
    },
    /// Audit-mode stealth scan — uses a `.qvview` file to scan the node
    /// for incoming payments WITHOUT needing a keystore or spend secret.
    /// Lists matched outpoints + values.
    AuditScan {
        /// Path to a `.qvview` file produced by `export-view-key`.
        #[arg(long)]
        view_key: PathBuf,
        /// Lower bound (currently informational — node scans live set).
        #[arg(long, default_value_t = 0)]
        from: u64,
        /// Upper bound.
        #[arg(long, default_value_t = u64::MAX)]
        to: u64,
    },
    /// Selective disclosure — produce a `.qvdisclose` file that proves
    /// the wallet owns a specific stealth UTXO, optionally revealing the
    /// amount. The verifier needs only the file + own crypto code; no
    /// access to the wallet or its mnemonic required.
    Disclose {
        /// Outpoint to disclose, formatted `<tx_id_hex>:<idx>`.
        #[arg(long)]
        utxo: String,
        /// Output file path.
        #[arg(long)]
        out: PathBuf,
        /// Optional human-readable label baked into the file.
        #[arg(long)]
        label: Option<String>,
        /// Account whose view key detected this UTXO. View key drift
        /// across accounts means the wrong account will silently
        /// fail-to-find the outpoint.
        #[arg(long, default_value_t = 0)]
        account: u32,
        /// Omit to keep the amount private (binding-hash-only). Pass an
        /// explicit value to also disclose the plaintext amount.
        #[arg(long)]
        amount: Option<u64>,
    },
    /// Verify a `.qvdisclose` file end-to-end against its own embedded
    /// data (self-contained). No keystore or RPC required.
    VerifyDisclosure {
        /// Path to a `.qvdisclose` file.
        #[arg(long)]
        proof: PathBuf,
    },
    /// Address book — add/list/remove labelled stealth-address contacts.
    /// Stored encrypted alongside the keystore (Argon2id + AES-256-GCM).
    #[command(subcommand)]
    Contacts(ContactsCmd),
    /// Run the local HTTP UI server (browse to the printed URL).
    ///
    /// Two modes:
    /// * **Single-user (default)** — no `--wallets-dir`. The global
    ///   `--keystore` points at one file; everyone who reaches the
    ///   server shares that one cüzdan. Original behaviour.
    /// * **Multi-tenant** — pass `--wallets-dir <path>`. Each user
    ///   registers with a username + password; the server creates
    ///   `<wallets-dir>/<username>/wallet.json` per user. CUSTODIAL —
    ///   the server holds plaintext spend secrets in RAM while users
    ///   are logged in. Devnet/demo only.
    Serve {
        /// Bind address. Use `0.0.0.0:7777` when serving the LAN; keep
        /// the localhost default for personal use.
        #[arg(long, default_value = "127.0.0.1:7777")]
        bind: String,
        /// Enable multi-tenant mode by pointing at a per-user wallets
        /// directory. Mutually exclusive with single-user `--keystore`.
        #[arg(long)]
        wallets_dir: Option<PathBuf>,
        /// Idle session TTL in seconds (multi-tenant only). Default
        /// 3600 (1 hour). Sessions older than this auto-expire and
        /// drop their spend secrets.
        #[arg(long, default_value_t = 3600)]
        session_ttl_secs: u64,
    },
    /// Build, sign and (optionally) broadcast a transfer transaction.
    ///
    /// For devnet, the recipient is identified by their raw Dilithium
    /// public key (`--to-pubkey <hex>`). The source UTXO is given
    /// explicitly as `--input <txid_hex>:<idx>`. A future Faz 5 update
    /// will replace these with bech32m stealth addresses + automatic
    /// scan-based UTXO selection.
    Send {
        /// Hex-encoded Dilithium-Level3 public key of the recipient.
        #[arg(long)]
        to_pubkey: String,

        /// Amount to send (smallest units).
        #[arg(long)]
        amount: u64,

        /// Source UTXO as `<txid_hex>:<output_index>`.
        #[arg(long)]
        input: String,

        /// Total value of the input UTXO (smallest units). Until
        /// `qv_getUtxo` RPC is wired into the wallet, the user must
        /// supply this from external lookup. Required.
        #[arg(long)]
        input_value: u64,

        /// Account index to spend from.
        #[arg(long, default_value_t = 0)]
        account: u32,

        /// Transaction fee (smallest units). Change = input_value − amount − fee.
        #[arg(long, default_value_t = 1000)]
        fee: u64,

        /// If set, broadcast via `qv_sendTransaction` to the configured RPC
        /// endpoint. Otherwise just print the signed tx hex for manual submission.
        #[arg(long)]
        broadcast: bool,
    },
}
