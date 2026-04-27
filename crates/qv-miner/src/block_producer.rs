//! Block production logic.
//!
//! When elected as slot leader, the operator pulls transactions from the clear
//! and encrypted mempools, applies deterministic ordering, builds AMM batches,
//! and produces a block signed with the KES key.

use crate::{MinerError, MinerResult};
use qv_consensus::BlockValidationContext;
use qv_core::{Block, BlockHeader, OutPoint, ProtocolParams, Slot, Timestamp, Transaction};
use qv_mempool::{ClearPool, EncryptedPool, deterministic_sort};

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
    encrypted_pool: &EncryptedPool,
    vrf_proof: &[u8],
    kes_signature: &[u8],
) -> MinerResult<Block> {
    // 1. Collect transactions from clear pool.
    let clear_txs = clear_pool.all_sorted().map_err(|e| {
        MinerError::BlockProduction(format!("failed to get sorted clear pool: {e}"))
    })?;

    // 2. Collect transactions from encrypted pool (if operator is on committee).
    // For now, we assume the encrypted pool has already been decrypted if needed.
    let encrypted_txs = encrypted_pool
        .get_all_decrypted()
        .map_err(|e| {
            MinerError::BlockProduction(format!("failed to get encrypted pool: {e}"))
        })?;

    // 3. Merge and sort all transactions.
    let mut all_txs = clear_txs;
    all_txs.extend(encrypted_txs);
    all_txs.sort_by(deterministic_sort);

    // 4. Build AMM batch (covered in detail in qv-defi; for now, use transactions as-is).
    // In a full implementation, this would:
    // - Identify swap orders in the transaction data.
    // - Build batches that satisfy AMM invariants (x*y >= k).
    // - Generate slashing evidence for any misorders.
    let batched_txs = all_txs; // Placeholder: no AMM batch logic yet.

    // 5. Compute merkle root over transactions.
    let merkle_root = qv_core::merkle_root_of_transactions(&batched_txs);

    // 6. Compute UTXO commitment (placeholder: in a real implementation, this would
    //    snapshot the current UTXO set and compute its commitment root).
    let utxo_commitment = qv_core::Hash256::ZERO;

    // 7. Assemble the block header.
    let header = BlockHeader {
        version: 1,
        prev_hash: ctx.parent_hash,
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
        body: batched_txs,
    };

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
        ClearPool::new(10000).map_err(|e| {
            MinerError::MempoolError(format!("failed to create clear pool: {e}"))
        })
    }

    fn snapshot_encrypted(&self) -> MinerResult<EncryptedPool> {
        // Placeholder: in a real implementation, fetch from RPC.
        EncryptedPool::new(qv_core::Epoch::from(0), 5000).map_err(|e| {
            MinerError::MempoolError(format!("failed to create encrypted pool: {e}"))
        })
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
        let clear_pool = ClearPool::new(10000).unwrap();
        let encrypted_pool = EncryptedPool::new(qv_core::Epoch::from(0), 5000).unwrap();

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
