//! JSON-RPC client.
use crate::{WalletError, WalletResult};
use reqwest::Client;
use serde_json::{json, Value};

pub struct RpcClient {
    url: String,
    client: Client,
}

impl std::fmt::Debug for RpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcClient").field("url", &self.url).finish()
    }
}

impl RpcClient {
    pub fn new(url: impl Into<String>) -> Self {
        RpcClient {
            url: url.into(),
            client: Client::new(),
        }
    }

    pub async fn call(&self, method: &str, params: Vec<Value>) -> WalletResult<Value> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });

        let response = self.client.post(&self.url).json(&payload).send().await
            .map_err(|e| WalletError::Rpc(format!("request: {}", e)))?;

        if !response.status().is_success() {
            return Err(WalletError::Rpc(format!("http {}", response.status())));
        }

        let body: Value = response.json().await
            .map_err(|e| WalletError::Rpc(format!("decode: {}", e)))?;

        if body.get("error").is_some() {
            return Err(WalletError::Rpc("rpc error".into()));
        }

        body.get("result").cloned()
            .ok_or_else(|| WalletError::Rpc("no result".into()))
    }
}
