//! Encrypted operator keystore (envanter M-04).
//!
//! Stake pool operator keys (VRF, KES, cold) are derived deterministically
//! from a single 32-byte master seed via [`OperatorKeys::from_seed`]. This
//! module persists that master seed plus minimal rotation state to disk
//! under Argon2id + AES-256-GCM encryption — the same construction as
//! [`qv_wallet::keystore`].
//!
//! On-disk envelope (JSON):
//! ```json
//! {
//!   "version": 1,
//!   "kdf":   { "algo": "argon2id", "params": "65540,3,1", "salt": "<hex>" },
//!   "cipher":{ "algo": "aes-256-gcm", "iv": "<hex>", "ciphertext": "<hex>", "tag": "" }
//! }
//! ```
//! The plaintext (after decrypt) is bincode-encoded [`OperatorKeystorePlaintext`]:
//! `{ version, master_seed, kes_period }`.
//!
//! - **KDF**: Argon2id (OWASP 2023: 64 MiB memory, 3 iterations, 1 lane) →
//!   32-byte symmetric key from `(password, salt)`.
//! - **Cipher**: AES-256-GCM, fresh random 96-bit nonce per save. The
//!   16-byte authentication tag is appended to ciphertext (standard
//!   `aes_gcm` Aead behavior).
//! - **Wrong password** ⇒ AES-GCM tag mismatch ⇒ decrypt fails ⇒ caller
//!   sees `MinerError::Keystore("wrong password or corrupted keystore")`.
//!
//! KES rotation state (`kes_period`) is captured at save time so that a
//! restart resumes at the same period rather than rolling back the
//! forward-secure scheme.

use crate::{MinerError, MinerResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Plaintext payload — what we encrypt at rest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatorKeystorePlaintext {
    /// Wire format version.
    pub version: u32,
    /// 32-byte master seed; all three operator keys are derived from this
    /// via `OperatorKeys::from_seed`. Lose this → lose pool identity.
    pub master_seed: [u8; 32],
    /// Current KES period at save time. Reload re-derives keys from
    /// `master_seed` and evolves the KES this many steps to restore the
    /// forward-secure pointer.
    pub kes_period: u32,
}

/// On-disk JSON envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperatorKeystoreEnvelope {
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

// Argon2 parameters (OWASP 2023 minimums).
const ARGON2_MEM_KIB: u32 = 65_540; // ≈ 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
const ARGON2_KEY_BYTES: usize = 32; // AES-256
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const ENVELOPE_VERSION: u32 = 1;

fn derive_key(password: &[u8], salt: &[u8]) -> MinerResult<[u8; ARGON2_KEY_BYTES]> {
    let params = argon2::Params::new(ARGON2_MEM_KIB, ARGON2_T_COST, ARGON2_P_COST, None)
        .map_err(|e| MinerError::Keystore(format!("argon2 params: {e}")))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; ARGON2_KEY_BYTES];
    argon2
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| MinerError::Keystore(format!("argon2 derive: {e}")))?;
    Ok(key)
}

/// Encrypt `plaintext` under `password` and write the envelope to `path`.
pub fn save(
    path: &Path,
    plaintext: &OperatorKeystorePlaintext,
    password: &str,
) -> MinerResult<()> {
    // 1. Random salt + nonce.
    let mut salt = [0u8; SALT_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let mut iv = [0u8; NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut iv);

    // 2. Derive 32-byte key from password.
    let key = derive_key(password.as_bytes(), &salt)?;

    // 3. Serialize plaintext.
    let plaintext_bytes = bincode::serialize(plaintext)
        .map_err(|e| MinerError::Keystore(format!("bincode encode: {e}")))?;

    // 4. AES-256-GCM encrypt (tag appended).
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(&iv);
    let ciphertext = cipher
        .encrypt(nonce, plaintext_bytes.as_slice())
        .map_err(|e| MinerError::Keystore(format!("aes-gcm encrypt: {e}")))?;

    // 5. Build envelope.
    let envelope = OperatorKeystoreEnvelope {
        version: ENVELOPE_VERSION,
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
        .map_err(|e| MinerError::Keystore(format!("json encode: {e}")))?;
    fs::write(path, json).map_err(|e| MinerError::Keystore(format!("write: {e}")))?;
    Ok(())
}

/// Read and decrypt a keystore at `path` using `password`.
pub fn load(path: &Path, password: &str) -> MinerResult<OperatorKeystorePlaintext> {
    // 1. Read envelope.
    let json =
        fs::read_to_string(path).map_err(|e| MinerError::Keystore(format!("read: {e}")))?;
    let envelope: OperatorKeystoreEnvelope = serde_json::from_str(&json)
        .map_err(|e| MinerError::Keystore(format!("json decode: {e}")))?;

    if envelope.version != ENVELOPE_VERSION {
        return Err(MinerError::Keystore(format!(
            "unsupported keystore version {}",
            envelope.version
        )));
    }
    if envelope.kdf.algo != "argon2id" {
        return Err(MinerError::Keystore(format!(
            "unsupported KDF: {}",
            envelope.kdf.algo
        )));
    }
    if envelope.cipher.algo != "aes-256-gcm" {
        return Err(MinerError::Keystore(format!(
            "unsupported cipher: {}",
            envelope.cipher.algo
        )));
    }

    // 2. Decode salt + IV + ciphertext.
    let salt = hex::decode(&envelope.kdf.salt)
        .map_err(|e| MinerError::Keystore(format!("hex salt: {e}")))?;
    let iv = hex::decode(&envelope.cipher.iv)
        .map_err(|e| MinerError::Keystore(format!("hex iv: {e}")))?;
    if iv.len() != NONCE_BYTES {
        return Err(MinerError::Keystore(format!(
            "iv must be {NONCE_BYTES} bytes, got {}",
            iv.len()
        )));
    }
    let ciphertext = hex::decode(&envelope.cipher.ciphertext)
        .map_err(|e| MinerError::Keystore(format!("hex ciphertext: {e}")))?;

    // 3. Derive key.
    let key = derive_key(password.as_bytes(), &salt)?;

    // 4. AES-256-GCM decrypt (tag check is automatic).
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(&iv);
    let plaintext_bytes = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| MinerError::Keystore("wrong password or corrupted keystore".into()))?;

    // 5. Deserialize.
    bincode::deserialize::<OperatorKeystorePlaintext>(&plaintext_bytes)
        .map_err(|e| MinerError::Keystore(format!("bincode decode: {e}")))
}

/// Re-encrypt a keystore with a new password (preserves contents).
pub fn change_password(path: &Path, old: &str, new: &str) -> MinerResult<()> {
    let plaintext = load(path, old)?;
    save(path, &plaintext, new)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn fixture() -> OperatorKeystorePlaintext {
        OperatorKeystorePlaintext {
            version: ENVELOPE_VERSION,
            master_seed: [0xABu8; 32],
            kes_period: 7,
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let pt = fixture();
        let file = NamedTempFile::new().unwrap();
        save(file.path(), &pt, "correct horse battery staple").unwrap();

        let loaded = load(file.path(), "correct horse battery staple").unwrap();
        assert_eq!(loaded.version, pt.version);
        assert_eq!(loaded.master_seed, pt.master_seed);
        assert_eq!(loaded.kes_period, pt.kes_period);
    }

    #[test]
    fn wrong_password_rejected() {
        let pt = fixture();
        let file = NamedTempFile::new().unwrap();
        save(file.path(), &pt, "right").unwrap();

        let res = load(file.path(), "wrong");
        assert!(res.is_err(), "wrong password must not decrypt");
    }

    #[test]
    fn change_password_works() {
        let pt = fixture();
        let file = NamedTempFile::new().unwrap();
        save(file.path(), &pt, "old").unwrap();
        change_password(file.path(), "old", "new").unwrap();

        // Old password no longer works.
        assert!(load(file.path(), "old").is_err());
        // New password does.
        let loaded = load(file.path(), "new").unwrap();
        assert_eq!(loaded.master_seed, pt.master_seed);
        assert_eq!(loaded.kes_period, pt.kes_period);
    }

    #[test]
    fn distinct_saves_produce_distinct_ciphertexts() {
        // Random salt + IV per save → on-disk envelopes must differ even
        // for identical plaintexts.
        let pt = fixture();
        let file_a = NamedTempFile::new().unwrap();
        let file_b = NamedTempFile::new().unwrap();
        save(file_a.path(), &pt, "pw").unwrap();
        save(file_b.path(), &pt, "pw").unwrap();

        let a = std::fs::read_to_string(file_a.path()).unwrap();
        let b = std::fs::read_to_string(file_b.path()).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_unsupported_envelope_version() {
        let file = NamedTempFile::new().unwrap();
        let bogus = OperatorKeystoreEnvelope {
            version: 99,
            kdf: KdfParams {
                algo: "argon2id".into(),
                params: "65540,3,1".into(),
                salt: hex::encode([0u8; SALT_BYTES]),
            },
            cipher: CipherParams {
                algo: "aes-256-gcm".into(),
                iv: hex::encode([0u8; NONCE_BYTES]),
                ciphertext: String::new(),
                tag: String::new(),
            },
        };
        std::fs::write(file.path(), serde_json::to_string(&bogus).unwrap()).unwrap();
        let err = load(file.path(), "any").unwrap_err();
        match err {
            MinerError::Keystore(msg) => assert!(msg.contains("version")),
            other => panic!("expected Keystore version error, got {other:?}"),
        }
    }
}
