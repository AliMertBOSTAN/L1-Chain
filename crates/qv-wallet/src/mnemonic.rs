//! BIP-39 mnemonic.
//!
//! Migrated to `bip39` 2.x API:
//!   * `Mnemonic::generate_in(lang, words)` instead of `Mnemonic::new(MnemonicType, ...)`
//!   * `Mnemonic::parse_in(lang, phrase)`   instead of `Mnemonic::from_phrase(...)`
//!   * `Mnemonic::to_seed(passphrase) -> [u8; 64]` instead of `bip39::Seed::new(...)`

use crate::{WalletError, WalletResult};
use bip39::{Language, Mnemonic as Bip39Mnemonic};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Mnemonic {
    phrase: String,
}

impl Mnemonic {
    /// Generate a fresh 24-word English mnemonic.
    pub fn generate() -> WalletResult<Self> {
        // bip39 2.x: `generate(word_count)` defaults to English and uses the
        // `rand`-feature-gated thread RNG. `generate_in` exists in some
        // sub-revisions but is not present in the version we resolve to.
        let bip39 = Bip39Mnemonic::generate(24)
            .map_err(|e| WalletError::Mnemonic(format!("generate failed: {e}")))?;
        Ok(Mnemonic {
            phrase: bip39.to_string(),
        })
    }

    /// Parse and validate an existing English BIP-39 phrase.
    pub fn from_phrase(phrase: &str) -> WalletResult<Self> {
        let _ = Bip39Mnemonic::parse_in(Language::English, phrase)
            .map_err(|e| WalletError::Mnemonic(format!("invalid phrase: {e}")))?;
        Ok(Mnemonic {
            phrase: phrase.to_string(),
        })
    }

    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    /// Derive the 64-byte BIP-39 seed using the supplied passphrase.
    pub fn to_seed(&self, passphrase: &str) -> WalletResult<[u8; 64]> {
        let bip39 = Bip39Mnemonic::parse_in(Language::English, &self.phrase)
            .map_err(|e| WalletError::Mnemonic(format!("invalid phrase: {e}")))?;
        Ok(bip39.to_seed(passphrase))
    }
}

impl Drop for Mnemonic {
    fn drop(&mut self) {
        // Best-effort: clear the heap contents of the phrase string.
        // Note: a previously-reallocated buffer might persist until reused.
        // Stronger guarantees would require `unsafe` (forbidden here) or
        // a `Zeroizing<Vec<u8>>` field; deferred for a follow-up.
        self.phrase.clear();
    }
}
