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
        /// `--to-qvaddr`.
        #[arg(long)]
        to_address: Option<String>,
        /// Path to a `.qvaddr` file describing the recipient. Mutually
        /// exclusive with `--to-address`.
        #[arg(long, conflicts_with = "to_address")]
        to_qvaddr: Option<PathBuf>,
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
    /// Run the local HTTP UI server (browse to the printed URL).
    Serve {
        /// Bind address. Default 127.0.0.1:7777 — never publish to a
        /// public interface, the unlocked view key would leak.
        #[arg(long, default_value = "127.0.0.1:7777")]
        bind: String,
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
