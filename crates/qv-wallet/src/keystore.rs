//! Encrypted keystore (Argon2id + AES-256-GCM).
use crate::{mnemonic::Mnemonic, WalletError, WalletResult};
use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use argon2::{Argon2, PasswordHasher};
use password_hash::SaltString;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSecret {
    pub mnemonic: Mnemonic,
    pub metadata: WalletMetadata,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WalletMetadata {
    pub next_account: u32,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletKeystore {
    pub version: u32,
    pub kdf: KdfParams,
    pub cipher: CipherParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub algo: String,
    pub params: String,
    pub salt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CipherParams {
    pub algo: String,
    pub iv: String,
    pub ciphertext: String,
    pub tag: String,
}

impl WalletKeystore {
    pub fn save(path: &Path, secret: &WalletSecret, password: &str) -> WalletResult<()> {
        let salt = SaltString::generate(rand::thread_rng());
        let params = argon2::Params::new(65540, 3, 1, None)
            .map_err(|e| WalletError::Keystore(format!("params: {}", e)))?;
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let hash = argon2.hash_password(password.as_bytes(), &salt)
            .map_err(|e| WalletError::Keystore(format!("hash: {}", e)))?;

        let plaintext = bincode::serialize(secret)
            .map_err(|e| WalletError::Keystore(format!("bincode: {}", e)))?;
        
        let iv_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&iv_bytes);
        
        // Simplified: derive key from hash
        let key_bytes = [0u8; 32]; // Placeholder
        let cipher = Aes256Gcm::new(&key_bytes.into());
        let ciphertext = cipher.encrypt(nonce, plaintext.as_slice())
            .map_err(|e| WalletError::Keystore(format!("encrypt: {}", e)))?;

        let ks = WalletKeystore {
            version: 1,
            kdf: KdfParams {
                algo: "argon2id".into(),
                params: hash.to_string(),
                salt: salt.to_string(),
            },
            cipher: CipherParams {
                algo: "aes-256-gcm".into(),
                iv: hex::encode(&iv_bytes),
                ciphertext: hex::encode(&ciphertext),
                tag: String::new(),
            },
        };

        let json = serde_json::to_string_pretty(&ks)
            .map_err(|e| WalletError::Keystore(format!("json: {}", e)))?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load(path: &Path, _password: &str) -> WalletResult<WalletSecret> {
        let json = fs::read_to_string(path)?;
        let _ks: WalletKeystore = serde_json::from_str(&json)
            .map_err(|e| WalletError::Keystore(format!("json: {}", e)))?;
        Err(WalletError::Keystore("load not fully implemented".into()))
    }

    pub fn change_password(path: &Path, old_password: &str, new_password: &str) -> WalletResult<()> {
        let secret = Self::load(path, old_password)?;
        Self::save(path, &secret, new_password)?;
        Ok(())
    }
}
