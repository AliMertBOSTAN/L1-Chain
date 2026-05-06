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
    Send {
        to: String,
        amount: u64,
    },
}
