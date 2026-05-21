//! JSON-RPC server for QuantumVault queries and transactions.
//!
//! Exposes methods via HTTP and WebSocket subscriptions via `jsonrpsee`.

use std::str::FromStr;
use std::sync::Arc;

use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use qv_consensus::epoch::EpochNonce;
use qv_consensus::slot::SlotClock;
use qv_consensus::stake::StakeDistribution;
use qv_consensus::ChainState;
use qv_core::{Amount, Block, BlockHash, Height, OutPoint, Transaction, TxId};
use qv_mempool::clear::{ClearPool, MempoolEntry};
use qv_mempool::encrypted::EncryptedPool;
use qv_storage::block_store::BlockStore;
use qv_storage::kv::KvStore;
use qv_storage::utxo_store::UtxoStore;
use tokio::sync::mpsc;

use crate::node::NodeEvent;

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

    /// Get the currently active stake distribution.
    ///
    /// This is the per-epoch frozen snapshot of `(pool_id, stake)` pairs
    /// used by VRF leader election. Stake pool operators (`qv-miner`)
    /// query this endpoint at startup and at every epoch boundary.
    #[method(name = "qv_getStakeDistribution")]
    async fn get_stake_distribution(&self) -> RpcResult<StakeDistributionSnapshot>;

    /// Get the current epoch nonce.
    ///
    /// The nonce is a 32-byte seed that parameterises VRF leader election
    /// for the current epoch. It evolves at every epoch boundary as
    /// `η_e = SHA3-256(η_{e-1} || extra_entropy || boundary_block_hash)`.
    /// Stake pool operators (`qv-miner`) must use the latest nonce when
    /// evaluating leadership.
    #[method(name = "qv_getEpochNonce")]
    async fn get_epoch_nonce(&self) -> RpcResult<EpochNonceInfo>;

    /// Drain the clear mempool snapshot and return every pending
    /// transaction in deterministic (fee-density descending, then
    /// tx-id ascending) order.
    ///
    /// Each entry is hex-encoded bincode bytes — clients deserialize
    /// to `Transaction`. The mempool itself is *not* mutated by this
    /// call; it is a snapshot read. Used by `qv-miner` to fill block
    /// bodies after winning a slot.
    #[method(name = "qv_getPendingTransactions")]
    async fn get_pending_transactions(&self) -> RpcResult<Vec<String>>;

    /// Submit a fully-signed block to the node.
    ///
    /// The payload is hex-encoded bincode bytes of `qv_core::Block`.
    /// The node performs structural validation, chain-linkage check,
    /// applies the block to UTXO storage, and gossips it to peers.
    /// Returns the canonical hex of the accepted block hash.
    #[method(name = "qv_submitBlock")]
    async fn submit_block(&self, block_bytes: String) -> RpcResult<String>;

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

/// One row in a stake distribution snapshot: `(pool_id_hex, stake)`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct PoolStakeInfo {
    /// SHA3-256 of the operator's VRF public key, lower-case hex.
    pub pool_id: String,
    /// Pool's absolute stake in smallest units.
    pub stake: u64,
}

/// Per-epoch frozen stake distribution snapshot returned by
/// `qv_getStakeDistribution`. Pools are sorted by `pool_id` for
/// deterministic wire serialisation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct StakeDistributionSnapshot {
    /// Epoch this snapshot applies to.
    pub epoch: u64,
    /// Sum of all pool stakes.
    pub total_stake: u64,
    /// Per-pool stake entries, sorted by `pool_id`.
    pub pools: Vec<PoolStakeInfo>,
}

impl StakeDistributionSnapshot {
    /// Build a snapshot from a `StakeDistribution`.
    #[must_use]
    pub fn from_distribution(d: &StakeDistribution) -> Self {
        let pools: Vec<PoolStakeInfo> = d
            .iter()
            .map(|(pid, stake)| PoolStakeInfo {
                pool_id: pid.0.to_hex(),
                stake: *stake,
            })
            .collect();
        Self {
            epoch: d.epoch.as_u64(),
            total_stake: d.total_stake(),
            pools,
        }
    }
}

/// Current epoch nonce returned by `qv_getEpochNonce`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "serde")]
pub struct EpochNonceInfo {
    /// 32-byte nonce, lower-case hex.
    pub nonce_hex: String,
    /// Epoch this nonce parameterises (the *current* epoch).
    pub epoch: u64,
}

impl EpochNonceInfo {
    /// Build the wire representation from a typed `EpochNonce` and epoch number.
    #[must_use]
    pub fn from_nonce(nonce: &EpochNonce, epoch: qv_core::Epoch) -> Self {
        Self {
            nonce_hex: hex::encode(nonce.as_bytes()),
            epoch: epoch.as_u64(),
        }
    }
}

/// RPC server holding references to storage, mempool, and consensus layers.
///
/// Note: `chain_state` and `clear_pool` are Mutex-wrapped because the Node
/// main loop mutates them. RPC queries acquire read locks briefly.
///
/// `stake_distribution` and `epoch_nonce` are `RwLock`-wrapped because the
/// epoch boundary handler updates them in-place at the start of every epoch
/// while RPC callers (`qv-miner`, observability tooling) only read them.
pub struct RpcServer<S: KvStore> {
    block_store: Arc<BlockStore<S>>,
    utxo_store: Arc<UtxoStore<S>>,
    chain_state: Arc<tokio::sync::Mutex<ChainState>>,
    clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
    encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
    stake_distribution: Arc<tokio::sync::RwLock<StakeDistribution>>,
    epoch_nonce: Arc<tokio::sync::RwLock<EpochNonce>>,
    /// Slot/epoch math derived from the node's `ProtocolParams`. Cheap to
    /// clone (just consensus constants). Used to map the tip slot to the
    /// current epoch for `qv_getEpochNonce`.
    slot_clock: SlotClock,
    /// Sender for dispatching events back into the node's main loop.
    /// Used by `qv_submitBlock` to hand the validated block to the same
    /// pipeline that processes network-sourced blocks (linkage check,
    /// UTXO apply, gossip relay).
    event_tx: mpsc::Sender<NodeEvent>,
}

impl<S: KvStore> RpcServer<S> {
    /// Create a new RPC server with references to the storage, consensus, and mempool layers.
    #[allow(clippy::too_many_arguments)] // ledger-facing endpoint surface; one ref per concern.
    pub fn new(
        block_store: Arc<BlockStore<S>>,
        utxo_store: Arc<UtxoStore<S>>,
        chain_state: Arc<tokio::sync::Mutex<ChainState>>,
        clear_pool: Arc<tokio::sync::Mutex<ClearPool>>,
        encrypted_pool: Arc<tokio::sync::Mutex<EncryptedPool>>,
        stake_distribution: Arc<tokio::sync::RwLock<StakeDistribution>>,
        epoch_nonce: Arc<tokio::sync::RwLock<EpochNonce>>,
        slot_clock: SlotClock,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Self {
        Self {
            block_store,
            utxo_store,
            chain_state,
            clear_pool,
            encrypted_pool,
            stake_distribution,
            epoch_nonce,
            slot_clock,
            event_tx,
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

        // Use canonical lower-case hex via `Display` (not the truncated
        // `Debug` form). Clients parse this back with `BlockHash::from_hex`.
        Ok(TipInfo {
            block_hash: tip.hash.to_hex(),
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
        ))
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
        ))
    }

    async fn get_stake_distribution(&self) -> RpcResult<StakeDistributionSnapshot> {
        tracing::debug!("RPC: getStakeDistribution");
        let dist = self.stake_distribution.read().await;
        Ok(StakeDistributionSnapshot::from_distribution(&*dist))
    }

    async fn get_epoch_nonce(&self) -> RpcResult<EpochNonceInfo> {
        tracing::debug!("RPC: getEpochNonce");
        // Snapshot the current nonce and the tip slot; release locks
        // before the slot→epoch translation so we don't hold them across
        // any potentially blocking work.
        let nonce = *self.epoch_nonce.read().await;
        let tip_slot = {
            let chain = self.chain_state.lock().await;
            chain.tip().slot
        };
        let epoch_num = self.slot_clock.slot_to_epoch(tip_slot);
        Ok(EpochNonceInfo::from_nonce(&nonce, epoch_num))
    }

    async fn get_pending_transactions(&self) -> RpcResult<Vec<String>> {
        tracing::debug!("RPC: getPendingTransactions");
        let pool = self.clear_pool.lock().await;
        let entries = pool.all_sorted();
        let mut out: Vec<String> = Vec::with_capacity(entries.len());
        for entry in entries {
            let bytes = bincode::serialize(&entry.tx).map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("serialize tx {} failed: {e}", entry.tx_id),
                    None::<()>,
                )
            })?;
            out.push(hex::encode(bytes));
        }
        Ok(out)
    }

    async fn submit_block(&self, block_bytes: String) -> RpcResult<String> {
        // 1. Hex decode.
        let raw = hex::decode(&block_bytes).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid hex encoding: {e}"),
                None::<()>,
            )
        })?;

        // 2. Bincode deserialize.
        let block: Block = bincode::deserialize(&raw).map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("invalid block encoding: {e}"),
                None::<()>,
            )
        })?;

        // 3. Structural validation — reject malformed blocks at the RPC
        //    boundary so bad input never enters the pipeline.
        block.validate_structure().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32602,
                format!("structural validation failed: {e}"),
                None::<()>,
            )
        })?;

        // 4. Compute the block hash for the response *before* dispatch so
        //    callers always get the canonical hash even if the chain-
        //    linkage check later rejects the block (in which case the hash
        //    can be used to look up failure logs).
        let block_hash = block.hash().map_err(|e| {
            jsonrpsee::types::ErrorObject::owned(
                -32603,
                format!("failed to compute block hash: {e}"),
                None::<()>,
            )
        })?;

        let height = block.header.height;
        let tx_count = block.transactions.len();

        // 5. Hand the block to the main loop. Linkage check, UTXO apply,
        //    chain-state update, mempool eviction, and gossip relay all
        //    happen there (`Node::handle_block`).
        self.event_tx
            .send(NodeEvent::BlockReceived(block))
            .await
            .map_err(|e| {
                jsonrpsee::types::ErrorObject::owned(
                    -32603,
                    format!("event channel closed: {e}"),
                    None::<()>,
                )
            })?;

        tracing::info!(
            block_hash = %block_hash,
            height = ?height,
            tx_count,
            "block accepted via RPC; dispatched to node pipeline"
        );

        Ok(block_hash.to_hex())
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

    #[test]
    fn stake_distribution_snapshot_serde() {
        let snap = StakeDistributionSnapshot {
            epoch: 7,
            total_stake: 1_000_000,
            pools: vec![PoolStakeInfo {
                pool_id: "aa".repeat(32),
                stake: 1_000_000,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: StakeDistributionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 7);
        assert_eq!(back.pools.len(), 1);
    }

    #[test]
    fn epoch_nonce_info_serde() {
        let info = EpochNonceInfo {
            nonce_hex: "ff".repeat(32),
            epoch: 12,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: EpochNonceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.epoch, 12);
        assert_eq!(back.nonce_hex.len(), 64);
    }
}
