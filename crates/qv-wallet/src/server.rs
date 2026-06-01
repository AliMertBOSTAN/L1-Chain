//! Local HTTP API + embedded UI for the stealth wallet (ADR-011 Faz 5).
//!
//! This module exposes a tiny axum server that wraps the same building
//! blocks the CLI uses (`Mnemonic`, `WalletKeystore`, `TxBuilder`,
//! `RpcClient`) and serves a single-page browser UI from the same binary.
//!
//! The server is intended to be run **locally** by the wallet owner — it
//! binds to `127.0.0.1:<port>` and never to a public interface. The
//! decrypted keys live in memory only while the wallet is unlocked; locking
//! drops them (zeroize-on-drop kicks in via [`qv_crypto::SecureBytes`] and
//! [`qv_crypto::SharedSecret`]).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use qv_core::{Amount, OutPoint, TxId, TxInput, TxOutput, ValidityInterval};
use qv_privacy::StealthKeys;

use crate::address::{decode_address, encode_address, fingerprint};
use crate::hd::{DefaultSeedDeriver, SeedDeriver};
use crate::keystore::{
    PersistedViewKey, WalletKeystore, WalletMetadata, WalletSecret,
};
use crate::qvaddr::{
    address_from_qr_parts, address_to_qr_parts, render_qr_svg, Qvaddr, DEFAULT_QR_PARTS,
};
use crate::rpc_client::{P2pkhMatch, RpcClient, StealthMatch};
use crate::tx_builder::TxBuilder;
use crate::{Mnemonic, WalletError};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// In-memory unlocked wallet. Dropped on lock — the spend secret zeroizes
/// automatically via `SecureBytes` inside [`StealthKeys`].
#[derive(Clone)]
struct UnlockedWallet {
    stealth: Arc<StealthKeys>,
    account: u32,
}

/// Server-wide state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    keystore_path: PathBuf,
    rpc: Arc<RpcClient>,
    unlocked: Arc<Mutex<Option<UnlockedWallet>>>,
}

impl AppState {
    /// Build server state bound to a keystore path and a node RPC URL.
    #[must_use]
    pub fn new(keystore_path: PathBuf, rpc_url: String) -> Self {
        Self {
            keystore_path,
            rpc: Arc::new(RpcClient::new(rpc_url)),
            unlocked: Arc::new(Mutex::new(None)),
        }
    }

    async fn require_unlocked(&self) -> Result<UnlockedWallet, ApiError> {
        self.unlocked.lock().await.clone().ok_or_else(|| {
            ApiError::Unauthorized("wallet is locked — call /api/wallet/unlock first".into())
        })
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
    pub unlocked: bool,
    pub address: Option<String>,
    pub fingerprint: Option<String>,
    pub account: Option<u32>,
    pub rpc_url: String,
    pub keystore_exists: bool,
}

// ---------------------------------------------------------------------------
// Router construction
// ---------------------------------------------------------------------------

/// Build the axum [`Router`] for the wallet HTTP API + UI.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/api/status", get(handle_status))
        .route("/api/wallet/create", post(handle_create))
        .route("/api/wallet/import", post(handle_import))
        .route("/api/wallet/unlock", post(handle_unlock))
        .route("/api/wallet/lock", post(handle_lock))
        .route("/api/wallet/address", get(handle_address))
        .route("/api/wallet/address.qvaddr", get(handle_address_qvaddr))
        .route("/api/wallet/fingerprint.svg", get(handle_fingerprint_svg))
        .route("/api/wallet/address-qr", get(handle_address_qr_parts))
        .route("/api/wallet/import-qvaddr", post(handle_import_qvaddr))
        .route("/api/wallet/qr-reassemble", post(handle_qr_reassemble))
        .route("/api/balance", get(handle_balance))
        .route("/api/utxos", get(handle_utxos))
        .route("/api/send", post(handle_send))
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
// Handlers
// ---------------------------------------------------------------------------

async fn serve_index() -> Html<&'static str> {
    Html(crate::server_ui::INDEX_HTML)
}

async fn handle_status(State(state): State<AppState>) -> impl IntoResponse {
    let unlocked = state.unlocked.lock().await.clone();
    let resp = match unlocked {
        Some(w) => StatusResponse {
            unlocked: true,
            address: Some(encode_address(&w.stealth.address()).unwrap_or_default()),
            fingerprint: Some(fingerprint(&w.stealth.address())),
            account: Some(w.account),
            rpc_url: state.rpc.url().to_string(),
            keystore_exists: state.keystore_path.exists(),
        },
        None => StatusResponse {
            unlocked: false,
            address: None,
            fingerprint: None,
            account: None,
            rpc_url: state.rpc.url().to_string(),
            keystore_exists: state.keystore_path.exists(),
        },
    };
    (StatusCode::OK, Json(resp)).into_response()
}

async fn handle_create(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    if state.keystore_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "keystore already exists at {} — refusing to overwrite",
            state.keystore_path.display()
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
    WalletKeystore::save(&state.keystore_path, &secret, &req.password)
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
    if state.keystore_path.exists() {
        return Err(ApiError::BadRequest(format!(
            "keystore already exists at {} — refusing to overwrite",
            state.keystore_path.display()
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
    WalletKeystore::save(&state.keystore_path, &secret, &req.password)
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
    let account = req.account.unwrap_or(0);
    let deriver = DefaultSeedDeriver::default_levels();
    // `unlock_account` reuses a persisted view keypair if present, else
    // generates one and re-saves the keystore — first unlock of each
    // account upgrades the file in place.
    let stealth = WalletKeystore::unlock_account(
        &state.keystore_path,
        &req.password,
        account,
        &deriver,
    )
    .map_err(|e| ApiError::Unauthorized(format!("unlock failed: {e}")))?;

    let unlocked = UnlockedWallet {
        stealth: Arc::new(stealth),
        account,
    };
    let resp = AddressResponse {
        address: encode_address(&unlocked.stealth.address())?,
        fingerprint: fingerprint(&unlocked.stealth.address()),
        account,
    };
    *state.unlocked.lock().await = Some(unlocked);
    Ok(Json(resp))
}

async fn handle_lock(State(state): State<AppState>) -> impl IntoResponse {
    *state.unlocked.lock().await = None;
    (StatusCode::OK, Json(serde_json::json!({ "locked": true })))
}

async fn handle_address(
    State(state): State<AppState>,
) -> Result<Json<AddressResponse>, ApiError> {
    let w = state.require_unlocked().await?;
    Ok(Json(AddressResponse {
        address: encode_address(&w.stealth.address())?,
        fingerprint: fingerprint(&w.stealth.address()),
        account: w.account,
    }))
}

async fn handle_balance(
    State(state): State<AppState>,
) -> Result<Json<BalanceResponse>, ApiError> {
    let w = state.require_unlocked().await?;
    let stealth = state
        .rpc
        .get_balance_for(&w.stealth)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&w);
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
) -> Result<Json<UtxosResponse>, ApiError> {
    let w = state.require_unlocked().await?;
    let stealth = state
        .rpc
        .scan_stealth(&w.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&w);
    let plain = state
        .rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(ApiError::Wallet)?;
    Ok(Json(UtxosResponse { stealth, plain }))
}

/// `SHA3-256` of the wallet's static Dilithium spend public key — used as
/// the pubkey-hash argument to `qv_scanP2pkh` and as the lock target for
/// `p2pkh_pqc` outputs paid to ourselves.
fn spend_pubkey_hash(w: &UnlockedWallet) -> [u8; 32] {
    qv_script::pubkey_hash(w.stealth.spend_kp.public.as_bytes())
}

async fn handle_send(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, ApiError> {
    if req.amount == 0 {
        return Err(ApiError::BadRequest("amount must be positive".into()));
    }
    let outflow = req
        .amount
        .checked_add(req.fee)
        .ok_or_else(|| ApiError::BadRequest("amount + fee overflows".into()))?;

    let w = state.require_unlocked().await?;
    let recipient = decode_address(&req.to_address)?;

    // 1. Fetch both pools — stealth UTXOs we already detected, and any
    //    plain p2pkh_pqc UTXOs locked to our spend pubkey hash (typically
    //    devnet genesis allocations or pre-stealth sends).
    let stealth_utxos = state
        .rpc
        .scan_stealth(&w.stealth, 0, u64::MAX)
        .await
        .map_err(ApiError::Wallet)?;
    let pk_hash = spend_pubkey_hash(&w);
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

    // 2. Coin selection — prefer stealth UTXOs first so plain genesis
    //    funds get rolled into stealth on the first send. Within each
    //    pool, greedy largest-first.
    enum Pick<'a> {
        Stealth(&'a StealthMatch),
        Plain(&'a P2pkhMatch),
    }
    let mut stealth_sorted: Vec<&StealthMatch> = stealth_utxos.iter().collect();
    stealth_sorted.sort_by(|a, b| b.value.cmp(&a.value));
    let mut plain_sorted: Vec<&P2pkhMatch> = plain_utxos.iter().collect();
    plain_sorted.sort_by(|a, b| b.value.cmp(&a.value));

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

    // 3. Build the transaction skeleton — inputs in the order they were picked.
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

    // 3a. Stealth output to the recipient.
    builder.add_stealth_output(Amount::from(req.amount), &recipient)?;

    // 3b. Change back to ourselves as a fresh stealth output — the output
    //     is unlinkable from any prior UTXO even if a plain input was used.
    if change > 0 {
        builder.add_stealth_output(Amount::from(change), &w.stealth.address())?;
    }

    // 4. Sign each input with the witness shape its locking script expects.
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
                    &w.stealth.spend_kp.secret,
                    &w.stealth.spend_kp.public,
                    &shared,
                )?;
            }
            Pick::Plain(_) => {
                builder.sign_plain_input(
                    idx,
                    &w.stealth.spend_kp.secret,
                    &w.stealth.spend_kp.public,
                )?;
            }
        }
    }

    // 5. Encode and broadcast.
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

    Ok(Json(SendResponse {
        tx_id: tx_id.to_hex(),
        tx_hex,
        broadcast: true,
        rpc_result,
    }))
}

// ---------------------------------------------------------------------------
// QR + .qvaddr handlers
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct QrPartsQuery {
    /// How many QR codes to split the full address across. Defaults to 2
    /// — enough for any Kyber-3 + Dilithium-3 address with margin.
    #[serde(default)]
    pub parts: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct QrPartsResponse {
    /// Number of QR codes returned (== `parts.len()`).
    pub total: usize,
    /// One SVG per QR code, in scan order (`k = 1..=total`).
    pub parts: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImportQvaddrRequest {
    /// Raw JSON content of a `.qvaddr` file. Validated server-side so the
    /// UI can keep its parsing logic trivial.
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
) -> Result<impl IntoResponse, ApiError> {
    let w = state.require_unlocked().await?;
    let addr = w.stealth.address();
    let q = Qvaddr::from_address(&addr, None)?;
    let body = q.to_json()?;
    let filename = format!("qv-account-{}.qvaddr", w.account);
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
) -> Result<impl IntoResponse, ApiError> {
    let w = state.require_unlocked().await?;
    let fp = fingerprint(&w.stealth.address());
    let svg = render_qr_svg(&fp)?;
    let headers = [(header::CONTENT_TYPE, "image/svg+xml")];
    Ok((StatusCode::OK, headers, svg))
}

async fn handle_address_qr_parts(
    State(state): State<AppState>,
    Query(q): Query<QrPartsQuery>,
) -> Result<Json<QrPartsResponse>, ApiError> {
    let w = state.require_unlocked().await?;
    let full = encode_address(&w.stealth.address())?;
    let parts = q.parts.unwrap_or(DEFAULT_QR_PARTS).max(1).min(8);
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

// ---------------------------------------------------------------------------
// QR reassembly helper (exposed so a scanner UI can validate parts before
// passing the recombined address to `/api/send`).
// ---------------------------------------------------------------------------

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
    async fn appstate_starts_locked() {
        let s = AppState::new(PathBuf::from("./qv-test-keystore.json"), "http://localhost".into());
        assert!(s.unlocked.lock().await.is_none());
    }

    #[test]
    fn default_fee_is_thousand() {
        assert_eq!(default_fee(), 1000);
    }

    #[test]
    fn send_request_default_fee_via_serde() {
        // Omitting `fee` should fall back to default_fee() = 1000.
        let json = r#"{ "to_address": "qvst1deadbeef", "amount": 42 }"#;
        let req: SendRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.fee, 1000);
        assert_eq!(req.amount, 42);
    }
}
