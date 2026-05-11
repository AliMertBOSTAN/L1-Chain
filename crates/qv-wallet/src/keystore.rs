//! Encrypted wallet keystore.
//!
//! On-disk format: a JSON envelope holding KDF parameters and an
//! authenticated-encryption blob.
//!
//! - **KDF**: Argon2id (memory-hard, OWASP 2023: 64 MiB memory, 3 iterations,
//!   1 lane). Derives a 32-byte symmetric key from `(password, salt)`.
//! - **Cipher**: AES-256-GCM with a fresh random 96-bit nonce per save.
//!   Returns ciphertext + 16-byte tag concatenated; we keep them together
//!   in the `ciphertext` field for simplicity (`aes_gcm::Aead::encrypt`
//!   already appends the tag).
//! - **Plaintext**: bincode-encoded `WalletSecret { mnemonic, metadata }`.
//!
//! Wrong password ⇒ AES-GCM tag mismatch ⇒ decrypt fails ⇒ caller sees
//! `WalletError::Keystore("wrong password or corrupted keystore")`.

use crate::{mnemonic::Mnemonic, WalletError, WalletResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Plaintext wallet contents — the secret material we encrypt at rest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSecret {
    /// BIP-39 mnemonic (24 words by default).
    pub mnemonic: Mnemonic,
    /// Wallet metadata (account index counter, creation time, etc.).
    pub metadata: WalletMetadata,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WalletMetadata {
    pub next_account: u32,
    pub created_at: u64,
}

/// On-disk encrypted keystore envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletKeystore {
    pub version: u32,
    pub kdf: KdfParams,
    pub cipher: CipherParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub algo: String,
    /// Argon2 parameters: `m_cost,t_cost,p_cost`.
    pub params: String,
    /// Hex-encoded random salt (16 bytes).
    pub salt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CipherParams {
    pub algo: String,
    /// Hex-encoded 96-bit (12-byte) nonce.
    pub iv: String,
    /// Hex-encoded ciphertext (includes the AES-GCM 16-byte tag).
    pub ciphertext: String,
    /// Reserved for future split formats; always empty in v1.
    pub tag: String,
}

// Argon2 parameters (OWASP 2023 minimums).
const ARGON2_MEM_KIB: u32 = 65_540; // ≈ 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
const ARGON2_KEY_BYTES: usize = 32; // AES-256
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

fn derive_key(password: &[u8], salt: &[u8]) -> WalletResult<[u8; ARGON2_KEY_BYTES]> {
    let params = argon2::Params::new(ARGON2_MEM_KIB, ARGON2_T_COST, ARGON2_P_COST, None)
        .map_err(|e| WalletError::Keystore(format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; ARGON2_KEY_BYTES];
    argon2
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| WalletError::Keystore(format!("argon2 derive: {e}")))?;
    Ok(key)
}

impl WalletKeystore {
    /// Encrypt and write a wallet secret to disk.
    pub fn save(path: &Path, secret: &WalletSecret, password: &str) -> WalletResult<()> {
        // 1. Random salt + IV.
        let mut salt = [0u8; SALT_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let mut iv = [0u8; NONCE_BYTES];
        rand::rngs::OsRng.fill_bytes(&mut iv);

        // 2. Derive 32-byte key from password.
        let key = derive_key(password.as_bytes(), &salt)?;

        // 3. Serialize plaintext.
        let plaintext = bincode::serialize(secret)
            .map_err(|e| WalletError::Keystore(format!("bincode encode: {e}")))?;

        // 4. AES-256-GCM encrypt; ciphertext includes tag.
        let cipher = Aes256Gcm::new(&key.into());
        let nonce = Nonce::from_slice(&iv);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_slice())
            .map_err(|e| WalletError::Keystore(format!("aes-gcm encrypt: {e}")))?;

        // 5. Build envelope.
        let envelope = WalletKeystore {
            version: 1,
            kdf: KdfParams {
                algo: "argon2id".into(),
                params: format!("{ARGON2_MEM_KIB},{ARGON2_T_COST},{ARGON2_P_COST}"),
                salt: hex::encode(salt),
            },
            cipher: CipherParams {
                algo: "aes-256-gcm".into(),
                iv: hex::encode(iv),
                ciphertext: hex::encode(&ciphertext),
                tag: String::new(),
            },
        };

        // 6. Write JSON.
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|e| WalletError::Keystore(format!("json encode: {e}")))?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Read and decrypt a wallet secret from disk.
    pub fn load(path: &Path, password: &str) -> WalletResult<WalletSecret> {
        // 1. Read envelope.
        let json = fs::read_to_string(path)?;
        let envelope: WalletKeystore = serde_json::from_str(&json)
            .map_err(|e| WalletError::Keystore(format!("json decode: {e}")))?;

        if envelope.version != 1 {
            return Err(WalletError::Keystore(format!(
                "unsupported keystore version {}",
                envelope.version
            )));
        }
        if envelope.kdf.algo != "argon2id" {
            return Err(WalletError::Keystore(format!(
                "unsupported KDF: {}",
                envelope.kdf.algo
            )));
        }
        if envelope.cipher.algo != "aes-256-gcm" {
            return Err(WalletError::Keystore(format!(
                "unsupported cipher: {}",
                envelope.cipher.algo
            )));
        }

        // 2. Decode salt + IV + ciphertext.
        let salt = hex::decode(&envelope.kdf.salt)
            .map_err(|e| WalletError::Keystore(format!("hex salt: {e}")))?;
        let iv = hex::decode(&envelope.cipher.iv)
            .map_err(|e| WalletError::Keystore(format!("hex iv: {e}")))?;
        if iv.len() != NONCE_BYTES {
            return Err(WalletError::Keystore(format!(
                "iv must be {NONCE_BYTES} bytes, got {}",
                iv.len()
            )));
        }
        let ciphertext = hex::decode(&envelope.cipher.ciphertext)
            .map_err(|e| WalletError::Keystore(format!("hex ciphertext: {e}")))?;

        // 3. Derive key.
        let key = derive_key(password.as_bytes(), &salt)?;

        // 4. AES-256-GCM decrypt (tag check is automatic).
        let cipher = Aes256Gcm::new(&key.into());
        let nonce = Nonce::from_slice(&iv);
        let plaintext = cipher.decrypt(nonce, ciphertext.as_slice()).map_err(|_| {
            WalletError::Keystore("wrong password or corrupted keystore".into())
        })?;

        // 5. Deserialize.
        bincode::deserialize::<WalletSecret>(&plaintext)
            .map_err(|e| WalletError::Keystore(format!("bincode decode: {e}")))
    }

    /// Re-encrypt a keystore with a new password.
    pub fn change_password(
        path: &Path,
        old_password: &str,
        new_password: &str,
    ) -> WalletResult<()> {
        let secret = Self::load(path, old_password)?;
        Self::save(path, &secret, new_password)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn fixture() -> WalletSecret {
        WalletSecret {
            mnemonic: Mnemonic::generate().unwrap(),
            metadata: WalletMetadata {
                next_account: 0,
                created_at: 1_700_000_000,
            },
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let secret = fixture();
        let file = NamedTempFile::new().unwrap();
        WalletKeystore::save(file.path(), &secret, "correct horse battery staple").unwrap();

        let loaded =
            WalletKeystore::load(file.path(), "correct horse battery staple").unwrap();
        assert_eq!(loaded.mnemonic.phrase(), secret.mnemonic.phrase());
        assert_eq!(loaded.metadata.next_account, secret.metadata.next_account);
        assert_eq!(loaded.metadata.created_at, secret.metadata.created_at);
    }

    #[test]
    fn wrong_password_rejected() {
        let secret = fixture();
        let file = NamedTempFile::new().unwrap();
        WalletKeystore::save(file.path(), &secret, "right").unwrap();

        let res = WalletKeystore::load(file.path(), "wrong");
        assert!(res.is_err(), "wrong password must not decrypt");
    }

    #[test]
    fn change_password_works() {
        let secret = fixture();
        let file = NamedTempFile::new().unwrap();
        WalletKeystore::save(file.path(), &secret, "old").unwrap();
        WalletKeystore::change_password(file.path(), "old", "new").unwrap();

        // Old password no longer works.
        assert!(WalletKeystore::load(file.path(), "old").is_err());
        // New password does.
        let loaded = WalletKeystore::load(file.path(), "new").unwrap();
        assert_eq!(loaded.mnemonic.phrase(), secret.mnemonic.phrase());
    }

    #[test]
    fn distinct_saves_produce_distinct_ciphertexts() {
        // Random salt + IV per save → ciphertexts must differ.
        let secret = fixture();
        let file_a = NamedTempFile::new().unwrap();
        let file_b = NamedTempFile::new().unwrap();
        WalletKeystore::save(file_a.path(), &secret, "pw").unwrap();
        WalletKeystore::save(file_b.path(), &secret, "pw").unwrap();

        let a = std::fs::read_to_string(file_a.path()).unwrap();
        let b = std::fs::read_to_string(file_b.path()).unwrap();
        assert_ne!(a, b);
    }
}
