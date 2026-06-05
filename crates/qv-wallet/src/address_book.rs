//! Encrypted address book (contacts) — labeled stealth-address aliases.
//!
//! QuantumVault stealth addresses are ~6.4 KB hex strings; pasting one into
//! a Send form every time is awful UX. This module lets the wallet store a
//! local map `{ label → qvst1… }` so the user can `send-stealth --to-contact
//! alice` (CLI) or pick `alice` from a dropdown (UI) instead.
//!
//! # Security
//!
//! The book reveals *who* the wallet transacts with — sensitive metadata
//! even if it doesn't leak funds. We therefore encrypt it with the same
//! Argon2id-derived AES-256-GCM scheme as the keystore (see
//! [`crate::keystore`]), independent envelope, persisted alongside the
//! keystore as `<keystore_path>.contacts`.
//!
//! Layout: salt (Argon2id) + nonce (AES-GCM) + ciphertext+tag of
//! `bincode(ContactsBook)`.

use crate::{WalletError, WalletResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

// Argon2id + AES-GCM constants — match `keystore.rs` so cracking the
// contacts file is exactly as hard as cracking the wallet itself.
const ARGON2_MEM_KIB: u32 = 65_540; // ≈ 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
const ARGON2_KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

/// File-format identifier for forwards-compatibility checks.
pub const QVCONTACTS_FORMAT_V1: &str = "qvcontacts-v1";

/// One entry in the address book.
///
/// `label` lives outside this struct (it's the map key); `address` is the
/// full `qvst1…` form. `fingerprint` is denormalised for fast display.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Contact {
    /// Full stealth address (`qvst1…`).
    pub address: String,
    /// Short fingerprint of `address` (`qvfp1…`) — convenience cache so
    /// the UI doesn't re-derive it on every render.
    pub fingerprint: String,
    /// Free-text note ("met at devcon", "vendor 2026 Q2", ...).
    #[serde(default)]
    pub notes: Option<String>,
    /// Unix seconds when this contact was first added.
    pub added_at: u64,
}

/// In-memory address book. Lives unencrypted only while the wallet is
/// unlocked and the user is editing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContactsBook {
    /// File-format tag — checked on load to reject future-incompatible
    /// payloads early.
    #[serde(default = "default_format_tag")]
    pub format: String,
    /// Labels are case-sensitive and unique; BTreeMap gives sorted
    /// iteration for deterministic CLI / UI output.
    #[serde(default)]
    pub contacts: BTreeMap<String, Contact>,
}

fn default_format_tag() -> String {
    QVCONTACTS_FORMAT_V1.to_string()
}

impl ContactsBook {
    /// Construct an empty book tagged with the current format.
    #[must_use]
    pub fn new() -> Self {
        Self {
            format: QVCONTACTS_FORMAT_V1.to_string(),
            contacts: BTreeMap::new(),
        }
    }

    /// Add a contact. Rejects duplicate labels and validates the address
    /// is a parseable `qvst1…` form.
    pub fn add(&mut self, label: &str, address: &str, notes: Option<String>) -> WalletResult<()> {
        let label = label.trim();
        if label.is_empty() {
            return Err(WalletError::InvalidArg("label cannot be empty".into()));
        }
        if self.contacts.contains_key(label) {
            return Err(WalletError::InvalidArg(format!(
                "contact `{label}` already exists — remove it first"
            )));
        }
        // Validate the address by attempting a decode. Avoid storing a
        // garbled string the user could never send to.
        let parsed = crate::address::decode_address(address)?;
        let fp = crate::address::fingerprint(&parsed);
        self.contacts.insert(
            label.to_string(),
            Contact {
                address: address.to_string(),
                fingerprint: fp,
                notes,
                added_at: now_unix_secs(),
            },
        );
        Ok(())
    }

    /// Remove a contact by label. Returns the removed entry.
    pub fn remove(&mut self, label: &str) -> WalletResult<Contact> {
        self.contacts
            .remove(label)
            .ok_or_else(|| WalletError::InvalidArg(format!("no contact named `{label}`")))
    }

    /// Look up a contact by label.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&Contact> {
        self.contacts.get(label)
    }

    /// Number of contacts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.contacts.len()
    }

    /// True iff there are no contacts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    /// Iterate `(label, contact)` pairs in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Contact)> {
        self.contacts.iter()
    }
}

// ---------------------------------------------------------------------------
// Encrypted persistence
// ---------------------------------------------------------------------------

/// Compute the on-disk path for a wallet's address book given its
/// keystore path. We append `.contacts` so `wallet.json` →
/// `wallet.json.contacts`. This keeps the pair obvious on disk.
#[must_use]
pub fn contacts_path_for(keystore_path: &Path) -> PathBuf {
    let mut s = keystore_path.as_os_str().to_owned();
    s.push(".contacts");
    PathBuf::from(s)
}

/// On-disk envelope. Same Argon2id + AES-256-GCM scheme as the keystore;
/// see `crate::keystore` for the rationale.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Envelope {
    version: u32,
    salt: String,       // hex, 16 bytes
    iv: String,         // hex, 12 bytes
    ciphertext: String, // hex, body+tag
}

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

/// Save the book to disk, encrypted under `password`.
pub fn save(path: &Path, book: &ContactsBook, password: &str) -> WalletResult<()> {
    let mut salt = [0u8; SALT_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let mut iv = [0u8; NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut iv);

    let key = derive_key(password.as_bytes(), &salt)?;
    let plaintext = bincode::serialize(book)
        .map_err(|e| WalletError::Keystore(format!("bincode encode: {e}")))?;
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(&iv);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| WalletError::Keystore(format!("aes-gcm encrypt: {e}")))?;

    let envelope = Envelope {
        version: 1,
        salt: hex::encode(salt),
        iv: hex::encode(iv),
        ciphertext: hex::encode(&ciphertext),
    };
    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| WalletError::Keystore(format!("json encode: {e}")))?;
    fs::write(path, json)?;
    Ok(())
}

/// Load and decrypt a book from disk. Returns `Ok(empty)` if the file
/// doesn't exist — that's the "no contacts yet" state.
pub fn load_or_empty(path: &Path, password: &str) -> WalletResult<ContactsBook> {
    if !path.exists() {
        return Ok(ContactsBook::new());
    }
    let json = fs::read_to_string(path)?;
    let envelope: Envelope = serde_json::from_str(&json)
        .map_err(|e| WalletError::Keystore(format!("json decode: {e}")))?;
    if envelope.version != 1 {
        return Err(WalletError::Keystore(format!(
            "unsupported contacts version {}",
            envelope.version
        )));
    }
    let salt =
        hex::decode(&envelope.salt).map_err(|e| WalletError::Keystore(format!("hex salt: {e}")))?;
    let iv =
        hex::decode(&envelope.iv).map_err(|e| WalletError::Keystore(format!("hex iv: {e}")))?;
    if iv.len() != NONCE_BYTES {
        return Err(WalletError::Keystore(format!(
            "iv must be {NONCE_BYTES} bytes, got {}",
            iv.len()
        )));
    }
    let ciphertext = hex::decode(&envelope.ciphertext)
        .map_err(|e| WalletError::Keystore(format!("hex ciphertext: {e}")))?;
    let key = derive_key(password.as_bytes(), &salt)?;
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(&iv);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| WalletError::Keystore("wrong password or corrupted contacts file".into()))?;
    let book: ContactsBook = bincode::deserialize(&plaintext)
        .map_err(|e| WalletError::Keystore(format!("bincode decode: {e}")))?;
    if book.format != QVCONTACTS_FORMAT_V1 {
        return Err(WalletError::Keystore(format!(
            "unsupported contacts payload format `{}` (expected `{}`)",
            book.format, QVCONTACTS_FORMAT_V1
        )));
    }
    Ok(book)
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_crypto::{DilithiumLevel, KyberLevel};
    use qv_privacy::StealthKeys;
    use tempfile::tempdir;

    fn fresh_qvst1() -> String {
        let addr = StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3)
            .unwrap()
            .address();
        crate::address::encode_address(&addr).unwrap()
    }

    #[test]
    fn add_get_remove_roundtrip() {
        let mut book = ContactsBook::new();
        let alice = fresh_qvst1();
        book.add("alice", &alice, Some("first contact".into())).unwrap();
        assert_eq!(book.len(), 1);
        let c = book.get("alice").unwrap();
        assert_eq!(c.address, alice);
        assert!(c.fingerprint.starts_with("qvfp1"));
        assert_eq!(c.notes.as_deref(), Some("first contact"));

        let removed = book.remove("alice").unwrap();
        assert_eq!(removed.address, alice);
        assert!(book.is_empty());
    }

    #[test]
    fn add_rejects_empty_label() {
        let mut book = ContactsBook::new();
        let err = book.add("   ", &fresh_qvst1(), None).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn add_rejects_duplicate_label() {
        let mut book = ContactsBook::new();
        let addr = fresh_qvst1();
        book.add("alice", &addr, None).unwrap();
        let err = book.add("alice", &addr, None).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn add_rejects_unparseable_address() {
        let mut book = ContactsBook::new();
        let err = book.add("bad", "not-a-real-address", None).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn save_load_roundtrip_encrypted() {
        let mut book = ContactsBook::new();
        let alice = fresh_qvst1();
        let bob = fresh_qvst1();
        book.add("alice", &alice, None).unwrap();
        book.add("bob", &bob, Some("vendor".into())).unwrap();

        let dir = tempdir().unwrap();
        let path = dir.path().join("wallet.json.contacts");
        save(&path, &book, "pw1234567").unwrap();

        // Wrong password rejected.
        let err = load_or_empty(&path, "wrong-pw").unwrap_err();
        assert!(matches!(err, WalletError::Keystore(_)));

        // Right password recovers identical book.
        let back = load_or_empty(&path, "pw1234567").unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.get("alice").unwrap().address, alice);
        assert_eq!(back.get("bob").unwrap().address, bob);
        assert_eq!(back.get("bob").unwrap().notes.as_deref(), Some("vendor"));
    }

    #[test]
    fn load_empty_for_nonexistent_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.contacts");
        let book = load_or_empty(&path, "anything").unwrap();
        assert!(book.is_empty());
    }

    #[test]
    fn distinct_saves_produce_distinct_ciphertexts() {
        // Random salt + IV per save → identical books still encrypt to
        // different bytes (forward secrecy on disk).
        let mut book = ContactsBook::new();
        book.add("alice", &fresh_qvst1(), None).unwrap();
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.contacts");
        let b = dir.path().join("b.contacts");
        save(&a, &book, "pw").unwrap();
        save(&b, &book, "pw").unwrap();
        let sa = fs::read_to_string(&a).unwrap();
        let sb = fs::read_to_string(&b).unwrap();
        assert_ne!(sa, sb);
    }

    #[test]
    fn contacts_path_for_appends_dot_contacts() {
        let p = contacts_path_for(Path::new("/tmp/wallet.json"));
        assert_eq!(p, PathBuf::from("/tmp/wallet.json.contacts"));
    }
}
