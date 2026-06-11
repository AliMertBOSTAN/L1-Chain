//! Block production logic.
//!
//! When elected as slot leader, the operator pulls transactions from the clear
//! and encrypted mempools, applies deterministic ordering, builds AMM batches,
//! and produces a block signed with the KES key.

use crate::{MinerError, MinerResult};
use qv_core::{
    Amount, Block, BlockHash, BlockHeader, MerkleRoot, ProtocolParams, Script, Slot, Timestamp,
    Transaction, TxId, TxOutput, UtxoCommitment, BLOCK_VERSION,
};
use qv_mempool::encrypted::{DecryptionShare, ThresholdDecryptor};
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

    /// 32-byte public-key hash the block reward is paid to. When `Some`,
    /// the producer prepends a coinbase transaction claiming
    /// `block_subsidy(height) + known_fees` locked with
    /// `p2pkh_pqc(reward_pubkey_hash)`. When `None`, no coinbase is built
    /// (the reward is forfeited — consensus allows underclaiming).
    pub reward_pubkey_hash: Option<[u8; 32]>,
}

/// Build the coinbase transaction for a block at `ctx.height` claiming
/// `subsidy + fees` to `ctx.reward_pubkey_hash`, or `None` when no reward
/// address is configured or there is nothing to claim (subsidy exhausted
/// and zero fees).
///
/// `fees` is the sum of the fees the producer *knows about* — entries whose
/// fee is unknown (e.g. just-decrypted encrypted-mempool txs) should simply
/// be excluded: underclaiming is always consensus-valid, overclaiming never.
fn build_coinbase(ctx: &BlockProductionContext, fees: Amount) -> Option<Transaction> {
    let pkh = ctx.reward_pubkey_hash?;
    let reward =
        qv_consensus::total_block_reward(ctx.height, fees, &ctx.protocol_params.monetary);
    if reward == Amount::ZERO {
        return None;
    }
    let locking_script = Script::new(qv_script::p2pkh_pqc(&pkh));
    Some(Transaction::new_coinbase(
        ctx.height,
        vec![TxOutput::new(reward, locking_script)],
    ))
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

    // 2. Encrypted-mempool decryption: this path is for the **non-committee**
    //    case — operator is not on the decryption committee for this epoch,
    //    so the encrypted batch is left empty. Committee members must use
    //    [`produce_block_with_decryption`] instead (envanter K-06 closure).
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

    // 4b. Coinbase: claim `subsidy + fees` to the configured reward address.
    //     Mempool entries carry their pre-computed fee (resolved at
    //     validation time), so the claim is exact for the clear pool.
    let fees = Amount::checked_sum(clear_entries.iter().map(|e| e.fee))
        .ok_or_else(|| MinerError::BlockProduction("fee sum overflow".to_string()))?;
    if let Some(coinbase) = build_coinbase(ctx, fees) {
        let coinbase_id = coinbase
            .id()
            .map_err(|e| MinerError::BlockProduction(format!("coinbase id failed: {e}")))?;
        tx_ids.insert(0, coinbase_id);
        batched_txs.insert(0, coinbase);
    }

    // 5. Compute merkle root over the txid leaves (coinbase included).
    let merkle_root: MerkleRoot = qv_core::merkle_root_of(&tx_ids);

    // 6. UTXO commitment.
    //
    // This reference helper does not have direct access to the node's UTXO
    // state, so it stamps `ZERO`. The **production** producer (the closure
    // in `qv-miner::main::cmd_run`) calls `qv_getPostApplyCommitment` RPC
    // and uses the returned root — that path is what closes envanter K-05.
    // Callers of this helper (mostly the in-crate tests) don't care about
    // the commitment value, since the verifier currently treats the field
    // as opaque.
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

/// Produce a block with **encrypted mempool decryption** (envanter K-06).
///
/// This is the production path used when the operator is on the encrypted
/// mempool decryption committee for the current epoch. It:
///
/// 1. Decrypts the encrypted mempool batch using the supplied threshold
///    decryptor and committee shares (`EncryptedPool::decrypt_batch`).
/// 2. Bincode-deserializes each decrypted plaintext into a `Transaction`.
/// 3. Merges decrypted txs with the clear-pool snapshot in canonical order:
///    clear (fee-density desc) || decrypted (committee order).
/// 4. Builds AMM batches from order intents (envanter K-07 — currently a
///    no-op extension point; see `AmmBatcher` below).
/// 5. Assembles the block header (same as [`produce_block`]).
///
/// # When to use which
///
/// - [`produce_block`]: operator NOT on committee — encrypted batch is left
///   empty; production-grade enough for follower nodes that just pass clear
///   txs.
/// - [`produce_block_with_decryption`]: operator IS on committee for this
///   epoch — must decrypt to include the encrypted-mempool throughput.
///
/// # Parameters
/// - `ctx`, `clear_pool`, `vrf_proof`, `kes_signature`: same as `produce_block`.
/// - `encrypted_pool`: **mutable** because `decrypt_batch` drains the pool.
/// - `decryptor`: any type implementing [`ThresholdDecryptor`]
///   (production: real threshold-Kyber; tests: `MockThresholdDecryptor`).
/// - `shares`: decryption shares collected from t-of-n committee members
///   for this epoch.
///
/// # Returns
/// A fully assembled block ready to be gossiped.
///
/// # Failures
/// Returns `MinerError::BlockProduction` if any decrypted plaintext fails
/// `bincode::deserialize::<Transaction>` — these txs are skipped with a
/// warn log but do not fail the entire block.
pub async fn produce_block_with_decryption<D: ThresholdDecryptor>(
    ctx: &BlockProductionContext,
    clear_pool: &ClearPool,
    encrypted_pool: &mut EncryptedPool,
    decryptor: &D,
    shares: &[DecryptionShare],
    vrf_proof: &[u8],
    kes_signature: &[u8],
) -> MinerResult<Block> {
    // 1. Clear-pool snapshot (already fee-density sorted).
    let clear_entries = clear_pool.all_sorted();

    // 2. Decrypt encrypted-mempool batch. `decrypt_batch` drains the pool
    //    and returns `Vec<(TxId, plaintext_bytes)>`. Failed decryptions
    //    inside the pool are logged and skipped silently — they do not
    //    prevent block production.
    let decrypted_batch = encrypted_pool
        .decrypt_batch(decryptor, shares)
        .map_err(|e| MinerError::BlockProduction(format!("decrypt_batch failed: {e}")))?;

    tracing::debug!(
        clear_count = clear_entries.len(),
        encrypted_decrypted = decrypted_batch.len(),
        "K-06 wire: merging clear + decrypted batches"
    );

    // 3. Deserialize decrypted plaintexts. Skip individual failures (logged)
    //    so a malformed encrypted-mempool entry can't halt block production.
    let mut decrypted_txs: Vec<(TxId, Transaction)> = Vec::with_capacity(decrypted_batch.len());
    for (tx_id, plaintext) in decrypted_batch {
        match bincode::deserialize::<Transaction>(&plaintext) {
            Ok(tx) => decrypted_txs.push((tx_id, tx)),
            Err(e) => {
                tracing::warn!(
                    tx_id = ?tx_id,
                    error = %e,
                    "failed to deserialize decrypted tx — skipping"
                );
            }
        }
    }

    // 4. K-07 wire point: AMM intent batching. For each decrypted/clear tx
    //    that carries a `SwapIntent` (or other order intent) datum, group by
    //    pool and call `qv_defi::batcher::build_amm_batch`. The result
    //    replaces the individual intent txs with a single batched trade tx
    //    that preserves the x·y=k invariant.
    //
    //    Current state: scaffolding only. The intent-detection + batcher
    //    invocation is gated on:
    //      - K-07 envanter (this point)
    //      - qv-defi `Intent::extract_swap` helper (not yet added)
    //      - A pool-state oracle (current reserves) — needs RPC `qv_getPool` (TBD)
    //
    //    Until K-07 lands, intent txs flow through as plain UTXO spends.

    // 5. Merge in canonical order.
    let total = clear_entries.len() + decrypted_txs.len();
    let mut tx_ids: Vec<TxId> = Vec::with_capacity(total);
    let mut batched_txs: Vec<Transaction> = Vec::with_capacity(total);
    for entry in &clear_entries {
        tx_ids.push(entry.tx_id);
        batched_txs.push(entry.tx.clone());
    }
    for (tx_id, tx) in decrypted_txs {
        tx_ids.push(tx_id);
        batched_txs.push(tx);
    }

    // 5b. Coinbase: clear-pool fees are known exactly; decrypted txs'
    //     fees are unknown without UTXO resolution, so they are excluded
    //     from the claim (underclaiming is consensus-valid, overclaiming
    //     would get the block rejected).
    let fees = Amount::checked_sum(clear_entries.iter().map(|e| e.fee))
        .ok_or_else(|| MinerError::BlockProduction("fee sum overflow".to_string()))?;
    if let Some(coinbase) = build_coinbase(ctx, fees) {
        let coinbase_id = coinbase
            .id()
            .map_err(|e| MinerError::BlockProduction(format!("coinbase id failed: {e}")))?;
        tx_ids.insert(0, coinbase_id);
        batched_txs.insert(0, coinbase);
    }

    // 6. Merkle root (coinbase included) + UTXO commitment placeholder.
    let merkle_root: MerkleRoot = qv_core::merkle_root_of(&tx_ids);
    // Same caveat as `produce_block` above: this reference helper stamps
    // `ZERO`; the production closure in `main::cmd_run` fetches the real
    // commitment via `qv_getPostApplyCommitment` RPC (K-05).
    let utxo_commitment = UtxoCommitment::ZERO;

    // 7. Header.
    let header = BlockHeader {
        version: BLOCK_VERSION,
        prev_hash: BlockHash(ctx.parent_hash),
        height: ctx.height,
        slot: ctx.slot,
        timestamp: ctx.timestamp,
        merkle_root,
        utxo_commitment,
        vrf_proof: vrf_proof.to_vec(),
        kes_sig: kes_signature.to_vec(),
        producer_key_hash: qv_core::Hash256::ZERO, // K-05 — placeholder
    };

    let _ = ctx.protocol_params;

    Ok(Block {
        header,
        transactions: batched_txs,
    })
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
            reward_pubkey_hash: None,
        }
    }

    #[tokio::test]
    async fn produce_block_with_reward_address_prepends_coinbase() {
        use qv_core::{OutPoint, Script, TxInput, TxOutput};
        use qv_mempool::clear::MempoolEntry;

        let mut ctx = sample_context();
        let reward_pkh = [0xCC; 32];
        ctx.reward_pubkey_hash = Some(reward_pkh);

        // Clear pool with one entry carrying a pre-computed fee of 10.
        let mut clear_pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1u8; 32]), 0))],
            vec![TxOutput::new(
                Amount::from_smallest_units(490),
                Script::new(vec![0x01]),
            )],
        );
        let tx_id = tx.id().unwrap();
        clear_pool
            .add(MempoolEntry::new(
                tx,
                tx_id,
                Amount::from_smallest_units(10),
                100,
            ))
            .unwrap();

        let cfg = EncryptedPoolConfig {
            max_tx_count: 16,
            max_pool_bytes: 1024,
            max_age_secs: 60,
        };
        let encrypted_pool = EncryptedPool::new(cfg, qv_core::Epoch::from(0));

        let block = produce_block(&ctx, &clear_pool, &encrypted_pool, &[1], &[2])
            .await
            .unwrap();

        assert_eq!(block.transactions.len(), 2, "coinbase + mempool tx");
        let coinbase = &block.transactions[0];
        assert!(coinbase.is_coinbase());
        assert_eq!(coinbase.coinbase_height(), Some(qv_core::Height::from(1)));
        // Mainnet subsidy at height 1 (5_000_000_000) + fee (10).
        assert_eq!(
            coinbase.outputs[0].value,
            Amount::from_smallest_units(5_000_000_010)
        );
        assert_eq!(
            coinbase.outputs[0].locking_script.as_bytes(),
            qv_script::p2pkh_pqc(&reward_pkh).as_slice()
        );

        // Merkle root commits to the coinbase-inclusive body. The header's
        // height is 1 (non-genesis), so the positional coinbase rule applies.
        block.validate_structure().unwrap();
    }

    #[tokio::test]
    async fn produce_block_without_reward_address_has_no_coinbase() {
        let ctx = sample_context();
        let clear_pool = ClearPool::new(ClearPoolConfig::ephemeral());
        let cfg = EncryptedPoolConfig {
            max_tx_count: 16,
            max_pool_bytes: 1024,
            max_age_secs: 60,
        };
        let encrypted_pool = EncryptedPool::new(cfg, qv_core::Epoch::from(0));

        let block = produce_block(&ctx, &clear_pool, &encrypted_pool, &[1], &[2])
            .await
            .unwrap();
        assert!(block.transactions.is_empty());
    }

    #[test]
    fn build_coinbase_skips_zero_reward() {
        // Exhausted subsidy + zero fees → no coinbase.
        let mut ctx = sample_context();
        ctx.reward_pubkey_hash = Some([0xCC; 32]);
        // 64 halvings beyond any subsidy.
        ctx.height = qv_core::Height::from(u64::MAX);
        assert!(build_coinbase(&ctx, Amount::ZERO).is_none());

        // No reward address → no coinbase even with fees.
        let ctx2 = sample_context();
        assert!(build_coinbase(&ctx2, Amount::from_smallest_units(1_000)).is_none());
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

    /// K-06 wire smoke test: feed one encrypted tx through
    /// `produce_block_with_decryption` and assert it lands in the block.
    ///
    /// Uses `MockThresholdDecryptor` (XOR-key threshold scheme). Real prod
    /// path will swap in a Pedersen-DKG-backed threshold-Kyber decryptor
    /// (envanter T-01 dependency).
    #[tokio::test]
    async fn produce_block_with_decryption_merges_encrypted_tx() {
        use qv_core::{Amount, OutPoint, Script, TxInput, TxOutput};
        use qv_mempool::encrypted::{EncryptedTx, MockThresholdDecryptor};

        // Build a small valid transaction.
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([7u8; 32]), 0))],
            vec![TxOutput::new(
                Amount::from_smallest_units(42),
                Script::new(vec![]),
            )],
        );
        let plaintext = bincode::serialize(&tx).unwrap();

        // XOR-encrypt with a fixed 16-byte key (MockThresholdDecryptor scheme).
        let key = vec![0xABu8; 16];
        let encrypted_body: Vec<u8> = plaintext
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % key.len()])
            .collect();
        let tx_id = tx.id().unwrap();
        let etx = EncryptedTx {
            id: tx_id,
            kem_ciphertext: vec![0; 32],
            encrypted_body,
            target_epoch: qv_core::Epoch::from(0),
            received_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Encrypted pool with one tx pending decryption.
        let cfg = EncryptedPoolConfig {
            max_tx_count: 1_000,
            max_pool_bytes: 1024 * 1024,
            max_age_secs: 60,
        };
        let mut encrypted_pool = EncryptedPool::new(cfg, qv_core::Epoch::from(0));
        encrypted_pool.add(etx).unwrap();

        // 2-of-3 mock committee; need 2 shares to reconstruct.
        let decryptor = MockThresholdDecryptor::new(2, 3);
        let shares = vec![
            DecryptionShare {
                member_index: 0,
                share_bytes: key.clone(),
            },
            DecryptionShare {
                member_index: 1,
                share_bytes: key,
            },
        ];

        let ctx = sample_context();
        let clear_pool = ClearPool::new(ClearPoolConfig::ephemeral());

        let block = produce_block_with_decryption(
            &ctx,
            &clear_pool,
            &mut encrypted_pool,
            &decryptor,
            &shares,
            &[9, 9, 9],
            &[8, 8, 8],
        )
        .await
        .unwrap();

        // Block should contain the decrypted tx (1 entry, since clear pool is empty).
        assert_eq!(
            block.transactions.len(),
            1,
            "decrypted tx must land in block"
        );
        assert_eq!(
            block.transactions[0].id().unwrap(),
            tx_id,
            "block's tx id must match the original pre-encryption tx id"
        );
        assert_eq!(block.header.vrf_proof, vec![9, 9, 9]);
        assert_eq!(block.header.kes_sig, vec![8, 8, 8]);

        // After decrypt_batch, the encrypted pool should be drained.
        assert_eq!(encrypted_pool.len(), 0, "decrypt_batch must drain the pool");
    }
}
