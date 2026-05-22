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
//! pool's relative stake.
//!
//! The **consensus path** evaluates this in deterministic fixed-point
//! integer arithmetic via [`is_slot_leader`] (ADR-009): `f64` `exp`/`ln`
//! are not bit-identical across platforms, so they are not used to decide
//! leadership. [`leader_threshold`] and [`VrfOutput::to_unit_interval`]
//! remain as `f64` helpers for diagnostics and display only.
//!
//! # VRF abstraction
//!
//! The actual VRF primitive is behind a trait ([`VrfEvaluator`]) so that:
//! - The consensus engine is testable with a deterministic mock VRF.
//! - The real VRF (Ristretto or lattice-based) can be swapped in when
//!   ADR-004 lands.

use qv_core::Slot;
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
    /// Map the VRF output to a uniform value in `[0.0, 1.0)` for the Praos
    /// leader-election threshold comparison.
    ///
    /// The value is built from the **top 53 bits** of the output — the most
    /// an `f64` mantissa holds exactly. Numerator (`< 2^53`) and denominator
    /// (`2^53`) are both exact in `f64`, so the division is exact and
    /// deterministic and the result is *always strictly below `1.0`* — no
    /// rounding can push it to the boundary.
    ///
    /// The remaining VRF bytes carry no extra precision for an `f64`-based
    /// comparison. A fully precise comparison would be done in integer space
    /// against `threshold · 2^256`; tracked as future work (see
    /// `docs/security/qv-consensus-fork-finality-audit.md`).
    ///
    /// Not the consensus path — leader election uses [`is_slot_leader`]
    /// (ADR-009). Retained for diagnostics and human-readable display.
    #[must_use]
    #[allow(clippy::float_arithmetic)] // diagnostics-only f64 mapping
    pub fn to_unit_interval(&self) -> f64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.0[..8]);
        // Top 53 bits of the big-endian output — exact in an f64 mantissa.
        let top53 = u64::from_be_bytes(buf) >> 11;
        // 2^53 is exact and top53 < 2^53, so the result is exact and in
        // [0.0, 1.0).
        top53 as f64 / (1u64 << 53) as f64
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

// ============================================================================
// Deterministic leader check (ADR-009)
// ============================================================================
//
// The Praos leadership test is `p < 1 − (1−f)^σ`. Evaluating `(1−f)^σ` with
// f64 `exp`/`ln` is not bit-identical across platforms — a consensus
// determinism risk. The functions below evaluate it in 2^64-scaled `u128`
// integer arithmetic, so every node computes the same result.

/// Fixed-point scale for the deterministic leader check: `2^64`.
const LEADER_FIXED_SCALE: u128 = 1u128 << 64;

/// `|ln(1 − f)|` scaled by `2^64`, for `f` = [`ACTIVE_SLOT_COEFF`] (0.05).
///
/// Precomputed offline to high precision (see ADR-009 and the reference
/// model `docs/security/leader_check_reference.py`); never recomputed at
/// runtime, which is what makes the leader check deterministic. **If
/// [`ACTIVE_SLOT_COEFF`]
/// changes this constant must be recomputed** — the test
/// `ln_constant_matches_active_slot_coeff` guards against a stale value.
const LN_ONE_MINUS_F_MAG: u128 = 946_194_274_264_587_207;

/// Taylor-series term count for the fixed-point `exp` (ADR-009). Nine terms
/// give sub-ulp error for every `σ ∈ (0, 1]`.
const LEADER_EXP_TERMS: u32 = 9;

/// `exp(−m_real)` scaled by `2^64`, where `m = m_real · 2^64` and `m ≥ 0`.
///
/// Bounded Taylor series `Σ (−m)^k / k!`, evaluated in non-negative `u128`
/// fixed-point. For `σ ≤ 1` every intermediate stays below `2^124`, so
/// `u128` never overflows (ADR-009).
fn exp_neg_fixed(m: u128) -> u128 {
    let mut term = LEADER_FIXED_SCALE; // k = 0: (−m)^0 / 0! = 1.0
    let mut acc = LEADER_FIXED_SCALE;
    let mut subtract = true; // k = 1 is an odd power → subtract
    let mut k: u32 = 1;
    while k <= LEADER_EXP_TERMS {
        // term ← term · m / 2^64 / k  =  magnitude of m^k / k!
        term = (term.saturating_mul(m) >> 64)
            .checked_div(u128::from(k))
            .unwrap_or(0);
        if subtract {
            acc = acc.saturating_sub(term);
        } else {
            acc = acc.saturating_add(term);
        }
        subtract = !subtract;
        k = k.saturating_add(1);
    }
    acc
}

/// The Praos leader threshold `1 − (1−f)^σ`, scaled by `2^64`, computed in
/// deterministic fixed-point integer arithmetic. `σ = stake_num / stake_den`;
/// `stake_num ≤ stake_den` is required (relative stake ≤ 1).
fn leader_threshold_fixed(stake_num: u64, stake_den: u64) -> u128 {
    // m = |σ · ln(1−f)| scaled by 2^64.
    // Budget: stake_num < 2^64 and LN_ONE_MINUS_F_MAG < 2^60 ⇒ product < 2^124.
    let m = u128::from(stake_num)
        .saturating_mul(LN_ONE_MINUS_F_MAG)
        .checked_div(u128::from(stake_den))
        .unwrap_or(0);
    // threshold = 1 − exp(σ · ln(1−f)); exp_neg_fixed(...) ≤ 2^64.
    LEADER_FIXED_SCALE.saturating_sub(exp_neg_fixed(m))
}

/// The VRF output's leader value: its top 64 bits, i.e. `p · 2^64` where
/// `p ∈ [0, 1)` is the value compared against the leader threshold.
fn leader_vrf_value(output: &VrfOutput) -> u128 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&output.0[..8]);
    u128::from(u64::from_be_bytes(buf))
}

/// Deterministic Praos leader check (ADR-009).
///
/// Returns `true` iff a pool with relative stake `stake_num / stake_den` is
/// elected for the slot that produced VRF output `output`. The whole
/// computation is integer arithmetic, so every node agrees bit-for-bit —
/// unlike the f64 `exp`/`ln` path whose last bit can vary across platforms.
#[must_use]
pub fn is_slot_leader(stake_num: u64, stake_den: u64, output: &VrfOutput) -> bool {
    if stake_num == 0 || stake_den == 0 {
        return false;
    }
    leader_vrf_value(output) < leader_threshold_fixed(stake_num, stake_den)
}

/// Canonical VRF input for a given slot: `SHA3-256("QV-VRF" || nonce || slot_be)`.
#[must_use]
pub fn vrf_input(nonce: &EpochNonce, slot: Slot) -> Vec<u8> {
    let mut buf = Vec::with_capacity(6 + 32 + 8);
    buf.extend_from_slice(b"QV-VRF");
    buf.extend_from_slice(nonce.as_bytes());
    buf.extend_from_slice(&slot.as_u64().to_be_bytes());
    sha3_256(&buf).to_vec()
}

/// Compute the Praos leader threshold `T = 1 − (1 − f)^σ` as an `f64`.
///
/// **Not the consensus path.** Leader election uses [`is_slot_leader`], an
/// exact integer computation (ADR-009); the f64 `exp`/`ln` used here are not
/// bit-identical across platforms. This function is retained only for
/// diagnostics and human-readable display.
///
/// `sigma` = pool's relative stake in `[0.0, 1.0]`.
#[must_use]
#[allow(clippy::float_arithmetic)] // diagnostics-only f64 approximation
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

    let input = vrf_input(nonce, slot);
    let (output, proof) = vrf.evaluate(&input)?;

    if is_slot_leader(stake_num, stake_den, &output) {
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

    let input = vrf_input(nonce, slot);
    let output = vrf.verify(vrf_pk, &input, proof)?;

    Ok(is_slot_leader(stake_num, stake_den, &output))
}

// ============================================================================
// Production VRF — Ristretto255-VRF wrapper around qv_crypto::vrf
// ============================================================================

/// Production VRF evaluator backed by Ristretto255-VRF (`qv_crypto::vrf`).
///
/// Per ADR-004, this is the MVP/v1 VRF. A future hybrid lattice-VRF will be
/// a drop-in replacement (different `RealVrfEvaluator`-style struct, same
/// `VrfEvaluator` trait). Existing callers that hold a `dyn VrfEvaluator` or
/// `impl VrfEvaluator` work unchanged.
///
/// # Example
///
/// ```rust,no_run
/// # use qv_consensus::leader_schedule::RistrettoVrfEvaluator;
/// let seed = [42u8; 32];
/// let vrf = RistrettoVrfEvaluator::from_seed(&seed).unwrap();
/// // pass `vrf` to `check_leadership`, `verify_leadership`, etc.
/// ```
#[derive(Clone, Debug)]
pub struct RistrettoVrfEvaluator {
    secret: qv_crypto::VrfSecretKey,
    public: qv_crypto::VrfPublicKey,
}

impl RistrettoVrfEvaluator {
    /// Wrap an existing `qv_crypto::VrfKeyPair`.
    #[must_use]
    pub fn new(kp: qv_crypto::VrfKeyPair) -> Self {
        Self {
            secret: kp.secret,
            public: kp.public,
        }
    }

    /// Derive deterministically from a 32-byte seed (e.g. operator's
    /// HD-derived VRF seed).
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, LeaderError> {
        let kp = qv_crypto::VrfKeyPair::from_seed(seed)
            .map_err(|e| LeaderError::VrfEvaluation(format!("vrf keypair from_seed: {e}")))?;
        Ok(Self::new(kp))
    }

    /// Generate a fresh keypair from OS entropy.
    pub fn generate() -> Result<Self, LeaderError> {
        let kp = qv_crypto::VrfKeyPair::generate()
            .map_err(|e| LeaderError::VrfEvaluation(format!("vrf keypair generate: {e}")))?;
        Ok(Self::new(kp))
    }

    /// Public key bytes — register on-chain as `StakePool.vrf_key`.
    #[must_use]
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }
}

impl VrfEvaluator for RistrettoVrfEvaluator {
    fn evaluate(&self, input: &[u8]) -> Result<(VrfOutput, VrfProof), LeaderError> {
        let (out, proof) = qv_crypto::vrf_evaluate(&self.secret, input)
            .map_err(|e| LeaderError::VrfEvaluation(e.to_string()))?;
        Ok((
            VrfOutput(*out.as_bytes()),
            VrfProof(proof.as_bytes().to_vec()),
        ))
    }

    fn verify(
        &self,
        vrf_pk: &[u8],
        input: &[u8],
        proof: &VrfProof,
    ) -> Result<VrfOutput, LeaderError> {
        let pk_bytes: [u8; 32] = vrf_pk
            .try_into()
            .map_err(|_| LeaderError::VrfVerification("vrf_pk must be 32 bytes".into()))?;
        let pk = qv_crypto::VrfPublicKey::from_bytes(pk_bytes)
            .map_err(|e| LeaderError::VrfVerification(e.to_string()))?;
        let qproof = qv_crypto::VrfProof::from_bytes(proof.0.clone());
        let out = qv_crypto::vrf_verify(&pk, input, &qproof)
            .map_err(|e| LeaderError::VrfVerification(e.to_string()))?;
        Ok(VrfOutput(*out.as_bytes()))
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
    use qv_core::{Amount, Epoch, Hash256};

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
    fn to_unit_interval_always_in_half_open_unit_range() {
        // Even an all-ones output must stay strictly below 1.0 (the
        // function promises `[0.0, 1.0)` — no rounding to the boundary).
        let max = VrfOutput([0xFF; 32]).to_unit_interval();
        assert!(max < 1.0, "all-0xFF must be < 1.0, got {max}");
        assert!(max > 0.999, "all-0xFF should be close to 1.0");
        // All-zero is exactly 0.0.
        assert_eq!(VrfOutput([0x00; 32]).to_unit_interval(), 0.0);
        // Every uniform-byte output lands in [0.0, 1.0).
        for b in 0u8..=255 {
            let v = VrfOutput([b; 32]).to_unit_interval();
            assert!((0.0..1.0).contains(&v), "byte {b}: {v} out of range");
        }
    }

    // ------------------------------------------------------------------
    // Deterministic leader check (ADR-009 — fork-finality audit).
    // ------------------------------------------------------------------

    #[test]
    fn ln_constant_matches_active_slot_coeff() {
        // If ACTIVE_SLOT_COEFF is ever changed, LN_ONE_MINUS_F_MAG must be
        // recomputed — this test catches a stale constant.
        let expected = -(1.0 - ACTIVE_SLOT_COEFF).ln() * (LEADER_FIXED_SCALE as f64);
        let actual = LN_ONE_MINUS_F_MAG as f64;
        let rel_err = ((actual - expected) / expected).abs();
        assert!(rel_err < 1e-12, "LN_ONE_MINUS_F_MAG stale: rel_err {rel_err}");
    }

    #[test]
    fn exp_neg_fixed_of_zero_is_one() {
        // exp(0) = 1.0 → scaled value is exactly 2^64.
        assert_eq!(exp_neg_fixed(0), LEADER_FIXED_SCALE);
    }

    #[test]
    fn leader_threshold_fixed_matches_reference() {
        // Vectors generated by leader_check_reference.py (ADR-009).
        assert_eq!(leader_threshold_fixed(1, 1), 922_337_203_685_477_581);
        assert_eq!(leader_threshold_fixed(1, 2), 467_081_991_932_498_921);
        assert_eq!(leader_threshold_fixed(1, 10), 94_377_174_694_180_085);
        assert_eq!(leader_threshold_fixed(7, 10), 650_586_348_333_290_270);
        assert_eq!(leader_threshold_fixed(1, 1_000), 946_170_007_968_760);
        assert_eq!(leader_threshold_fixed(1, 1_000_000), 946_194_249_998);
    }

    #[test]
    fn sigma_one_threshold_approximates_f() {
        // σ = 1 ⇒ threshold = f = 0.05. floor(0.05 · 2^64) = 922337203685477580.
        let thr = leader_threshold_fixed(1, 1);
        assert!(thr.abs_diff(922_337_203_685_477_580) <= 2);
    }

    #[test]
    fn is_slot_leader_boundaries() {
        // VRF value 0 is below any positive threshold → leader.
        assert!(is_slot_leader(1, 1, &VrfOutput([0x00; 32])));
        // VRF value all-ones is far above the threshold → not leader.
        assert!(!is_slot_leader(1, 1, &VrfOutput([0xFF; 32])));
        // Zero stake → never leader.
        assert!(!is_slot_leader(0, 1, &VrfOutput([0x00; 32])));
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

    // ========================================================================
    // RistrettoVrfEvaluator (real VRF) — minimal smoke tests
    // ========================================================================

    #[test]
    fn ristretto_vrf_from_seed_is_deterministic() {
        let v1 = RistrettoVrfEvaluator::from_seed(&[7u8; 32]).unwrap();
        let v2 = RistrettoVrfEvaluator::from_seed(&[7u8; 32]).unwrap();
        assert_eq!(v1.public_key_bytes(), v2.public_key_bytes());
    }

    #[test]
    fn ristretto_vrf_evaluate_verify_roundtrip() {
        let vrf = RistrettoVrfEvaluator::from_seed(&[0x42u8; 32]).unwrap();
        let input = vrf_input(&EpochNonce::GENESIS, Slot::from(123));
        let (out, proof) = vrf.evaluate(&input).unwrap();
        let pk = vrf.public_key_bytes();
        let recovered = vrf.verify(&pk, &input, &proof).unwrap();
        assert_eq!(out, recovered);
    }

    #[test]
    fn ristretto_vrf_wrong_pk_fails() {
        let v1 = RistrettoVrfEvaluator::from_seed(&[1u8; 32]).unwrap();
        let v2 = RistrettoVrfEvaluator::from_seed(&[2u8; 32]).unwrap();
        let input = vrf_input(&EpochNonce::GENESIS, Slot::from(0));
        let (_out, proof) = v1.evaluate(&input).unwrap();
        let res = v1.verify(&v2.public_key_bytes(), &input, &proof);
        assert!(res.is_err());
    }

    #[test]
    fn ristretto_vrf_check_leadership_runs() {
        // Sanity: real VRF works inside check_leadership without panicking.
        let (dist, p1, _) = test_distribution();
        let vrf = RistrettoVrfEvaluator::from_seed(&[0xABu8; 32]).unwrap();
        let nonce = EpochNonce::GENESIS;

        let mut elected = 0u32;
        for s in 0..1_000u64 {
            if let Ok(Some(_)) = check_leadership(&vrf, &p1, &nonce, Slot::from(s), &dist) {
                elected += 1;
            }
        }
        // 70% stake @ f=0.05 → ~3.5% election rate → ~35/1000. Wide margins.
        assert!(elected > 0 && elected < 1_000);
    }
}
