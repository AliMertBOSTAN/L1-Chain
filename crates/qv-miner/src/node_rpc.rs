//! JSON-RPC client used by the operator daemon to query the local `qv-node`.
//!
//! The miner needs four pieces of live information from the node:
//!
//! 1. **Stake distribution** (`qv_getStakeDistribution`) — the frozen
//!    per-epoch snapshot of `(pool_id, stake)` pairs used by VRF leader
//!    election.
//! 2. **Epoch nonce** (`qv_getEpochNonce`) — the 32-byte seed parameterising
//!    leadership evaluation for the current epoch.
//! 3. **Mempool snapshot** (`qv_getMempoolStatus`) — used by the block
//!    producer; lives elsewhere but uses this same transport.
//! 4. **Block submission** (`qv_submitBlock`, M-09c) — the path by which
//!    the operator publishes blocks it has produced.
//!
//! This module is the transport layer only; it does not interpret protocol
//! semantics. Callers should reconstruct typed `StakeDistribution` /
//! `EpochNonce` values from the deserialised payloads.

use crate::{MinerError, MinerResult};
use qv_consensus::epoch::EpochNonce;
use qv_consensus::stake::{PoolId, StakeDistribution};
use qv_core::{Amount, Epoch, Hash256};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Lightweight JSON-RPC 2.0 client targeting a single `qv-node` endpoint.
///
/// The client is `Clone` because the underlying `reqwest::Client` keeps a
/// connection pool internally; cloning is cheap and shares state.
#[derive(Clone)]
pub struct NodeRpcClient {
    url: String,
    inner: Client,
}

impl std::fmt::Debug for NodeRpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRpcClient")
            .field("url", &self.url)
            .finish()
    }
}

impl NodeRpcClient {
    /// Build a client targeting `url` (e.g. `http://localhost:8080`).
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            inner: Client::new(),
        }
    }

    /// Make a single JSON-RPC call.
    ///
    /// Returns the `result` field on success. Surfaces transport errors,
    /// HTTP-level failures, and explicit JSON-RPC `error` objects as
    /// `MinerError::RpcError` with the original message attached.
    pub async fn call(&self, method: &str, params: Vec<Value>) -> MinerResult<Value> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });

        let response = self
            .inner
            .post(&self.url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| MinerError::RpcError(format!("{method}: request: {e}")))?;

        if !response.status().is_success() {
            return Err(MinerError::RpcError(format!(
                "{method}: http {}",
                response.status()
            )));
        }

        let body: Value = response
            .json()
            .await
            .map_err(|e| MinerError::RpcError(format!("{method}: decode: {e}")))?;

        if let Some(err) = body.get("error") {
            return Err(MinerError::RpcError(format!("{method}: {err}")));
        }

        body.get("result").cloned().ok_or_else(|| {
            MinerError::RpcError(format!("{method}: response missing `result` field"))
        })
    }

    /// Fetch the currently-active stake distribution and reconstruct a
    /// typed `StakeDistribution`.
    ///
    /// Pool IDs are decoded from lower-case 64-char hex; stake values are
    /// re-wrapped into `Amount`. The returned distribution is empty (but
    /// not an error) if the node has not yet registered any pools.
    pub async fn get_stake_distribution(&self) -> MinerResult<StakeDistribution> {
        let result = self.call("qv_getStakeDistribution", vec![]).await?;
        let snapshot: StakeDistributionSnapshotWire =
            serde_json::from_value(result).map_err(|e| {
                MinerError::RpcError(format!(
                    "qv_getStakeDistribution: malformed payload: {e}"
                ))
            })?;

        let mut entries: Vec<(PoolId, Amount)> = Vec::with_capacity(snapshot.pools.len());
        for row in snapshot.pools {
            let id_bytes = decode_pool_id_hex(&row.pool_id)?;
            entries.push((
                PoolId(Hash256::from_bytes(id_bytes)),
                Amount::from_smallest_units(row.stake),
            ));
        }

        StakeDistribution::new(Epoch::from(snapshot.epoch), entries).map_err(|e| {
            MinerError::Consensus(format!(
                "qv_getStakeDistribution: invalid distribution: {e}"
            ))
        })
    }

    /// Fetch the current epoch nonce and return the typed `EpochNonce`
    /// plus the epoch it applies to.
    pub async fn get_epoch_nonce(&self) -> MinerResult<(EpochNonce, Epoch)> {
        let result = self.call("qv_getEpochNonce", vec![]).await?;
        let info: EpochNonceInfoWire = serde_json::from_value(result).map_err(|e| {
            MinerError::RpcError(format!("qv_getEpochNonce: malformed payload: {e}"))
        })?;

        let bytes = hex::decode(&info.nonce_hex).map_err(|e| {
            MinerError::RpcError(format!("qv_getEpochNonce: invalid hex: {e}"))
        })?;
        if bytes.len() != 32 {
            return Err(MinerError::RpcError(format!(
                "qv_getEpochNonce: nonce must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut nonce_bytes = [0u8; 32];
        nonce_bytes.copy_from_slice(&bytes);
        Ok((
            EpochNonce(Hash256::from_bytes(nonce_bytes)),
            Epoch::from(info.epoch),
        ))
    }

    /// Submit a bincode-serialised block to the node via `qv_submitBlock`.
    /// Returns the block hash echoed by the node.
    pub async fn submit_block(&self, block_bytes: &[u8]) -> MinerResult<String> {
        let hex_payload = hex::encode(block_bytes);
        let result = self
            .call("qv_submitBlock", vec![Value::String(hex_payload)])
            .await?;
        match result {
            Value::String(s) => Ok(s),
            other => Err(MinerError::RpcError(format!(
                "qv_submitBlock: expected string result, got {other}"
            ))),
        }
    }

    /// Fetch the current chain tip via `qv_getTip`.
    pub async fn get_tip(&self) -> MinerResult<TipInfoWire> {
        let result = self.call("qv_getTip", vec![]).await?;
        serde_json::from_value(result).map_err(|e| {
            MinerError::RpcError(format!("qv_getTip: malformed payload: {e}"))
        })
    }

    /// Fetch the pending clear-mempool transactions via
    /// `qv_getPendingTransactions`. Each entry is hex-encoded bincode of a
    /// `Transaction`.
    pub async fn get_pending_transactions(&self) -> MinerResult<Vec<String>> {
        let result = self.call("qv_getPendingTransactions", vec![]).await?;
        serde_json::from_value(result).map_err(|e| {
            MinerError::RpcError(format!(
                "qv_getPendingTransactions: malformed payload: {e}"
            ))
        })
    }

    /// Ask the node to compute the post-apply UTXO commitment for the
    /// given candidate block (`qv_getPostApplyCommitment`).
    ///
    /// Each entry in `tx_bytes_hex` is the hex-encoded bincode of a
    /// `Transaction` — the same shape `get_pending_transactions` returns,
    /// so the producer can pass the fetched list straight through.
    /// Returns the 32-byte commitment root parsed back into the typed
    /// `UtxoCommitment`. Closes envanter **K-05**.
    pub async fn get_post_apply_commitment(
        &self,
        tx_bytes_hex: Vec<String>,
    ) -> MinerResult<qv_core::UtxoCommitment> {
        let params = vec![
            serde_json::to_value(tx_bytes_hex).map_err(|e| {
                MinerError::RpcError(format!(
                    "qv_getPostApplyCommitment: cannot encode tx list: {e}"
                ))
            })?,
        ];
        let result = self.call("qv_getPostApplyCommitment", params).await?;
        let hex_str = match result {
            Value::String(s) => s,
            other => {
                return Err(MinerError::RpcError(format!(
                    "qv_getPostApplyCommitment: expected string result, got {other}"
                )));
            }
        };
        let bytes = hex::decode(&hex_str).map_err(|e| {
            MinerError::RpcError(format!(
                "qv_getPostApplyCommitment: invalid hex: {e}"
            ))
        })?;
        let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
            MinerError::RpcError(format!(
                "qv_getPostApplyCommitment: expected 32 bytes, got {}",
                bytes.len()
            ))
        })?;
        Ok(qv_core::UtxoCommitment::from_bytes(arr))
    }
}

/// Wire-shape representation of `qv_node::rpc::TipInfo` used by the miner.
/// Mirrors the JSON field names exactly so deserialisation succeeds without
/// custom serde plumbing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TipInfoWire {
    pub block_hash: String,
    pub height: u64,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Wire types (match `qv_node::rpc` shapes exactly).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PoolStakeInfoWire {
    pool_id: String,
    stake: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StakeDistributionSnapshotWire {
    epoch: u64,
    total_stake: u64,
    pools: Vec<PoolStakeInfoWire>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EpochNonceInfoWire {
    nonce_hex: String,
    epoch: u64,
}

fn decode_pool_id_hex(s: &str) -> MinerResult<[u8; 32]> {
    let bytes = hex::decode(s).map_err(|e| {
        MinerError::RpcError(format!("invalid pool_id hex `{s}`: {e}"))
    })?;
    if bytes.len() != 32 {
        return Err(MinerError::RpcError(format!(
            "pool_id must be 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid_pool_id() {
        let hex_str = "00".repeat(32);
        let decoded = decode_pool_id_hex(&hex_str).unwrap();
        assert_eq!(decoded, [0u8; 32]);
    }

    #[test]
    fn decode_pool_id_wrong_length() {
        assert!(decode_pool_id_hex("00").is_err());
    }

    #[test]
    fn decode_pool_id_invalid_hex() {
        assert!(decode_pool_id_hex("zz".repeat(32).as_str()).is_err());
    }

    #[test]
    fn snapshot_wire_roundtrip() {
        let snap = StakeDistributionSnapshotWire {
            epoch: 7,
            total_stake: 1_000_000,
            pools: vec![PoolStakeInfoWire {
                pool_id: "aa".repeat(32),
                stake: 1_000_000,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: StakeDistributionSnapshotWire = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 7);
        assert_eq!(back.pools.len(), 1);
        assert_eq!(back.pools[0].stake, 1_000_000);
    }

    #[test]
    fn nonce_wire_roundtrip() {
        let info = EpochNonceInfoWire {
            nonce_hex: "ff".repeat(32),
            epoch: 12,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: EpochNonceInfoWire = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 12);
        assert_eq!(back.nonce_hex.len(), 64);
    }
}
