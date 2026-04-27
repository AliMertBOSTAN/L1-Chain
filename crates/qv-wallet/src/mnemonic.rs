//\! BIP-39 mnemonic.
use crate::{WalletError, WalletResult};
use bip39::{Language, Mnemonic as Bip39Mnemonic, MnemonicType};
use zeroize::Zeroize;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Mnemonic {
    phrase: String,
}

impl Mnemonic {
    pub fn generate() -> WalletResult<Self> {
        let bip39 = Bip39Mnemonic::new(MnemonicType::Words24, Language::English);
        Ok(Mnemonic {
            phrase: bip39.phrase().to_string(),
        })
    }

    pub fn from_phrase(phrase: &str) -> WalletResult<Self> {
        let _bip39 = Bip39Mnemonic::from_phrase(phrase, Language::English)
            .map_err(|e| WalletError::Mnemonic(format\!("invalid phrase: {}", e)))?;
        Ok(Mnemonic {
            phrase: phrase.to_string(),
        })
    }

    pub fn phrase(&self) -> &str {
        &self.phrase
    }

    pub fn to_seed(&self, passphrase: &str) -> WalletResult<[u8; 64]> {
        let bip39 = Bip39Mnemonic::from_phrase(&self.phrase, Language::English)
            .map_err(|e| WalletError::Mnemonic(format\!("invalid phrase: {}", e)))?;
        let seed = bip39::Seed::new(&bip39, passphrase);
        let bytes = seed.as_bytes();
        if bytes.len() \!= 64 {
            return Err(WalletError::Mnemonic("seed length mismatch".into()));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(bytes);
        Ok(arr)
    }
}

impl Drop for Mnemonic {
    fn drop(&mut self) {
        self.phrase.zeroize();
    }
}
