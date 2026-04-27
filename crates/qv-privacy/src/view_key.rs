//! View key mechanism for selective disclosure.
//!
//! A recipient can share their **view key** (the Kyber hybrid secret key)
//! with a trusted auditor. The auditor can then:
//!
//! - Scan the chain for outputs belonging to the recipient.
//! - Decrypt stealth metadata to see amounts and senders.
//! - **Cannot** spend the outputs (requires the spend key).
//!
//! # Selective disclosure
//!
//! Rather than sharing the full view key, users can produce per-output
//! **disclosure proofs**: a bundle of (shared_secret, amount, blinding_factor)
//! that proves a specific output belongs to them and contains a specific value,
//! without revealing the view key itself.
//!
//! # Privacy levels
//!
//! | Level        | View key shared? | Disclosure proofs? | Auditor sees          |
//! |--------------|------------------|--------------------|------------------------|
//! | Full privacy | No               | No                 | Nothing               |
//! | Audit mode   | Yes              | No                 | All incoming outputs  |
//! | Selective    | No               | Per-output         | Only disclosed outputs|

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use qv_crypto::{sha3_256, HashDigest, HybridKeyPair, HybridPublicKey, SharedSecret};

use crate::confidential::{BlindingFactor, Commitment, Committer, MockCommitter};
use crate::stealth::{compute_onetime_pk_hash, compute_view_tag, StealthOutput};
use crate::PrivacyError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Domain separator for disclosure proof binding.
const DISCLOSURE_DOMAIN: &[u8] = b"QuantumVault-Disclosure-v1";

// ---------------------------------------------------------------------------
// View key (shared for audit)
// ---------------------------------------------------------------------------

/// An exportable view key that allows scanning the chain for incoming outputs.
///
/// This is essentially the Kyber hybrid secret key material. Sharing it
/// with an auditor lets them run [`scan_output`](crate::stealth::scan_output)
/// on the recipient's behalf.
///
/// **Warning**: sharing the view key reveals ALL incoming outputs forever.
/// For selective disclosure, use [`DisclosureProof`] instead.
#[derive(Clone, Debug)]
pub struct ViewKey {
    /// The Kyber hybrid public key (identifier / lookup).
    pub public: HybridPublicKey,
    /// The Kyber hybrid keypair (secret included for decapsulation).
    keypair: HybridKeyPair,
}

impl ViewKey {
    /// Create a view key from a hybrid keypair.
    #[must_use]
    pub fn new(keypair: HybridKeyPair) -> Self {
        Self {
            public: keypair.public.clone(),
            keypair,
        }
    }

    /// Get the underlying keypair for decapsulation.
    #[must_use]
    pub fn keypair(&self) -> &HybridKeyPair {
        &self.keypair
    }
}

// ---------------------------------------------------------------------------
// Disclosure proof (per-output selective disclosure)
// ---------------------------------------------------------------------------

/// A per-output disclosure proof: proves that a specific stealth output
/// belongs to the discloser and (optionally) reveals the amount.
///
/// An auditor who receives this can verify:
/// 1. The shared secret matches the stealth output (via view tag + pk hash).
/// 2. The commitment opens to the disclosed value (if amount is disclosed).
/// 3. The binding hash ties everything together.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisclosureProof {
    /// The shared secret that was derived from KEM decapsulation.
    /// This lets the auditor re-derive view tag and one-time pk hash.
    pub shared_secret_bytes: [u8; 32],
    /// Optionally disclosed plaintext amount.
    pub disclosed_amount: Option<u64>,
    /// Optionally disclosed blinding factor (for commitment opening).
    pub disclosed_blinding: Option<[u8; 32]>,
    /// Binding hash: `H(DISCLOSURE_DOMAIN || shared_secret || onetime_pk_hash || amount? || blinding?)`.
    pub binding_hash: HashDigest,
}

impl DisclosureProof {
    /// Create a disclosure proof for a stealth output.
    ///
    /// - `shared_secret`: from a successful `scan_output`.
    /// - `onetime_pk_hash`: from the [`StealthOutput`].
    /// - `amount`: optionally disclose the value.
    /// - `blinding`: optionally disclose the blinding factor.
    #[must_use]
    pub fn create(
        shared_secret: &SharedSecret,
        onetime_pk_hash: &HashDigest,
        amount: Option<u64>,
        blinding: Option<&BlindingFactor>,
    ) -> Self {
        let binding_hash =
            compute_binding_hash(shared_secret, onetime_pk_hash, amount, blinding);

        Self {
            shared_secret_bytes: *shared_secret.as_bytes(),
            disclosed_amount: amount,
            disclosed_blinding: blinding.map(|b| *b.as_bytes()),
            binding_hash,
        }
    }

    /// Verify the disclosure proof against a stealth output.
    ///
    /// Checks:
    /// 1. View tag matches.
    /// 2. One-time pk hash matches.
    /// 3. Binding hash is consistent.
    /// 4. If amount + blinding are disclosed, commitment opens correctly.
    pub fn verify(
        &self,
        output: &StealthOutput,
        spend_pk: &qv_crypto::PqcPublicKey,
        commitment: Option<&Commitment>,
        committer: &dyn Committer,
    ) -> Result<bool, PrivacyError> {
        let ss = SharedSecret(self.shared_secret_bytes);

        // 1. Check view tag.
        let expected_tag = compute_view_tag(&ss);
        if expected_tag != output.view_tag {
            return Ok(false);
        }

        // 2. Check one-time pk hash.
        let expected_hash = compute_onetime_pk_hash(&ss, spend_pk);
        if expected_hash != output.onetime_pk_hash {
            return Ok(false);
        }

        // 3. Verify binding hash.
        let blinding_ref = self.disclosed_blinding.as_ref().map(|b| BlindingFactor(*b));
        let expected_binding = compute_binding_hash(
            &ss,
            &output.onetime_pk_hash,
            self.disclosed_amount,
            blinding_ref.as_ref(),
        );
        if self.binding_hash != expected_binding {
            return Ok(false);
        }

        // 4. If amount + blinding + commitment are all present, verify opening.
        if let (Some(amt), Some(blinding_bytes), Some(comm)) =
            (self.disclosed_amount, &self.disclosed_blinding, commitment)
        {
            let blinding = BlindingFactor(*blinding_bytes);
            if !committer.verify_opening(comm, amt, &blinding)? {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Privacy mode configuration
// ---------------------------------------------------------------------------

/// Privacy level for a transaction or wallet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivacyMode {
    /// Default: stealth addresses only, amounts visible.
    StealthOnly,
    /// Full privacy: stealth addresses + confidential amounts.
    Full,
    /// Transparent: no stealth, no confidential (for exchange compliance).
    Transparent,
}

impl Default for PrivacyMode {
    fn default() -> Self {
        Self::StealthOnly
    }
}

impl PrivacyMode {
    /// Whether stealth addresses are active.
    #[must_use]
    pub fn stealth_enabled(self) -> bool {
        matches!(self, Self::StealthOnly | Self::Full)
    }

    /// Whether confidential amounts are active.
    #[must_use]
    pub fn confidential_enabled(self) -> bool {
        matches!(self, Self::Full)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_binding_hash(
    shared_secret: &SharedSecret,
    onetime_pk_hash: &HashDigest,
    amount: Option<u64>,
    blinding: Option<&BlindingFactor>,
) -> HashDigest {
    let mut input = Vec::with_capacity(128);
    input.extend_from_slice(DISCLOSURE_DOMAIN);
    input.extend_from_slice(shared_secret.as_bytes());
    input.extend_from_slice(onetime_pk_hash);
    if let Some(amt) = amount {
        input.extend_from_slice(&amt.to_le_bytes());
    }
    if let Some(b) = blinding {
        input.extend_from_slice(b.as_bytes());
    }
    sha3_256(&input)
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
    use crate::stealth::{create_stealth_output, scan_output, StealthKeys};
    use qv_crypto::{DilithiumLevel, KyberLevel};

    fn test_keys() -> StealthKeys {
        StealthKeys::generate(KyberLevel::default(), DilithiumLevel::default()).unwrap()
    }

    #[test]
    fn view_key_creation() {
        let keys = test_keys();
        let vk = ViewKey::new(keys.view_kp.clone());
        assert_eq!(vk.public.level, KyberLevel::Level3);
    }

    #[test]
    fn disclosure_proof_roundtrip() {
        let keys = test_keys();
        let addr = keys.address();
        let (output, _) = create_stealth_output(&addr).unwrap();
        let scan = scan_output(&keys, &output).unwrap().unwrap();

        // Create disclosure (amount only, no commitment).
        let proof = DisclosureProof::create(
            &scan.shared_secret,
            &output.onetime_pk_hash,
            Some(1000),
            None,
        );

        // Verify (no commitment verification since blinding is None).
        let committer = MockCommitter::new();
        let valid = proof
            .verify(&output, &keys.spend_kp.public, None, &committer)
            .unwrap();
        assert!(valid);
    }

    #[test]
    fn disclosure_proof_with_commitment() {
        let keys = test_keys();
        let addr = keys.address();
        let (output, _) = create_stealth_output(&addr).unwrap();
        let scan = scan_output(&keys, &output).unwrap().unwrap();

        let committer = MockCommitter::new();
        let blinding = BlindingFactor::from_seed(b"test-blinding");
        let commitment = committer.commit(500, &blinding).unwrap();

        let proof = DisclosureProof::create(
            &scan.shared_secret,
            &output.onetime_pk_hash,
            Some(500),
            Some(&blinding),
        );

        let valid = proof
            .verify(&output, &keys.spend_kp.public, Some(&commitment), &committer)
            .unwrap();
        assert!(valid);
    }

    #[test]
    fn disclosure_proof_wrong_amount_fails() {
        let keys = test_keys();
        let addr = keys.address();
        let (output, _) = create_stealth_output(&addr).unwrap();
        let scan = scan_output(&keys, &output).unwrap().unwrap();

        let committer = MockCommitter::new();
        let blinding = BlindingFactor::from_seed(b"test-blinding");
        let commitment = committer.commit(500, &blinding).unwrap();

        // Disclose wrong amount.
        let proof = DisclosureProof::create(
            &scan.shared_secret,
            &output.onetime_pk_hash,
            Some(999), // wrong amount
            Some(&blinding),
        );

        let valid = proof
            .verify(&output, &keys.spend_kp.public, Some(&commitment), &committer)
            .unwrap();
        assert!(!valid, "wrong amount should fail commitment verification");
    }

    #[test]
    fn disclosure_proof_wrong_secret_fails() {
        let keys = test_keys();
        let addr = keys.address();
        let (output, _) = create_stealth_output(&addr).unwrap();

        // Use a wrong shared secret.
        let fake_ss = SharedSecret([0xFF; 32]);
        let proof = DisclosureProof::create(
            &fake_ss,
            &output.onetime_pk_hash,
            Some(1000),
            None,
        );

        let committer = MockCommitter::new();
        let valid = proof
            .verify(&output, &keys.spend_kp.public, None, &committer)
            .unwrap();
        assert!(!valid, "wrong shared secret should fail");
    }

    #[test]
    fn privacy_mode_defaults() {
        assert_eq!(PrivacyMode::default(), PrivacyMode::StealthOnly);
        assert!(PrivacyMode::StealthOnly.stealth_enabled());
        assert!(!PrivacyMode::StealthOnly.confidential_enabled());
        assert!(PrivacyMode::Full.stealth_enabled());
        assert!(PrivacyMode::Full.confidential_enabled());
        assert!(!PrivacyMode::Transparent.stealth_enabled());
        assert!(!PrivacyMode::Transparent.confidential_enabled());
    }

    #[test]
    fn binding_hash_deterministic() {
        let ss = SharedSecret([0x42; 32]);
        let pk_hash: HashDigest = [0xAA; 32];
        let h1 = compute_binding_hash(&ss, &pk_hash, Some(100), None);
        let h2 = compute_binding_hash(&ss, &pk_hash, Some(100), None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn binding_hash_changes_with_amount() {
        let ss = SharedSecret([0x42; 32]);
        let pk_hash: HashDigest = [0xAA; 32];
        let h1 = compute_binding_hash(&ss, &pk_hash, Some(100), None);
        let h2 = compute_binding_hash(&ss, &pk_hash, Some(200), None);
        assert_ne!(h1, h2);
    }
}
