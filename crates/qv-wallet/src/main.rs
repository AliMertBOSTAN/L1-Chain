//! qv-wallet CLI entry point.
use clap::Parser;
use qv_wallet::cli::{Cli, Commands};
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
            let _path = out.unwrap_or_else(|| cli.keystore.clone());
            tracing::info!("initializing wallet");
            let mnemonic = Mnemonic::generate()?;
            println!("Mnemonic: {}", mnemonic.phrase());
            println!("SAVE THIS SECURELY!");
            let _seed = mnemonic.to_seed("")?;
            tracing::info!("wallet initialized");
        }
        Commands::ImportMnemonic { phrase, out } => {
            let _path = out.unwrap_or_else(|| cli.keystore.clone());
            let _mnemonic = Mnemonic::from_phrase(&phrase)?;
            tracing::info!("mnemonic imported");
        }
        Commands::Address { account } => {
            tracing::info!(account, "showing address");
        }
        Commands::Scan { from, to } => {
            tracing::info!(from, to, "scanning blocks");
        }
        Commands::Balance => {
            tracing::info!("showing balance");
        }
        Commands::Send { to, amount } => {
            tracing::info!(to, amount, "sending transaction");
        }
    }

    Ok(())
}
