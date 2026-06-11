//! JSON-RPC client for talking to a `qv-node` over HTTP.
use crate::{WalletError, WalletResult};
use qv_privacy::StealthKeys;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Thin JSON-RPC 2.0 over HTTP client.
pub struct RpcClient {
    url: String,
    client: Client,
}

impl std::fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcClient").field("url", &self.url).finish()
    }
}

/// One match returned by `qv_scanStealth` — mirror of the server's
/// `StealthScan` wire format (kept loosely coupled with serde, no qv-node
/// dep to avoid a workspace cycle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthMatch {
    /// Reserved — currently always `0`.
    pub height: u64,
    /// Hex-encoded funding `TxId`.
    pub tx_id: String,
    /// Output index inside the funding transaction.
    pub output_index: u32,
    /// Value (smallest units).
    pub value: u64,
    /// Recovered shared secret (hex). Needed to spend this UTXO.
    pub shared_secret_hex: String,
    /// One-time PK hash commitment from the locking script (hex).
    pub onetime_pk_hash_hex: String,
    /// Hybrid-KEM ciphertext from the on-chain `StealthInfo` (hex).
    /// Needed when producing selective-disclosure proofs.
    #[serde(default)]
    pub kem_ciphertext_hex: String,
    /// 1-byte view tag from the on-chain `StealthInfo` (2-char hex).
    #[serde(default)]
    pub view_tag_hex: String,
    /// Kyber parameter set baked into the on-chain `StealthInfo`.
    #[serde(default)]
    pub kyber_level: u8,
}

/// One match returned by `qv_scanP2pkh` — a plain `p2pkh_pqc` UTXO
/// locked to the wallet's static spend public-key hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2pkhMatch {
    /// Hex-encoded funding `TxId`.
    pub tx_id: String,
    /// Output index inside the funding transaction.
    pub output_index: u32,
    /// Value (smallest units).
    pub value: u64,
}

/// One live AMM pool returned by `qv_listPools` — mirror of the server's
/// `PoolInfo` wire format (kept loosely coupled with serde, no qv-node dep
/// to avoid a workspace cycle). Faz 6 / D-5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    /// Pool UTXO outpoint in canonical `<txid_hex>#<idx>` form — feed it
    /// directly to the swap flow's `--pool` argument.
    pub outpoint: String,
    /// Native value carried by the pool UTXO (smallest units).
    pub value: u64,
    /// Token A identifier (32-byte hex).
    pub token_a_id: String,
    /// Token B identifier (32-byte hex).
    pub token_b_id: String,
    /// Current reserve of token A (smallest units).
    pub reserve_a: u64,
    /// Current reserve of token B (smallest units).
    pub reserve_b: u64,
    /// Total LP shares issued (datum-level accounting; no on-chain LP token).
    pub lp_total: u64,
    /// Swap fee in basis points.
    pub fee_bps: u16,
}

/// `qv_getUtxo` response — mirror of the server's `UtxoInfo` wire format
/// (kept loosely coupled with serde, no qv-node dep to avoid a workspace
/// cycle).
///
/// `script_hex` / `datum_hex` are the Faz 6 (D-4) extension used by the
/// swap flow; they are `#[serde(default)]` so responses from pre-extension
/// nodes still parse (fields absent → `None`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoInfo {
    /// Value (smallest units).
    pub value: u64,
    /// Hex of the locking script's hash.
    pub script_hash: String,
    /// Whether the UTXO carries a datum.
    pub has_datum: bool,
    /// Whether the UTXO carries stealth info.
    pub has_stealth: bool,
    /// Raw locking-script bytes, hex-encoded (node ≥ Faz 6 D-4).
    #[serde(default)]
    pub script_hex: Option<String>,
    /// Raw datum bytes, hex-encoded; `None` if there is no datum or the
    /// node predates the extension (check `has_datum` to distinguish).
    #[serde(default)]
    pub datum_hex: Option<String>,
}

impl RpcClient {
    /// Build a client pointing at the given HTTP endpoint, e.g.
    /// `http://127.0.0.1:8080`.
    pub fn new(url: impl Into<String>) -> Self {
        RpcClient {
            url: url.into(),
            client: Client::new(),
        }
    }

    /// Issue a generic JSON-RPC call. Returns the `result` field on success;
    /// any `error` field is propagated as [`WalletError::Rpc`].
    pub async fn call(&self, method: &str, params: Value) -> WalletResult<Value> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });

        let response = self
            .client
            .post(&self.url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| WalletError::Rpc(format!("request: {e}")))?;

        if !response.status().is_success() {
            return Err(WalletError::Rpc(format!("http {}", response.status())));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| WalletError::Rpc(format!("decode: {e}")))?;

        if let Some(err) = body.get("error") {
            return Err(WalletError::Rpc(format!("rpc error: {err}")));
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| WalletError::Rpc("no result field in response".into()))
    }

    /// Same as [`Self::call`] but accepts a positional `Vec<Value>` (the
    /// legacy shape used by older callers).
    pub async fn call_positional(
        &self,
        method: &str,
        params: Vec<Value>,
    ) -> WalletResult<Value> {
        self.call(method, Value::Array(params)).await
    }

    /// Wire-encode a `StealthKeys` into the JSON object that
    /// `qv_getBalanceFor` / `qv_scanStealth` expect.
    ///
    /// **Important.** This exposes the recipient's view secret keys — never
    /// send it to anyone but your own trusted node.
    fn view_key_payload(keys: &StealthKeys) -> Value {
        Self::view_key_payload_from_parts(&keys.view_kp, &keys.spend_kp.public)
    }

    /// Same as [`Self::view_key_payload`] but takes the components
    /// individually — used by the audit-mode scan path that never has a
    /// full `StealthKeys` (the spend secret is intentionally absent).
    fn view_key_payload_from_parts(
        view_kp: &qv_crypto::HybridKeyPair,
        spend_pk: &qv_crypto::PqcPublicKey,
    ) -> Value {
        let kyber_level = match view_kp.public.level {
            qv_crypto::KyberLevel::Level1 => 1,
            qv_crypto::KyberLevel::Level3 => 3,
            qv_crypto::KyberLevel::Level5 => 5,
        };
        let dilithium_level = match spend_pk.level() {
            qv_crypto::DilithiumLevel::Level2 => 2,
            qv_crypto::DilithiumLevel::Level3 => 3,
            qv_crypto::DilithiumLevel::Level5 => 5,
        };
        json!({
            "kyber_level": kyber_level,
            "dilithium_level": dilithium_level,
            "x25519_pk_hex": hex::encode(view_kp.public.x25519),
            "x25519_sk_hex": hex::encode(view_kp.x25519_secret_bytes()),
            "kyber_pk_hex": hex::encode(&view_kp.public.kyber),
            "kyber_sk_hex": hex::encode(view_kp.kyber_secret_bytes()),
            "spend_pk_hex": hex::encode(spend_pk.as_bytes()),
        })
    }

    /// Query the total stealth-detectable balance for `keys` (`qv_getBalanceFor`).
    pub async fn get_balance_for(&self, keys: &StealthKeys) -> WalletResult<u64> {
        let result = self
            .call("qv_getBalanceFor", json!([Self::view_key_payload(keys)]))
            .await?;
        result
            .as_u64()
            .ok_or_else(|| WalletError::Rpc(format!("expected u64 balance, got {result}")))
    }

    /// **Audit-mode** scan — call `qv_scanStealth` with a view keypair +
    /// spend **public** key only (no spend secret needed). Used by
    /// auditors holding a `.qvview` export.
    pub async fn scan_stealth_with_view_key(
        &self,
        view_kp: &qv_crypto::HybridKeyPair,
        spend_pk: &qv_crypto::PqcPublicKey,
        from_height: u64,
        to_height: u64,
    ) -> WalletResult<Vec<StealthMatch>> {
        let payload = Self::view_key_payload_from_parts(view_kp, spend_pk);
        let result = self
            .call("qv_scanStealth", json!([payload, from_height, to_height]))
            .await?;
        serde_json::from_value::<Vec<StealthMatch>>(result)
            .map_err(|e| WalletError::Rpc(format!("scan response parse: {e}")))
    }

    /// Scan the live UTXO set for stealth outputs that `keys` can detect
    /// (`qv_scanStealth`). The height range is currently best-effort — see
    /// the node-side docs.
    pub async fn scan_stealth(
        &self,
        keys: &StealthKeys,
        from_height: u64,
        to_height: u64,
    ) -> WalletResult<Vec<StealthMatch>> {
        let result = self
            .call(
                "qv_scanStealth",
                json!([Self::view_key_payload(keys), from_height, to_height]),
            )
            .await?;
        serde_json::from_value::<Vec<StealthMatch>>(result)
            .map_err(|e| WalletError::Rpc(format!("scan response parse: {e}")))
    }

    /// Submit a hex-encoded bincode transaction (`qv_sendTransaction`).
    pub async fn send_transaction(&self, tx_hex: &str) -> WalletResult<Value> {
        self.call("qv_sendTransaction", json!([tx_hex])).await
    }

    /// Scan the UTXO set for plain `p2pkh_pqc` outputs locked to the given
    /// 32-byte pubkey hash (`qv_scanP2pkh`). Used by wallets to discover
    /// non-stealth funds such as genesis allocations.
    pub async fn scan_p2pkh(
        &self,
        pubkey_hash: &[u8; 32],
    ) -> WalletResult<Vec<P2pkhMatch>> {
        let result = self
            .call("qv_scanP2pkh", json!([hex::encode(pubkey_hash)]))
            .await?;
        serde_json::from_value::<Vec<P2pkhMatch>>(result)
            .map_err(|e| WalletError::Rpc(format!("scan_p2pkh response parse: {e}")))
    }

    /// Fetch a single live UTXO by outpoint (`qv_getUtxo`). The outpoint
    /// string may use either the canonical `txid#idx` or the Bitcoin-style
    /// `txid:idx` form — the node parses both. Returns `None` when the
    /// UTXO does not exist (never created, or already spent).
    pub async fn get_utxo(&self, outpoint: &str) -> WalletResult<Option<UtxoInfo>> {
        let result = self.call("qv_getUtxo", json!([outpoint])).await?;
        serde_json::from_value::<Option<UtxoInfo>>(result)
            .map_err(|e| WalletError::Rpc(format!("get_utxo response parse: {e}")))
    }

    /// Discover every live AMM pool UTXO on the node (`qv_listPools`,
    /// Faz 6 / D-5). The node already filters fake pools (script bytes
    /// must match the `amm_pool_lock` regenerated from the datum), and
    /// returns entries in deterministic outpoint order.
    pub async fn list_pools(&self) -> WalletResult<Vec<PoolEntry>> {
        let result = self.call("qv_listPools", json!([])).await?;
        serde_json::from_value::<Vec<PoolEntry>>(result)
            .map_err(|e| WalletError::Rpc(format!("list_pools response parse: {e}")))
    }

    /// Borrow the configured RPC URL (useful for logging / UI display).
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn client_debug_contains_url() {
        let c = RpcClient::new("http://localhost:1234");
        let dbg = format!("{c:?}");
        assert!(dbg.contains("http://localhost:1234"));
    }

    #[test]
    fn utxo_info_parses_extended_and_legacy_json() {
        // New node: script_hex + datum_hex present.
        let full = r#"{
            "value": 1000,
            "script_hash": "ab",
            "has_datum": true,
            "has_stealth": false,
            "script_hex": "51",
            "datum_hex": "00ff"
        }"#;
        let info: UtxoInfo = serde_json::from_str(full).unwrap();
        assert_eq!(info.value, 1000);
        assert_eq!(info.script_hex.as_deref(), Some("51"));
        assert_eq!(info.datum_hex.as_deref(), Some("00ff"));

        // Old node: fields absent must default to None, not fail.
        let legacy =
            r#"{"value":7,"script_hash":"cd","has_datum":false,"has_stealth":true}"#;
        let info: UtxoInfo = serde_json::from_str(legacy).unwrap();
        assert_eq!(info.value, 7);
        assert!(info.script_hex.is_none());
        assert!(info.datum_hex.is_none());
    }

    #[test]
    fn pool_entry_parses_node_json() {
        let json = r#"{
            "outpoint": "aa#0",
            "value": 1000,
            "token_a_id": "a1",
            "token_b_id": "b2",
            "reserve_a": 10000,
            "reserve_b": 20000,
            "lp_total": 14142,
            "fee_bps": 30
        }"#;
        let p: PoolEntry = serde_json::from_str(json).unwrap();
        assert_eq!(p.outpoint, "aa#0");
        assert_eq!(p.value, 1000);
        assert_eq!(p.reserve_a, 10_000);
        assert_eq!(p.reserve_b, 20_000);
        assert_eq!(p.lp_total, 14_142);
        assert_eq!(p.fee_bps, 30);
    }

    #[test]
    fn view_key_payload_shape_is_correct() {
        use qv_crypto::{DilithiumLevel, KyberLevel};
        let keys = StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).unwrap();
        let v = RpcClient::view_key_payload(&keys);
        // Required fields are present and well-typed.
        assert_eq!(v["kyber_level"], 3);
        assert_eq!(v["dilithium_level"], 3);
        for field in [
            "x25519_pk_hex",
            "x25519_sk_hex",
            "kyber_pk_hex",
            "kyber_sk_hex",
            "spend_pk_hex",
        ] {
            assert!(v[field].is_string(), "{field} must be a hex string");
        }
    }
}
