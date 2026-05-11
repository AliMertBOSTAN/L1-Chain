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
    Address {
        #[arg(default_value = "0")]
        account: u32,
    },
    Scan {
        #[arg(long)]
        from: u64,
        #[arg(long)]
        to: u64,
    },
    Balance,
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
