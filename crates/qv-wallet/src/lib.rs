//\! # qv-wallet
//\!
//\! QuantumVault CLI wallet with PQC keys, stealth address scanning, and
//\! transaction building.

#\![forbid(unsafe_code)]
#\![warn(missing_docs)]

pub mod cli;
pub mod coin_select;
pub mod hd;
pub mod keystore;
pub mod mnemonic;
pub mod rpc_client;
pub mod scanner;
pub mod tx_builder;

/// Wallet-layer error type.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("keystore error: {0}")]
    Keystore(String),
    #[error("mnemonic error: {0}")]
    Mnemonic(String),
    #[error("hd derivation error: {0}")]
    HdDerivation(String),
    #[error("tx builder error: {0}")]
    TxBuilder(String),
    #[error("coin selection error: {0}")]
    CoinSelection(String),
    #[error("scanner error: {0}")]
    Scanner(String),
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("privacy error: {0}")]
    Privacy(String),
    #[error("core error: {0}")]
    Core(String),
    #[error("script error: {0}")]
    Script(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("invalid argument: {0}")]
    InvalidArg(String),
}

/// Convenience alias for `Result<T, WalletError>`.
pub type WalletResult<T> = core::result::Result<T, WalletError>;

pub use cli::{Cli, Commands};
pub use keystore::WalletKeystore;
pub use mnemonic::Mnemonic;
pub use scanner::MatchStore;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = WalletError::Keystore("test".into());
        assert_eq\!(e.to_string(), "keystore error: test");
    }
}
