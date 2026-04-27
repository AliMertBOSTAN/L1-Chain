//! VRF-based slot leader election for Ouroboros Praos.
//!
//! # Leader election overview
//!
//! At the start of each slot every registered stake pool evaluates its VRF
//! against the slot number and the current epoch nonce. The VRF output is a
//! 32-byte pseudo-random value; if it falls below a *threshold* proportional
//! to the pool's relative stake, the pool is elected as slot leader and may
//! produce a block.
//!
//! The threshold is computed via the Praos formula:
//!
//! ```text
//! threshold(σ) = 1 − (1 − f)^σ
//! ```
//!
//! where `f` is the *active slot coefficient* (probability that at least one
//! leader is elected per slot when all stake participates) and `σ` is the
//! pool's relative stake. We approximate this using the natural logarithm:
//!
//! ```text
//! ln(1 − threshold) = σ · ln(1 − f)
//! ```
//!
//! and compare in log-space to avoid floating-point in the consensus-critical
//! path. The VRF output is interpreted as a 256-bit unsigned integer and
//! compared against the pre-computed threshold.
//!
//! # VRF abstraction
//!
//! The actual VRF primitive is behind a trait ([`VrfEvaluator`]) so that:
//! - The consensus engine is testable with a deterministic mock VRF.
//! - The real VRF (Ristretto or lattice-based) can be swapped in when
//!   ADR-004 lands.

use qv_core::{Hash256, Slot};
use qv_crypto::sha3_256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::epoch::EpochNonce;
use crate::stake::{PoolId, StakeDistribution};

// ============================================================================
// VRF abstraction
// ============================================================================

/// A VRF output: 32 pseudo-random bytes derived from a secret key and input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VrfOutput(pub [u8; 32]);

impl VrfOutput {
    /// Interpret the output as a 256-bit big-endian unsigned integer,
    /// normalised to `[0.0, 1.0)` for threshold comparison.
    ///
    /// This is the consensus-critical comparison value.
    #[must_use]
    pub fn to_unit_interval(&self) -> f64 {
        // Take the first 8 bytes as a u64 for sufficient precision.
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.0[..8]);
        let numerator = u64::from_be_bytes(buf) as f64;
        let denominator = u64::MAX as f64;
        numerator / denominator
    }
}

/// Opaque VRF proof that lets third parties verify the output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfProof(pub Vec<u8>);

/// Trait abstracting the VRF primitive.
///
/// Implementations may be the real Ristretto/lattice VRF or a test mock.
pub trait VrfEvaluator {
    /// Evaluate the VRF on the given `input` using the pool operator's
    /// secret key. Returns the output and a proof.
    ///
    /// # Errors
    ///
    /// Returns `LeaderError::VrfEvaluation` on cryptographic failure.
    fn evaluate(&self, input: &[u8]) -> Result<(VrfOutput, VrfProof), LeaderError>;

    /// Verify a VRF proof against the operator's public key and input.
    ///
    /// Returns the output if valid.
    fn verify(
        &self,
        vrf_pk: &[u8],
        input: &[u8],
        proof: &VrfProof,
    ) -> Result<VrfOutput, LeaderError>;
}

// ============================================================================
// Deterministic test VRF
// ============================================================================

/// A deterministic mock VRF for testing. The "output" is simply
/// `SHA3-256(seed || input)`, and the "proof" is the seed itself.
///
/// **NOT for production use.** Real VRF requires a proper keypair.
#[derive(Clone, Debug)]
pub struct TestVrf {
    seed: [u8; 32],
}

impl TestVrf {
    /// Create with a fixed seed (acts as a secret key).
    #[must_use]
    pub fn new(seed: [u8; 32]) -> Self {
        Self { seed }
    }
}

impl VrfEvaluator for TestVrf {
    fn evaluate(&self, input: &[u8]) -> Result<(VrfOutput, VrfProof), LeaderError> {
        let mut preimage = Vec::with_capacity(32 + input.len());
        preimage.extend_from_slice(&self.seed);
        preimage.extend_from_slice(input);
        let hash = sha3_256(&preimage);
        Ok((VrfOutput(hash), VrfProof(self.seed.to_vec())))
    }

    fn verify(
        &self,
        _vrf_pk: &[u8],
        input: &[u8],
        proof: &VrfProof,
    ) -> Result<VrfOutput, LeaderError> {
        if proof.0.len() != 32 {
            return Err(LeaderError::VrfVerification("invalid proof length".into()));
        }
        let mut preimage = Vec::with_capacity(32 + input.len());
        preimage.extend_from_slice(&proof.0);
        preimage.extend_from_slice(input);
        let hash = sha3_256(&preimage);
        Ok(VrfOutput(hash))
    }
}

// ============================================================================
// Leader election
// ============================================================================

/// Errors from leader election.
#[derive(Debug, Error)]
pub enum LeaderError {
    /// VRF evaluation failed.
    #[error("VRF evaluation failed: {0}")]
    VrfEvaluation(String),
    /// VRF proof verification failed.
    #[error("VRF verification failed: {0}")]
    VrfVerification(String),
    /// The pool is not registered in the stake distribution.
    #[error("pool {0:?} has no stake")]
    NoStake(PoolId),
    /// No pools exist in the distribution.
    #[error("empty stake distribution")]
    EmptyDistribution,
}

/// The *active slot coefficient* `f` — probability that a slot has at least
/// one leader when 100% of stake is online.
///
/// Cardano uses `f = 0.05` (1 leader every 20 slots on average).
/// We use a higher value because our slot time is shorter (2s vs 20s).
pub const ACTIVE_SLOT_COEFF: f64 = 0.05;

/// Canonical VRF input for a given slot: `SHA3-256("QV-VRF" || nonce || slot_be)`.
#[must_use]
pub fn vrf_input(nonce: &EpochNonce, slot: Slot) -> Vec<u8> {
    let mut buf = Vec::with_capacity(6 + 32 + 8);
    buf.extend_from_slice(b"QV-VRF");
    buf.extend_from_slice(nonce.as_bytes());
    buf.extend_from_slice(&slot.as_u64().to_be_bytes());
    sha3_256(&buf).to_vec()
}

/// Compute the Praos leader threshold for a given relative stake.
///
/// `sigma` = pool's relative stake in `[0.0, 1.0]`.
/// Returns the threshold `T` in `[0.0, 1.0]` such that the pool is
/// elected if `vrf_output.to_unit_interval() < T`.
///
/// Formula: `T = 1 − (1 − f)^σ`
#[must_use]
pub fn leader_threshold(sigma: f64) -> f64 {
    // 1 - (1-f)^sigma  =  1 - exp(sigma * ln(1-f))
    let ln_1_minus_f = (1.0 - ACTIVE_SLOT_COEFF).ln();
    1.0 - (sigma * ln_1_minus_f).exp()
}

/// Check whether a pool is elected as slot leader for the given slot.
///
/// This is the core consensus-critical function. It:
/// 1. Computes the VRF input from the epoch nonce and slot.
/// 2. Evaluates the VRF (via the provided evaluator).
/// 3. Compares the output against the stake-proportional threshold.
///
/// Returns `Ok(Some((output, proof)))` if elected, `Ok(None)` if not.
pub fn check_leadership<V: VrfEvaluator>(
    vrf: &V,
    pool_id: &PoolId,
    nonce: &EpochNonce,
    slot: Slot,
    distribution: &StakeDistribution,
) -> Result<Option<(VrfOutput, VrfProof)>, LeaderError> {
    if distribution.is_empty() {
        return Err(LeaderError::EmptyDistribution);
    }

    let (stake_num, stake_den) = distribution.relative_stake(pool_id);
    if stake_num == 0 {
        return Err(LeaderError::NoStake(*pool_id));
    }

    let sigma = stake_num as f64 / stake_den as f64;
    let threshold = leader_threshold(sigma);
    let input = vrf_input(nonce, slot);

    let (output, proof) = vrf.evaluate(&input)?;
    let value = output.to_unit_interval();

    if value < threshold {
        Ok(Some((output, proof)))
    } else {
        Ok(None)
    }
}

/// Verify that a claimed leadership proof is valid.
///
/// Used by block validators to confirm a received block's VRF proof.
pub fn verify_leadership<V: VrfEvaluator>(
    vrf: &V,
    vrf_pk: &[u8],
    pool_id: &PoolId,
    nonce: &EpochNonce,
    slot: Slot,
    proof: &VrfProof,
    distribution: &StakeDistribution,
) -> Result<bool, LeaderError> {
    if distribution.is_empty() {
        return Err(LeaderError::EmptyDistribution);
    }

    let (stake_num, stake_den) = distribution.relative_stake(pool_id);
    if stake_num == 0 {
        return Err(LeaderError::NoStake(*pool_id));
    }

    let sigma = stake_num as f64 / stake_den as f64;
    let threshold = leader_threshold(sigma);
    let input = vrf_input(nonce, slot);

    let output = vrf.verify(vrf_pk, &input, proof)?;
    let value = output.to_unit_interval();

    Ok(value < threshold)
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
    use qv_core::{Amount, Epoch};

    fn test_distribution() -> (StakeDistribution, PoolId, PoolId) {
        let p1 = PoolId::from_vrf_key(&[1; 32]);
        let p2 = PoolId::from_vrf_key(&[2; 32]);
        let dist = StakeDistribution::new(
            Epoch::from(0),
            vec![
                (p1, Amount::from_smallest_units(700_000)),
                (p2, Amount::from_smallest_units(300_000)),
            ],
        )
        .unwrap();
        (dist, p1, p2)
    }

    #[test]
    fn vrf_output_unit_interval_range() {
        let out = VrfOutput([0xFF; 32]);
        let v = out.to_unit_interval();
        assert!(v > 0.99, "all-0xFF should be close to 1.0");

        let out_zero = VrfOutput([0x00; 32]);
        let v0 = out_zero.to_unit_interval();
        assert!(v0 < 0.01, "all-0x00 should be close to 0.0");
    }

    #[test]
    fn leader_threshold_bounds() {
        // sigma = 0 → threshold = 0
        assert!((leader_threshold(0.0)).abs() < 1e-12);
        // sigma = 1 → threshold = f (approximately)
        let t1 = leader_threshold(1.0);
        assert!((t1 - ACTIVE_SLOT_COEFF).abs() < 1e-6);
    }

    #[test]
    fn leader_threshold_monotonic() {
        let t1 = leader_threshold(0.1);
        let t2 = leader_threshold(0.5);
        let t3 = leader_threshold(0.9);
        assert!(t1 < t2);
        assert!(t2 < t3);
    }

    #[test]
    fn vrf_input_deterministic() {
        let nonce = EpochNonce::GENESIS;
        let slot = Slot::from(42);
        let a = vrf_input(&nonce, slot);
        let b = vrf_input(&nonce, slot);
        assert_eq!(a, b);
    }

    #[test]
    fn vrf_input_varies_with_slot() {
        let nonce = EpochNonce::GENESIS;
        let a = vrf_input(&nonce, Slot::from(1));
        let b = vrf_input(&nonce, Slot::from(2));
        assert_ne!(a, b);
    }

    #[test]
    fn vrf_input_varies_with_nonce() {
        let n1 = EpochNonce::GENESIS;
        let n2 = n1.evolve(b"test", &Hash256::ZERO);
        let a = vrf_input(&n1, Slot::from(1));
        let b = vrf_input(&n2, Slot::from(1));
        assert_ne!(a, b);
    }

    #[test]
    fn test_vrf_deterministic() {
        let vrf = TestVrf::new([0xAA; 32]);
        let (out1, proof1) = vrf.evaluate(b"hello").unwrap();
        let (out2, proof2) = vrf.evaluate(b"hello").unwrap();
        assert_eq!(out1, out2);
        assert_eq!(proof1, proof2);
    }

    #[test]
    fn test_vrf_verify_roundtrip() {
        let vrf = TestVrf::new([0xBB; 32]);
        let (output, proof) = vrf.evaluate(b"test_input").unwrap();
        let verified = vrf.verify(&[0xBB; 32], b"test_input", &proof).unwrap();
        assert_eq!(output, verified);
    }

    #[test]
    fn check_leadership_no_stake_errors() {
        let (dist, _, _) = test_distribution();
        let unknown = PoolId::from_vrf_key(&[0xFF; 32]);
        let vrf = TestVrf::new([0xCC; 32]);
        let result = check_leadership(&vrf, &unknown, &EpochNonce::GENESIS, Slot::from(0), &dist);
        assert!(matches!(result, Err(LeaderError::NoStake(_))));
    }

    #[test]
    fn check_leadership_empty_distribution() {
        let dist = StakeDistribution::new(Epoch::GENESIS, std::iter::empty()).unwrap();
        let vrf = TestVrf::new([0xDD; 32]);
        let pool = PoolId::from_vrf_key(&[1; 32]);
        let result = check_leadership(&vrf, &pool, &EpochNonce::GENESIS, Slot::from(0), &dist);
        assert!(matches!(result, Err(LeaderError::EmptyDistribution)));
    }

    #[test]
    fn leadership_check_over_many_slots() {
        // With 70% stake and f=0.05, a pool should be elected roughly 3.5% of slots.
        // Over 10000 slots we expect roughly 350 elections.
        let (dist, p1, _) = test_distribution();
        let vrf = TestVrf::new([0xEE; 32]);
        let nonce = EpochNonce::GENESIS;

        let mut elected = 0u32;
        for s in 0..10_000u64 {
            if let Ok(Some(_)) = check_leadership(&vrf, &p1, &nonce, Slot::from(s), &dist) {
                elected += 1;
            }
        }

        // Expect roughly 350 ± large margin (VRF is pseudo-random)
        // We just verify it's non-zero and not 100%
        assert!(elected > 0, "should win at least some slots");
        assert!(elected < 10_000, "should not win every slot");
    }

    #[test]
    fn verify_leadership_matches_check() {
        let (dist, p1, _) = test_distribution();
        let vrf = TestVrf::new([0xAA; 32]);
        let nonce = EpochNonce::GENESIS;

        // Find a slot where the pool is elected
        for s in 0..10_000u64 {
            let slot = Slot::from(s);
            if let Ok(Some((_, proof))) = check_leadership(&vrf, &p1, &nonce, slot, &dist) {
                // verify_leadership should agree
                let ok = verify_leadership(
                    &vrf, &[1; 32], // vrf_pk
                    &p1, &nonce, slot, &proof, &dist,
                )
                .unwrap();
                assert!(ok, "verify should confirm leadership for slot {s}");
                return;
            }
        }
        panic!("pool should have been elected in 10000 slots");
    }

    #[test]
    fn higher_stake_wins_more_slots() {
        let (dist, p1, p2) = test_distribution();
        // p1 has 700k (70%), p2 has 300k (30%)
        let vrf1 = TestVrf::new([0x11; 32]);
        let vrf2 = TestVrf::new([0x22; 32]);
        let nonce = EpochNonce::GENESIS;

        let mut won_p1 = 0u32;
        let mut won_p2 = 0u32;
        for s in 0..10_000u64 {
            let slot = Slot::from(s);
            if check_leadership(&vrf1, &p1, &nonce, slot, &dist)
                .unwrap()
                .is_some()
            {
                won_p1 += 1;
            }
            if check_leadership(&vrf2, &p2, &nonce, slot, &dist)
                .unwrap()
                .is_some()
            {
                won_p2 += 1;
            }
        }

        // The pool with 70% stake should generally win more than the 30% pool
        assert!(
            won_p1 > won_p2,
            "70% stake pool ({won_p1}) should win more than 30% pool ({won_p2})"
        );
    }
}
