//! Encrypted local transaction history journal.
//!
//! The chain itself is privacy-preserving by design — stealth outputs
//! are unlinkable on-chain, so we can never reconstruct a wallet's full
//! transaction history just by scanning. The wallet therefore keeps a
//! **local-only**, encrypted journal of:
//!
//! * **Sent** transfers — appended at the moment of broadcast (so we always
//!   remember "I paid Bob 100 units last Tuesday" even after the change
//!   UTXO is itself spent).
//! * **Received** notes — derived live from `qv_scanStealth` and
//!   `qv_scanP2pkh`; not persisted (they're observable on-chain by us
//!   already, and re-deriving them avoids out-of-sync state when the chain
//!   reorgs).
//!
//! # Security
//!
//! Same Argon2id + AES-256-GCM envelope as [`crate::keystore`] and
//! [`crate::address_book`]. The journal reveals counterparties and amounts
//! — sensitive metadata even if it cannot move funds — so cracking it must
//! cost the same as cracking the wallet itself.
//!
//! On-disk path: `<keystore>.history` (so `wallet.json` →
//! `wallet.json.history`). Missing file ⇒ empty journal (the "fresh
//! wallet" state).

use crate::{WalletError, WalletResult};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// Argon2id + AES-GCM constants — match `keystore.rs` and
// `address_book.rs` so a single wallet password protects every encrypted
// sidecar with identical strength.
const ARGON2_MEM_KIB: u32 = 65_540; // ≈ 64 MiB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 1;
const ARGON2_KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

/// File-format identifier for forwards-compatibility checks.
pub const QVHISTORY_FORMAT_V1: &str = "qvhistory-v1";

/// Direction of a journal entry.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A transaction we initiated and broadcast.
    Sent,
    /// A receipt observed via scan. Synthesised live; not persisted in
    /// the journal — kept here so the merged-view DTO can encode both
    /// kinds uniformly.
    Received,
}

/// One row in the local history journal.
///
/// Persisted entries always have `kind = Sent`. `Received` entries are
/// produced on demand by [`merge_with_received`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Send vs receive.
    pub kind: EntryKind,
    /// Hex-encoded transaction id.
    pub tx_id: String,
    /// Output index (`Received` only). Sent rows leave this `None` —
    /// a single broadcast typically has multiple outputs and we record the
    /// transaction as a whole.
    #[serde(default)]
    pub output_index: Option<u32>,
    /// Unix seconds at the moment the entry was written.
    pub timestamp: u64,
    /// Headline amount (smallest units).
    /// * `Sent` — value paid to the recipient (excludes change + fee).
    /// * `Received` — `value` field of the matched stealth/plain output.
    pub amount: u64,
    /// Sent rows only — the transaction fee in smallest units.
    #[serde(default)]
    pub fee: Option<u64>,
    /// Stealth-address fingerprint of the counterparty when known.
    /// * `Sent` — recipient fingerprint, always populated.
    /// * `Received` — `None`. Stealth payments hide the sender by design.
    #[serde(default)]
    pub counterparty_fingerprint: Option<String>,
    /// Optional human-readable label resolved from the address book at
    /// the time of send. Kept verbatim — renaming a contact later does
    /// **not** rewrite history.
    #[serde(default)]
    pub counterparty_label: Option<String>,
    /// Active account index at the time of the entry. Lets us filter
    /// history per-account without re-scanning the chain.
    pub account: u32,
    /// Free-text status:
    /// * `Sent` — typically `"broadcasted"` (set by [`record_send`]).
    /// * `Received` — typically `"unspent"` (set in the merged view).
    pub status: String,
    /// Optional free-text note (currently unused; reserved for a future
    /// "annotate transfer" UI).
    #[serde(default)]
    pub note: Option<String>,
}

/// Encrypted journal — a flat append-only list (sorted by `timestamp` on
/// read).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryLog {
    /// File-format tag — checked on load to reject future-incompatible
    /// payloads early.
    #[serde(default = "default_format_tag")]
    pub format: String,
    /// Sent entries only. `Received` is synthesised at read time.
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
}

fn default_format_tag() -> String {
    QVHISTORY_FORMAT_V1.to_string()
}

impl HistoryLog {
    /// Empty journal tagged with the current format.
    #[must_use]
    pub fn new() -> Self {
        Self {
            format: QVHISTORY_FORMAT_V1.to_string(),
            entries: Vec::new(),
        }
    }

    /// Append a new entry. The journal is **not** automatically persisted
    /// — call [`save`] when you're ready.
    pub fn append(&mut self, e: HistoryEntry) {
        self.entries.push(e);
    }

    /// Iterate persisted entries in the order they were appended.
    pub fn iter(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    /// Number of persisted entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True iff there are no persisted entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Encrypted persistence
// ---------------------------------------------------------------------------

/// Compute the on-disk path for a wallet's history journal given its
/// keystore path. `wallet.json` → `wallet.json.history`.
#[must_use]
pub fn history_path_for(keystore_path: &Path) -> PathBuf {
    let mut s = keystore_path.as_os_str().to_owned();
    s.push(".history");
    PathBuf::from(s)
}

/// On-disk envelope. Mirrors `crate::address_book::Envelope` so the disk
/// layout is uniform across sidecars.
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

/// Encrypt and write the journal to `path`.
pub fn save(path: &Path, log: &HistoryLog, password: &str) -> WalletResult<()> {
    let mut salt = [0u8; SALT_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let mut iv = [0u8; NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut iv);

    let key = derive_key(password.as_bytes(), &salt)?;
    let plaintext = bincode::serialize(log)
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

/// Load and decrypt a journal from disk. Returns `Ok(empty)` if the file
/// doesn't exist (fresh-wallet state).
pub fn load_or_empty(path: &Path, password: &str) -> WalletResult<HistoryLog> {
    if !path.exists() {
        return Ok(HistoryLog::new());
    }
    let json = fs::read_to_string(path)?;
    let envelope: Envelope = serde_json::from_str(&json)
        .map_err(|e| WalletError::Keystore(format!("json decode: {e}")))?;
    if envelope.version != 1 {
        return Err(WalletError::Keystore(format!(
            "unsupported history version {}",
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
        .map_err(|_| WalletError::Keystore("wrong password or corrupted history file".into()))?;
    let log: HistoryLog = bincode::deserialize(&plaintext)
        .map_err(|e| WalletError::Keystore(format!("bincode decode: {e}")))?;
    if log.format != QVHISTORY_FORMAT_V1 {
        return Err(WalletError::Keystore(format!(
            "unsupported history payload format `{}` (expected `{}`)",
            log.format, QVHISTORY_FORMAT_V1
        )));
    }
    Ok(log)
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

/// Convenience: record a successful send and persist the journal.
///
/// Pulls `now_unix_secs()` itself so callers can't accidentally pass a
/// stale clock. `password` is required because the file is encrypted —
/// the server holds it in memory while the wallet is unlocked.
#[allow(clippy::too_many_arguments)]
pub fn record_send(
    path: &Path,
    password: &str,
    account: u32,
    tx_id: &str,
    amount: u64,
    fee: u64,
    counterparty_fingerprint: Option<&str>,
    counterparty_label: Option<&str>,
) -> WalletResult<()> {
    let mut log = load_or_empty(path, password)?;
    log.append(HistoryEntry {
        kind: EntryKind::Sent,
        tx_id: tx_id.to_string(),
        output_index: None,
        timestamp: now_unix_secs(),
        amount,
        fee: Some(fee),
        counterparty_fingerprint: counterparty_fingerprint.map(str::to_string),
        counterparty_label: counterparty_label.map(str::to_string),
        account,
        status: "broadcasted".to_string(),
        note: None,
    });
    save(path, &log, password)?;
    Ok(())
}

/// Merge persisted `Sent` entries with `Received` entries synthesised
/// from a live scan. The returned `Vec` is sorted newest-first by
/// timestamp, with `Received` entries timestamped at `now` (we don't have
/// per-block timestamps in the scan response yet — a future RPC extension
/// could push real timestamps in).
#[must_use]
pub fn merge_with_received(
    log: &HistoryLog,
    received_stealth: &[ReceivedRow],
    received_plain: &[ReceivedRow],
    now: u64,
    current_account: u32,
) -> Vec<HistoryEntry> {
    let mut out: Vec<HistoryEntry> = log.entries.clone();
    for r in received_stealth {
        out.push(HistoryEntry {
            kind: EntryKind::Received,
            tx_id: r.tx_id.clone(),
            output_index: Some(r.output_index),
            timestamp: now,
            amount: r.value,
            fee: None,
            counterparty_fingerprint: None,
            counterparty_label: Some("stealth".to_string()),
            account: current_account,
            status: "unspent".to_string(),
            note: None,
        });
    }
    for r in received_plain {
        out.push(HistoryEntry {
            kind: EntryKind::Received,
            tx_id: r.tx_id.clone(),
            output_index: Some(r.output_index),
            timestamp: now,
            amount: r.value,
            fee: None,
            counterparty_fingerprint: None,
            counterparty_label: Some("plain p2pkh".to_string()),
            account: current_account,
            status: "unspent".to_string(),
            note: None,
        });
    }
    // Newest first. Ties broken by tx_id so the order is stable across
    // re-fetches.
    out.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.tx_id.cmp(&b.tx_id))
    });
    out
}

/// Tiny DTO covering both `StealthMatch` and `P2pkhMatch` for the merge
/// helper — keeps `history.rs` decoupled from `rpc_client.rs` types.
#[derive(Clone, Debug)]
pub struct ReceivedRow {
    pub tx_id: String,
    pub output_index: u32,
    pub value: u64,
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
    use tempfile::tempdir;

    fn fixture_sent(tx: &str, amount: u64, account: u32) -> HistoryEntry {
        HistoryEntry {
            kind: EntryKind::Sent,
            tx_id: tx.to_string(),
            output_index: None,
            timestamp: 1_700_000_000,
            amount,
            fee: Some(1000),
            counterparty_fingerprint: Some("qvfp1abcd".to_string()),
            counterparty_label: Some("alice".to_string()),
            account,
            status: "broadcasted".to_string(),
            note: None,
        }
    }

    #[test]
    fn append_and_iterate() {
        let mut log = HistoryLog::new();
        assert!(log.is_empty());
        log.append(fixture_sent("aa", 100, 0));
        log.append(fixture_sent("bb", 200, 0));
        assert_eq!(log.len(), 2);
        let txs: Vec<&str> = log.iter().map(|e| e.tx_id.as_str()).collect();
        assert_eq!(txs, vec!["aa", "bb"]);
    }

    #[test]
    fn save_load_roundtrip_encrypted() {
        let mut log = HistoryLog::new();
        log.append(fixture_sent("dead", 42, 0));
        log.append(fixture_sent("beef", 99, 1));

        let dir = tempdir().unwrap();
        let path = dir.path().join("wallet.json.history");
        save(&path, &log, "pw1234567").unwrap();

        // Wrong password rejected.
        let err = load_or_empty(&path, "wrong-pw").unwrap_err();
        assert!(matches!(err, WalletError::Keystore(_)));

        // Right password recovers identical journal.
        let back = load_or_empty(&path, "pw1234567").unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.entries[0].tx_id, "dead");
        assert_eq!(back.entries[0].amount, 42);
        assert_eq!(back.entries[1].tx_id, "beef");
        assert_eq!(back.entries[1].account, 1);
    }

    #[test]
    fn load_empty_for_nonexistent_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.history");
        let log = load_or_empty(&path, "anything").unwrap();
        assert!(log.is_empty());
    }

    #[test]
    fn distinct_saves_produce_distinct_ciphertexts() {
        let log = {
            let mut l = HistoryLog::new();
            l.append(fixture_sent("aa", 1, 0));
            l
        };
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.history");
        let b = dir.path().join("b.history");
        save(&a, &log, "pw").unwrap();
        save(&b, &log, "pw").unwrap();
        let sa = fs::read_to_string(&a).unwrap();
        let sb = fs::read_to_string(&b).unwrap();
        assert_ne!(sa, sb);
    }

    #[test]
    fn record_send_appends_and_persists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wallet.json.history");
        record_send(
            &path,
            "pw1234567",
            0,
            "feedface",
            100,
            10,
            Some("qvfp1xyz"),
            Some("bob"),
        )
        .unwrap();
        record_send(
            &path,
            "pw1234567",
            0,
            "cafe",
            50,
            5,
            None,
            None,
        )
        .unwrap();

        let log = load_or_empty(&path, "pw1234567").unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries[0].tx_id, "feedface");
        assert_eq!(log.entries[0].counterparty_label.as_deref(), Some("bob"));
        assert_eq!(log.entries[0].fee, Some(10));
        assert_eq!(log.entries[1].tx_id, "cafe");
        assert_eq!(log.entries[1].counterparty_fingerprint, None);
    }

    #[test]
    fn merge_with_received_sorts_newest_first() {
        let mut log = HistoryLog::new();
        let mut s = fixture_sent("old", 10, 0);
        s.timestamp = 100;
        log.append(s);
        let mut s2 = fixture_sent("mid", 20, 0);
        s2.timestamp = 200;
        log.append(s2);

        let recv = vec![ReceivedRow {
            tx_id: "new".to_string(),
            output_index: 0,
            value: 5,
        }];
        let merged = merge_with_received(&log, &recv, &[], 300, 0);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].tx_id, "new");
        assert_eq!(merged[0].kind, EntryKind::Received);
        assert_eq!(merged[1].tx_id, "mid");
        assert_eq!(merged[2].tx_id, "old");
    }

    #[test]
    fn history_path_for_appends_dot_history() {
        let p = history_path_for(Path::new("/tmp/wallet.json"));
        assert_eq!(p, PathBuf::from("/tmp/wallet.json.history"));
    }

    #[test]
    fn entry_kind_round_trips_through_serde() {
        let s = serde_json::to_string(&EntryKind::Sent).unwrap();
        assert_eq!(s, "\"sent\"");
        let r = serde_json::to_string(&EntryKind::Received).unwrap();
        assert_eq!(r, "\"received\"");
        let back: EntryKind = serde_json::from_str("\"sent\"").unwrap();
        assert_eq!(back, EntryKind::Sent);
    }
}
