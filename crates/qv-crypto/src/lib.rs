//! QuantumVault cryptographic primitives.
//!
//! This crate bundles every primitive the rest of the workspace needs:
//!
//! | Module         | Purpose                                                |
//! |----------------|--------------------------------------------------------|
//! | [`hash`]       | SHA3-256, BLAKE3, streaming hasher, double-hash        |
//! | [`secure_bytes`] | `zeroize`-on-drop byte buffer for secret material    |
//! | [`pqc_sign`]   | Dilithium / ML-DSA signatures (Levels 2, 3, 5) + FIPS 204 seeded keygen |
//! | [`hybrid_kem`] | X25519 + Kyber hybrid KEM with transcript-bound KDF    |
//! | [`vrf`]        | Ouroboros-Praos verifiable random function (ADR-004 — impl pending) |
//! | [`kes`]        | Forward-secure key-evolving signatures (ADR-005 — impl pending) |
//! | [`threshold`]  | Threshold cryptography (Shamir + Feldman VSS + Pedersen DKG + ElGamal-style decryption) |
//!
//! # Error model
//!
//! All fallible operations return [`Result<T>`] — a crate-local alias for
//! `core::result::Result<T, CryptoError>`. Consumers that aggregate many
//! error types should convert `CryptoError` via their own `From` impl.

#![forbid(unsafe_code)]
// `missing_docs` workspace-managed; see Cargo.toml. Re-tightened in Faz 9.

pub mod hash;
pub mod hybrid_kem;
pub mod kes;
pub mod pqc_sign;
pub mod secure_bytes;
pub mod threshold;
pub mod vrf;

/// Crate-level error type.
///
/// Individual modules avoid leaking the exact upstream library error so that
/// the crate surface is stable if we swap a crypto backend. Per ADR-006 the
/// signature backend is `ml-dsa = "0.0.4"` (FIPS 204 final); previously
/// `pqcrypto-dilithium` (NIST round-3, swapped out 2026-05-07).
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// Byte slice had the wrong length for a key, signature, ciphertext, etc.
    #[error("invalid input size: expected {expected}, got {actual}")]
    InvalidSize {
        /// Expected size in bytes.
        expected: usize,
        /// Observed size in bytes.
        actual: usize,
    },

    /// Hash computation produced no output (should never happen for valid input).
    #[error("hash computation failed")]
    HashFailed,

    /// Key generation failed (entropy source / upstream library).
    #[error("key generation failed")]
    KeyGenerationFailed,

    /// Digital signature creation failed.
    #[error("signature creation failed")]
    SignFailed,

    /// Signature verification returned "invalid" (not an error, but a distinct
    /// outcome that callers should treat as the signature being bad).
    #[error("signature verification rejected")]
    SignatureRejected,

    /// KEM encapsulation failed.
    #[error("kem encapsulation failed")]
    EncapsulationFailed,

    /// KEM decapsulation failed.
    #[error("kem decapsulation failed")]
    DecapsulationFailed,

    /// Ciphertext was malformed (length or structure).
    #[error("ciphertext malformed")]
    MalformedCiphertext,

    /// Public key was malformed.
    #[error("public key malformed")]
    MalformedPublicKey,

    /// Catch-all for error paths we have not yet modelled precisely.
    #[error("crypto error: {0}")]
    Other(String),
}

/// Crate-level result alias.
///
/// # Examples
///
/// ```rust,no_run
/// # use qv_crypto::{Result, generate_pqc_keypair, sign_pqc, verify_pqc, DilithiumLevel};
/// fn sign_and_verify() -> Result<()> {
///     let keypair = generate_pqc_keypair(DilithiumLevel::Level3)?;
///     let message = b"hello";
///     let sig = sign_pqc(&keypair.secret, message)?;
///     let valid = verify_pqc(&keypair.public, message, &sig)?;
///     assert!(valid);
///     Ok(())
/// }
/// ```
pub type Result<T> = core::result::Result<T, CryptoError>;

// ----------------------------------------------------------------------------
// Re-exports — a flat convenience surface for downstream crates.
// ----------------------------------------------------------------------------

/// Hash algorithm implementations (SHA3-256, BLAKE3, double-hash); see [`hash`] module.
pub use hash::{
    blake3, double_blake3, double_hash, double_sha3_256, hash, sha3_256, HashAlgorithm, HashDigest,
    Hasher,
};

/// Secure byte buffer that zeroes memory on drop; see [`secure_bytes`] module.
pub use secure_bytes::SecureBytes;

/// Post-quantum signature (Dilithium/ML-DSA) generation and verification;
/// see [`pqc_sign`] module. Functions are aliased with `_pqc` suffix.
///
/// **`from_seed_pqc` is currently a stub returning `Err` (envanter C-04
/// REOPENED 2026-05-07).** The previous `fips204` integration was reverted
/// because `fips204` 0.4 does not expose seeded keygen. Swap to `ml-dsa`
/// 0.0.4 is tracked under envanter C-06.
pub use pqc_sign::{
    from_seed as from_seed_pqc, generate_keypair as generate_pqc_keypair, sign as sign_pqc,
    verify as verify_pqc, DilithiumLevel, PqcKeyPair, PqcPublicKey, PqcSecretKey, PqcSignature,
};

/// Hybrid X25519 + Kyber key encapsulation and shared-secret derivation;
/// see [`hybrid_kem`] module. Functions are aliased with `_hybrid` suffix.
pub use hybrid_kem::{
    decapsulate as decapsulate_hybrid, encapsulate as encapsulate_hybrid,
    generate_keypair as generate_hybrid_keypair, HybridCiphertext, HybridKeyPair, HybridPublicKey,
    KyberLevel, SharedSecret,
};

/// Threshold cryptography for encrypted mempool (DKG, Shamir, decryption shares);
/// see [`threshold`] module.
pub use threshold::{
    reconstruct_secret, run_pedersen_dkg, split_secret, DecryptionShare, DkgCommitment,
    DkgParticipant, DkgResult, DkgShare, DkgThresholdDecryptor, FeldmanVssParticipant,
    MockDkgParticipant, MockThresholdDecryptor, ShamirShare, ThresholdDecryptor,
    ThresholdPublicKey,
};

/// Verifiable Random Function for Ouroboros Praos slot leader election;
/// see [`vrf`] module. Per ADR-004 we ship Ristretto255-VRF for MVP/v1
/// and a hybrid Ristretto + lattice-VRF is planned for v2 (trait swap point
/// is `qv_consensus::leader_schedule::VrfEvaluator`).
pub use vrf::{
    evaluate as vrf_evaluate, verify as vrf_verify, VrfKeyPair, VrfOutput, VrfProof,
    VrfPublicKey, VrfSecretKey, VRF_DOMAIN_TAG, VRF_OUTPUT_BYTES, VRF_PUBLIC_KEY_BYTES,
};

/// Forward-secure key-evolving signatures (Sum-KES on Dilithium); see
/// [`kes`] module. Per ADR-005, the leaf primitive is Dilithium Level 3 and
/// the binary tree depth is 11 (`N = 2048` periods). Trait swap point for
/// consensus is `qv_consensus::block_validator::KesVerifier`.
pub use kes::{
    current_period as kes_current_period, evolve as kes_evolve, generate as kes_generate,
    sign as kes_sign, verify as kes_verify, KesPublicKey, KesSecretKey, KesSignature,
    KES_LEAF_LEVEL, KES_TOTAL_PERIODS, KES_TREE_DEPTH,
};
