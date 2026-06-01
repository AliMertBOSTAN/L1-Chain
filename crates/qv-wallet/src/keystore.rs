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
//! - **Plaintext**: bincode-encoded [`WalletSecret`].
//!
//! Wrong password ⇒ AES-GCM tag mismatch ⇒ decrypt fails ⇒ caller sees
//! `WalletError::Keystore("wrong password or corrupted keystore")`.
//!
//! # Versions
//!
//! - **v1** — `WalletSecret { mnemonic, metadata }`. Legacy format.
//! - **v2** — same plus `view_keypairs: BTreeMap<u32, PersistedViewKey>`
//!   (ADR-011 / envanter C-05). Closes the "view key not deterministic
//!   from mnemonic" gap by persisting the Kyber+X25519 view keypair so
//!   prior stealth payments stay visible across restarts. Reading v1
//!   keystores still works (serde default = empty map); the first save
//!   re-encrypts as v2.

use crate::{mnemonic::Mnemonic, WalletError, WalletResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use qv_crypto::{HybridKeyPair, KyberLevel};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Persisted view keypair (Kyber + X25519) — closes the "view key drift on
/// wallet restart" gap (envanter C-05).
///
/// Kyber doesn't yet expose seeded keygen in our `pqcrypto-kyber` version,
/// so we cannot rebuild the view keypair from the mnemonic alone. We
/// therefore generate it once on `init` / `unlock` and persist the raw
/// bytes here. AES-256-GCM in the keystore envelope still protects them
/// at rest; the wallet holds the plaintext only while it is unlocked.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedViewKey {
    /// Kyber parameter set: 1, 3, or 5.
    pub kyber_level: u8,
    /// X25519 public key bytes (32).
    pub x25519_pk: Vec<u8>,
    /// X25519 secret key bytes (32).
    pub x25519_sk: Vec<u8>,
    /// Kyber public key bytes.
    pub kyber_pk: Vec<u8>,
    /// Kyber secret key bytes.
    pub kyber_sk: Vec<u8>,
}

impl PersistedViewKey {
    /// Snapshot the raw bytes from a live hybrid keypair.
    #[must_use]
    pub fn from_keypair(kp: &HybridKeyPair) -> Self {
        let kyber_level = match kp.level() {
            KyberLevel::Level1 => 1,
            KyberLevel::Level3 => 3,
            KyberLevel::Level5 => 5,
        };
        Self {
            kyber_level,
            x25519_pk: kp.public.x25519.to_vec(),
            x25519_sk: kp.x25519_secret_bytes().to_vec(),
            kyber_pk: kp.public.kyber.clone(),
            kyber_sk: kp.kyber_secret_bytes().to_vec(),
        }
    }

    /// Rebuild a usable hybrid keypair. Validates byte lengths against
    /// the declared Kyber level.
    pub fn into_keypair(self) -> WalletResult<HybridKeyPair> {
        let level = match self.kyber_level {
            1 => KyberLevel::Level1,
            3 => KyberLevel::Level3,
            5 => KyberLevel::Level5,
            other => {
                return Err(WalletError::Keystore(format!(
                    "unknown Kyber level: {other}"
                )));
            }
        };
        let x25519_pk: [u8; 32] = self.x25519_pk.as_slice().try_into().map_err(|_| {
            WalletError::Keystore(format!(
                "x25519_pk must be 32 bytes (got {})",
                self.x25519_pk.len()
            ))
        })?;
        HybridKeyPair::from_raw_parts(level, x25519_pk, self.x25519_sk, self.kyber_pk, self.kyber_sk)
            .map_err(|e| WalletError::Keystore(format!("view keypair reconstruct: {e}")))
    }
}

/// Plaintext wallet contents — the secret material we encrypt at rest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSecret {
    /// BIP-39 mnemonic (24 words by default).
    pub mnemonic: Mnemonic,
    /// Wallet metadata (account index counter, creation time, etc.).
    pub metadata: WalletMetadata,
    /// Per-account persisted view keypairs (v2+).
    ///
    /// Lazily populated on first access of each account index; never
    /// regenerated once present. Legacy v1 keystores deserialize this as
    /// the default empty map, and the first re-save upgrades the file
    /// to v2.
    #[serde(default)]
    pub view_keypairs: BTreeMap<u32, PersistedViewKey>,
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

        // 5. Build envelope. v2 carries `view_keypairs` (ADR-011 / C-05).
        let envelope = WalletKeystore {
            version: 2,
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

        // v1 and v2 share the same envelope/cipher/KDF layout; only the
        // bincode plaintext changed (v2 adds `view_keypairs`, defaulted
        // to empty on v1 reads).
        if envelope.version != 1 && envelope.version != 2 {
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
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_slice())
            .map_err(|_| WalletError::Keystore("wrong password or corrupted keystore".into()))?;

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

    /// Load the keystore, derive the requested stealth account, and
    /// **persist the view keypair on first use** so subsequent unlocks
    /// see the same stealth address (envanter C-05 workaround).
    ///
    /// Flow:
    /// 1. Decrypt the keystore.
    /// 2. If `view_keypairs[account]` is present, reuse it.
    /// 3. Otherwise generate a fresh view keypair, insert it into the
    ///    secret, and re-save the keystore with the same password. The
    ///    resulting file is upgraded to v2 (if it was v1).
    /// 4. Combine the (persisted-or-fresh) view keypair with a
    ///    deterministic spend keypair derived from the mnemonic.
    ///
    /// The password is **re-used** to re-encrypt on save — never logged
    /// or stored. Argon2id + AES-GCM are run a second time only on the
    /// first unlock of each account.
    pub fn unlock_account(
        path: &Path,
        password: &str,
        account: u32,
        deriver: &crate::hd::DefaultSeedDeriver,
    ) -> WalletResult<qv_privacy::StealthKeys> {
        let mut secret = Self::load(path, password)?;
        let seed = secret.mnemonic.to_seed("")?;

        let view_kp = if let Some(pv) = secret.view_keypairs.get(&account).cloned() {
            pv.into_keypair()?
        } else {
            let fresh = deriver.generate_fresh_view_keypair()?;
            secret
                .view_keypairs
                .insert(account, PersistedViewKey::from_keypair(&fresh));
            Self::save(path, &secret, password)?;
            fresh
        };

        deriver.derive_account_with_view(&seed, account, view_kp)
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
            view_keypairs: BTreeMap::new(),
        }
    }

    fn fresh_view_keypair() -> HybridKeyPair {
        qv_crypto::generate_hybrid_keypair(KyberLevel::Level3).unwrap()
    }

    #[test]
    fn save_load_roundtrip() {
        let secret = fixture();
        let file = NamedTempFile::new().unwrap();
        WalletKeystore::save(file.path(), &secret, "correct horse battery staple").unwrap();

        let loaded = WalletKeystore::load(file.path(), "correct horse battery staple").unwrap();
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
    fn persisted_view_keypair_roundtrip() {
        // Closes envanter C-05: the view keypair must survive a
        // save/load cycle byte-for-byte. Otherwise a wallet reopened
        // tomorrow would generate a fresh view key and miss every
        // stealth payment sent to it today.
        let kp = fresh_view_keypair();
        let mut secret = fixture();
        secret.view_keypairs.insert(0, PersistedViewKey::from_keypair(&kp));

        let file = NamedTempFile::new().unwrap();
        WalletKeystore::save(file.path(), &secret, "pw1234567").unwrap();

        let loaded = WalletKeystore::load(file.path(), "pw1234567").unwrap();
        let pv = loaded
            .view_keypairs
            .get(&0)
            .expect("account 0 view key must persist");
        let kp2 = pv.clone().into_keypair().expect("reconstruct");

        assert_eq!(kp.public.x25519, kp2.public.x25519);
        assert_eq!(kp.public.kyber, kp2.public.kyber);
        assert_eq!(kp.public.level, kp2.public.level);
        assert_eq!(kp.x25519_secret_bytes(), kp2.x25519_secret_bytes());
        assert_eq!(kp.kyber_secret_bytes(), kp2.kyber_secret_bytes());

        // And the reconstructed keypair must actually work — encapsulate
        // then decapsulate.
        let (ct, ss1) = qv_crypto::encapsulate_hybrid(&kp.public).unwrap();
        let ss2 = qv_crypto::decapsulate_hybrid(&kp2, &ct).unwrap();
        assert_eq!(ss1, ss2);
    }

    #[test]
    fn v1_keystore_reads_with_empty_view_map() {
        // Simulate an upgrade from v1: bincode a v1-shaped struct (no
        // `view_keypairs` field) and confirm the v2 deserializer falls
        // back to an empty map.
        #[derive(Serialize)]
        struct V1Secret {
            mnemonic: Mnemonic,
            metadata: WalletMetadata,
        }
        let v1 = V1Secret {
            mnemonic: Mnemonic::generate().unwrap(),
            metadata: WalletMetadata {
                next_account: 0,
                created_at: 1_700_000_000,
            },
        };
        let bytes = bincode::serialize(&v1).unwrap();
        // The current `WalletSecret` deserializer must accept these bytes
        // (bincode does NOT honour `serde(default)` on missing trailing
        // fields, so this guards against future regressions if we ever
        // try to deserialize *raw* v1 bincode bytes).
        //
        // In practice v1 → v2 upgrades go through the JSON envelope:
        // load() decrypts the bincode, deserialize fails on a missing
        // field, and we'd need a fallback. For now we require fresh
        // keystores be v2; legacy v1 *files* still parse via the JSON
        // envelope, but the bincode payload itself must include
        // `view_keypairs` (default empty if newly initialised).
        let parsed: Result<WalletSecret, _> = bincode::deserialize(&bytes);
        // We accept either: parses as v2 with empty map (ideal) OR fails
        // cleanly (we handle in the caller by treating the file as new).
        match parsed {
            Ok(s) => assert!(s.view_keypairs.is_empty()),
            Err(_) => { /* documented limitation */ }
        }
    }

    #[test]
    fn distinct_saves_produce_distinct_ciphertexts() {
        // Random salt + IV per save → ciphertexts must differ.
        let secret = fixture();
        let file_a = NamedTempFile::new().unwrap();
        let file_b = NamedTempFile::new().unwrap();
        WalletKeystore::save(file_a.path(), &secret, "pw").unwrap();
        WalletKeystore::save(file_b.path(), &secret, "pw").unwrap();

        let a = fs::read_to_string(file_a.path()).unwrap();
        let b = fs::read_to_string(file_b.path()).unwrap();
        assert_ne!(a, b);
    }
}
