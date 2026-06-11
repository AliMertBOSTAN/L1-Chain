//! Local HTTP API + embedded UI for the stealth wallet (ADR-011 Faz 5).
//!
//! Two modes:
//!
//! * **Single-user** — one keystore, one global unlocked slot. Everyone
//!   who reaches `127.0.0.1:7777` shares the same cüzdan. Original
//!   ADR-011 design.
//! * **Multi-tenant (custodial demo)** — `--wallets-dir <path>` enables
//!   per-user keystores under `<wallets_dir>/<username>/wallet.json`
//!   and session-token auth. CUSTODIAL — the server sees plaintext
//!   passwords on register/login and holds plaintext Dilithium spend
//!   secrets in RAM for the life of each session. Devnet/demo only.
//!
//! The server binds to `127.0.0.1` by default. For LAN access pass
//! `--bind 0.0.0.0:7777`; pair with `Authorization: Bearer <token>` so
//! random LAN neighbours can't hit `/api/send` against an unlocked
//! cüzdan.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::async_trait;
use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use qv_core::{Amount, OutPoint, TxId, TxInput, ValidityInterval};
use qv_privacy::StealthKeys;

use crate::address::{decode_address, encode_address, fingerprint};
use crate::address_book::{contacts_path_for, load_or_empty as load_contacts, save as save_contacts};
use crate::disclose::{create_disclosure, DisclosureFile};
use crate::faucet::drip as faucet_drip;
use crate::hd::DefaultSeedDeriver;
use crate::history::{
    history_path_for, load_or_empty as load_history, merge_with_received, record_send,
    HistoryEntry, ReceivedRow,
};
use crate::keystore::{
    PersistedViewKey, WalletKeystore, WalletMetadata, WalletSecret,
};
use crate::qvaddr::{
    address_from_qr_parts, address_to_qr_parts, render_qr_svg, Qvaddr, DEFAULT_QR_PARTS,
};
use crate::rpc_client::{P2pkhMatch, PoolEntry, RpcClient, StealthMatch};
use crate::session::{
    user_keystore_path, validate_username, SessionEntry, SessionStore,
};
use crate::swap::{
    direction_from_str, execute_create_pool, execute_swap, CreatePoolParams, SwapParams,
};
use crate::tx_builder::TxBuilder;
use crate::view_export::ViewKeyExport;
use crate::{Mnemonic, WalletError};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// In-memory unlocked cüzdan for **single-user** mode.
///
/// Dropped on lock — the spend secret zeroizes automatically via
/// `SecureBytes` inside [`StealthKeys`].
#[derive(Clone)]
struct UnlockedWallet {
    stealth: Arc<StealthKeys>,
    account: u32,
    /// Argon2id password — kept in memory only while unlocked so the
    /// address-book / history / account-switch endpoints can read &
    /// re-encrypt sidecars without re-prompting on every call. Dropped
    /// when the wallet is locked.
    password: String,
}

/// Per-mode runtime backing.
///
/// * `Single` — original ADR-011 layout, one keystore + one Mutex slot.
/// * `Multi`  — wallets directory + session map, every endpoint
///   identifies the caller by `Authorization: Bearer <token>`.
#[derive(Clone)]
enum Backend {
    Single {
        keystore_path: PathBuf,
        unlocked: Arc<Mutex<Option<UnlockedWallet>>>,
    },
    Multi {
        wallets_dir: PathBuf,
        sessions: Arc<SessionStore>,
    },
}

/// Server-wide state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    backend: Backend,
    rpc: Arc<RpcClient>,
}

impl AppState {
    /// Build single-user state — one global keystore, one global lock.
    #[must_use]
    pub fn new(keystore_path: PathBuf, rpc_url: String) -> Self {
        Self {
            backend: Backend::Single {
                keystore_path,
                unlocked: Arc::new(Mutex::new(None)),
            },
            rpc: Arc::new(RpcClient::new(rpc_url)),
        }
    }

    /// Build multi-tenant state. `session_ttl` controls how long an
    /// idle session lives before auto-expiring (typical: 1 hour).
    #[must_use]
    pub fn new_multi(wallets_dir: PathBuf, rpc_url: String, session_ttl: Duration) -> Self {
        Self {
            backend: Backend::Multi {
                wallets_dir,
                sessions: Arc::new(SessionStore::new(session_ttl)),
            },
            rpc: Arc::new(RpcClient::new(rpc_url)),
        }
    }

    fn multi_mode(&self) -> bool {
        matches!(self.backend, Backend::Multi { .. })
    }

    /// Resolve a request to an active cüzdan, regardless of mode.
    ///
    /// * **Single-user** — `bearer` is ignored; returns the global
    ///   `UnlockedWallet` if any.
    /// * **Multi-tenant** — `bearer` is required; looks up + touches
    ///   the session, returns its [`SessionEntry`]-derived view.
    async fn require_active(&self, bearer: &Bearer) -> Result<ActiveWallet, ApiError> {
        match &self.backend {
            Backend::Single { unlocked, keystore_path } => unlocked
                .lock()
                .await
                .clone()
                .map(|w| ActiveWallet {
                    username: None,
                    keystore_path: keystore_path.clone(),
                    account: w.account,
                    stealth: w.stealth,
                    password: w.password,
                    session_token: None,
                })
                .ok_or_else(|| {
                    ApiError::Unauthorized("wallet is locked — call /api/wallet/unlock first".into())
                }),
            Backend::Multi { sessions, .. } => {
                let token = bearer
                    .0
                    .as_deref()
                    .ok_or_else(|| ApiError::Unauthorized("missing session token".into()))?;
                let entry = sessions
                    .touch(token)
                    .await
                    .ok_or_else(|| ApiError::Unauthorized("session expired or unknown".into()))?;
                Ok(ActiveWallet {
                    username: Some(entry.username),
                    keystore_path: entry.keystore_path,
                    account: entry.account,
                    stealth: entry.stealth,
                    password: entry.password,
                    session_token: Some(token.to_string()),
                })
            }
        }
    }
}

/// View of an unlocked cüzdan as seen by every handler.
///
/// Bridges the two backends so the original handlers can stay
/// mode-agnostic. The fields are pulled either from the single global
/// slot or from the per-session entry the bearer token unlocked.
struct ActiveWallet {
    username: Option<String>,
    keystore_path: PathBuf,
    account: u32,
    stealth: Arc<StealthKeys>,
    password: String,
    session_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Bearer-token extractor
// ---------------------------------------------------------------------------

/// Best-effort `Authorization: Bearer <token>` extractor — never fails;
/// missing header simply gives `Bearer(None)`. Handlers that require a
/// session call [`AppState::require_active`] which then 401s on missing.
struct Bearer(Option<String>);

#[async_trait]
impl<S> FromRequestParts<S> for Bearer
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        // 1. Authorization: Bearer <token>
        if let Some(v) = parts.headers.get(header::AUTHORIZATION) {
            if let Ok(s) = v.to_str() {
                if let Some(tok) = s.strip_prefix("Bearer ") {
                    return Ok(Bearer(Some(tok.trim().to_string())));
                }
            }
        }
        // 2. X-QV-Session: <token> (lighter alternative for fetch())
        if let Some(v) = parts.headers.get("x-qv-session") {
            if let Ok(s) = v.to_str() {
                return Ok(Bearer(Some(s.trim().to_string())));
            }
        }
        Ok(Bearer(None))
    }
}

// ---------------------------------------------------------------------------
// API error envelope
// ---------------------------------------------------------------------------

/// Anything that can fail inside an HTTP handler. Serialises to a JSON
/// `{ "error": "<message>" }` payload with a sensible status code.
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Wallet(WalletError),
    NotFound(String),
    Internal(String),
}

impl From<WalletError> for ApiError {
    fn from(e: WalletError) -> Self {
        ApiError::Wallet(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::Wallet(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (
            status,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Request / response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct CreateResponse {
    pub mnemonic: String,
    pub address: String,
    pub fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub phrase: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub address: String,
    pub fingerprint: String,
}

#[derive(Debug, Deserialize)]
pub struct UnlockRequest {
    pub password: String,
    #[serde(default)]
    pub account: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AddressResponse {
    pub address: String,
    pub fingerprint: String,
    pub account: u32,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub balance: u64,
}

#[derive(Debug, Serialize)]
pub struct UtxosResponse {
    /// Stealth UTXOs detected via Kyber KEM decapsulation (ADR-011).
    pub stealth: Vec<StealthMatch>,
    /// Plain `p2pkh_pqc` UTXOs locked to our spend-pubkey hash — typically
    /// genesis allocations or pre-stealth sends.
    pub plain: Vec<P2pkhMatch>,
}

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub to_address: String,
    pub amount: u64,
    #[serde(default = "default_fee")]
    pub fee: u64,
}

fn default_fee() -> u64 {
    1000
}

#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub tx_id: String,
    pub tx_hex: String,
    pub broadcast: bool,
    pub rpc_result: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    /// Multi-tenant mode? UI uses this to choose between login/register
    /// vs. the legacy single-cüzdan create/import/unlock flow.
    pub multi_tenant: bool,
    /// True iff the caller's session is currently active. In
    /// single-user mode this just means "the global wallet is
    /// unlocked".
    pub unlocked: bool,
    pub address: Option<String>,
    pub fingerprint: Option<String>,
    pub account: Option<u32>,
    /// Multi-tenant only — the logged-in username.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub rpc_url: String,
    /// Single-user only — does the configured keystore file exist?
    /// Multi-tenant always reports `true` to keep the UI simple.
    pub keystore_exists: bool,
    /// Session token TTL in seconds (multi-tenant only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_ttl_secs: Option<u64>,
}

/// One row in the `/api/wallet/accounts` response.
#[derive(Debug, Serialize)]
pub struct AccountInfo {
    pub account: u32,
    pub address: String,
    pub fingerprint: String,
}

#[derive(Debug, Serialize)]
pub struct AccountsResponse {
    pub accounts: Vec<AccountInfo>,
    pub current: u32,
    pub next_account: u32,
}

#[derive(Debug, Deserialize)]
pub struct SwitchAccountRequest {
    pub account: u32,
}

// Multi-tenant auth DTOs ------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    /// Optional — when present, import this BIP-39 phrase instead of
    /// generating a fresh one. Useful for migrating an existing
    /// keystore into the multi-tenant flow.
    #[serde(default)]
    pub phrase: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub username: String,
    /// Generated mnemonic — shown ONCE so the user can write it down.
    /// `None` when the caller imported their own phrase.
    pub mnemonic: Option<String>,
    pub address: String,
    pub fingerprint: String,
    pub session_token: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Optional — switch to this account on login. Defaults to 0.
    #[serde(default)]
    pub account: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub username: String,
    pub address: String,
    pub fingerprint: String,
    pub account: u32,
    pub session_token: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub username: String,
    pub address: String,
    pub fingerprint: String,
    pub account: u32,
}

// ---------------------------------------------------------------------------
// Router construction
// ---------------------------------------------------------------------------

/// Build the axum [`Router`] for the wallet HTTP API + UI.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(handle_status))
        // ---- single-user auth (mode-aware: rejected in multi-tenant) ----
        .route("/api/wallet/create", post(handle_create))
        .route("/api/wallet/import", post(handle_import))
        .route("/api/wallet/unlock", post(handle_unlock))
        .route("/api/wallet/lock", post(handle_lock))
        // ---- multi-tenant auth (mode-aware: rejected in single-user) ----
        .route("/api/auth/register", post(handle_auth_register))
        .route("/api/auth/login", post(handle_auth_login))
        .route("/api/auth/logout", post(handle_auth_logout))
        .route("/api/auth/me", get(handle_auth_me))
        // ---- wallet endpoints (mode-agnostic; session-or-global) ----
        .route("/api/wallet/accounts", get(handle_accounts_list))
        .route("/api/wallet/switch-account", post(handle_switch_account))
        .route("/api/wallet/address", get(handle_address))
        .route("/api/wallet/address.qvaddr", get(handle_address_qvaddr))
        .route("/api/wallet/fingerprint.svg", get(handle_fingerprint_svg))
        .route("/api/wallet/address-qr", get(handle_address_qr_parts))
        .route("/api/wallet/import-qvaddr", post(handle_import_qvaddr))
        .route("/api/wallet/qr-reassemble", post(handle_qr_reassemble))
        .route("/api/wallet/view-key.qvview", get(handle_view_key_export))
        .route("/api/wallet/disclose", post(handle_disclose))
        .route("/api/wallet/verify-disclosure", post(handle_verify_disclosure))
        .route("/api/contacts", get(handle_contacts_list).post(handle_contacts_add))
        .route("/api/contacts/remove", post(handle_contacts_remove))
        .route("/api/history", get(handle_history_list))
        .route("/api/devnet/faucet", post(handle_devnet_faucet))
        .route("/api/balance", get(handle_balance))
        .route("/api/utxos", get(handle_utxos))
        .route("/api/send", post(handle_send))
        // ---- DeFi (Faz 6 D-5; mode-agnostic, session-or-global) ----
        .route("/api/defi/pools", get(handle_defi_pools))
        .route("/api/defi/swap", post(handle_defi_swap))
        .route("/api/defi/create-pool", post(handle_defi_create_pool))
        .with_state(state)
}

/// Bind a TCP listener and run the wallet UI server until the process
/// receives a Ctrl-C.
pub async fn serve(state: AppState, addr: SocketAddr) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(?addr, "qv-wallet UI listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers — generic
// ---------------------------------------------------------------------------

async fn serve_index() -> Html<&'static str> {
    Html(crate::server_ui::INDEX_HTML)
}

async fn handle_status(State(state): State<AppState>, bearer: Bearer) -> impl IntoResponse {
    let multi = state.multi_mode();
    let session_ttl = if let Backend::Multi { sessions, .. } = &state.backend {
        Some(sessions.ttl().as_secs())
    } else {
        None
    };
    let keystore_exists = match &state.backend {
        Backend::Single { keystore_path, .. } => keystore_path.exists(),
        Backend::Multi { .. } => true,
    };

    let resp = match state.require_active(&bearer).await {
        Ok(a) => StatusResponse {
            multi_tenant: multi,
            unlocked: true,
            address: Some(encode_address(&a.stealth.address()).unwrap_or_default()),
            fingerprint: Some(fingerprint(&a.stealth.address())),
            account: Some(a.account),
            username: a.username,
            rpc_url: state.rpc.url().to_string(),
            keystore_exists,
            session_ttl_secs: session_ttl,
        },
        Err(_) => StatusResponse {
            multi_tenant: multi,
            unlocked: false,
            address: None,
            fingerprint: None,
            account: None,
            username: None,
            rpc_url: state.rpc.url().to_string(),
            keystore_exists,
            session_ttl_secs: session_ttl,
        },
    };
    (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// Single-user auth handlers (rejected in multi-tenant mode)
// ---------------------------------------------------------------------------

fn require_single(
    state: &AppState,
) -> Result<(&PathBuf, &Mutex<Option<UnlockedWallet>>), ApiError> {
    match &state.backend {
        Backend::Single { keystore_path, unlocked } => Ok((keystore_path, unlocked.as_ref())),
        Backend::Multi { .. } => Err(ApiError::BadRequest(
            "this endpoint is single-user; in multi-tenant mode use /api/auth/{register,login,logout}".into(),
        )),
    }
}

async fn handle_create(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    let (keystore_path, _) = require_single(&state)?;
    if keystore_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "keystore already exists at {} — refusing to overwrite",
            keystore_path.display()
        )));
    }
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let mnemonic = Mnemonic::generate()?;
    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver
        .generate_fresh_view_keypair()
        .map_err(ApiError::Wallet)?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };
    WalletKeystore::save(keystore_path, &secret, &req.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;

    let seed = mnemonic
        .to_seed("")
        .map_err(|e| ApiError::Wallet(WalletError::Mnemonic(e.to_string())))?;
    let stealth = deriver
        .derive_account_with_view(&seed, 0, view_kp)
        .map_err(|e| ApiError::Wallet(WalletError::HdDerivation(e.to_string())))?;
    let address = encode_address(&stealth.address())?;
    let fp = fingerprint(&stealth.address());

    Ok(Json(CreateResponse {
        mnemonic: mnemonic.phrase().to_string(),
        address,
        fingerprint: fp,
    }))
}

async fn handle_import(
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportResponse>, ApiError> {
    let (keystore_path, _) = require_single(&state)?;
    if keystore_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "keystore already exists at {} — refusing to overwrite",
            keystore_path.display()
        )));
    }
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }

    let mnemonic = Mnemonic::from_phrase(&req.phrase)
        .map_err(|e| ApiError::BadRequest(format!("invalid mnemonic: {e}")))?;

    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver
        .generate_fresh_view_keypair()
        .map_err(ApiError::Wallet)?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };
    WalletKeystore::save(keystore_path, &secret, &req.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;

    let seed = mnemonic
        .to_seed("")
        .map_err(|e| ApiError::Wallet(WalletError::Mnemonic(e.to_string())))?;
    let stealth = deriver
        .derive_account_with_view(&seed, 0, view_kp)
        .map_err(|e| ApiError::Wallet(WalletError::HdDerivation(e.to_string())))?;
    Ok(Json(ImportResponse {
        address: encode_address(&stealth.address())?,
        fingerprint: fingerprint(&stealth.address()),
    }))
}

async fn handle_unlock(
    State(state): State<AppState>,
    Json(req): Json<UnlockRequest>,
) -> Result<Json<AddressResponse>, ApiError> {
    let (keystore_path, unlocked_slot) = require_single(&state)?;
    let account = req.account.unwrap_or(0);
    let deriver = DefaultSeedDeriver::default_levels();
    let stealth = WalletKeystore::unlock_account(
        keystore_path,
        &req.password,
        account,
        &deriver,
    )
    .map_err(|e| ApiError::Unauthorized(format!("unlock failed: {e}")))?;

    let unlocked = UnlockedWallet {
        stealth: Arc::new(stealth),
        account,
        password: req.password.clone(),
    };
    let resp = AddressResponse {
        address: encode_address(&unlocked.stealth.address())?,
        fingerprint: fingerprint(&unlocked.stealth.address()),
        account,
    };
    *unlocked_slot.lock().await = Some(unlocked);
    Ok(Json(resp))
}

async fn handle_lock(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let (_, slot) = require_single(&state)?;
    *slot.lock().await = None;
    Ok((StatusCode::OK, Json(serde_json::json!({ "locked": true }))))
}

// ---------------------------------------------------------------------------
// Multi-tenant auth handlers (rejected in single-user mode)
// ---------------------------------------------------------------------------

fn require_multi(state: &AppState) -> Result<(&PathBuf, &SessionStore), ApiError> {
    match &state.backend {
        Backend::Multi { wallets_dir, sessions } => Ok((wallets_dir, sessions.as_ref())),
        Backend::Single { .. } => Err(ApiError::BadRequest(
            "this endpoint requires multi-tenant mode; start with --wallets-dir to enable".into(),
        )),
    }
}

/// Create a fresh user. Per request:
/// 1. Validate username
/// 2. Refuse if `<wallets_dir>/<username>/wallet.json` exists
/// 3. Build the per-user directory
/// 4. Generate (or import) a mnemonic; persist keystore
/// 5. Derive account 0 stealth keys
/// 6. Auto-login — issue a session token
async fn handle_auth_register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ApiError> {
    let (wallets_dir, sessions) = require_multi(&state)?;
    validate_username(&req.username).map_err(ApiError::BadRequest)?;
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    let keystore_path = user_keystore_path(wallets_dir, &req.username);
    if keystore_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "user `{}` already exists",
            req.username
        )));
    }

    // Create the per-user directory.
    if let Some(parent) = keystore_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ApiError::Internal(format!("create dir {}: {e}", parent.display()))
        })?;
    }

    // Mnemonic — fresh or imported.
    let (mnemonic, show_mnemonic) = if let Some(phrase) = req.phrase.as_deref() {
        let m = Mnemonic::from_phrase(phrase.trim())
            .map_err(|e| ApiError::BadRequest(format!("invalid mnemonic: {e}")))?;
        (m, false)
    } else {
        (Mnemonic::generate()?, true)
    };

    let deriver = DefaultSeedDeriver::default_levels();
    let view_kp = deriver
        .generate_fresh_view_keypair()
        .map_err(ApiError::Wallet)?;
    let mut view_keypairs = std::collections::BTreeMap::new();
    view_keypairs.insert(0, PersistedViewKey::from_keypair(&view_kp));

    let secret = WalletSecret {
        mnemonic: mnemonic.clone(),
        metadata: WalletMetadata {
            next_account: 0,
            created_at: now_unix_secs(),
        },
        view_keypairs,
    };
    WalletKeystore::save(&keystore_path, &secret, &req.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;

    // Derive account 0 stealth keys for the auto-login.
    let seed = mnemonic
        .to_seed("")
        .map_err(|e| ApiError::Wallet(WalletError::Mnemonic(e.to_string())))?;
    let stealth = deriver
        .derive_account_with_view(&seed, 0, view_kp)
        .map_err(|e| ApiError::Wallet(WalletError::HdDerivation(e.to_string())))?;
    let address = encode_address(&stealth.address())?;
    let fp = fingerprint(&stealth.address());

    let entry = SessionEntry {
        username: req.username.clone(),
        keystore_path,
        account: 0,
        stealth: Arc::new(stealth),
        password: req.password.clone(),
        last_seen_unix: 0,
    };
    let token = sessions.insert(entry).await;

    Ok(Json(RegisterResponse {
        username: req.username,
        mnemonic: if show_mnemonic {
            Some(mnemonic.phrase().to_string())
        } else {
            None
        },
        address,
        fingerprint: fp,
        session_token: token,
    }))
}

async fn handle_auth_login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let (wallets_dir, sessions) = require_multi(&state)?;
    validate_username(&req.username).map_err(ApiError::BadRequest)?;
    let keystore_path = user_keystore_path(wallets_dir, &req.username);
    if !keystore_path.exists() {
        return Err(ApiError::Unauthorized(format!(
            "no such user `{}`",
            req.username
        )));
    }
    let account = req.account.unwrap_or(0);
    let deriver = DefaultSeedDeriver::default_levels();
    let stealth = WalletKeystore::unlock_account(
        &keystore_path,
        &req.password,
        account,
        &deriver,
    )
    .map_err(|e| ApiError::Unauthorized(format!("login failed: {e}")))?;

    let address = encode_address(&stealth.address())?;
    let fp = fingerprint(&stealth.address());

    let entry = SessionEntry {
        username: req.username.clone(),
        keystore_path,
        account,
        stealth: Arc::new(stealth),
        password: req.password.clone(),
        last_seen_unix: 0,
    };
    let token = sessions.insert(entry).await;

    Ok(Json(LoginResponse {
        username: req.username,
        address,
        fingerprint: fp,
        account,
        session_token: token,
    }))
}

async fn handle_auth_logout(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, sessions) = require_multi(&state)?;
    if let Some(tok) = bearer.0.as_deref() {
        sessions.remove(tok).await;
    }
    Ok(Json(serde_json::json!({ "logged_out": true })))
}

async fn handle_auth_me(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<MeResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    Ok(Json(MeResponse {
        username: a.username.unwrap_or_default(),
        address: encode_address(&a.stealth.address())?,
        fingerprint: fingerprint(&a.stealth.address()),
        account: a.account,
    }))
}

// ---------------------------------------------------------------------------
// Account management (mode-agnostic)
// ---------------------------------------------------------------------------

async fn handle_accounts_list(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<AccountsResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let secret = WalletKeystore::load(&a.keystore_path, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    let deriver = DefaultSeedDeriver::default_levels();
    let seed = secret
        .mnemonic
        .to_seed("")
        .map_err(|e| ApiError::Wallet(WalletError::Mnemonic(e.to_string())))?;

    let mut accounts: Vec<AccountInfo> = Vec::new();
    let mut max_acct: Option<u32> = None;
    for (&acct, pv) in &secret.view_keypairs {
        let view_kp = pv.clone().into_keypair()?;
        let stealth = deriver
            .derive_account_with_view(&seed, acct, view_kp)
            .map_err(|e| ApiError::Wallet(WalletError::HdDerivation(e.to_string())))?;
        accounts.push(AccountInfo {
            account: acct,
            address: encode_address(&stealth.address())?,
            fingerprint: fingerprint(&stealth.address()),
        });
        max_acct = Some(max_acct.map_or(acct, |m| m.max(acct)));
    }
    let next_account = max_acct.map_or(0, |m| m.saturating_add(1));
    Ok(Json(AccountsResponse {
        accounts,
        current: a.account,
        next_account,
    }))
}

async fn handle_switch_account(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<SwitchAccountRequest>,
) -> Result<Json<AddressResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    if req.account == a.account {
        return Ok(Json(AddressResponse {
            address: encode_address(&a.stealth.address())?,
            fingerprint: fingerprint(&a.stealth.address()),
            account: a.account,
        }));
    }
    let deriver = DefaultSeedDeriver::default_levels();
    let new_stealth = WalletKeystore::unlock_account(
        &a.keystore_path,
        &a.password,
        req.account,
        &deriver,
    )
    .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;

    let resp = AddressResponse {
        address: encode_address(&new_stealth.address())?,
        fingerprint: fingerprint(&new_stealth.address()),
        account: req.account,
    };

    // Swap the backing storage in place.
    match &state.backend {
        Backend::Single { unlocked, .. } => {
            *unlocked.lock().await = Some(UnlockedWallet {
                stealth: Arc::new(new_stealth),
                account: req.account,
                password: a.password.clone(),
            });
        }
        Backend::Multi { sessions, .. } => {
            if let (Some(tok), Some(username)) = (a.session_token.as_deref(), a.username.as_deref())
            {
                sessions
                    .replace(
                        tok,
                        SessionEntry {
                            username: username.to_string(),
                            keystore_path: a.keystore_path.clone(),
                            account: req.account,
                            stealth: Arc::new(new_stealth),
                            password: a.password.clone(),
                            last_seen_unix: 0,
                        },
                    )
                    .await;
            }
        }
    }
    Ok(Json(resp))
}

async fn handle_address(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<AddressResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    Ok(Json(AddressResponse {
        address: encode_address(&a.stealth.address())?,
        fingerprint: fingerprint(&a.stealth.address()),
        account: a.account,
    }))
}

async fn handle_balance(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<BalanceResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let stealth = state
        .rpc
        .get_balance_for(&a.stealth)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&a);
    let plain_utxos = state
        .rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(ApiError::Wallet)?;
    let plain: u64 = plain_utxos.iter().map(|u| u.value).sum();
    Ok(Json(BalanceResponse {
        balance: stealth.saturating_add(plain),
    }))
}

async fn handle_utxos(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<UtxosResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let stealth = state
        .rpc
        .scan_stealth(&a.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&a);
    let plain = state
        .rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(ApiError::Wallet)?;
    Ok(Json(UtxosResponse { stealth, plain }))
}

fn spend_pubkey_hash(a: &ActiveWallet) -> [u8; 32] {
    qv_script::pubkey_hash(a.stealth.spend_kp.public.as_bytes())
}

async fn handle_send(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, ApiError> {
    if req.amount == 0 {
        return Err(ApiError::BadRequest("amount must be positive".into()));
    }
    let outflow = req
        .amount
        .checked_add(req.fee)
        .ok_or_else(|| ApiError::BadRequest("amount + fee overflows".into()))?;

    let a = state.require_active(&bearer).await?;
    let recipient = decode_address(&req.to_address)?;

    let stealth_utxos = state
        .rpc
        .scan_stealth(&a.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&a);
    let plain_utxos = state
        .rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(ApiError::Wallet)?;

    let total_available: u64 = stealth_utxos.iter().map(|u| u.value).sum::<u64>()
        + plain_utxos.iter().map(|u| u.value).sum::<u64>();
    if total_available < outflow {
        return Err(ApiError::BadRequest(format!(
            "insufficient balance: need {outflow}, have {total_available} (stealth + plain)"
        )));
    }

    enum Pick<'a> {
        Stealth(&'a StealthMatch),
        Plain(&'a P2pkhMatch),
    }
    let mut stealth_sorted: Vec<&StealthMatch> = stealth_utxos.iter().collect();
    stealth_sorted.sort_by(|x, y| y.value.cmp(&x.value));
    let mut plain_sorted: Vec<&P2pkhMatch> = plain_utxos.iter().collect();
    plain_sorted.sort_by(|x, y| y.value.cmp(&x.value));

    let mut picks: Vec<Pick<'_>> = Vec::new();
    let mut total: u64 = 0;
    for u in &stealth_sorted {
        if total >= outflow {
            break;
        }
        total = total.saturating_add(u.value);
        picks.push(Pick::Stealth(u));
    }
    for u in &plain_sorted {
        if total >= outflow {
            break;
        }
        total = total.saturating_add(u.value);
        picks.push(Pick::Plain(u));
    }
    if total < outflow {
        return Err(ApiError::BadRequest(format!(
            "insufficient balance after selection: need {outflow}, picked {total}"
        )));
    }
    let change = total.saturating_sub(outflow);

    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    for pick in &picks {
        let (tx_id_hex, out_idx) = match pick {
            Pick::Stealth(u) => (&u.tx_id, u.output_index),
            Pick::Plain(u) => (&u.tx_id, u.output_index),
        };
        let tx_id_bytes = hex::decode(tx_id_hex)
            .map_err(|e| ApiError::Internal(format!("server returned bad tx_id hex: {e}")))?;
        let tx_id_arr: [u8; 32] = tx_id_bytes
            .as_slice()
            .try_into()
            .map_err(|_| ApiError::Internal("server tx_id is not 32 bytes".into()))?;
        let op = OutPoint::new(TxId::from_bytes(tx_id_arr), out_idx);
        builder.add_input(TxInput::new(op));
    }

    builder.add_stealth_output(Amount::from(req.amount), &recipient)?;
    if change > 0 {
        builder.add_stealth_output(Amount::from(change), &a.stealth.address())?;
    }

    for (idx, pick) in picks.iter().enumerate() {
        match pick {
            Pick::Stealth(u) => {
                let ss_bytes = hex::decode(&u.shared_secret_hex)
                    .map_err(|e| ApiError::Internal(format!("bad shared_secret hex: {e}")))?;
                let ss_arr: [u8; 32] = ss_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| ApiError::Internal("shared_secret must be 32 bytes".into()))?;
                let shared = qv_crypto::SharedSecret(ss_arr);
                builder.sign_stealth_input(
                    idx,
                    &a.stealth.spend_kp.secret,
                    &a.stealth.spend_kp.public,
                    &shared,
                )?;
            }
            Pick::Plain(_) => {
                builder.sign_plain_input(
                    idx,
                    &a.stealth.spend_kp.secret,
                    &a.stealth.spend_kp.public,
                )?;
            }
        }
    }

    let tx = builder.build_unsigned()?;
    let tx_id = tx
        .id()
        .map_err(|e| ApiError::Internal(format!("tx id compute failed: {e}")))?;
    let tx_bytes = bincode::serialize(&tx).map_err(WalletError::Bincode)?;
    let tx_hex = hex::encode(&tx_bytes);

    let rpc_result = state
        .rpc
        .send_transaction(&tx_hex)
        .await
        .map_err(ApiError::Wallet)?;

    // Append to the local journal. Failure to write must NOT clobber a
    // successful broadcast — log and swallow.
    let recipient_fp = fingerprint(&recipient);
    let recipient_label = {
        let book_path = contacts_path_for(&a.keystore_path);
        match load_contacts(&book_path, &a.password) {
            Ok(book) => book.iter().find_map(|(label, c)| {
                if c.address == req.to_address {
                    Some(label.clone())
                } else {
                    None
                }
            }),
            Err(_) => None,
        }
    };
    let history_path = history_path_for(&a.keystore_path);
    if let Err(e) = record_send(
        &history_path,
        &a.password,
        a.account,
        &tx_id.to_hex(),
        req.amount,
        req.fee,
        Some(&recipient_fp),
        recipient_label.as_deref(),
    ) {
        tracing::warn!(?e, "failed to record send to local history (broadcast succeeded)");
    }

    Ok(Json(SendResponse {
        tx_id: tx_id.to_hex(),
        tx_hex,
        broadcast: true,
        rpc_result,
    }))
}

// ---------------------------------------------------------------------------
// DeFi — pool discovery, swap, create-pool (Faz 6 / D-5)
// ---------------------------------------------------------------------------

/// `GET /api/defi/pools` response.
#[derive(Debug, Serialize)]
pub struct PoolsResponse {
    /// Live pools as reported by the node's `qv_listPools` (already
    /// fake-pool-filtered, deterministic outpoint order).
    pub pools: Vec<PoolEntry>,
}

/// `POST /api/defi/swap` request body.
#[derive(Debug, Deserialize)]
pub struct DefiSwapRequest {
    /// Pool UTXO outpoint (`txid#idx` or `txid:idx`).
    pub pool: String,
    /// `"a-to-b"` or `"b-to-a"` (same spelling as the CLI).
    pub direction: String,
    /// Amount of the input token to sell (smallest units).
    pub amount: u64,
    /// Slippage floor: minimum acceptable output amount.
    pub min_receive: u64,
    /// Network fee; defaults to 1000 like `/api/send`.
    #[serde(default = "default_fee")]
    pub fee: u64,
}

/// `POST /api/defi/swap` response.
#[derive(Debug, Serialize)]
pub struct DefiSwapResponse {
    pub tx_id: String,
    pub amount_out: u64,
    pub pool_fee_paid: u64,
    pub new_reserve_a: u64,
    pub new_reserve_b: u64,
    pub new_lp_total: u64,
    pub change: u64,
    pub broadcast: bool,
    pub rpc_result: serde_json::Value,
}

/// `POST /api/defi/create-pool` request body.
#[derive(Debug, Deserialize)]
pub struct DefiCreatePoolRequest {
    /// Token A identifier (32-byte hex).
    pub token_a: String,
    /// Token B identifier (32-byte hex).
    pub token_b: String,
    /// Swap fee in basis points (0..=10000).
    pub fee_bps: u16,
    /// Initial reserve of token A (smallest units).
    pub reserve_a: u64,
    /// Initial reserve of token B (smallest units).
    pub reserve_b: u64,
    /// Native value locked into the pool UTXO; defaults to 1000 (the CLI
    /// default).
    #[serde(default = "default_pool_value")]
    pub pool_value: u64,
    /// Network fee; defaults to 1000.
    #[serde(default = "default_fee")]
    pub fee: u64,
}

fn default_pool_value() -> u64 {
    1000
}

/// `POST /api/defi/create-pool` response.
#[derive(Debug, Serialize)]
pub struct DefiCreatePoolResponse {
    pub tx_id: String,
    /// Canonical `<txid>#0` of the new pool — pass to the swap flow.
    pub pool_outpoint: String,
    pub reserve_a: u64,
    pub reserve_b: u64,
    pub lp_total: u64,
    pub fee_bps: u16,
    pub change: u64,
    /// Honest D-5 scope note (LP accounting is datum-level only).
    pub note: String,
    pub broadcast: bool,
    pub rpc_result: serde_json::Value,
}

/// Map shared-flow errors onto HTTP status codes: user mistakes
/// (`InvalidArg`) become 400s, everything else surfaces as the usual
/// wallet error envelope.
fn defi_api_error(e: WalletError) -> ApiError {
    match e {
        WalletError::InvalidArg(m) => ApiError::BadRequest(m),
        other => ApiError::Wallet(other),
    }
}

async fn handle_defi_pools(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<PoolsResponse>, ApiError> {
    let _a = state.require_active(&bearer).await?;
    let pools = state.rpc.list_pools().await.map_err(ApiError::Wallet)?;
    Ok(Json(PoolsResponse { pools }))
}

async fn handle_defi_swap(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<DefiSwapRequest>,
) -> Result<Json<DefiSwapResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let direction = direction_from_str(&req.direction).map_err(defi_api_error)?;

    // Same core path as `qv-wallet swap` (cmd_swap → execute_swap); the
    // HTTP surface always auto-selects the funding UTXO and broadcasts.
    let params = SwapParams {
        pool: req.pool,
        direction,
        amount_in: req.amount,
        min_receive: req.min_receive,
        input: None,
        input_value: None,
        fee: req.fee,
    };
    let outcome = execute_swap(
        &state.rpc,
        &a.stealth.spend_kp.secret,
        &a.stealth.spend_kp.public,
        &params,
    )
    .await
    .map_err(defi_api_error)?;

    let rpc_result = state
        .rpc
        .send_transaction(&outcome.tx_hex)
        .await
        .map_err(ApiError::Wallet)?;

    Ok(Json(DefiSwapResponse {
        tx_id: outcome.tx_id_hex,
        amount_out: outcome.amount_out,
        pool_fee_paid: outcome.pool_fee_paid,
        new_reserve_a: outcome.new_pool_datum.reserve_a,
        new_reserve_b: outcome.new_pool_datum.reserve_b,
        new_lp_total: outcome.new_pool_datum.lp_total,
        change: outcome.change,
        broadcast: true,
        rpc_result,
    }))
}

async fn handle_defi_create_pool(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<DefiCreatePoolRequest>,
) -> Result<Json<DefiCreatePoolResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;

    // Same core path as `qv-wallet create-pool` (cmd_create_pool →
    // execute_create_pool); the HTTP surface always auto-selects the
    // funding UTXO and broadcasts.
    let params = CreatePoolParams {
        token_a_hex: req.token_a,
        token_b_hex: req.token_b,
        fee_bps: req.fee_bps,
        reserve_a: req.reserve_a,
        reserve_b: req.reserve_b,
        pool_value: req.pool_value,
        input: None,
        input_value: None,
        fee: req.fee,
    };
    let outcome = execute_create_pool(
        &state.rpc,
        &a.stealth.spend_kp.secret,
        &a.stealth.spend_kp.public,
        &params,
    )
    .await
    .map_err(defi_api_error)?;

    let rpc_result = state
        .rpc
        .send_transaction(&outcome.tx_hex)
        .await
        .map_err(ApiError::Wallet)?;

    Ok(Json(DefiCreatePoolResponse {
        tx_id: outcome.tx_id_hex,
        pool_outpoint: outcome.pool_outpoint,
        reserve_a: outcome.pool_datum.reserve_a,
        reserve_b: outcome.pool_datum.reserve_b,
        lp_total: outcome.lp_total,
        fee_bps: outcome.pool_datum.fee_bps,
        change: outcome.change,
        note: "LP shares are datum-level lp_total accounting only — no on-chain LP token; \
               add/remove-liquidity spend paths are D-6+ scope."
            .to_string(),
        broadcast: true,
        rpc_result,
    }))
}

// ---------------------------------------------------------------------------
// QR + .qvaddr handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct QrPartsQuery {
    #[serde(default)]
    pub parts: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct QrPartsResponse {
    pub total: usize,
    pub parts: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportQvaddrRequest {
    pub json: String,
}

#[derive(Debug, Serialize)]
pub struct ImportQvaddrResponse {
    pub address: String,
    pub fingerprint: String,
    pub label: Option<String>,
}

async fn handle_address_qvaddr(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<impl IntoResponse, ApiError> {
    let a = state.require_active(&bearer).await?;
    let addr = a.stealth.address();
    let q = Qvaddr::from_address(&addr, None)?;
    let body = q.to_json()?;
    let filename = format!("qv-account-{}.qvaddr", a.account);
    let headers = [
        (header::CONTENT_TYPE, "application/json".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    Ok((StatusCode::OK, headers, body))
}

async fn handle_fingerprint_svg(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<impl IntoResponse, ApiError> {
    let a = state.require_active(&bearer).await?;
    let fp = fingerprint(&a.stealth.address());
    let svg = render_qr_svg(&fp)?;
    let headers = [(header::CONTENT_TYPE, "image/svg+xml")];
    Ok((StatusCode::OK, headers, svg))
}

async fn handle_address_qr_parts(
    State(state): State<AppState>,
    bearer: Bearer,
    Query(q): Query<QrPartsQuery>,
) -> Result<Json<QrPartsResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let full = encode_address(&a.stealth.address())?;
    let parts = q.parts.unwrap_or(DEFAULT_QR_PARTS).clamp(1, 8);
    let payloads = address_to_qr_parts(&full, parts)?;
    let svgs = payloads
        .iter()
        .map(|p| render_qr_svg(p))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(QrPartsResponse {
        total: svgs.len(),
        parts: svgs,
    }))
}

async fn handle_import_qvaddr(
    Json(req): Json<ImportQvaddrRequest>,
) -> Result<Json<ImportQvaddrResponse>, ApiError> {
    let q = Qvaddr::from_json(&req.json).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(ImportQvaddrResponse {
        address: q.address,
        fingerprint: q.fingerprint,
        label: q.label,
    }))
}

#[derive(Debug, Deserialize)]
pub struct QrReassembleRequest {
    pub parts: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct QrReassembleResponse {
    pub address: String,
}

async fn handle_qr_reassemble(
    Json(req): Json<QrReassembleRequest>,
) -> Result<Json<QrReassembleResponse>, ApiError> {
    let address = address_from_qr_parts(&req.parts)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(QrReassembleResponse { address }))
}

// ---------------------------------------------------------------------------
// Audit-mode view-key export & per-output selective disclosure
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DiscloseRequest {
    pub tx_id: String,
    pub output_index: u32,
    #[serde(default)]
    pub amount: Option<u64>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscloseResponse {
    pub qvdisclose_json: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyDisclosureRequest {
    pub json: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyDisclosureResponse {
    pub valid: bool,
    pub tx_id: String,
    pub output_index: u32,
    pub disclosed_amount: Option<u64>,
    pub label: Option<String>,
    pub spend_pk_fingerprint: String,
}

async fn handle_view_key_export(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<impl IntoResponse, ApiError> {
    let a = state.require_active(&bearer).await?;
    let export = ViewKeyExport::from_keys(
        &a.stealth.view_kp,
        &a.stealth.spend_kp.public,
        Some(format!("Account {} — UI export", a.account)),
    );
    let body = export.to_json()?;
    let filename = format!("qv-account-{}.qvview", a.account);
    let headers = [
        (header::CONTENT_TYPE, "application/json".to_string()),
        (
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ),
    ];
    Ok((StatusCode::OK, headers, body))
}

async fn handle_disclose(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<DiscloseRequest>,
) -> Result<Json<DiscloseResponse>, ApiError> {
    use qv_crypto::{DilithiumLevel, KyberLevel, SharedSecret};

    let a = state.require_active(&bearer).await?;

    let matches = state
        .rpc
        .scan_stealth(&a.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let m = matches
        .iter()
        .find(|m| m.tx_id == req.tx_id && m.output_index == req.output_index)
        .ok_or_else(|| {
            ApiError::BadRequest(format!(
                "outpoint {}:{} is not in this wallet's stealth UTXO set",
                req.tx_id, req.output_index
            ))
        })?;

    let kyber_level = match m.kyber_level {
        1 => KyberLevel::Level1,
        3 => KyberLevel::Level3,
        5 => KyberLevel::Level5,
        other => {
            return Err(ApiError::Internal(format!(
                "unknown Kyber level on chain: {other}"
            )))
        }
    };
    let view_tag_bytes = hex::decode(&m.view_tag_hex)
        .map_err(|e| ApiError::Internal(format!("view_tag hex: {e}")))?;
    if view_tag_bytes.len() != 1 {
        return Err(ApiError::Internal(format!(
            "view_tag must be 1 byte, got {}",
            view_tag_bytes.len()
        )));
    }
    let view_tag = view_tag_bytes[0];

    let ss_bytes = hex::decode(&m.shared_secret_hex)
        .map_err(|e| ApiError::Internal(format!("shared_secret hex: {e}")))?;
    let ss_arr: [u8; 32] = ss_bytes
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::Internal("shared_secret must be 32 bytes".into()))?;
    let shared_secret = SharedSecret(ss_arr);

    let opk_bytes = hex::decode(&m.onetime_pk_hash_hex)
        .map_err(|e| ApiError::Internal(format!("onetime_pk_hash hex: {e}")))?;
    let opk: [u8; 32] = opk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::Internal("onetime_pk_hash must be 32 bytes".into()))?;

    let file = create_disclosure(
        &m.tx_id,
        m.output_index,
        kyber_level,
        DilithiumLevel::Level3,
        &m.kem_ciphertext_hex,
        view_tag,
        &opk,
        &a.stealth.spend_kp.public,
        &shared_secret,
        req.amount,
        req.label,
    )?;
    Ok(Json(DiscloseResponse {
        qvdisclose_json: file.to_json()?,
    }))
}

// ---------------------------------------------------------------------------
// Address book
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ContactDto {
    pub label: String,
    pub address: String,
    pub fingerprint: String,
    pub notes: Option<String>,
    pub added_at: u64,
}

#[derive(Debug, Serialize)]
pub struct ContactsListResponse {
    pub contacts: Vec<ContactDto>,
}

#[derive(Debug, Deserialize)]
pub struct AddContactRequest {
    pub label: String,
    pub address: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoveContactRequest {
    pub label: String,
}

async fn handle_contacts_list(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<ContactsListResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let book_path = contacts_path_for(&a.keystore_path);
    let book = load_contacts(&book_path, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    let contacts = book
        .iter()
        .map(|(label, c)| ContactDto {
            label: label.clone(),
            address: c.address.clone(),
            fingerprint: c.fingerprint.clone(),
            notes: c.notes.clone(),
            added_at: c.added_at,
        })
        .collect();
    Ok(Json(ContactsListResponse { contacts }))
}

async fn handle_contacts_add(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<AddContactRequest>,
) -> Result<Json<ContactDto>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let book_path = contacts_path_for(&a.keystore_path);
    let mut book = load_contacts(&book_path, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    book.add(&req.label, &req.address, req.notes)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    save_contacts(&book_path, &book, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    let c = book
        .get(&req.label)
        .ok_or_else(|| ApiError::Internal("contact disappeared after add".into()))?;
    Ok(Json(ContactDto {
        label: req.label,
        address: c.address.clone(),
        fingerprint: c.fingerprint.clone(),
        notes: c.notes.clone(),
        added_at: c.added_at,
    }))
}

async fn handle_contacts_remove(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<RemoveContactRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let book_path = contacts_path_for(&a.keystore_path);
    let mut book = load_contacts(&book_path, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    let removed = book.remove(&req.label).map_err(ApiError::Wallet)?;
    save_contacts(&book_path, &book, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;
    Ok(Json(serde_json::json!({
        "removed_label": req.label,
        "removed_fingerprint": removed.fingerprint,
    })))
}

// ---------------------------------------------------------------------------
// Transaction history
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub entries: Vec<HistoryEntry>,
    pub current_account: u32,
}

async fn handle_history_list(
    State(state): State<AppState>,
    bearer: Bearer,
) -> Result<Json<HistoryResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let path = history_path_for(&a.keystore_path);
    let log = load_history(&path, &a.password)
        .map_err(|e| ApiError::Wallet(WalletError::Keystore(e.to_string())))?;

    let stealth_matches = state
        .rpc
        .scan_stealth(&a.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&a);
    let plain_matches = state
        .rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(ApiError::Wallet)?;

    let stealth_rows: Vec<ReceivedRow> = stealth_matches
        .iter()
        .map(|m| ReceivedRow {
            tx_id: m.tx_id.clone(),
            output_index: m.output_index,
            value: m.value,
        })
        .collect();
    let plain_rows: Vec<ReceivedRow> = plain_matches
        .iter()
        .map(|m| ReceivedRow {
            tx_id: m.tx_id.clone(),
            output_index: m.output_index,
            value: m.value,
        })
        .collect();

    let now = now_unix_secs();
    let entries = merge_with_received(&log, &stealth_rows, &plain_rows, now, a.account);
    Ok(Json(HistoryResponse {
        entries,
        current_account: a.account,
    }))
}

// ---------------------------------------------------------------------------
// Devnet faucet
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct FaucetRequest {
    #[serde(default = "default_faucet_amount")]
    pub amount: u64,
    #[serde(default = "default_fee")]
    pub fee: u64,
}

fn default_faucet_amount() -> u64 {
    1_000_000
}

#[derive(Debug, Serialize)]
pub struct FaucetResponse {
    pub tx_id: String,
    pub tx_hex: String,
    pub amount: u64,
    pub fee: u64,
    pub recipient_address: String,
    pub recipient_fingerprint: String,
    pub rpc_result: serde_json::Value,
}

async fn handle_devnet_faucet(
    State(state): State<AppState>,
    bearer: Bearer,
    Json(req): Json<FaucetRequest>,
) -> Result<Json<FaucetResponse>, ApiError> {
    let a = state.require_active(&bearer).await?;
    let recipient_address = encode_address(&a.stealth.address())?;
    let recipient_fp = fingerprint(&a.stealth.address());

    let receipt = faucet_drip(&state.rpc, &recipient_address, req.amount, req.fee)
        .await
        .map_err(|e| match e {
            WalletError::InvalidArg(m) => ApiError::BadRequest(m),
            other => ApiError::Wallet(other),
        })?;

    Ok(Json(FaucetResponse {
        tx_id: receipt.tx_id_hex,
        tx_hex: receipt.tx_hex,
        amount: receipt.amount,
        fee: receipt.fee,
        recipient_address,
        recipient_fingerprint: recipient_fp,
        rpc_result: receipt.rpc_result,
    }))
}

async fn handle_verify_disclosure(
    Json(req): Json<VerifyDisclosureRequest>,
) -> Result<Json<VerifyDisclosureResponse>, ApiError> {
    let file =
        DisclosureFile::from_json(&req.json).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let valid = file.verify_self_contained()?;
    let fp = {
        let s = &file.spend_pk_hex;
        if s.len() <= 16 {
            s.clone()
        } else {
            format!("{}…{}", &s[..8], &s[s.len() - 8..])
        }
    };
    Ok(Json(VerifyDisclosureResponse {
        valid,
        tx_id: file.tx_id_hex,
        output_index: file.output_index,
        disclosed_amount: file.disclosed_amount,
        label: file.label,
        spend_pk_fingerprint: fp,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn single_appstate_starts_locked() {
        let s = AppState::new(PathBuf::from("./qv-test-keystore.json"), "http://localhost".into());
        let result = s.require_active(&Bearer(None)).await;
        assert!(result.is_err(), "must be locked at start");
    }

    #[tokio::test]
    async fn multi_appstate_requires_bearer() {
        let s = AppState::new_multi(
            PathBuf::from("./qv-test-wallets"),
            "http://localhost".into(),
            Duration::from_secs(3600),
        );
        let result = s.require_active(&Bearer(None)).await;
        assert!(matches!(result, Err(ApiError::Unauthorized(_))));
    }

    #[tokio::test]
    async fn multi_appstate_rejects_unknown_token() {
        let s = AppState::new_multi(
            PathBuf::from("./qv-test-wallets"),
            "http://localhost".into(),
            Duration::from_secs(3600),
        );
        let result = s.require_active(&Bearer(Some("deadbeef".into()))).await;
        assert!(matches!(result, Err(ApiError::Unauthorized(_))));
    }

    #[test]
    fn default_fee_is_thousand() {
        assert_eq!(default_fee(), 1000);
    }

    #[test]
    fn send_request_default_fee_via_serde() {
        let json = r#"{ "to_address": "qvst1deadbeef", "amount": 42 }"#;
        let req: SendRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.fee, 1000);
        assert_eq!(req.amount, 42);
    }

    #[test]
    fn defi_swap_request_defaults_fee_via_serde() {
        let json = r#"{ "pool": "ab#0", "direction": "a-to-b", "amount": 10, "min_receive": 9 }"#;
        let req: DefiSwapRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.fee, 1000);
        assert_eq!(req.pool, "ab#0");
        assert_eq!(req.direction, "a-to-b");
        assert_eq!(req.amount, 10);
        assert_eq!(req.min_receive, 9);
    }

    #[test]
    fn defi_create_pool_request_defaults_via_serde() {
        let json = r#"{
            "token_a": "a1", "token_b": "b2",
            "fee_bps": 30, "reserve_a": 1000, "reserve_b": 2000
        }"#;
        let req: DefiCreatePoolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.pool_value, 1000);
        assert_eq!(req.fee, 1000);
        assert_eq!(req.fee_bps, 30);
    }

    #[test]
    fn defi_api_error_maps_invalid_arg_to_bad_request() {
        let e = defi_api_error(WalletError::InvalidArg("nope".into()));
        assert!(matches!(e, ApiError::BadRequest(m) if m == "nope"));
        let e = defi_api_error(WalletError::Rpc("down".into()));
        assert!(matches!(e, ApiError::Wallet(WalletError::Rpc(_))));
    }
}
