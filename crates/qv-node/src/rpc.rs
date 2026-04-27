//! JSON-RPC server for QuantumVault queries and transactions.
//!
//! Exposes methods via HTTP and WebSocket subscriptions via `jsonrpsee`.

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use qv_core::{Block, BlockHash, Hash256, ProtocolParams, Transaction, TxId};
use std::sync::Arc;

/// RPC method results that may fail with JSON-RPC errors.
pub type RpcApiResult<T> = RpcResult<T>;

/// Describes the RPC API surface.
#[rpc(server, client)]
pub trait QvNodeApi {
    /// Get a block by its hash.
    #[method(name = "qv_getBlockByHash")]
    async fn get_block_by_hash(&self, block_hash: String) -> RpcApiResult<Option<Block>>;

    /// Get a block by its height (indexed).
    #[method(name = "qv_getBlockByHeight")]
    async fn get_block_by_height(&self, height: u64) -> RpcApiResult<Option<Block>>;

    /// Get the current tip (latest block header).
    #[method(name = "qv_getTip")]
    async fn get_tip(&self) -> RpcApiResult<TipInfo>;

    /// Get a transaction by ID.
    #[method(name = "qv_getTx")]
    async fn get_tx(&self, tx_id: String) -> RpcApiResult<Option<Transaction>>;

    /// Submit a signed transaction to the mempool.
    #[method(name = "qv_sendTransaction")]
    async fn send_transaction(&self, tx_bytes: String) -> RpcApiResult<TxId>;

    /// Get a UTXO by outpoint.
    #[method(name = "qv_getUtxo")]
    async fn get_utxo(&self, outpoint: String) -> RpcApiResult<Option<UtxoInfo>>;

    /// Get the balance for a stealth address (scanning helper).
    #[method(name = "qv_getBalanceFor")]
    async fn get_balance_for(&self, view_key_hex: String) -> RpcApiResult<u64>;

    /// Scan stealth outputs in a range.
    #[method(name = "qv_scanStealth")]
    async fn scan_stealth(
        &self,
        view_key_hex: String,
        from_height: u64,
        to_height: u64,
    ) -> RpcApiResult<Vec<StealthScan>>;

    /// Get mempool status.
    #[method(name = "qv_getMempoolStatus")]
    async fn get_mempool_status(&self) -> RpcApiResult<MempoolStatus>;

    /// Subscribe to new blocks.
    #[subscription(
        name = "qv_subscribeNewBlocks",
        unsubscribe = "qv_unsubscribeNewBlocks",
        item = Block
    )]
    fn subscribe_new_blocks(&self);

    /// Subscribe to new transactions.
    #[subscription(
        name = "qv_subscribeNewTx",
        unsubscribe = "qv_unsubscribeNewTx",
        item = Transaction
    )]
    fn subscribe_new_tx(&self);
}

/// Information about the chain tip.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct TipInfo {
    pub block_hash: String,
    pub height: u64,
    pub timestamp: u64,
}

/// Information about a UTXO.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct UtxoInfo {
    pub value: u64,
    pub script_hash: String,
    pub has_datum: bool,
    pub has_stealth: bool,
}

/// Stealth address scan result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct StealthScan {
    pub height: u64,
    pub tx_id: String,
    pub output_index: u32,
    pub value: u64,
}

/// Current mempool status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct MempoolStatus {
    pub clear_pool_size: usize,
    pub encrypted_pool_size: usize,
    pub min_fee_rate: u64,
    pub total_value: u64,
}

/// Server implementation (empty stub for now; composition happens in `Node`).
pub struct RpcServer {
    // Will hold references to storage, network, consensus layer.
}

#[async_trait::async_trait]
impl QvNodeApiServer for RpcServer {
    async fn get_block_by_hash(&self, block_hash: String) -> RpcApiResult<Option<Block>> {
        // TODO: query storage
        tracing::debug!(hash = %block_hash, "RPC: getBlockByHash");
        Ok(None)
    }

    async fn get_block_by_height(&self, height: u64) -> RpcApiResult<Option<Block>> {
        // TODO: query storage
        tracing::debug!(height = %height, "RPC: getBlockByHeight");
        Ok(None)
    }

    async fn get_tip(&self) -> RpcApiResult<TipInfo> {
        // TODO: query chain state
        Ok(TipInfo {
            block_hash: "0x00".to_string(),
            height: 0,
            timestamp: 0,
        })
    }

    async fn get_tx(&self, tx_id: String) -> RpcApiResult<Option<Transaction>> {
        // TODO: query storage or mempool
        tracing::debug!(tx_id = %tx_id, "RPC: getTx");
        Ok(None)
    }

    async fn send_transaction(&self, tx_bytes: String) -> RpcApiResult<TxId> {
        // TODO: deserialize, validate, insert into mempool
        tracing::debug!(tx_bytes_len = tx_bytes.len(), "RPC: sendTransaction");
        Err(jsonrpsee::types::error::ErrorCode::InternalError.into())
    }

    async fn get_utxo(&self, outpoint: String) -> RpcApiResult<Option<UtxoInfo>> {
        // TODO: query UTXO store
        tracing::debug!(outpoint = %outpoint, "RPC: getUtxo");
        Ok(None)
    }

    async fn get_balance_for(&self, view_key_hex: String) -> RpcApiResult<u64> {
        // TODO: scan with stealth address view key
        tracing::debug!(view_key = %view_key_hex, "RPC: getBalanceFor");
        Ok(0)
    }

    async fn scan_stealth(
        &self,
        view_key_hex: String,
        from_height: u64,
        to_height: u64,
    ) -> RpcApiResult<Vec<StealthScan>> {
        // TODO: block range scan with privacy decryption
        tracing::debug!(
            view_key = %view_key_hex,
            from_height = %from_height,
            to_height = %to_height,
            "RPC: scanStealth"
        );
        Ok(vec![])
    }

    async fn get_mempool_status(&self) -> RpcApiResult<MempoolStatus> {
        // TODO: query mempool stats
        Ok(MempoolStatus {
            clear_pool_size: 0,
            encrypted_pool_size: 0,
            min_fee_rate: 1,
            total_value: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_types_serde() {
        let tip = TipInfo {
            block_hash: "abc123".to_string(),
            height: 100,
            timestamp: 1000000,
        };
        let json = serde_json::to_string(&tip).unwrap();
        let _deserialized: TipInfo = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_mempool_status_serde() {
        let status = MempoolStatus {
            clear_pool_size: 50,
            encrypted_pool_size: 10,
            min_fee_rate: 1,
            total_value: 1000000,
        };
        let json = serde_json::to_string(&status).unwrap();
        let _deserialized: MempoolStatus = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_utxo_info_serde() {
        let utxo = UtxoInfo {
            value: 100000,
            script_hash: "abc".to_string(),
            has_datum: true,
            has_stealth: false,
        };
        let json = serde_json::to_string(&utxo).unwrap();
        let _deserialized: UtxoInfo = serde_json::from_str(&json).unwrap();
    }
}
