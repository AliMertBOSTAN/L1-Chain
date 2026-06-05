//! Multi-tenant session store for the HTTP wallet server.
//!
//! In single-user mode the wallet server keeps one global `UnlockedWallet`
//! and every HTTP client that reaches it talks to the same in-memory
//! cüzdan. In multi-tenant ("demo B") mode that single slot is replaced
//! by this module's [`SessionStore`] — a TTL-bounded map from random
//! session tokens to per-user unlocked sessions. Each browser tab holds
//! one token (in localStorage), sent as `Authorization: Bearer <token>`,
//! and the server uses it to look up "which user is calling".
//!
//! # Security model — custodial
//!
//! By design the server sees plaintext passwords on register/login and
//! holds plaintext Dilithium spend secrets in RAM for the lifetime of
//! every session. **The server operator can technically spend any user's
//! funds.** Acceptable on a devnet/demo; never deploy this to mainnet
//! without first switching to Option A (binary distribution) or
//! Option C (WASM client-side wallet) — see project README.
//!
//! # TTL
//!
//! Sessions auto-expire after `ttl` seconds of idleness. On the next
//! request, `touch()` notices the expiry, removes the entry, and returns
//! `None` → the handler responds 401 and the UI drops back to login.
//! Spend secrets zeroize on drop via `SecureBytes`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use qv_privacy::StealthKeys;
use rand::RngCore;
use tokio::sync::RwLock;

/// Mint a fresh 32-byte random session token, hex-encoded.
///
/// 256 bits of entropy from `OsRng` — strong enough that an attacker
/// cannot brute-force a valid token in any realistic horizon.
#[must_use]
pub fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// One unlocked cüzdan attached to one session.
///
/// Mirrors the old single-user `UnlockedWallet` plus a `username` and
/// `last_seen` for TTL bookkeeping. Cloning shares `Arc<StealthKeys>`,
/// so the spend secret lives in one place — when the last session
/// referencing it is removed, drop ⇒ zeroize.
#[derive(Clone)]
pub struct SessionEntry {
    /// Owner of this session.
    pub username: String,
    /// Per-user keystore path (`<wallets_dir>/<username>/wallet.json`).
    pub keystore_path: PathBuf,
    /// Currently active account index for this session.
    pub account: u32,
    /// Live keypair derived from the keystore.
    pub stealth: Arc<StealthKeys>,
    /// Keystore password — kept in RAM so the address-book, history and
    /// account-switch endpoints can decrypt/re-encrypt sidecars without
    /// re-prompting on every call. Dropped on logout / TTL expiry.
    pub password: String,
    /// Unix seconds when this session was last accessed.
    pub last_seen_unix: u64,
}

/// In-memory session map with TTL.
///
/// Reads use [`tokio::sync::RwLock`] so balance/UTXO scans don't
/// serialize against each other; only writes (insert / remove / touch's
/// last_seen update) take the exclusive lock.
pub struct SessionStore {
    map: RwLock<HashMap<String, SessionEntry>>,
    ttl: Duration,
}

impl SessionStore {
    /// Build an empty store with a per-session inactivity TTL.
    ///
    /// Typical TTL: 1 hour. A request that finds an expired session
    /// removes it and returns `None`, forcing re-login.
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Inactivity TTL configured at construction time.
    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Insert or replace a session. Returns the token used to look it
    /// up — random per insert.
    pub async fn insert(&self, mut entry: SessionEntry) -> String {
        let id = new_session_id();
        entry.last_seen_unix = now_unix_secs();
        self.map.write().await.insert(id.clone(), entry);
        id
    }

    /// Replace the entry for `token` in place (used by
    /// `switch-account`). No-op if the token is unknown — the caller is
    /// expected to have already touched the session and confirmed it
    /// existed.
    pub async fn replace(&self, token: &str, mut entry: SessionEntry) {
        entry.last_seen_unix = now_unix_secs();
        let mut g = self.map.write().await;
        if g.contains_key(token) {
            g.insert(token.to_string(), entry);
        }
    }

    /// Look up a session, bump its `last_seen`, and return a clone.
    ///
    /// If the session has been idle longer than [`Self::ttl`] it is
    /// removed and `None` is returned — the next handler turn responds
    /// 401 and the UI drops back to the login screen.
    pub async fn touch(&self, token: &str) -> Option<SessionEntry> {
        let now = now_unix_secs();
        // Single-pass under the write lock: read the entry, decide
        // expire-or-bump, and return the bumped clone.
        let mut g = self.map.write().await;
        let expired = g
            .get(token)
            .map(|e| now.saturating_sub(e.last_seen_unix) > self.ttl.as_secs())
            .unwrap_or(false);
        if expired {
            g.remove(token);
            return None;
        }
        if let Some(e) = g.get_mut(token) {
            e.last_seen_unix = now;
            return Some(e.clone());
        }
        None
    }

    /// Remove a session (logout). Silent if the token is unknown.
    pub async fn remove(&self, token: &str) {
        self.map.write().await.remove(token);
    }

    /// Number of live sessions. Test-only / metrics-style.
    pub async fn len(&self) -> usize {
        self.map.read().await.len()
    }

    /// True iff there are no live sessions. Paired with [`Self::len`]
    /// to satisfy `clippy::len_without_is_empty`.
    pub async fn is_empty(&self) -> bool {
        self.map.read().await.is_empty()
    }

    /// Sweep expired sessions in one pass. Intended for a background
    /// task on a coarse interval; not strictly required because
    /// `touch()` also expires on access.
    pub async fn gc(&self) -> usize {
        let now = now_unix_secs();
        let ttl_secs = self.ttl.as_secs();
        let mut g = self.map.write().await;
        let before = g.len();
        g.retain(|_, e| now.saturating_sub(e.last_seen_unix) <= ttl_secs);
        before.saturating_sub(g.len())
    }
}

// ---------------------------------------------------------------------------
// Username validation + on-disk layout
// ---------------------------------------------------------------------------

/// Maximum username length on disk.
pub const USERNAME_MAX_LEN: usize = 32;
/// Minimum username length.
pub const USERNAME_MIN_LEN: usize = 3;

/// Validate a username string.
///
/// Strict: lowercase ASCII alphanumeric + `-` + `_`, length
/// 3..=[`USERNAME_MAX_LEN`]. Rejects every character that could escape
/// the per-user wallet directory (path traversal: `.`, `/`, `\`, etc.).
pub fn validate_username(s: &str) -> Result<(), String> {
    if s.len() < USERNAME_MIN_LEN {
        return Err(format!(
            "username must be at least {USERNAME_MIN_LEN} characters"
        ));
    }
    if s.len() > USERNAME_MAX_LEN {
        return Err(format!(
            "username must be at most {USERNAME_MAX_LEN} characters"
        ));
    }
    for (i, ch) in s.chars().enumerate() {
        let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-';
        if !ok {
            return Err(format!(
                "username character {} (`{ch}`) must be lowercase a-z, 0-9, `_`, or `-`",
                i + 1
            ));
        }
    }
    // Already covered by the loop above but spell out for clarity.
    if s.starts_with('-') || s.starts_with('_') {
        return Err("username cannot start with `-` or `_`".into());
    }
    Ok(())
}

/// Path of a user's keystore inside the wallets directory.
///
/// `wallets_dir/<username>/wallet.json` — callers MUST first call
/// [`validate_username`] before passing into this function. Doing
/// otherwise risks path traversal.
#[must_use]
pub fn user_keystore_path(wallets_dir: &std::path::Path, username: &str) -> PathBuf {
    wallets_dir.join(username).join("wallet.json")
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_crypto::{DilithiumLevel, KyberLevel};

    fn fixture_session(name: &str) -> SessionEntry {
        let stealth = StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).unwrap();
        SessionEntry {
            username: name.to_string(),
            keystore_path: PathBuf::from(format!("wallets/{name}/wallet.json")),
            account: 0,
            stealth: Arc::new(stealth),
            password: "pw1234567".to_string(),
            last_seen_unix: 0,
        }
    }

    #[test]
    fn token_format() {
        let t = new_session_id();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        let t2 = new_session_id();
        assert_ne!(t, t2);
    }

    #[tokio::test]
    async fn insert_and_touch() {
        let store = SessionStore::new(Duration::from_secs(3600));
        let id = store.insert(fixture_session("alice")).await;
        let got = store.touch(&id).await.expect("session must exist");
        assert_eq!(got.username, "alice");
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn remove_kills_session() {
        let store = SessionStore::new(Duration::from_secs(3600));
        let id = store.insert(fixture_session("bob")).await;
        store.remove(&id).await;
        assert!(store.touch(&id).await.is_none());
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn touch_unknown_returns_none() {
        let store = SessionStore::new(Duration::from_secs(3600));
        assert!(store.touch("nope").await.is_none());
    }

    #[tokio::test]
    async fn ttl_expires_idle_sessions() {
        // 0-second TTL: every touch after insert sees an expired entry.
        let store = SessionStore::new(Duration::from_secs(0));
        let id = store.insert(fixture_session("idle")).await;
        // Force last_seen back in time so the next touch counts as
        // idle > ttl.
        {
            let mut g = store.map.write().await;
            if let Some(e) = g.get_mut(&id) {
                e.last_seen_unix = 0;
            }
        }
        // Now any non-zero current time will exceed 0-second TTL.
        std::thread::sleep(Duration::from_secs(1));
        assert!(store.touch(&id).await.is_none());
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn replace_swaps_entry_in_place() {
        let store = SessionStore::new(Duration::from_secs(3600));
        let id = store.insert(fixture_session("alice")).await;
        let mut new_entry = fixture_session("alice");
        new_entry.account = 7;
        store.replace(&id, new_entry).await;
        let got = store.touch(&id).await.unwrap();
        assert_eq!(got.account, 7);
    }

    #[tokio::test]
    async fn gc_removes_only_expired() {
        // Use a roomy TTL so `b` (just inserted) is NOT expired even
        // when measured a few seconds later, while `a` is artificially
        // pushed >TTL into the past.
        let store = SessionStore::new(Duration::from_secs(60));
        let id_a = store.insert(fixture_session("a")).await;
        let id_b = store.insert(fixture_session("b")).await;
        // Force `a` to be one hour idle, well past the 60-second TTL.
        {
            let mut g = store.map.write().await;
            if let Some(e) = g.get_mut(&id_a) {
                e.last_seen_unix = e.last_seen_unix.saturating_sub(3600);
            }
        }
        let removed = store.gc().await;
        assert_eq!(removed, 1);
        assert!(store.touch(&id_a).await.is_none());
        assert!(store.touch(&id_b).await.is_some());
    }

    #[test]
    fn username_validation_happy_paths() {
        // Each entry must be ≥ USERNAME_MIN_LEN (3) chars.
        for s in ["alice", "bob", "test_user", "user-1", "x9a", "abc123"] {
            assert!(validate_username(s).is_ok(), "expected `{s}` valid");
        }
    }

    #[test]
    fn username_validation_rejects_bad() {
        for s in [
            "", "a", "ab",                       // too short
            "ALICE",                              // uppercase
            "alice@home",                         // special char
            "..",                                 // path traversal
            "a/b",                                // slash
            "a\\b",                               // backslash
            "_alice",                             // leading underscore
            "-alice",                             // leading dash
            " alice",                             // space
            "alice ",                             // trailing space
        ] {
            assert!(
                validate_username(s).is_err(),
                "expected `{s}` rejected, got Ok"
            );
        }
    }

    #[test]
    fn username_max_length_enforced() {
        let too_long = "a".repeat(USERNAME_MAX_LEN + 1);
        assert!(validate_username(&too_long).is_err());
        let exactly_max = "a".repeat(USERNAME_MAX_LEN);
        assert!(validate_username(&exactly_max).is_ok());
    }

    #[test]
    fn user_keystore_path_layout() {
        let p = user_keystore_path(std::path::Path::new("/srv/wallets"), "alice");
        assert!(p.ends_with("alice/wallet.json"));
    }
}
