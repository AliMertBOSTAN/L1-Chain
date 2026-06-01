//! Stealth addresses for QuantumVault.
//!
//! Provides unlinkable one-time addresses using **Kyber** (view key) and
//! **Dilithium** (spend key). The protocol:
//!
//! 1. **Recipient** publishes `StealthAddress { view_pk, spend_pk }`.
//! 2. **Sender** calls [`create_stealth_output`]: performs Kyber KEM encapsulation
//!    against the view key, derives a one-time locking script hash, and produces
//!    a [`StealthOutput`] containing the ephemeral KEM ciphertext and a view tag.
//! 3. **Recipient** calls [`scan_output`] with their Kyber view secret key to
//!    check if an output belongs to them (fast view-tag pre-filter).
//! 4. **Recipient** calls [`recover_spend_key`] to derive the one-time secret
//!    key that can spend the UTXO.
//!
//! # Security assumptions
//!
//! - Kyber (ML-KEM) provides IND-CCA2 post-quantum KEM.
//! - Dilithium provides EU-CMA post-quantum signatures.
//! - The shared secret is derived via SHA3-256 with a domain-separation tag.
//! - The one-time spend key is derived deterministically from the shared secret
//!   and the recipient's static spend public key (hash-based key derivation).
//!
//! # Limitations
//!
//! - Dilithium spend-key derivation from a seed is now fully supported via
//!   `qv_crypto::from_seed_pqc` (envanter C-04, kapatildi 2026-05-06). The
//!   [`SpendKeyDeriver`] trait + `MockSpendKeyDeriver` are kept for legacy
//!   tests; new code paths should call `qv_crypto::from_seed_pqc` directly.
//! - **Hybrid KEM (view key) seeded derivation is NOT yet supported** — see
//!   envanter ID C-05. View keys today are generated with OS entropy.
//! - The view tag is a single byte (1/256 false-positive rate).

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use qv_crypto::{
    decapsulate_hybrid, encapsulate_hybrid, sha3_256, DilithiumLevel, HashDigest, HybridCiphertext,
    HybridKeyPair, HybridPublicKey, KyberLevel, PqcKeyPair, PqcPublicKey, PqcSecretKey,
    SharedSecret,
};

use crate::PrivacyError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Domain separator for one-time address derivation.
const STEALTH_KDF_TAG: &[u8] = b"QuantumVault-Stealth-v1";

/// Domain separator for view tag derivation.
const VIEW_TAG_DOMAIN: &[u8] = b"QuantumVault-ViewTag-v1";

/// Domain separator for one-time spend key derivation.
const SPEND_KEY_DOMAIN: &[u8] = b"QuantumVault-SpendKey-v1";

// ---------------------------------------------------------------------------
// Stealth address (public, publishable)
// ---------------------------------------------------------------------------

/// A stealth address that a recipient publishes so senders can create
/// unlinkable one-time outputs.
#[derive(Clone, Debug)]
pub struct StealthAddress {
    /// Kyber hybrid view public key (used for KEM encapsulation).
    pub view_pk: HybridPublicKey,
    /// Dilithium spend public key.
    pub spend_pk: PqcPublicKey,
}

/// Full stealth key material held by the recipient.
#[derive(Clone, Debug)]
pub struct StealthKeys {
    /// Kyber hybrid view keypair (secret needed to scan outputs).
    pub view_kp: HybridKeyPair,
    /// Dilithium spend keypair (secret needed to spend UTXOs).
    pub spend_kp: PqcKeyPair,
}

impl StealthKeys {
    /// Generate a fresh stealth keypair.
    ///
    /// Uses the specified Kyber level for the view key and Dilithium level
    /// for the spend key.
    pub fn generate(
        kyber_level: KyberLevel,
        dilithium_level: DilithiumLevel,
    ) -> Result<Self, PrivacyError> {
        let view_kp = qv_crypto::hybrid_kem::generate_keypair(kyber_level)
            .map_err(|e| PrivacyError::Crypto(e.to_string()))?;
        let spend_kp = qv_crypto::pqc_sign::generate_keypair(dilithium_level)
            .map_err(|e| PrivacyError::Crypto(e.to_string()))?;
        Ok(Self { view_kp, spend_kp })
    }

    /// Extract the publishable stealth address.
    #[must_use]
    pub fn address(&self) -> StealthAddress {
        StealthAddress {
            view_pk: self.view_kp.public.clone(),
            spend_pk: self.spend_kp.public.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Stealth output (produced by sender)
// ---------------------------------------------------------------------------

/// The stealth payload that the sender attaches to a transaction output.
///
/// This contains enough information for the recipient to detect and claim
/// the output, but reveals nothing to third parties.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StealthOutput {
    /// KEM ciphertext (Kyber hybrid). Recipient decapsulates with view key.
    pub kem_ciphertext: Vec<u8>,
    /// Kyber level used for this ciphertext.
    pub kyber_level: u8,
    /// One-byte view tag for fast scanning (first byte of `H(shared_secret || VIEW_TAG_DOMAIN)`).
    pub view_tag: u8,
    /// One-time locking script hash: `H(shared_secret || spend_pk || STEALTH_KDF_TAG)`.
    /// The recipient must reproduce this to verify ownership.
    pub onetime_pk_hash: HashDigest,
}

/// Result of a successful output scan. Tells the recipient they own the
/// output and provides the derived shared secret for spend-key recovery.
#[derive(Clone, Debug)]
pub struct ScanResult {
    /// The shared secret derived from KEM decapsulation.
    pub shared_secret: SharedSecret,
    /// The one-time public key hash that was matched.
    pub onetime_pk_hash: HashDigest,
}

// ---------------------------------------------------------------------------
// Spend-key derivation trait + mock
// ---------------------------------------------------------------------------

/// Trait for deriving a one-time Dilithium spend keypair from a shared secret
/// and the static spend key.
///
/// Real Dilithium doesn't support deterministic key-from-seed in the
/// `pqcrypto-dilithium` crate yet. This trait allows mock testing now and
/// real implementation later.
pub trait SpendKeyDeriver: Send + Sync {
    /// Derive a one-time spend keypair from the shared secret and the
    /// recipient's static spend secret key.
    fn derive_onetime_keypair(
        &self,
        shared_secret: &SharedSecret,
        spend_sk: &PqcSecretKey,
    ) -> Result<PqcKeyPair, PrivacyError>;
}

/// Mock spend-key deriver for testing.
///
/// Derives a "one-time" keypair by simply generating a fresh keypair seeded
/// from the hash of (shared_secret || spend_sk). Since pqcrypto-dilithium
/// doesn't expose deterministic keygen, the mock just generates a random
/// keypair and records the derivation input hash for verification purposes.
#[derive(Clone, Debug)]
pub struct MockSpendKeyDeriver {
    level: DilithiumLevel,
}

impl MockSpendKeyDeriver {
    /// Create a new mock deriver at the given Dilithium level.
    #[must_use]
    pub fn new(level: DilithiumLevel) -> Self {
        Self { level }
    }
}

impl SpendKeyDeriver for MockSpendKeyDeriver {
    fn derive_onetime_keypair(
        &self,
        _shared_secret: &SharedSecret,
        _spend_sk: &PqcSecretKey,
    ) -> Result<PqcKeyPair, PrivacyError> {
        // In real implementation: deterministic keygen from
        //   seed = H(SPEND_KEY_DOMAIN || shared_secret || spend_sk)
        // For now: just generate a fresh keypair.
        qv_crypto::pqc_sign::generate_keypair(self.level)
            .map_err(|e| PrivacyError::Crypto(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Core protocol operations
// ---------------------------------------------------------------------------

/// Compute the one-byte view tag from a shared secret.
///
/// `view_tag = SHA3-256(VIEW_TAG_DOMAIN || shared_secret)[0]`
#[must_use]
pub fn compute_view_tag(shared_secret: &SharedSecret) -> u8 {
    let mut input = Vec::with_capacity(VIEW_TAG_DOMAIN.len() + 32);
    input.extend_from_slice(VIEW_TAG_DOMAIN);
    input.extend_from_slice(shared_secret.as_bytes());
    sha3_256(&input)[0]
}

/// Compute the one-time locking-script hash.
///
/// `onetime_pk_hash = SHA3-256(STEALTH_KDF_TAG || shared_secret || spend_pk)`
#[must_use]
pub fn compute_onetime_pk_hash(
    shared_secret: &SharedSecret,
    spend_pk: &PqcPublicKey,
) -> HashDigest {
    let mut input = Vec::with_capacity(STEALTH_KDF_TAG.len() + 32 + spend_pk.as_bytes().len());
    input.extend_from_slice(STEALTH_KDF_TAG);
    input.extend_from_slice(shared_secret.as_bytes());
    input.extend_from_slice(spend_pk.as_bytes());
    sha3_256(&input)
}

/// Compute the one-time spend-key derivation seed.
///
/// `seed = SHA3-256(SPEND_KEY_DOMAIN || shared_secret || spend_sk)`
#[must_use]
pub fn compute_spend_derivation_seed(
    shared_secret: &SharedSecret,
    spend_sk: &PqcSecretKey,
) -> HashDigest {
    let mut input =
        Vec::with_capacity(SPEND_KEY_DOMAIN.len() + 32 + spend_sk.expose_secret().len());
    input.extend_from_slice(SPEND_KEY_DOMAIN);
    input.extend_from_slice(shared_secret.as_bytes());
    input.extend_from_slice(spend_sk.expose_secret());
    sha3_256(&input)
}

/// **Sender** creates a stealth output for the given recipient.
///
/// Performs Kyber KEM encapsulation against the recipient's view public key,
/// derives a shared secret, computes the view tag and one-time locking hash.
pub fn create_stealth_output(
    recipient: &StealthAddress,
) -> Result<(StealthOutput, SharedSecret), PrivacyError> {
    // 1. KEM encapsulate against recipient's view key.
    let (ciphertext, shared_secret) =
        encapsulate_hybrid(&recipient.view_pk).map_err(|e| PrivacyError::Crypto(e.to_string()))?;

    // 2. Derive view tag.
    let view_tag = compute_view_tag(&shared_secret);

    // 3. Derive one-time pk hash.
    let onetime_pk_hash = compute_onetime_pk_hash(&shared_secret, &recipient.spend_pk);

    // 4. Encode Kyber level as u8.
    let kyber_level = match recipient.view_pk.level {
        KyberLevel::Level1 => 1,
        KyberLevel::Level3 => 3,
        KyberLevel::Level5 => 5,
    };

    Ok((
        StealthOutput {
            kem_ciphertext: ciphertext.bytes,
            kyber_level,
            view_tag,
            onetime_pk_hash,
        },
        shared_secret,
    ))
}

/// **Recipient** scans an output to check if it belongs to them.
///
/// Decapsulates the KEM ciphertext with the view key and checks the view
/// tag (a 1/256 pre-filter). On a tag match it returns `Some(ScanResult)`
/// carrying the **recomputed** `onetime_pk_hash`.
///
/// The caller MUST then verify that the output's locking script commits to
/// that hash (`qv_script::stealth_p2pkh`) — the view tag alone is not proof
/// of ownership. The commitment is not carried in the on-chain `StealthInfo`
/// (ADR-011), so it cannot be checked here.
pub fn scan_output(
    stealth_keys: &StealthKeys,
    output: &StealthOutput,
) -> Result<Option<ScanResult>, PrivacyError> {
    scan_output_view(&stealth_keys.view_kp, &stealth_keys.spend_kp.public, output)
}

/// Same as [`scan_output`] but takes the view keypair and spend **public**
/// key separately, so callers that don't hold the spend secret (e.g. an RPC
/// server scanning the UTXO set on the recipient's behalf) can still detect
/// outputs without ever touching spending material.
///
/// The spend secret is **not** required to detect ownership — only to spend
/// the discovered UTXO afterwards. This separation lets a wallet publish its
/// view key + spend public key to its own node for balance/scan RPCs while
/// keeping the spend secret entirely on the client.
pub fn scan_output_view(
    view_kp: &HybridKeyPair,
    spend_pk: &PqcPublicKey,
    output: &StealthOutput,
) -> Result<Option<ScanResult>, PrivacyError> {
    // 1. Reconstruct the KEM ciphertext.
    let kyber_level = match output.kyber_level {
        1 => KyberLevel::Level1,
        3 => KyberLevel::Level3,
        5 => KyberLevel::Level5,
        other => {
            return Err(PrivacyError::InvalidStealthOutput(format!(
                "unknown Kyber level: {other}"
            )));
        }
    };

    let ciphertext = HybridCiphertext {
        bytes: output.kem_ciphertext.clone(),
        level: kyber_level,
    };

    // 2. Decapsulate to recover shared secret.
    let shared_secret = decapsulate_hybrid(view_kp, &ciphertext)
        .map_err(|e| PrivacyError::Crypto(e.to_string()))?;

    // 3. Check view tag (fast filter).
    let expected_tag = compute_view_tag(&shared_secret);
    if expected_tag != output.view_tag {
        // Not ours (Kyber IND-CCA2: wrong recipient → different shared secret → different tag).
        return Ok(None);
    }

    // 4. Recompute the one-time commitment from *our* static spend key.
    //    `onetime_pk_hash` is NOT carried in the on-chain `StealthInfo` — it
    //    is the output's locking-script commitment (`stealth_p2pkh`, ADR-011).
    //    The caller must verify the returned hash against that locking
    //    script; the view tag alone is only a 1/256 pre-filter.
    let onetime_pk_hash = compute_onetime_pk_hash(&shared_secret, spend_pk);

    Ok(Some(ScanResult {
        shared_secret,
        onetime_pk_hash,
    }))
}

/// **Recipient** recovers the one-time spend keypair for a confirmed output.
///
/// Uses the [`SpendKeyDeriver`] trait for the actual Dilithium key derivation.
pub fn recover_spend_key(
    stealth_keys: &StealthKeys,
    scan_result: &ScanResult,
    deriver: &dyn SpendKeyDeriver,
) -> Result<PqcKeyPair, PrivacyError> {
    deriver.derive_onetime_keypair(&scan_result.shared_secret, &stealth_keys.spend_kp.secret)
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

    fn test_keys() -> StealthKeys {
        StealthKeys::generate(KyberLevel::default(), DilithiumLevel::default()).unwrap()
    }

    #[test]
    fn stealth_keys_generate() {
        let keys = test_keys();
        assert_eq!(keys.view_kp.public.level, KyberLevel::Level3);
        assert_eq!(keys.spend_kp.public.level(), DilithiumLevel::Level3);
    }

    #[test]
    fn create_and_scan_roundtrip() {
        let keys = test_keys();
        let addr = keys.address();

        let (output, _sender_ss) = create_stealth_output(&addr).unwrap();

        let scan = scan_output(&keys, &output).unwrap();
        assert!(scan.is_some(), "recipient should detect own output");

        let result = scan.unwrap();
        assert_eq!(result.onetime_pk_hash, output.onetime_pk_hash);
    }

    #[test]
    fn wrong_recipient_does_not_match() {
        let alice = test_keys();
        let bob = test_keys();

        let (output, _) = create_stealth_output(&alice.address()).unwrap();

        // Bob scans Alice's output — Kyber IND-CCA2 means different shared secret.
        let scan = scan_output(&bob, &output).unwrap();
        assert!(scan.is_none(), "wrong recipient should not detect output");
    }

    #[test]
    fn view_tag_deterministic() {
        let ss = SharedSecret([0x42; 32]);
        let tag1 = compute_view_tag(&ss);
        let tag2 = compute_view_tag(&ss);
        assert_eq!(tag1, tag2);
    }

    #[test]
    fn onetime_pk_hash_deterministic() {
        let ss = SharedSecret([0x42; 32]);
        let kp = qv_crypto::pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap();
        let h1 = compute_onetime_pk_hash(&ss, &kp.public);
        let h2 = compute_onetime_pk_hash(&ss, &kp.public);
        assert_eq!(h1, h2);
    }

    #[test]
    fn onetime_pk_hash_changes_with_different_secret() {
        let ss1 = SharedSecret([0x42; 32]);
        let ss2 = SharedSecret([0x43; 32]);
        let kp = qv_crypto::pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap();
        let h1 = compute_onetime_pk_hash(&ss1, &kp.public);
        let h2 = compute_onetime_pk_hash(&ss2, &kp.public);
        assert_ne!(h1, h2);
    }

    #[test]
    fn recover_spend_key_with_mock() {
        let keys = test_keys();
        let addr = keys.address();

        let (output, _) = create_stealth_output(&addr).unwrap();
        let scan = scan_output(&keys, &output).unwrap().unwrap();

        let deriver = MockSpendKeyDeriver::new(DilithiumLevel::Level3);
        let onetime_kp = recover_spend_key(&keys, &scan, &deriver).unwrap();

        // The mock just generates a fresh keypair, so we can only verify it exists.
        assert_eq!(onetime_kp.public.level(), DilithiumLevel::Level3);
    }

    #[test]
    fn stealth_output_kyber_level_encoding() {
        let keys = test_keys();
        let (output, _) = create_stealth_output(&keys.address()).unwrap();
        assert_eq!(output.kyber_level, 3); // default Level3
    }
}
