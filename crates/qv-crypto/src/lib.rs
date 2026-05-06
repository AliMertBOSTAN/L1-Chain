//! QuantumVault cryptographic primitives.
//!
//! This crate bundles every primitive the rest of the workspace needs:
//!
//! | Module         | Purpose                                                |
//! |----------------|--------------------------------------------------------|
//! | [`hash`]       | SHA3-256, BLAKE3, streaming hasher, double-hash        |
//! | [`secure_bytes`] | `zeroize`-on-drop byte buffer for secret material    |
//! | [`pqc_sign`]   | Dilithium / ML-DSA signatures (Levels 2, 3, 5)         |
//! | [`hybrid_kem`] | X25519 + Kyber hybrid KEM with transcript-bound KDF    |
//! | [`vrf`]        | Ouroboros-Praos verifiable random function (TODO)      |
//! | [`kes`]        | Forward-secure key-evolving signatures (TODO)          |
//! | [`threshold`]  | Threshold Kyber DKG for encrypted mempool (TODO)       |
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
/// the crate surface is stable if we swap a crypto backend (e.g. moving from
/// `pqcrypto-dilithium` to a future `pqcrypto-ml-dsa`).
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
pub use pqc_sign::{
    generate_keypair as generate_pqc_keypair, sign as sign_pqc, verify as verify_pqc,
    DilithiumLevel, PqcKeyPair, PqcPublicKey, PqcSecretKey, PqcSignature,
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
