//! Block production logic.
//!
//! When elected as slot leader, the operator pulls transactions from the clear
//! and encrypted mempools, applies deterministic ordering, builds AMM batches,
//! and produces a block signed with the KES key.

use crate::{MinerError, MinerResult};
use qv_core::{
    Block, BlockHash, BlockHeader, MerkleRoot, ProtocolParams, Slot, Timestamp, Transaction, TxId,
    UtxoCommitment, BLOCK_VERSION,
};
use qv_mempool::{ClearPool, ClearPoolConfig, EncryptedPool, EncryptedPoolConfig};

/// Block production context.
#[derive(Clone, Debug)]
pub struct BlockProductionContext {
    /// Current slot.
    pub slot: Slot,

    /// Parent block hash (previous block in the chain).
    pub parent_hash: qv_core::Hash256,

    /// Block height.
    pub height: qv_core::Height,

    /// Current timestamp (Unix milliseconds).
    pub timestamp: Timestamp,

    /// Protocol parameters.
    pub protocol_params: ProtocolParams,
}

/// Produce a block as the slot leader.
///
/// # Process
/// 1. Take a snapshot of the clear mempool.
/// 2. If the operator is on the decryption committee for this epoch,
///    decrypt the encrypted mempool batch.
/// 3. Merge and sort all transactions deterministically.
/// 4. Build AMM batches from order intents.
/// 5. Assemble the block header with VRF proof and KES signature.
/// 6. Return the block ready to be gossiped.
///
/// # Parameters
/// - `ctx`: Block production context (slot, parent, height, timestamp, params).
/// - `clear_pool`: Snapshot of the clear (unencrypted) mempool.
/// - `encrypted_pool`: Snapshot of the encrypted mempool (may be empty if operator not on committee).
/// - `vrf_proof`: The VRF proof of leadership (from leader election).
/// - `kes_signature`: The KES signature over the block header.
///
/// # Returns
/// A fully assembled block ready to be gossiped to the network.
pub async fn produce_block(
    ctx: &BlockProductionContext,
    clear_pool: &ClearPool,
    _encrypted_pool: &EncryptedPool,
    vrf_proof: &[u8],
    kes_signature: &[u8],
) -> MinerResult<Block> {
    // 1. Collect transactions from clear pool.
    //    `all_sorted` returns a fee-density-sorted view of mempool entries.
    let clear_entries = clear_pool.all_sorted();

    // 2. Encrypted-mempool decryption is the committee's job; if this operator
    //    is on the committee for this epoch, we'd call `decrypt_batch` here
    //    with the threshold decryptor. For block-production scaffolding we
    //    leave the encrypted batch empty and document the integration point.
    //    TODO: integrate `EncryptedPool::decrypt_batch<D>` once the committee
    //    decryptor wiring lands (see qv-mempool::encrypted::ThresholdDecryptor).
    let encrypted_txs: Vec<Transaction> = Vec::new();

    // 3. Merge — clear-pool entries are already deterministically ordered by
    //    fee density (descending), then tx_id; encrypted-pool batch (when
    //    available) will be appended in committee-decryption order. AMM batch
    //    construction (step 4) replaces ad-hoc sort-by-tx with the canonical
    //    qv-defi batcher.
    let mut tx_ids: Vec<TxId> = Vec::with_capacity(clear_entries.len() + encrypted_txs.len());
    let mut batched_txs: Vec<Transaction> =
        Vec::with_capacity(clear_entries.len() + encrypted_txs.len());
    for entry in &clear_entries {
        tx_ids.push(entry.tx_id);
        batched_txs.push(entry.tx.clone());
    }
    for tx in encrypted_txs {
        // Encrypted-batch decryption already produces a TxId alongside the tx;
        // recompute here as a best-effort placeholder.
        let id = tx
            .id()
            .map_err(|e| MinerError::BlockProduction(format!("tx id failed: {e}")))?;
        tx_ids.push(id);
        batched_txs.push(tx);
    }

    // 4. AMM batch logic (covered in qv-defi) is not yet wired in; we ship
    //    transactions in their merged order. See qv-defi::batcher.

    // 5. Compute merkle root over the txid leaves.
    let merkle_root: MerkleRoot = qv_core::merkle_root_of(&tx_ids);

    // 6. UTXO commitment placeholder. In production this snapshots the UTXO
    //    set *after* applying this block and commits to its root.
    let utxo_commitment = UtxoCommitment::ZERO;

    // 7. Assemble the block header.
    let header = BlockHeader {
        version: BLOCK_VERSION,
        // BlockHeader.prev_hash is a typed `BlockHash`; wrap the raw 32 bytes
        // we receive from the production context.
        prev_hash: BlockHash(ctx.parent_hash),
        height: ctx.height,
        slot: ctx.slot,
        timestamp: ctx.timestamp,
        merkle_root,
        utxo_commitment,
        vrf_proof: vrf_proof.to_vec(),
        kes_sig: kes_signature.to_vec(),
        producer_key_hash: qv_core::Hash256::ZERO, // Placeholder: should hash the pool ID.
    };

    // 8. Assemble the block.
    let block = Block {
        header,
        transactions: batched_txs,
    };

    let _ = ctx.protocol_params; // suppress unused-field-of-context warning until wired

    Ok(block)
}

/// Trait for external mempool snapshot providers (can be mocked for testing).
pub trait MempoolProvider: Send + Sync {
    /// Get a snapshot of the clear mempool.
    fn snapshot_clear(&self) -> MinerResult<ClearPool>;

    /// Get a snapshot of the encrypted mempool.
    fn snapshot_encrypted(&self) -> MinerResult<EncryptedPool>;
}

/// RPC-based mempool provider (calls the node's RPC methods).
pub struct RpcMempoolProvider {
    /// Node RPC endpoint (kept for the upcoming `Faz 1` wiring; see ROADMAP).
    #[allow(dead_code)]
    rpc_url: String,
}

impl RpcMempoolProvider {
    /// Create a new RPC-based mempool provider.
    pub fn new(rpc_url: String) -> Self {
        Self { rpc_url }
    }

    /// Get mempool status from node RPC (placeholder).
    pub async fn get_mempool_status(&self) -> MinerResult<MempoolStatus> {
        // In a real implementation, call qv_getMempoolStatus RPC method.
        // For now, return a placeholder.
        Ok(MempoolStatus {
            clear_size: 0,
            encrypted_size: 0,
        })
    }
}

impl MempoolProvider for RpcMempoolProvider {
    fn snapshot_clear(&self) -> MinerResult<ClearPool> {
        // Placeholder: in a real implementation, fetch from RPC.
        // `ClearPool::new` is infallible — it just wraps the supplied config.
        Ok(ClearPool::new(ClearPoolConfig::ephemeral()))
    }

    fn snapshot_encrypted(&self) -> MinerResult<EncryptedPool> {
        // Placeholder: in a real implementation, fetch from RPC.
        // `EncryptedPool::new` takes (config, epoch); it is infallible.
        // The config has plain public fields — there's no `::new` constructor.
        let cfg = EncryptedPoolConfig {
            max_tx_count: 5_000,
            max_pool_bytes: 4 * 1024 * 1024,
            max_age_secs: 60,
        };
        Ok(EncryptedPool::new(cfg, qv_core::Epoch::from(0)))
    }
}

/// Mempool status from RPC.
#[derive(Clone, Debug)]
pub struct MempoolStatus {
    /// Number of transactions in clear pool.
    pub clear_size: usize,

    /// Number of transactions in encrypted pool.
    pub encrypted_size: usize,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn sample_context() -> BlockProductionContext {
        BlockProductionContext {
            slot: Slot::from(100),
            parent_hash: qv_core::Hash256::ZERO,
            height: qv_core::Height::from(1),
            timestamp: Timestamp::from(1_000_000),
            protocol_params: ProtocolParams::mainnet(),
        }
    }

    #[tokio::test]
    async fn produce_block_basic() {
        let ctx = sample_context();
        let clear_pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let cfg = EncryptedPoolConfig {
            max_tx_count: 1_000,
            max_pool_bytes: 1024 * 1024,
            max_age_secs: 60,
        };
        let encrypted_pool = EncryptedPool::new(cfg, qv_core::Epoch::from(0));

        let vrf_proof = vec![1, 2, 3];
        let kes_sig = vec![4, 5, 6];

        let block = produce_block(&ctx, &clear_pool, &encrypted_pool, &vrf_proof, &kes_sig)
            .await
            .unwrap();

        assert_eq!(block.header.slot, Slot::from(100));
        assert_eq!(block.header.height, qv_core::Height::from(1));
        assert_eq!(block.header.vrf_proof, vrf_proof);
        assert_eq!(block.header.kes_sig, kes_sig);
    }

    #[test]
    fn rpc_mempool_provider_creation() {
        let provider = RpcMempoolProvider::new("http://localhost:8080".to_string());
        assert_eq!(provider.rpc_url, "http://localhost:8080");
    }

    #[test]
    fn mempool_provider_snapshot_clear() {
        let provider = RpcMempoolProvider::new("http://localhost:8080".to_string());
        let result = provider.snapshot_clear();
        assert!(result.is_ok());
    }

    #[test]
    fn mempool_provider_snapshot_encrypted() {
        let provider = RpcMempoolProvider::new("http://localhost:8080".to_string());
        let result = provider.snapshot_encrypted();
        assert!(result.is_ok());
    }
}
