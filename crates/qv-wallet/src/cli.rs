//! CLI using clap.
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Tek satırlık `--version` çıktısı. Üç parça birleşik:
///   `<cargo_pkg_version> (<build_tag>, git <short_hash>)`
/// build.rs `QV_GIT_HASH`/`QV_BUILD_TAG` rustc-env'lerini doldurur.
pub const VERSION_STRING: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("QV_BUILD_TAG"),
    ", git ",
    env!("QV_GIT_HASH"),
    ")",
);

/// Resmi public devnet RPC endpoint'i. `--network devnet` alias'ı bu URL'ye
/// çözülür. Operator değişimi olursa burayı + INSTALL.md'yi güncellemek
/// yeterli.
pub const PUBLIC_DEVNET_RPC_URL: &str = "https://rpc.testnet.quantumvault.example";

/// Yerel single-node devnet için varsayılan URL — `run-single.ps1`
/// ve `run-all.ps1` script'lerinin başlattığı node bu adresi dinler.
pub const LOCAL_DEVNET_RPC_URL: &str = "http://127.0.0.1:8545";

/// Bootstrap modunda `--network` flag'i, kullanıcının uzun bir URL
/// yazmadan iyi-bilinen bir endpoint'e bağlanmasını sağlar.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum NetworkAlias {
    /// Resmi public devnet RPC ([`PUBLIC_DEVNET_RPC_URL`]).
    Devnet,
    /// Yerel single-node devnet ([`LOCAL_DEVNET_RPC_URL`]).
    Local,
}

impl NetworkAlias {
    /// Alias'a karşılık gelen RPC URL'i.
    #[must_use]
    pub fn rpc_url(self) -> &'static str {
        match self {
            NetworkAlias::Devnet => PUBLIC_DEVNET_RPC_URL,
            NetworkAlias::Local => LOCAL_DEVNET_RPC_URL,
        }
    }
}

#[derive(Parser, Debug)]
#[command(version = VERSION_STRING, about = "QuantumVault CLI wallet")]
pub struct Cli {
    #[arg(long, default_value = "wallet.json", global = true)]
    pub keystore: PathBuf,
    /// Doğrudan node JSON-RPC URL'i. `--network` ile birlikte verilirse
    /// `--rpc` üstünden gelir.
    #[arg(long, default_value = "http://localhost:8080", global = true)]
    pub rpc: String,
    /// İyi-bilinen ağa kısa-yol. Set edilirse `--rpc` değerinin yerine
    /// karşılık gelen URL kullanılır. (`devnet` = public, `local` = 127.0.0.1:8545)
    #[arg(long, global = true, value_enum)]
    pub network: Option<NetworkAlias>,
    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    /// Etkin RPC URL'i. `--network` öncelikli, yoksa `--rpc`.
    #[must_use]
    pub fn effective_rpc_url(&self) -> String {
        self.network
            .map(|n| n.rpc_url().to_string())
            .unwrap_or_else(|| self.rpc.clone())
    }
}

/// `swap --direction` flag: which pool token the user sells.
///
/// Maps 1:1 onto `qv_defi::SwapDirection`; the conversion lives in
/// `crate::swap::direction_from_arg` (this module stays clap-only).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapDirectionArg {
    /// Sell token A, receive token B (`a-to-b`).
    AToB,
    /// Sell token B, receive token A (`b-to-a`).
    BToA,
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
    /// Swap against an on-chain AMM pool UTXO (Faz 6 / D-4, devnet).
    ///
    /// Fetches the pool UTXO (canonical datum + locking script) via
    /// `qv_getUtxo`, computes the constant-product output, builds the
    /// covenant transaction (`qv-defi::build_swap_tx` — pool input #0
    /// witness-less, user input #1 signed by this wallet), and optionally
    /// broadcasts it. Token accounting is currently **datum-level** (the
    /// pool's reserves move inside its datum); the user's funding input
    /// only needs to cover the network `--fee`, and change plus swap
    /// proceeds are paid to the account's own plain p2pkh output.
    Swap {
        /// Pool UTXO outpoint as `<txid_hex>#<idx>` or `<txid_hex>:<idx>`.
        #[arg(long)]
        pool: String,

        /// Which pool token you are selling.
        #[arg(long, value_enum)]
        direction: SwapDirectionArg,

        /// Amount of the input token to sell (smallest units).
        #[arg(long)]
        amount: u64,

        /// Slippage floor: minimum acceptable output amount. The swap is
        /// rejected locally if the computed output falls below this.
        #[arg(long)]
        min_receive: u64,

        /// Funding UTXO as `<txid_hex>:<idx>` (or `#`). If omitted, the
        /// wallet auto-selects the smallest sufficient plain p2pkh UTXO
        /// of the account via `qv_scanP2pkh`.
        #[arg(long)]
        input: Option<String>,

        /// Total value of `--input` (smallest units). Only meaningful
        /// together with `--input`; if omitted there, the wallet resolves
        /// the value via `qv_getUtxo`.
        #[arg(long, requires = "input")]
        input_value: Option<u64>,

        /// Account index to spend from.
        #[arg(long, default_value_t = 0)]
        account: u32,

        /// Transaction fee (smallest units). Change = input_value − fee.
        #[arg(long, default_value_t = 1000)]
        fee: u64,

        /// If set, broadcast via `qv_sendTransaction` to the configured RPC
        /// endpoint. Otherwise just print the signed tx hex for manual submission.
        #[arg(long)]
        broadcast: bool,
    },
    /// Bootstrap a brand-new on-chain AMM pool UTXO (Faz 6 / D-5, devnet).
    ///
    /// Builds the genesis pool transaction: output #0 is the pool UTXO
    /// (locked by `amm_pool_lock(token_a, token_b, fee_bps)`, carrying the
    /// canonical `PoolDatum` with the initial reserves and
    /// `lp_total = ⌊sqrt(reserve_a · reserve_b)⌋`), output #1 is your
    /// change. The funding input must cover `--pool-value` + `--fee`.
    ///
    /// **Scope note:** LP shares are tracked only as the datum's
    /// `lp_total` — there is no on-chain LP token, and add/remove-liquidity
    /// spend paths are not implemented yet (D-6+). The pool covenant's
    /// `x·y ≥ k` check would admit add-liquidity-shaped transitions but
    /// can never admit remove-liquidity, so locked reserves cannot be
    /// withdrawn until a dedicated spend path ships.
    CreatePool {
        /// Token A identifier — 32-byte hex (64 chars).
        #[arg(long)]
        token_a: String,

        /// Token B identifier — 32-byte hex (64 chars).
        #[arg(long)]
        token_b: String,

        /// Swap fee in basis points (0..=10000), e.g. 30 = 0.3%.
        #[arg(long)]
        fee_bps: u16,

        /// Initial reserve of token A (smallest units, datum-level).
        #[arg(long)]
        reserve_a: u64,

        /// Initial reserve of token B (smallest units, datum-level).
        #[arg(long)]
        reserve_b: u64,

        /// Native value to lock into the pool UTXO. Carried through every
        /// subsequent swap unchanged.
        #[arg(long, default_value_t = 1000)]
        pool_value: u64,

        /// Funding UTXO as `<txid_hex>:<idx>` (or `#`). If omitted, the
        /// wallet auto-selects the smallest sufficient plain p2pkh UTXO
        /// of the account via `qv_scanP2pkh`.
        #[arg(long)]
        input: Option<String>,

        /// Total value of `--input` (smallest units). Only meaningful
        /// together with `--input`; if omitted there, the wallet resolves
        /// the value via `qv_getUtxo`.
        #[arg(long, requires = "input")]
        input_value: Option<u64>,

        /// Account index to spend from.
        #[arg(long, default_value_t = 0)]
        account: u32,

        /// Transaction fee (smallest units).
        /// Change = input_value − pool_value − fee.
        #[arg(long, default_value_t = 1000)]
        fee: u64,

        /// If set, broadcast via `qv_sendTransaction` to the configured RPC
        /// endpoint. Otherwise just print the signed tx hex for manual submission.
        #[arg(long)]
        broadcast: bool,
    },
}
