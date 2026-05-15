//! Slot ticker — periodic slot advancement and block production for Ouroboros Praos.
//!
//! This module provides background task machinery that:
//! 1. Ticks every slot duration (e.g., 2 seconds)
//! 2. Checks if the local pool is elected as slot leader via VRF
//! 3. If elected, produces a block from mempool transactions
//! 4. Otherwise, logs at debug level and advances to the next slot
//!
//! The [`SlotTicker`] is parametrized over a [`VrfEvaluator`] so it can work
//! with both test deterministic VRFs and production lattice-based ones.

use qv_consensus::epoch::EpochNonce;
use qv_consensus::leader_schedule::{check_leadership, VrfEvaluator, VrfOutput, VrfProof};
use qv_consensus::slot::SlotClock;
use qv_consensus::stake::{PoolId, StakeDistribution};
use qv_core::{Block, BlockHeader, Hash256, Height, MerkleRoot, Slot, Timestamp};
use qv_crypto::sha3_256;
use qv_storage::block_store::BlockStore;
use qv_storage::kv::MemoryKvStore;
use qv_storage::utxo_store::UtxoStore;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use tracing::{debug, warn};

use qv_consensus::ChainState;
use qv_mempool::clear::ClearPool;

/// Slot ticker — runs the slot-by-slot block production loop.
///
/// Holds references to consensus parameters, stake information, VRF evaluator,
/// and storage/mempool. On each tick, checks leadership and produces a block if elected.
pub struct SlotTicker<V: VrfEvaluator> {
    /// Slot clock for time<→slot mapping.
    slot_clock: SlotClock,
    /// This pool's unique identifier.
    pool_id: PoolId,
    /// Current stake distribution (frozen per epoch).
    stake_distribution: Arc<StakeDistribution>,
    /// Current epoch nonce (evolves at epoch boundaries).
    epoch_nonce: EpochNonce,
    /// VRF evaluator (test mock or production implementation).
    vrf: V,
    /// Block storage.
    block_store: Arc<BlockStore<MemoryKvStore>>,
    /// UTXO storage for inclusion in headers (used during block production).
    _utxo_store: Arc<UtxoStore<MemoryKvStore>>,
    /// Consensus chain state (tip height, hash, slot).
    chain_state: Arc<Mutex<ChainState>>,
    /// Transaction mempool (locked for concurrent access).
    clear_pool: Arc<Mutex<ClearPool>>,
    /// Active slot coefficient for Praos threshold.
    _active_slot_coeff: f64,
    /// Optional KES secret key for real header signing (per ADR-005).
    ///
    /// - `None` → produces blocks with `kes_sig: Vec::new()` (legacy/test path).
    /// - `Some` → calls `qv_crypto::kes_sign` over the header bytes-to-sign and
    ///   includes the bincode-serialized `KesSignature` in the block header.
    ///   Each call uses the current period; callers should evolve the key
    ///   externally on epoch boundaries.
    kes_sk: Option<Arc<Mutex<qv_crypto::KesSecretKey>>>,
}

impl<V: VrfEvaluator> SlotTicker<V> {
    /// Create a new slot ticker.
    ///
    /// # Arguments
    ///
    /// * `slot_clock` — converts between wall-clock time and slot numbers
    /// * `pool_id` — the operator's pool identifier
    /// * `stake_distribution` — frozen snapshot of stake for the epoch
    /// * `epoch_nonce` — nonce seeding VRF inputs for this epoch
    /// * `vrf` — VRF evaluator (test or production)
    /// * `block_store` — persistent block storage
    /// * `utxo_store` — persistent UTXO set
    /// * `chain_state` — in-memory consensus state (tip)
    /// * `clear_pool` — transaction mempool
    /// * `active_slot_coeff` — Praos f parameter
    #[allow(clippy::too_many_arguments)] // 10 wiring deps; a struct-arg refactor is tracked in Faz 9
    pub fn new(
        slot_clock: SlotClock,
        pool_id: PoolId,
        stake_distribution: Arc<StakeDistribution>,
        epoch_nonce: EpochNonce,
        vrf: V,
        block_store: Arc<BlockStore<MemoryKvStore>>,
        utxo_store: Arc<UtxoStore<MemoryKvStore>>,
        chain_state: Arc<Mutex<ChainState>>,
        clear_pool: Arc<Mutex<ClearPool>>,
        active_slot_coeff: f64,
    ) -> Self {
        Self {
            slot_clock,
            pool_id,
            stake_distribution,
            epoch_nonce,
            vrf,
            block_store,
            _utxo_store: utxo_store,
            chain_state,
            clear_pool,
            _active_slot_coeff: active_slot_coeff,
            kes_sk: None,
        }
    }

    /// Attach a KES secret key for real header signing (per ADR-005).
    ///
    /// Without this, produced blocks carry an empty `kes_sig` and will fail
    /// verification under [`qv_consensus::DilithiumSumKesVerifier`]. Wire
    /// this in once the operator has loaded its KES keypair.
    #[must_use]
    pub fn with_kes_signing(mut self, kes_sk: Arc<Mutex<qv_crypto::KesSecretKey>>) -> Self {
        self.kes_sk = Some(kes_sk);
        self
    }

    /// Run the slot ticker: tick every slot duration and check for leadership.
    ///
    /// This is the main event loop that should be spawned as a background task.
    /// It runs indefinitely until dropped (or a shutdown signal is received
    /// outside this method).
    pub async fn run(self) -> Result<(), SlotTickerError> {
        let slot_duration_ms = self.slot_clock.slot_duration_ms();
        let mut interval_handle = interval(Duration::from_millis(slot_duration_ms));

        loop {
            // Tick at the slot boundary.
            interval_handle.tick().await;

            // Determine the current slot.
            let now_ms = current_time_ms();
            let current_slot_info = match self.slot_clock.current_info(now_ms) {
                Some(info) => info,
                None => {
                    // We're before genesis — shouldn't happen in normal operation.
                    debug!("before genesis, skipping slot");
                    continue;
                }
            };

            debug!(
                slot = current_slot_info.slot.as_u64(),
                epoch = current_slot_info.epoch.as_u64(),
                "processing slot"
            );

            // Check if we're elected as the slot leader.
            match check_leadership(
                &self.vrf,
                &self.pool_id,
                &self.epoch_nonce,
                current_slot_info.slot,
                &self.stake_distribution,
            ) {
                Ok(Some((vrf_output, vrf_proof))) => {
                    debug!(
                        slot = current_slot_info.slot.as_u64(),
                        "elected as slot leader"
                    );

                    // We're elected — produce a block.
                    if let Err(e) = self
                        .produce_block(current_slot_info.slot, vrf_output, vrf_proof)
                        .await
                    {
                        warn!(
                            slot = current_slot_info.slot.as_u64(),
                            error = %e,
                            "failed to produce block"
                        );
                    }
                }
                Ok(None) => {
                    // Not elected for this slot — just continue.
                    debug!(
                        slot = current_slot_info.slot.as_u64(),
                        "not elected for slot"
                    );
                }
                Err(e) => {
                    // Leadership check failed (e.g., VRF evaluation error, no stake).
                    warn!(
                        slot = current_slot_info.slot.as_u64(),
                        error = %e,
                        "leadership check failed"
                    );
                }
            }
        }
    }

    /// Produce a block for the given slot with the provided VRF proof.
    ///
    /// # Process
    ///
    /// 1. Lock the chain state and get the current tip (height, hash, slot)
    /// 2. Lock the mempool and drain the top transactions by fee density
    /// 3. Compute the Merkle root of the transactions
    /// 4. Build a BlockHeader with the VRF proof, KES signature (placeholder),
    ///    and producer key hash (derived from pool ID)
    /// 5. Construct the full Block and validate it
    /// 6. Store the block and update chain state
    /// 7. Emit appropriate logging
    async fn produce_block(
        &self,
        slot: Slot,
        _vrf_output: VrfOutput,
        vrf_proof: VrfProof,
    ) -> Result<(), SlotTickerError> {
        // Step 1: Get the current chain tip.
        let chain_state_lock = self.chain_state.lock().await;
        let tip = chain_state_lock.tip();
        let prev_hash = tip.hash;
        let parent_height = tip.height;
        drop(chain_state_lock); // Release lock early.

        let new_height = Height::from(parent_height.as_u64() + 1);

        // Step 2: Drain transactions from mempool (up to 100 for now).
        let clear_pool_lock = self.clear_pool.lock().await;
        let batch = clear_pool_lock.get_batch(100);
        let transactions: Vec<_> = batch.iter().map(|e| e.tx.clone()).collect();
        drop(clear_pool_lock); // Release lock early.

        // Step 3: Compute the Merkle root from transactions.
        let mut tx_ids = Vec::with_capacity(transactions.len());
        for tx in &transactions {
            let tx_id = tx.id().map_err(|_| SlotTickerError::TransactionError)?;
            tx_ids.push(tx_id);
        }
        let merkle_root = merkle_root_of(&tx_ids);

        // Step 4: Get the current UTXO commitment.
        // For now, use a placeholder. In production, this would be the hash
        // of the UTXO set after applying this block's transactions (envanter K-03).
        let utxo_commitment = qv_core::UtxoCommitment::ZERO;

        // Step 5: Build the block header — first WITHOUT the KES signature.
        // We then compute the bytes-to-sign over the unsigned header, sign
        // with KES, and re-build with the signature attached. This keeps the
        // hash domain (header bytes excluding kes_sig) stable.
        let now_ts = Timestamp::from_unix_secs(current_time_ms() / 1_000);
        let producer_key_hash = Hash256::from_bytes(sha3_256(self.pool_id.as_bytes()));

        // Build the unsigned header (kes_sig: Vec::new()) first; serialize it
        // canonically; sign those bytes; then attach the signature. Verifier
        // performs the symmetric operation: clear kes_sig, serialize, verify
        // signature against those bytes.
        let mut header = BlockHeader {
            version: qv_core::block::BLOCK_VERSION,
            prev_hash,
            height: new_height,
            slot,
            timestamp: now_ts,
            merkle_root,
            utxo_commitment,
            vrf_proof: vrf_proof.0.clone(),
            kes_sig: Vec::new(),
            producer_key_hash,
        };

        if let Some(kes_sk) = self.kes_sk.as_ref() {
            // Real KES path (per ADR-005).
            let bytes_to_sign =
                bincode::serialize(&header).map_err(|_| SlotTickerError::KesSignFailed)?;

            let sk_lock = kes_sk.lock().await;
            let sig = qv_crypto::kes_sign(&sk_lock, &bytes_to_sign)
                .map_err(|_| SlotTickerError::KesSignFailed)?;
            drop(sk_lock);

            header.kes_sig =
                bincode::serialize(&sig).map_err(|_| SlotTickerError::KesSignFailed)?;
        }
        // Else: legacy/test path — header.kes_sig stays empty. Verifiers
        // using `DilithiumSumKesVerifier` will reject such blocks.

        // Step 6: Construct the full block.
        let block = Block::new(header, transactions.clone());

        // Step 7: Validate the block structure.
        block
            .validate_structure()
            .map_err(|_| SlotTickerError::BlockValidation)?;

        // Step 8: Store the block.
        self.block_store
            .put_block(&block)
            .map_err(|_| SlotTickerError::Storage)?;

        // Step 9: Update chain state.
        let block_hash = block.hash().map_err(|_| SlotTickerError::BlockValidation)?;

        let mut chain_state_lock = self.chain_state.lock().await;
        let chain_entry = qv_consensus::ChainEntry {
            hash: block_hash,
            parent_hash: prev_hash,
            height: new_height,
            slot,
            producer_key_hash,
        };
        chain_state_lock
            .add_block(chain_entry)
            .map_err(|_| SlotTickerError::Consensus)?;
        drop(chain_state_lock);

        // Step 10: Log the successful block production.
        debug!(
            slot = slot.as_u64(),
            height = new_height.as_u64(),
            hash = %block_hash.to_hex(),
            tx_count = transactions.len(),
            "produced block"
        );

        Ok(())
    }
}

/// Errors that can occur during slot ticking or block production.
#[derive(Debug, thiserror::Error)]
pub enum SlotTickerError {
    /// Transaction ID computation failed.
    #[error("transaction error")]
    TransactionError,
    /// Block validation failed.
    #[error("block validation error")]
    BlockValidation,
    /// Storage operation failed.
    #[error("storage error")]
    Storage,
    /// Consensus state update failed.
    #[error("consensus error")]
    Consensus,
    /// KES sign / serialize failed (envanter K-04).
    #[error("kes sign failed")]
    KesSignFailed,
}

/// Compute the Merkle root from a list of transaction IDs.
///
/// Uses the Bitcoin-style Merkle tree construction: binary tree over TxIds,
/// padding the last level by duplicating the rightmost leaf until the level
/// is a power of two. Internal nodes hash left || right with SHA3-256.
/// Empty block yields [`MerkleRoot::ZERO`].
fn merkle_root_of(tx_ids: &[qv_core::TxId]) -> MerkleRoot {
    if tx_ids.is_empty() {
        return MerkleRoot::ZERO;
    }

    let mut level: Vec<Vec<u8>> = tx_ids.iter().map(|id| id.as_bytes().to_vec()).collect();

    while level.len() > 1 {
        let mut next_level = Vec::new();

        for i in (0..level.len()).step_by(2) {
            let left = &level[i];
            let right = if i + 1 < level.len() {
                &level[i + 1]
            } else {
                // Duplicate the last leaf if odd count.
                &level[i]
            };

            let mut preimage = Vec::with_capacity(64);
            preimage.extend_from_slice(left);
            preimage.extend_from_slice(right);
            let hash = sha3_256(&preimage);
            next_level.push(hash.to_vec());
        }

        level = next_level;
    }

    if level.is_empty() {
        MerkleRoot::ZERO
    } else {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&level[0][..32]);
        MerkleRoot(Hash256::from_bytes(bytes))
    }
}

/// Get the current time in milliseconds since UNIX_EPOCH.
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use qv_consensus::leader_schedule::TestVrf;
    use qv_core::{Amount, ConsensusParams, Epoch};

    fn test_vrf() -> TestVrf {
        TestVrf::new([0xAA; 32])
    }

    fn test_slot_clock() -> SlotClock {
        SlotClock::new(&ConsensusParams::mainnet(), 1_700_000_000)
    }

    fn test_stake_distribution() -> (StakeDistribution, PoolId) {
        let pool_id = PoolId::from_vrf_key(&[0xBB; 32]);
        let dist = StakeDistribution::new(
            Epoch::GENESIS,
            vec![(pool_id, Amount::from_smallest_units(1_000_000))],
        )
        .unwrap();
        (dist, pool_id)
    }

    #[test]
    fn merkle_root_empty_is_zero() {
        let root = merkle_root_of(&[]);
        assert_eq!(root, MerkleRoot::ZERO);
    }

    #[test]
    fn merkle_root_single_tx_is_tx_hash() {
        let tx_id = qv_core::TxId(Hash256::from_bytes([0x42; 32]));
        let root = merkle_root_of(&[tx_id]);
        // With a single leaf, the root's inner Hash256 should equal the
        // tx's inner Hash256 (TxId wraps Hash256).
        assert_eq!(root.0, tx_id.0);
    }

    #[test]
    fn merkle_root_two_txs() {
        let tx1 = qv_core::TxId(Hash256::from_bytes([0x11; 32]));
        let tx2 = qv_core::TxId(Hash256::from_bytes([0x22; 32]));
        let root = merkle_root_of(&[tx1, tx2]);
        // Root should be hash(tx1 || tx2), which is deterministic.
        let expected = {
            let mut preimage = Vec::with_capacity(64);
            preimage.extend_from_slice(tx1.as_bytes());
            preimage.extend_from_slice(tx2.as_bytes());
            Hash256::from_bytes(sha3_256(&preimage))
        };
        assert_eq!(root.0, expected);
    }

    #[test]
    fn merkle_root_odd_count_duplicates_last() {
        let tx1 = qv_core::TxId(Hash256::from_bytes([0x11; 32]));
        let tx2 = qv_core::TxId(Hash256::from_bytes([0x22; 32]));
        let tx3 = qv_core::TxId(Hash256::from_bytes([0x33; 32]));
        let root_123 = merkle_root_of(&[tx1, tx2, tx3]);

        // With 3 txs: [0,1] hash to h01, [2,2] (duplicated) hash to h22,
        // then h01 || h22 hash to root.
        let h01 = {
            let mut preimage = Vec::with_capacity(64);
            preimage.extend_from_slice(tx1.as_bytes());
            preimage.extend_from_slice(tx2.as_bytes());
            sha3_256(&preimage)
        };
        let h22 = {
            let mut preimage = Vec::with_capacity(64);
            preimage.extend_from_slice(tx3.as_bytes());
            preimage.extend_from_slice(tx3.as_bytes());
            sha3_256(&preimage)
        };
        let expected = {
            let mut preimage = Vec::with_capacity(64);
            preimage.extend_from_slice(&h01);
            preimage.extend_from_slice(&h22);
            Hash256::from_bytes(sha3_256(&preimage))
        };
        assert_eq!(root_123.0, expected);
    }

    #[test]
    fn current_time_ms_is_positive() {
        let t = current_time_ms();
        assert!(t > 0, "current time should be positive (after 1970)");
    }

    #[tokio::test]
    async fn slot_ticker_creation() {
        let clock = test_slot_clock();
        let (dist, pool_id) = test_stake_distribution();
        let vrf = test_vrf();

        // Create minimal storage/mempool/state for testing.
        let block_store = Arc::new(BlockStore::new(MemoryKvStore::new()));
        let utxo_store = Arc::new(UtxoStore::new(MemoryKvStore::new()));
        let chain_state = Arc::new(Mutex::new(ChainState::genesis(&ConsensusParams::mainnet())));
        let clear_pool = Arc::new(Mutex::new(ClearPool::new(
            qv_mempool::clear::ClearPoolConfig::ephemeral(),
        )));

        let _ticker = SlotTicker::new(
            clock,
            pool_id,
            Arc::new(dist),
            EpochNonce::GENESIS,
            vrf,
            block_store,
            utxo_store,
            chain_state,
            clear_pool,
            0.05,
        );

        // Just verify it was constructed without panicking.
    }

    #[test]
    fn merkle_root_is_deterministic() {
        let tx1 = qv_core::TxId(Hash256::from_bytes([0xAA; 32]));
        let tx2 = qv_core::TxId(Hash256::from_bytes([0xBB; 32]));
        let root_a = merkle_root_of(&[tx1, tx2]);
        let root_b = merkle_root_of(&[tx1, tx2]);
        assert_eq!(root_a, root_b);
    }
}
