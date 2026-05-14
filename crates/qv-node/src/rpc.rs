//! JSON-RPC server for QuantumVault queries and transactions.
//!
//! Exposes methods via HTTP and WebSocket subscriptions via `jsonrpsee`.

use std::str::FromStr;
use std::sync::Arc;

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use qv_consensus::ChainState;
use qv_core::{Amount, Block, BlockHash, Height, OutPoint, Transaction, TxId};
use qv_mempool::clear::{ClearPool, MempoolEntry};
use qv_mempool::encrypted::EncryptedPool;
use qv_storage::block_store::BlockStore;
use qv_storage::kv::KvStore;
use qv_storage::utxo_store::UtxoStore;

/// Describes the RPC API surface.
#[rpc(server, client)]
pub trait QvNodeApi {
    /// Get a block by its hash.
    #[method(name = "qv_getBlockByHash")]
    async fn get_block_by_hash(&self, block_hash: String) -> RpcResult<Option<Block>>;

    /// Get a block by its height (indexed).
    #[method(name = "qv_getBlockByHeight")]
    async fn get_block_by_height(&self, height: u64) -> RpcResult<Option<Block>>;

    /// Get the current tip (latest block header).
    #[method(name = "qv_getTip")]
    async fn get_tip(&self) -> RpcResult<TipInfo>;

    /// Get a transaction by ID.
    #[method(name = "qv_getTx")]
    async fn get_tx(&self, tx_id: String) -> RpcResult<Option<Transaction>>;

    /// Submit a signed transaction to the mempool.
    #[method(name = "qv_sendTransaction")]
    async fn send_transaction(&self, tx_bytes: String) -> RpcResult<TxId>;

    /// Get a UTXO by outpoint.
    #[method(name = "qv_getUtxo")]
    async fn get_utxo(&self, outpoint: String) -> RpcResult<Option<UtxoInfo>>;

    /// Get the balance for a stealth address (scanning helper).
    #[method(name = "qv_getBalanceFor")]
    async fn get_balance_for(&self, view_key_hex: String) -> RpcResult<u64>;

    /// Scan stealth outputs in a range.
    #[method(name = "qv_scanStealth")]
    async fn scan_stealth(
        &self,
        view_key_hex: String,
        from_height: u64,
        to_height: u64,
    ) -> RpcResult<Vec<StealthScan>>;

    /// Get mempool status.
    #[method(name = "qv_getMempoolStatus")]
    async fn get_mempool_status(&self) -> RpcResult<MempoolStatus>;

    // Subscription endpoints are deferred until the node wires up a real
    // event source (block/tx notifier channels). Re-add when implementing:
    //
    //   #[subscription(name = "qv_subscribeNewBlocks",
    //                  unsubscribe = "qv_unsubscribeNewBlocks", item = Block)]
    //   async fn subscribe_new_blocks(&self) -> jsonrpsee::core::SubscriptionResult;
    //
    //   #[subscription(name = "qv_subscribeNewTx",
    //                  unsubscribe = "qv_unsubscribeNewTx", item = Transaction)]
    //   async fn subscribe_new_tx(&self) -> jsonrpsee::core::SubscriptionResult;
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

/// RPC server holding references to storage, mempool, and consensus layers.
///
/// Note: `chain_state` and `clear_pool` are Mutex-wrapped because the Node
/// main loop mutates them. RPC queries acquire read locks briefly.
pub struct RpcServer<S: KvStore> {
    block_store: Arc<BlockStore<S>>,
    utxo_store: Arc<UtxoStore<S>>,
    chain_state: Arc<tokio::sync::Mutex<ChainState>>,
    clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
    encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
}

impl<S: KvStore> RpcServer<S> {
    /// Create a new RPC server with references to the storage, consensus, and mempool layers.
    pub fn new(
        block_store: Arc<BlockStore<S>>,
        utxo_store: Arc<UtxoStore<S>>,
        chain_state: Arc<tokio::sync::Mutex<ChainState>>,
        clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
        encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
    ) -> Self {
        Self {
            block_store,
            utxo_store,
            chain_state,
            clear_pool,
            encrypted_pool,
        }
    }
}

#[async_trait::async_trait]
impl<S: KvStore + Send + Sync + 'static> QvNodeApiServer for RpcServer<S> {
    async fn get_block_by_hash(&self, block_hash: String) -> RpcResult<Option<Block>> {
        let hash = BlockHash::from_hex(&block_hash).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid block hash: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(hash = %hash, "RPC: getBlockByHash");

        match self.block_store.get_block(&hash).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(block) => Ok(Some(block)),
            None => Ok(None),
        }
    }

    async fn get_block_by_height(&self, height: u64) -> RpcResult<Option<Block>> {
        let h = Height::from(height);
        tracing::debug!(height = %height, "RPC: getBlockByHeight");

        match self.block_store.get_block_by_height(h).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(block) => Ok(Some(block)),
            None => Ok(None),
        }
    }

    async fn get_tip(&self) -> RpcResult<TipInfo> {
        let chain = self.chain_state.lock().await;
        let tip = chain.tip();
        tracing::debug!("RPC: getTip");

        Ok(TipInfo {
            block_hash: format!("{:?}", tip.hash),
            height: tip.height.as_u64(),
            timestamp: 0, // ChainEntry doesn't store timestamp
        })
    }

    async fn get_tx(&self, tx_id: String) -> RpcResult<Option<Transaction>> {
        let target_id = TxId::from_hex(&tx_id).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid tx id: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(tx_id = %tx_id, "RPC: getTx");

        // Search mempool first
        {
            let pool = self.clear_pool.lock().await;
            for entry in pool.all_sorted() {
                if entry.tx_id == target_id {
                    return Ok(Some(entry.tx.clone()));
                }
            }
        }

        // Then search recent blocks (iterate from tip backwards up to k blocks)
        let ancestors: Vec<_> = {
            let chain = self.chain_state.lock().await;
            let tip_hash = chain.tip().hash;
            chain.ancestors(tip_hash, 50).into_iter().cloned().collect()
        }; // lock released here

        for entry in &ancestors {
            if let Ok(Some(block)) = self.block_store.get_block(&entry.hash) {
                for tx in &block.transactions {
                    if tx.id().ok() == Some(target_id) {
                        return Ok(Some(tx.clone()));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn send_transaction(&self, tx_bytes: String) -> RpcResult<TxId> {
        // Hex-decode the transaction bytes
        let raw_bytes = hex::decode(&tx_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid hex encoding: {}", e),
                None::<()>,
            )
        })?;

        // Deserialize as bincode
        let tx: Transaction = bincode::deserialize(&raw_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid transaction encoding: {}", e),
                None::<()>,
            )
        })?;

        // Validate transaction structure
        tx.validate_structure().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("transaction validation failed: {}", e),
                None::<()>,
            )
        })?;

        // Compute transaction ID
        let tx_id = tx.id().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("failed to compute tx id: {}", e),
                None::<()>,
            )
        })?;

        // Simplified fee: use 0 for now; full validation uses qv-node/validation.rs
        // which resolves UTXOs and computes real fee. RPC insertion is a fast path.
        let fee = Amount::from_smallest_units(0u64);
        let estimated_size = bincode::serialized_size(&tx).unwrap_or(0) as usize;

        // Create mempool entry and insert into clear mempool via Mutex
        {
            let entry = MempoolEntry::new(tx, tx_id, fee, estimated_size);
            let mut pool = self.clear_pool.lock().await;
            pool.add(entry).map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("mempool insertion failed: {}", e),
                    None::<()>,
                )
            })?;
        }

        tracing::info!(%tx_id, "transaction accepted into mempool via RPC");
        Ok(tx_id)
    }

    async fn get_utxo(&self, outpoint: String) -> RpcResult<Option<UtxoInfo>> {
        let op = OutPoint::from_str(&outpoint).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid outpoint: {}", e),
                None::<()>,
            )
        })?;

        tracing::debug!(outpoint = %op, "RPC: getUtxo");

        match self.utxo_store.get(&op).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("storage error: {}", e),
                None::<()>,
            )
        })? {
            Some(output) => Ok(Some(UtxoInfo {
                value: output.value.as_u64(),
                script_hash: output.locking_script.hash().to_hex(),
                has_datum: output.datum.is_some(),
                has_stealth: output.stealth_info.is_some(),
            })),
            None => Ok(None),
        }
    }

    async fn get_balance_for(&self, _view_key_hex: String) -> RpcResult<u64> {
        tracing::debug!(view_key = %_view_key_hex, "RPC: getBalanceFor");

        // Note: A full implementation would:
        // 1. Parse the view_key_hex into stealth view keys
        // 2. Iterate over UTXO set entries
        // 3. For each UTXO with stealth_info, call stealth::scan_output()
        // 4. Sum the values of matching outputs
        //
        // This requires the privacy module and a way to iterate all UTXOs efficiently.
        // For now, return a stub error since stealth key parsing requires actual key material.

        Err(jsonrpsee::types::ErrorObject::owned(
            -32603,
            "stealth key scanning not yet implemented in RPC",
            None::<()>,
        )
        .into())
    }

    async fn scan_stealth(
        &self,
        _view_key_hex: String,
        _from_height: u64,
        _to_height: u64,
    ) -> RpcResult<Vec<StealthScan>> {
        tracing::debug!(
            view_key = %_view_key_hex,
            from_height = %_from_height,
            to_height = %_to_height,
            "RPC: scanStealth"
        );

        // Note: A full implementation would:
        // 1. Parse the view_key_hex into StealthKeys
        // 2. Fetch blocks in [from_height, to_height] range
        // 3. For each transaction output with stealth_info:
        //    - Call stealth::scan_output() with the view keys
        //    - If it matches, record the output as StealthScan
        // 4. Return the list of matching outputs
        //
        // For now, return a stub error since this requires key parsing.

        Err(jsonrpsee::types::ErrorObject::owned(
            -32603,
            "stealth scanning not yet implemented in RPC",
            None::<()>,
        )
        .into())
    }

    async fn get_mempool_status(&self) -> RpcResult<MempoolStatus> {
        let pool = self.clear_pool.lock().await;
        let clear_size = pool.len();

        let enc_pool = self.encrypted_pool.lock().await;
        let encrypted_size = enc_pool.len();
        drop(enc_pool);

        // Compute total value in clear pool (sum of output values)
        let mut total_value = 0u64;
        for entry in pool.all_sorted() {
            // Sum output values as a proxy for pool value
            for output in &entry.tx.outputs {
                total_value = total_value.saturating_add(output.value.as_u64());
            }
        }
        drop(pool);

        Ok(MempoolStatus {
            clear_pool_size: clear_size,
            encrypted_pool_size: encrypted_size,
            min_fee_rate: 1,
            total_value,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
