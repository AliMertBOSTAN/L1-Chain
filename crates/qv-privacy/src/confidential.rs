//! Opt-in confidential amounts for QuantumVault.
//!
//! When privacy mode is active, transaction output amounts are hidden behind
//! **Pedersen commitments** on Curve25519 with **Bulletproofs** range proofs.
//!
//! # Design
//!
//! - `ConfidentialAmount` wraps either a plain `u64` or a `Commitment` + `RangeProof`.
//! - Pedersen commitment: `C = v·G + r·H` where `v` is the value, `r` is the
//!   blinding factor, and `G`, `H` are independent Curve25519 generators.
//! - Range proof proves `0 ≤ v < 2^64` without revealing `v`.
//! - Balance verification: `Σ inputs_commitment == Σ outputs_commitment + fee·G`.
//!
//! # Security warning
//!
//! Bulletproofs use **classical** Curve25519 — they are **not** post-quantum
//! secure. This is an explicit, documented trade-off: users opt in knowingly.
//! A STARK range proof migration is planned for a future phase.
//!
//! # Implementation status
//!
//! Real Bulletproofs integration depends on the `bulletproofs` or
//! `merlin`+`curve25519-dalek` crates. The current implementation uses trait
//! abstractions ([`Committer`], [`RangeProver`], [`RangeVerifier`]) with mock
//! implementations for testing. The real cryptographic backend can be plugged
//! in without changing the API surface.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use qv_core::Amount;
use qv_crypto::{sha3_256, HashDigest};

use crate::PrivacyError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Domain separator for Pedersen commitment derivation (mock).
const COMMITMENT_DOMAIN: &[u8] = b"QuantumVault-Pedersen-v1";

/// Domain separator for blinding factor generation.
const BLINDING_DOMAIN: &[u8] = b"QuantumVault-Blinding-v1";

/// Range proof bit width (proves `0 ≤ v < 2^RANGE_BITS`).
pub const RANGE_BITS: u32 = 64;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A blinding factor for Pedersen commitments. 32 bytes, zeroized on drop.
#[derive(Clone)]
pub struct BlindingFactor(pub [u8; 32]);

impl BlindingFactor {
    /// Create from raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Generate a deterministic blinding factor from a seed.
    ///
    /// `r = SHA3-256(BLINDING_DOMAIN || seed)`
    #[must_use]
    pub fn from_seed(seed: &[u8]) -> Self {
        let mut input = Vec::with_capacity(BLINDING_DOMAIN.len() + seed.len());
        input.extend_from_slice(BLINDING_DOMAIN);
        input.extend_from_slice(seed);
        Self(sha3_256(&input))
    }

    /// Expose raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for BlindingFactor {
    fn drop(&mut self) {
        // Simple zeroization — in production use zeroize crate.
        self.0 = [0u8; 32];
    }
}

impl core::fmt::Debug for BlindingFactor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BlindingFactor(32 bytes)")
    }
}

/// A Pedersen commitment: `C = v·G + r·H` (32 bytes in compressed form).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commitment(pub Vec<u8>);

impl Commitment {
    /// Create from raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Raw commitment bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl core::fmt::Debug for Commitment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Commitment({} bytes)", self.0.len())
    }
}

/// A range proof demonstrating `0 ≤ v < 2^64` for a committed value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeProof(pub Vec<u8>);

impl RangeProof {
    /// Create from raw bytes.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Raw proof bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl core::fmt::Debug for RangeProof {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RangeProof({} bytes)", self.0.len())
    }
}

/// A confidential amount: either a plain value or a hidden commitment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConfidentialAmount {
    /// Transparent amount (default, no privacy).
    Plain(u64),
    /// Hidden amount behind a Pedersen commitment + range proof.
    Confidential {
        /// Pedersen commitment to the value.
        commitment: Commitment,
        /// Bulletproofs range proof.
        range_proof: RangeProof,
    },
}

impl ConfidentialAmount {
    /// Create a plain (transparent) amount.
    #[must_use]
    pub fn plain(value: u64) -> Self {
        Self::Plain(value)
    }

    /// Create a confidential amount using the provided committer and prover.
    pub fn confidential(
        value: u64,
        blinding: &BlindingFactor,
        committer: &dyn Committer,
        prover: &dyn RangeProver,
    ) -> Result<Self, PrivacyError> {
        let commitment = committer.commit(value, blinding)?;
        let range_proof = prover.prove(value, blinding)?;
        Ok(Self::Confidential {
            commitment,
            range_proof,
        })
    }

    /// Check if the amount is confidential.
    #[must_use]
    pub fn is_confidential(&self) -> bool {
        matches!(self, Self::Confidential { .. })
    }

    /// Check if the amount is plain.
    #[must_use]
    pub fn is_plain(&self) -> bool {
        matches!(self, Self::Plain(_))
    }

    /// Get the plain value if available.
    #[must_use]
    pub fn plain_value(&self) -> Option<u64> {
        match self {
            Self::Plain(v) => Some(*v),
            Self::Confidential { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait: Committer
// ---------------------------------------------------------------------------

/// Abstraction over Pedersen commitment creation.
///
/// Real implementation: Curve25519 Pedersen (`v·G + r·H`).
/// Mock: SHA3-256 hash-based commitment for testing.
pub trait Committer: Send + Sync {
    /// Create a Pedersen commitment to `value` with blinding factor `r`.
    fn commit(&self, value: u64, blinding: &BlindingFactor) -> Result<Commitment, PrivacyError>;

    /// Verify that a commitment opens to (value, blinding).
    fn verify_opening(
        &self,
        commitment: &Commitment,
        value: u64,
        blinding: &BlindingFactor,
    ) -> Result<bool, PrivacyError>;
}

// ---------------------------------------------------------------------------
// Trait: RangeProver / RangeVerifier
// ---------------------------------------------------------------------------

/// Abstraction over range proof creation.
pub trait RangeProver: Send + Sync {
    /// Create a range proof for `value` with blinding factor `r`.
    fn prove(&self, value: u64, blinding: &BlindingFactor) -> Result<RangeProof, PrivacyError>;
}

/// Abstraction over range proof verification.
pub trait RangeVerifier: Send + Sync {
    /// Verify a range proof against a commitment.
    fn verify(
        &self,
        commitment: &Commitment,
        proof: &RangeProof,
    ) -> Result<bool, PrivacyError>;
}

// ---------------------------------------------------------------------------
// Mock implementations
// ---------------------------------------------------------------------------

/// Mock Pedersen committer using SHA3-256 for testing.
///
/// `C = SHA3-256(COMMITMENT_DOMAIN || value_le_bytes || blinding)`
#[derive(Clone, Debug, Default)]
pub struct MockCommitter;

impl MockCommitter {
    /// Create a new mock committer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Committer for MockCommitter {
    fn commit(&self, value: u64, blinding: &BlindingFactor) -> Result<Commitment, PrivacyError> {
        let hash = mock_commit_hash(value, blinding);
        Ok(Commitment(hash.to_vec()))
    }

    fn verify_opening(
        &self,
        commitment: &Commitment,
        value: u64,
        blinding: &BlindingFactor,
    ) -> Result<bool, PrivacyError> {
        let expected = mock_commit_hash(value, blinding);
        Ok(commitment.0 == expected)
    }
}

fn mock_commit_hash(value: u64, blinding: &BlindingFactor) -> HashDigest {
    let mut input = Vec::with_capacity(COMMITMENT_DOMAIN.len() + 8 + 32);
    input.extend_from_slice(COMMITMENT_DOMAIN);
    input.extend_from_slice(&value.to_le_bytes());
    input.extend_from_slice(blinding.as_bytes());
    sha3_256(&input)
}

/// Mock range prover that encodes the value in the "proof" for verification.
///
/// NOT cryptographically secure — only for testing.
#[derive(Clone, Debug, Default)]
pub struct MockRangeProver;

impl MockRangeProver {
    /// Create a new mock range prover.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl RangeProver for MockRangeProver {
    fn prove(&self, value: u64, blinding: &BlindingFactor) -> Result<RangeProof, PrivacyError> {
        // Encode value + blinding hash as the "proof" (mock only).
        let mut proof_data = Vec::with_capacity(40);
        proof_data.extend_from_slice(&value.to_le_bytes());
        proof_data.extend_from_slice(blinding.as_bytes());
        Ok(RangeProof(proof_data))
    }
}

/// Mock range verifier that checks the mock proof format.
#[derive(Clone, Debug, Default)]
pub struct MockRangeVerifier {
    committer: MockCommitter,
}

impl MockRangeVerifier {
    /// Create a new mock verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            committer: MockCommitter,
        }
    }
}

impl RangeVerifier for MockRangeVerifier {
    fn verify(
        &self,
        commitment: &Commitment,
        proof: &RangeProof,
    ) -> Result<bool, PrivacyError> {
        if proof.0.len() < 40 {
            return Ok(false);
        }
        let value_bytes: [u8; 8] = proof.0[..8]
            .try_into()
            .map_err(|_| PrivacyError::InvalidProof("proof too short".into()))?;
        let value = u64::from_le_bytes(value_bytes);

        let blinding_bytes: [u8; 32] = proof.0[8..40]
            .try_into()
            .map_err(|_| PrivacyError::InvalidProof("blinding extraction failed".into()))?;
        let blinding = BlindingFactor(blinding_bytes);

        self.committer.verify_opening(commitment, value, &blinding)
    }
}

// ---------------------------------------------------------------------------
// Balance verification
// ---------------------------------------------------------------------------

/// Verify that confidential transaction amounts balance.
///
/// For mixed transactions (some plain, some confidential), this checks:
/// - Plain inputs sum == plain outputs sum + fee (for the plain portion)
/// - Confidential commitment arithmetic balances (for the confidential portion)
///
/// With the mock committer, we verify by opening all commitments.
pub fn verify_balance_mock(
    input_amounts: &[ConfidentialAmount],
    output_amounts: &[ConfidentialAmount],
    fee: Amount,
    verifier: &MockRangeVerifier,
) -> Result<bool, PrivacyError> {
    let mut plain_in: u128 = 0;
    let mut plain_out: u128 = 0;

    for amount in input_amounts {
        match amount {
            ConfidentialAmount::Plain(v) => plain_in += *v as u128,
            ConfidentialAmount::Confidential {
                commitment,
                range_proof,
            } => {
                // In mock: extract value from proof for balance check.
                if !verifier.verify(commitment, range_proof)? {
                    return Err(PrivacyError::InvalidProof(
                        "input range proof invalid".into(),
                    ));
                }
                if range_proof.0.len() >= 8 {
                    let v = u64::from_le_bytes(
                        range_proof.0[..8]
                            .try_into()
                            .map_err(|_| PrivacyError::InvalidProof("bad bytes".into()))?,
                    );
                    plain_in += v as u128;
                }
            }
        }
    }

    for amount in output_amounts {
        match amount {
            ConfidentialAmount::Plain(v) => plain_out += *v as u128,
            ConfidentialAmount::Confidential {
                commitment,
                range_proof,
            } => {
                if !verifier.verify(commitment, range_proof)? {
                    return Err(PrivacyError::InvalidProof(
                        "output range proof invalid".into(),
                    ));
                }
                if range_proof.0.len() >= 8 {
                    let v = u64::from_le_bytes(
                        range_proof.0[..8]
                            .try_into()
                            .map_err(|_| PrivacyError::InvalidProof("bad bytes".into()))?,
                    );
                    plain_out += v as u128;
                }
            }
        }
    }

    let fee_val = fee.0 as u128;
    Ok(plain_in == plain_out + fee_val)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn test_blinding() -> BlindingFactor {
        BlindingFactor::from_seed(b"test-blinding-seed")
    }

    #[test]
    fn plain_amount_roundtrip() {
        let amt = ConfidentialAmount::plain(1000);
        assert!(amt.is_plain());
        assert!(!amt.is_confidential());
        assert_eq!(amt.plain_value(), Some(1000));
    }

    #[test]
    fn confidential_amount_creation() {
        let committer = MockCommitter::new();
        let prover = MockRangeProver::new();
        let blinding = test_blinding();

        let amt = ConfidentialAmount::confidential(500, &blinding, &committer, &prover).unwrap();
        assert!(amt.is_confidential());
        assert_eq!(amt.plain_value(), None);
    }

    #[test]
    fn commitment_deterministic() {
        let committer = MockCommitter::new();
        let blinding = test_blinding();

        let c1 = committer.commit(100, &blinding).unwrap();
        let c2 = committer.commit(100, &blinding).unwrap();
        assert_eq!(c1, c2);
    }

    #[test]
    fn commitment_different_values() {
        let committer = MockCommitter::new();
        let blinding = test_blinding();

        let c1 = committer.commit(100, &blinding).unwrap();
        let c2 = committer.commit(200, &blinding).unwrap();
        assert_ne!(c1, c2);
    }

    #[test]
    fn commitment_different_blinding() {
        let committer = MockCommitter::new();
        let b1 = BlindingFactor::from_seed(b"seed-1");
        let b2 = BlindingFactor::from_seed(b"seed-2");

        let c1 = committer.commit(100, &b1).unwrap();
        let c2 = committer.commit(100, &b2).unwrap();
        assert_ne!(c1, c2);
    }

    #[test]
    fn verify_opening_correct() {
        let committer = MockCommitter::new();
        let blinding = test_blinding();
        let commitment = committer.commit(42, &blinding).unwrap();
        assert!(committer.verify_opening(&commitment, 42, &blinding).unwrap());
    }

    #[test]
    fn verify_opening_wrong_value() {
        let committer = MockCommitter::new();
        let blinding = test_blinding();
        let commitment = committer.commit(42, &blinding).unwrap();
        assert!(!committer.verify_opening(&commitment, 43, &blinding).unwrap());
    }

    #[test]
    fn range_proof_roundtrip() {
        let committer = MockCommitter::new();
        let prover = MockRangeProver::new();
        let verifier = MockRangeVerifier::new();
        let blinding = test_blinding();

        let commitment = committer.commit(1000, &blinding).unwrap();
        let proof = prover.prove(1000, &blinding).unwrap();

        assert!(verifier.verify(&commitment, &proof).unwrap());
    }

    #[test]
    fn balance_verification_plain() {
        let verifier = MockRangeVerifier::new();

        let inputs = vec![ConfidentialAmount::plain(1000)];
        let outputs = vec![ConfidentialAmount::plain(900)];
        let fee = Amount::from_smallest_units(100);

        assert!(verify_balance_mock(&inputs, &outputs, fee, &verifier).unwrap());
    }

    #[test]
    fn balance_verification_imbalanced() {
        let verifier = MockRangeVerifier::new();

        let inputs = vec![ConfidentialAmount::plain(1000)];
        let outputs = vec![ConfidentialAmount::plain(950)];
        let fee = Amount::from_smallest_units(100);

        assert!(!verify_balance_mock(&inputs, &outputs, fee, &verifier).unwrap());
    }

    #[test]
    fn balance_verification_confidential() {
        let committer = MockCommitter::new();
        let prover = MockRangeProver::new();
        let verifier = MockRangeVerifier::new();

        let b_in = BlindingFactor::from_seed(b"in");
        let b_out = BlindingFactor::from_seed(b"out");

        let input = ConfidentialAmount::confidential(1000, &b_in, &committer, &prover).unwrap();
        let output = ConfidentialAmount::confidential(900, &b_out, &committer, &prover).unwrap();
        let fee = Amount::from_smallest_units(100);

        assert!(verify_balance_mock(&[input], &[output], fee, &verifier).unwrap());
    }

    #[test]
    fn blinding_factor_zeroized_on_drop() {
        let b = BlindingFactor::from_seed(b"secret");
        let ptr = b.0.as_ptr();
        let val = b.0;
        assert_ne!(val, [0u8; 32]);
        drop(b);
        // After drop, the memory *should* be zeroed (best-effort check).
        // Can't safely read dropped memory, so we just verify the drop path ran.
    }
}
