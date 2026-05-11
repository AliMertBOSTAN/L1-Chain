//! Block production and consensus-level validation.
//!
//! This module sits above `qv-core::Block` structural validation and adds
//! consensus-specific checks:
//!
//! 1. **Slot monotonicity** — a block's slot must exceed its parent's slot.
//! 2. **Height continuity** — `height = parent.height + 1`.
//! 3. **VRF leadership proof** — the producer must be an elected slot leader.
//! 4. **KES signature** — the header is signed by the producer's evolving key.
//! 5. **Timestamp sanity** — the block's wall-clock timestamp must be within
//!    the expected window for its slot.
//! 6. **Version check** — only known block versions are accepted.
//!
//! # KES abstraction
//!
//! Like the VRF, the KES primitive is behind a trait ([`KesVerifier`]) so
//! the consensus engine can be tested without a real KES implementation.

use qv_core::{Block, BlockHash, BlockHeader, ConsensusParams, Height, Slot, BLOCK_VERSION};
use thiserror::Error;

use crate::epoch::EpochNonce;
use crate::leader_schedule::{verify_leadership, LeaderError, VrfEvaluator, VrfProof};
use crate::slot::SlotClock;
use crate::stake::{PoolId, StakeDistribution, StakePool};

// ============================================================================
// KES abstraction
// ============================================================================

/// Trait abstracting the Key-Evolving Signature scheme.
///
/// Implementations may be the real Dilithium-sum KES or a test stub.
pub trait KesVerifier {
    /// Verify a KES signature over `message` using the operator's KES
    /// public key for the given slot's KES period.
    ///
    /// Returns `Ok(())` if the signature is valid.
    fn verify_kes(
        &self,
        kes_pk: &[u8],
        slot: Slot,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), BlockValidationError>;
}

/// A test KES verifier that accepts anything non-empty.
#[derive(Clone, Debug)]
pub struct TestKesVerifier;

impl KesVerifier for TestKesVerifier {
    fn verify_kes(
        &self,
        _kes_pk: &[u8],
        _slot: Slot,
        _message: &[u8],
        signature: &[u8],
    ) -> Result<(), BlockValidationError> {
        if signature.is_empty() {
            return Err(BlockValidationError::InvalidKesSignature(
                "empty KES signature".into(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Consensus-level block validation errors.
#[derive(Debug, Error)]
pub enum BlockValidationError {
    /// Block version is not supported.
    #[error("unsupported block version {0}")]
    UnsupportedVersion(u32),

    /// Block slot is not strictly greater than parent's slot.
    #[error("slot {block_slot} must be > parent slot {parent_slot}")]
    SlotNotMonotonic { block_slot: u64, parent_slot: u64 },

    /// Block height is not exactly parent height + 1.
    #[error("height {block_height} must be parent height {parent_height} + 1")]
    HeightMismatch {
        block_height: u64,
        parent_height: u64,
    },

    /// The declared prev_hash does not match the expected parent hash.
    #[error("prev_hash mismatch")]
    PrevHashMismatch,

    /// Block timestamp is outside the acceptable window for its slot.
    #[error("timestamp {timestamp} outside window [{slot_start}, {slot_end}) for slot {slot}")]
    TimestampOutOfRange {
        slot: u64,
        timestamp: u64,
        slot_start: u64,
        slot_end: u64,
    },

    /// VRF leadership check failed.
    #[error("leadership verification failed: {0}")]
    Leadership(#[from] LeaderError),

    /// The block's producer_key_hash does not correspond to any known pool.
    #[error("unknown block producer: {0:?}")]
    UnknownProducer(PoolId),

    /// KES signature is invalid.
    #[error("invalid KES signature: {0}")]
    InvalidKesSignature(String),

    /// Structural validation from qv-core failed.
    #[error("structural error: {0}")]
    Structure(String),
}

// ============================================================================
// Validator
// ============================================================================

/// Context needed to validate a single block at the consensus level.
pub struct BlockValidationContext<'a> {
    /// Parent block's hash.
    pub parent_hash: BlockHash,
    /// Parent block's slot.
    pub parent_slot: Slot,
    /// Parent block's height.
    pub parent_height: Height,
    /// The slot clock for time calculations.
    pub clock: &'a SlotClock,
    /// Consensus parameters.
    pub consensus_params: &'a ConsensusParams,
    /// Current epoch nonce for VRF verification.
    pub epoch_nonce: &'a EpochNonce,
    /// Frozen stake distribution for this epoch.
    pub stake_distribution: &'a StakeDistribution,
    /// Pool registry for looking up VRF/KES keys.
    pub pools: &'a [StakePool],
}

/// Validate a block header at the consensus level (no transaction execution).
///
/// This performs all checks that require consensus state but not UTXO
/// execution. Transaction-level validation (scripts, double-spend, etc.)
/// is handled by the ledger layer.
pub fn validate_block_header<V: VrfEvaluator, K: KesVerifier>(
    header: &BlockHeader,
    ctx: &BlockValidationContext<'_>,
    vrf: &V,
    kes: &K,
) -> Result<(), BlockValidationError> {
    // 1. Version check
    if header.version != BLOCK_VERSION {
        return Err(BlockValidationError::UnsupportedVersion(header.version));
    }

    // 2. Chain linkage: prev_hash
    if header.prev_hash != ctx.parent_hash {
        return Err(BlockValidationError::PrevHashMismatch);
    }

    // 3. Slot monotonicity
    if header.slot.as_u64() <= ctx.parent_slot.as_u64() {
        return Err(BlockValidationError::SlotNotMonotonic {
            block_slot: header.slot.as_u64(),
            parent_slot: ctx.parent_slot.as_u64(),
        });
    }

    // 4. Height continuity
    let expected_height = ctx.parent_height.as_u64().saturating_add(1);
    if header.height.as_u64() != expected_height {
        return Err(BlockValidationError::HeightMismatch {
            block_height: header.height.as_u64(),
            parent_height: ctx.parent_height.as_u64(),
        });
    }

    // 5. Timestamp sanity: must be within [slot_start, slot_end) in seconds.
    let slot_start_secs = ctx
        .clock
        .slot_start_time_ms(header.slot)
        .saturating_div(1_000);
    let slot_end_secs =
        slot_start_secs.saturating_add(ctx.consensus_params.slot_duration_ms.saturating_div(1_000));
    let ts = header.timestamp.as_u64();
    if ts < slot_start_secs || ts >= slot_end_secs {
        return Err(BlockValidationError::TimestampOutOfRange {
            slot: header.slot.as_u64(),
            timestamp: ts,
            slot_start: slot_start_secs,
            slot_end: slot_end_secs,
        });
    }

    // 6. Identify the producer pool from the key hash.
    let producer_pool_id = find_pool_by_key_hash(ctx.pools, &header.producer_key_hash)
        .ok_or_else(|| BlockValidationError::UnknownProducer(PoolId(header.producer_key_hash)))?;

    // 7. VRF leadership proof verification.
    let pool = ctx
        .pools
        .iter()
        .find(|p| p.id == producer_pool_id)
        .ok_or_else(|| BlockValidationError::UnknownProducer(producer_pool_id))?;

    let vrf_proof = VrfProof(header.vrf_proof.clone());
    let is_leader = verify_leadership(
        vrf,
        &pool.vrf_key,
        &producer_pool_id,
        ctx.epoch_nonce,
        header.slot,
        &vrf_proof,
        ctx.stake_distribution,
    )?;

    if !is_leader {
        return Err(BlockValidationError::Leadership(
            LeaderError::VrfVerification("VRF output above threshold".into()),
        ));
    }

    // 8. KES signature verification.
    let header_bytes = header
        .canonical_bytes()
        .map_err(|e| BlockValidationError::InvalidKesSignature(e.to_string()))?;

    kes.verify_kes(&pool.kes_key, header.slot, &header_bytes, &header.kes_sig)?;

    Ok(())
}

/// Validate a full block (header + structure).
pub fn validate_block<V: VrfEvaluator, K: KesVerifier>(
    block: &Block,
    ctx: &BlockValidationContext<'_>,
    vrf: &V,
    kes: &K,
) -> Result<(), BlockValidationError> {
    // Structural validation first (fast, no crypto)
    block
        .validate_structure()
        .map_err(|e| BlockValidationError::Structure(e.to_string()))?;

    // Then consensus-level header validation
    validate_block_header(&block.header, ctx, vrf, kes)?;

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Look up a pool by its VRF key hash (the `producer_key_hash` field in the
/// block header is `SHA3-256(vrf_pk)`).
fn find_pool_by_key_hash(pools: &[StakePool], key_hash: &qv_core::Hash256) -> Option<PoolId> {
    for pool in pools {
        let pool_key_hash = qv_core::Hash256::from_bytes(qv_crypto::sha3_256(&pool.vrf_key));
        if pool_key_hash == *key_hash {
            return Some(pool.id);
        }
    }
    None
}

// ============================================================================
// Production KES verifier — Sum-KES on Dilithium (qv_crypto::kes)
// ============================================================================

/// Production KES verifier backed by Sum-KES on Dilithium (`qv_crypto::kes`).
///
/// Per ADR-005, this is the MVP/v1 KES. The verifier is stateless: it accepts
/// `kes_pk` (the 32-byte Merkle root) and a bincode-encoded `KesSignature`,
/// and checks the leaf Dilithium signature + Merkle path.
///
/// `slot` is informational; the actual period is embedded in the signature
/// as `KesSignature.period`. A consensus-level cross-check between `slot`
/// and `period` (via `slot_to_kes_period`) is the caller's responsibility
/// when binding to a specific epoch — for now this verifier accepts any
/// well-formed signature.
#[derive(Clone, Debug, Default)]
pub struct DilithiumSumKesVerifier;

impl DilithiumSumKesVerifier {
    /// Construct a default verifier (stateless; no setup required).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl KesVerifier for DilithiumSumKesVerifier {
    fn verify_kes(
        &self,
        kes_pk: &[u8],
        _slot: Slot,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), BlockValidationError> {
        let pk_bytes: [u8; 32] = kes_pk.try_into().map_err(|_| {
            BlockValidationError::InvalidKesSignature("kes_pk must be 32 bytes".into())
        })?;
        let pk = qv_crypto::KesPublicKey::from_bytes(pk_bytes);

        let sig: qv_crypto::KesSignature = bincode::deserialize(signature).map_err(|e| {
            BlockValidationError::InvalidKesSignature(format!("bincode decode: {e}"))
        })?;

        let valid = qv_crypto::kes_verify(&pk, &sig, message)
            .map_err(|e| BlockValidationError::InvalidKesSignature(e.to_string()))?;

        if !valid {
            return Err(BlockValidationError::InvalidKesSignature(
                "kes signature verification rejected".into(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division,
    clippy::float_arithmetic
)]
mod tests {
    use super::*;
    use qv_core::{
        Amount, BlockHash, BlockHeader, Epoch, Hash256, Height, MerkleRoot, Slot, Timestamp,
        UtxoCommitment, BLOCK_VERSION,
    };

    use crate::epoch::EpochNonce;
    use crate::leader_schedule::TestVrf;
    use crate::stake::{PoolId, StakeDistribution, StakePool};

    fn make_pool(id_byte: u8) -> StakePool {
        let vrf_key = vec![id_byte; 32];
        StakePool {
            id: PoolId::from_vrf_key(&vrf_key),
            vrf_key,
            kes_key: vec![id_byte; 32],
            pledge: Amount::from_smallest_units(1_000_000),
            margin_num: 5,
            margin_den: 100,
            fixed_cost: Amount::from_smallest_units(340_000_000),
            active: true,
        }
    }

    fn test_setup() -> (
        StakePool,
        StakeDistribution,
        SlotClock,
        ConsensusParams,
        EpochNonce,
    ) {
        let pool = make_pool(0x11);
        let dist = StakeDistribution::new(
            Epoch::from(0),
            vec![(pool.id, Amount::from_smallest_units(1_000_000))],
        )
        .unwrap();
        let params = ConsensusParams::mainnet();
        let clock = SlotClock::new(&params, 1_000_000);
        let nonce = EpochNonce::GENESIS;
        (pool, dist, clock, params, nonce)
    }

    fn make_header(
        pool: &StakePool,
        slot: Slot,
        parent_hash: BlockHash,
        parent_height: Height,
        clock: &SlotClock,
    ) -> BlockHeader {
        let key_hash = Hash256::from_bytes(qv_crypto::sha3_256(&pool.vrf_key));
        BlockHeader {
            version: BLOCK_VERSION,
            prev_hash: parent_hash,
            height: Height::from(parent_height.as_u64() + 1),
            slot,
            timestamp: clock.slot_start_timestamp(slot),
            merkle_root: MerkleRoot::ZERO,
            utxo_commitment: UtxoCommitment::ZERO,
            vrf_proof: pool.vrf_key.clone(), // TestVrf uses seed as proof
            kes_sig: vec![0xAB; 1],          // TestKesVerifier accepts non-empty
            producer_key_hash: key_hash,
        }
    }

    fn make_ctx<'a>(
        pool: &'a StakePool,
        dist: &'a StakeDistribution,
        clock: &'a SlotClock,
        params: &'a ConsensusParams,
        nonce: &'a EpochNonce,
        parent_hash: BlockHash,
        parent_slot: Slot,
        parent_height: Height,
    ) -> BlockValidationContext<'a> {
        BlockValidationContext {
            parent_hash,
            parent_slot,
            parent_height,
            clock,
            consensus_params: params,
            epoch_nonce: nonce,
            stake_distribution: dist,
            pools: std::slice::from_ref(pool),
        }
    }

    #[test]
    fn valid_header_passes() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new(pool.vrf_key.clone().try_into().unwrap());
        let kes = TestKesVerifier;

        // Find a slot where the pool is actually elected
        let parent_hash = BlockHash::ZERO;
        let parent_slot = Slot::GENESIS;
        let parent_height = Height::GENESIS;

        for s in 1..10_000u64 {
            let slot = Slot::from(s);
            let input = crate::leader_schedule::vrf_input(&nonce, slot);
            let (output, _) = vrf.evaluate(&input).unwrap();
            let sigma = 1.0; // sole pool = 100% stake
            let threshold = crate::leader_schedule::leader_threshold(sigma);
            if output.to_unit_interval() < threshold {
                let header = make_header(&pool, slot, parent_hash, parent_height, &clock);
                let ctx = make_ctx(
                    &pool,
                    &dist,
                    &clock,
                    &params,
                    &nonce,
                    parent_hash,
                    parent_slot,
                    parent_height,
                );
                let result = validate_block_header(&header, &ctx, &vrf, &kes);
                assert!(result.is_ok(), "slot {s}: {result:?}");
                return;
            }
        }
        panic!("could not find an elected slot in 10000 tries");
    }

    #[test]
    fn wrong_version_rejected() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new([0x11; 32]);
        let kes = TestKesVerifier;
        let mut header = make_header(
            &pool,
            Slot::from(1),
            BlockHash::ZERO,
            Height::GENESIS,
            &clock,
        );
        header.version = 99;
        let ctx = make_ctx(
            &pool,
            &dist,
            &clock,
            &params,
            &nonce,
            BlockHash::ZERO,
            Slot::GENESIS,
            Height::GENESIS,
        );
        let result = validate_block_header(&header, &ctx, &vrf, &kes);
        assert!(matches!(
            result,
            Err(BlockValidationError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn wrong_prev_hash_rejected() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new([0x11; 32]);
        let kes = TestKesVerifier;
        let header = make_header(
            &pool,
            Slot::from(1),
            BlockHash::from_bytes([0xFF; 32]), // wrong parent
            Height::GENESIS,
            &clock,
        );
        let ctx = make_ctx(
            &pool,
            &dist,
            &clock,
            &params,
            &nonce,
            BlockHash::ZERO,
            Slot::GENESIS,
            Height::GENESIS,
        );
        let result = validate_block_header(&header, &ctx, &vrf, &kes);
        assert!(matches!(
            result,
            Err(BlockValidationError::PrevHashMismatch)
        ));
    }

    #[test]
    fn slot_not_monotonic_rejected() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new([0x11; 32]);
        let kes = TestKesVerifier;
        let header = make_header(
            &pool,
            Slot::from(5), // same or earlier than parent
            BlockHash::ZERO,
            Height::GENESIS,
            &clock,
        );
        let ctx = make_ctx(
            &pool,
            &dist,
            &clock,
            &params,
            &nonce,
            BlockHash::ZERO,
            Slot::from(10), // parent slot is later
            Height::GENESIS,
        );
        let result = validate_block_header(&header, &ctx, &vrf, &kes);
        assert!(matches!(
            result,
            Err(BlockValidationError::SlotNotMonotonic { .. })
        ));
    }

    #[test]
    fn height_mismatch_rejected() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new([0x11; 32]);
        let kes = TestKesVerifier;
        let mut header = make_header(
            &pool,
            Slot::from(1),
            BlockHash::ZERO,
            Height::GENESIS,
            &clock,
        );
        header.height = Height::from(5); // should be 1
        let ctx = make_ctx(
            &pool,
            &dist,
            &clock,
            &params,
            &nonce,
            BlockHash::ZERO,
            Slot::GENESIS,
            Height::GENESIS,
        );
        let result = validate_block_header(&header, &ctx, &vrf, &kes);
        assert!(matches!(
            result,
            Err(BlockValidationError::HeightMismatch { .. })
        ));
    }

    #[test]
    fn empty_kes_sig_rejected() {
        let (pool, dist, clock, params, nonce) = test_setup();
        let vrf = TestVrf::new([0x11; 32]);
        let kes = TestKesVerifier;

        // Find an elected slot
        for s in 1..10_000u64 {
            let slot = Slot::from(s);
            let input = crate::leader_schedule::vrf_input(&nonce, slot);
            let (output, _) = vrf.evaluate(&input).unwrap();
            if output.to_unit_interval() < crate::leader_schedule::leader_threshold(1.0) {
                let mut header = make_header(&pool, slot, BlockHash::ZERO, Height::GENESIS, &clock);
                header.kes_sig = Vec::new(); // empty → rejected
                let ctx = make_ctx(
                    &pool,
                    &dist,
                    &clock,
                    &params,
                    &nonce,
                    BlockHash::ZERO,
                    Slot::GENESIS,
                    Height::GENESIS,
                );
                let result = validate_block_header(&header, &ctx, &vrf, &kes);
                assert!(matches!(
                    result,
                    Err(BlockValidationError::InvalidKesSignature(_))
                ));
                return;
            }
        }
        panic!("no elected slot found");
    }
}
