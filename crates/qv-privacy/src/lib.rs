//! # qv-privacy
//!
//! Privacy layer for QuantumVault L1: stealth addresses (default) and
//! opt-in confidential amounts.
//!
//! ## Modules
//!
//! - [`stealth`] — Kyber view key + Dilithium spend key stealth addresses.
//! - [`confidential`] — Pedersen commitments + Bulletproofs range proofs (opt-in).
//! - [`view_key`] — View key sharing for audit + selective disclosure proofs.
//!
//! ## Privacy model
//!
//! | Feature              | Default? | PQC?    | Notes                           |
//! |----------------------|----------|---------|---------------------------------|
//! | Stealth addresses    | Yes      | Yes     | Kyber KEM + Dilithium           |
//! | Confidential amounts | Opt-in   | **No**  | Curve25519 Bulletproofs         |
//! | View key audit       | Opt-in   | Yes     | Share Kyber view key            |
//! | Selective disclosure | Opt-in   | Yes     | Per-output proof                |
//!
//! ## Security warning
//!
//! Confidential amounts use classical Curve25519 Bulletproofs — **not**
//! post-quantum secure. Users opt in knowingly. STARK range proof migration
//! is planned for a future phase.

#![forbid(unsafe_code)]

pub mod confidential;
pub mod stealth;
pub mod view_key;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Privacy-layer error type.
#[derive(Debug, thiserror::Error)]
pub enum PrivacyError {
    /// Underlying cryptographic operation failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Stealth output is malformed or uses unsupported parameters.
    #[error("invalid stealth output: {0}")]
    InvalidStealthOutput(String),

    /// Range proof or commitment proof is invalid.
    #[error("invalid proof: {0}")]
    InvalidProof(String),

    /// Disclosure verification failed.
    #[error("disclosure verification failed: {0}")]
    DisclosureFailed(String),

    /// Amount balance check failed.
    #[error("balance mismatch")]
    BalanceMismatch,

    /// Wrong privacy mode for the requested operation.
    #[error("privacy mode mismatch: {0}")]
    ModeMismatch(String),
}

// ---------------------------------------------------------------------------
// Convenience re-exports
// ---------------------------------------------------------------------------

pub use confidential::{
    BlindingFactor, Commitment, Committer, ConfidentialAmount, MockCommitter, MockRangeProver,
    MockRangeVerifier, RangeProof, RangeProver, RangeVerifier, RANGE_BITS,
};
pub use stealth::{
    compute_onetime_pk_hash, compute_view_tag, create_stealth_output, recover_spend_key,
    scan_output, MockSpendKeyDeriver, ScanResult, SpendKeyDeriver, StealthAddress, StealthKeys,
    StealthOutput,
};
pub use view_key::{DisclosureProof, PrivacyMode, ViewKey};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let e = PrivacyError::Crypto("test".into());
        assert_eq!(e.to_string(), "crypto error: test");

        let e = PrivacyError::InvalidStealthOutput("bad tag".into());
        assert!(e.to_string().contains("bad tag"));

        let e = PrivacyError::BalanceMismatch;
        assert_eq!(e.to_string(), "balance mismatch");
    }

    #[test]
    fn re_exports_accessible() {
        // Verify the public surface is reachable from the crate root.
        let _: Option<StealthKeys> = None;
        let _: Option<StealthAddress> = None;
        let _: Option<StealthOutput> = None;
        let _: Option<ConfidentialAmount> = None;
        let _: Option<Commitment> = None;
        let _: Option<RangeProof> = None;
        let _: Option<BlindingFactor> = None;
        let _: Option<ViewKey> = None;
        let _: Option<DisclosureProof> = None;
        let _: Option<PrivacyMode> = None;
    }

    #[test]
    fn privacy_mode_is_default_stealth() {
        assert_eq!(PrivacyMode::default(), PrivacyMode::StealthOnly);
    }
}
