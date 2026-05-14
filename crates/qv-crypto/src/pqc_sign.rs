//! Post-quantum digital signatures — FIPS 204 ML-DSA.
//!
//! Backed by the RustCrypto [`ml-dsa`] crate (per ADR-006). The previous
//! [`pqcrypto-dilithium`] backend was removed 2026-05-07 because its
//! NIST round-3 wire format is incompatible with FIPS 204 (different
//! secret-key and signature sizes).
//!
//! Dispatches at runtime on [`DilithiumLevel`] because each parameter set
//! is exposed as a distinct concrete type (`MlDsa44`, `MlDsa65`, `MlDsa87`)
//! by `ml-dsa`. Internally we always carry raw byte vectors and delegate to
//! the typed API only inside the per-level match arms.
//!
//! # Security levels (NIST FIPS 204, ML-DSA)
//!
//! | Variant | Classical sec. | PQ sec. | pk bytes | sk bytes | sig bytes |
//! |---------|---------------:|--------:|---------:|---------:|----------:|
//! | Level 2 | 128            | 128     | 1312     | 2560     | 2420      |
//! | Level 3 | 192            | 192     | 1952     | 4032     | 3309      |
//! | Level 5 | 256            | 256     | 2592     | 4896     | 4627      |
//!
//! Sizes come straight from `ml-dsa`'s typenum-based size constants and are
//! returned dynamically via [`DilithiumLevel::public_key_bytes`] etc.
//!
//! [`ml-dsa`]: https://docs.rs/ml-dsa/0.0.4
//! [`pqcrypto-dilithium`]: https://docs.rs/pqcrypto-dilithium

use ml_dsa::signature::{Signer, Verifier};
use ml_dsa::{
    EncodedSignature, EncodedSigningKey, EncodedVerifyingKey, KeyGen, MlDsa44, MlDsa65, MlDsa87,
    Signature as MlDsaSignature, SigningKey as MlDsaSigningKey, VerifyingKey as MlDsaVerifyingKey,
    B32,
};
use rand_core::OsRng;

use crate::{CryptoError, Result, SecureBytes};

/// Dilithium / ML-DSA security level (FIPS 204 parameter set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DilithiumLevel {
    /// ML-DSA-44 — NIST category 2 (≈128-bit post-quantum).
    Level2,
    /// ML-DSA-65 — NIST category 3 (≈192-bit post-quantum). **Default.**
    Level3,
    /// ML-DSA-87 — NIST category 5 (≈256-bit post-quantum).
    Level5,
}

impl Default for DilithiumLevel {
    fn default() -> Self {
        Self::Level3
    }
}

impl DilithiumLevel {
    /// Public (verifying) key length in bytes.
    #[must_use]
    pub fn public_key_bytes(self) -> usize {
        match self {
            Self::Level2 => EncodedVerifyingKey::<MlDsa44>::default().as_slice().len(),
            Self::Level3 => EncodedVerifyingKey::<MlDsa65>::default().as_slice().len(),
            Self::Level5 => EncodedVerifyingKey::<MlDsa87>::default().as_slice().len(),
        }
    }

    /// Secret (signing) key length in bytes.
    #[must_use]
    pub fn secret_key_bytes(self) -> usize {
        match self {
            Self::Level2 => EncodedSigningKey::<MlDsa44>::default().as_slice().len(),
            Self::Level3 => EncodedSigningKey::<MlDsa65>::default().as_slice().len(),
            Self::Level5 => EncodedSigningKey::<MlDsa87>::default().as_slice().len(),
        }
    }

    /// Detached signature length in bytes.
    #[must_use]
    pub fn signature_bytes(self) -> usize {
        match self {
            Self::Level2 => EncodedSignature::<MlDsa44>::default().as_slice().len(),
            Self::Level3 => EncodedSignature::<MlDsa65>::default().as_slice().len(),
            Self::Level5 => EncodedSignature::<MlDsa87>::default().as_slice().len(),
        }
    }
}

// ============================================================================
// Typed wrappers — keep byte buffers behind opaque newtypes
// ============================================================================

/// ML-DSA public (verifying) key.
#[derive(Clone, PartialEq, Eq)]
pub struct PqcPublicKey {
    level: DilithiumLevel,
    bytes: Vec<u8>,
}

impl PqcPublicKey {
    /// Construct from raw bytes, validating length against the parameter-set spec.
    pub fn from_bytes(level: DilithiumLevel, bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        if bytes.len() != level.public_key_bytes() {
            return Err(CryptoError::InvalidSize {
                expected: level.public_key_bytes(),
                actual: bytes.len(),
            });
        }
        Ok(Self { level, bytes })
    }

    /// Raw bytes of the public key.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The parameter set this key belongs to.
    #[must_use]
    pub fn level(&self) -> DilithiumLevel {
        self.level
    }
}

impl core::fmt::Debug for PqcPublicKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PqcPublicKey({:?}, {} bytes)",
            self.level,
            self.bytes.len()
        )
    }
}

/// ML-DSA secret (signing) key. Stored inside a [`SecureBytes`] so it zeroes on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct PqcSecretKey {
    level: DilithiumLevel,
    bytes: SecureBytes,
}

impl PqcSecretKey {
    /// Construct from raw bytes, validating length.
    pub fn from_bytes(level: DilithiumLevel, bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        if bytes.len() != level.secret_key_bytes() {
            return Err(CryptoError::InvalidSize {
                expected: level.secret_key_bytes(),
                actual: bytes.len(),
            });
        }
        Ok(Self {
            level,
            bytes: SecureBytes::from_vec(bytes),
        })
    }

    /// Expose the raw secret bytes.
    ///
    /// Callers **must** treat the return value as sensitive and avoid copying
    /// it into non-zeroizing storage.
    #[must_use]
    pub fn expose_secret(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// The parameter set this key belongs to.
    #[must_use]
    pub fn level(&self) -> DilithiumLevel {
        self.level
    }
}

impl core::fmt::Debug for PqcSecretKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Never print bytes.
        write!(
            f,
            "PqcSecretKey({:?}, {} bytes)",
            self.level,
            self.bytes.len()
        )
    }
}

/// Paired ML-DSA keys.
#[derive(Clone, Debug)]
pub struct PqcKeyPair {
    /// Public half.
    pub public: PqcPublicKey,
    /// Secret half.
    pub secret: PqcSecretKey,
}

impl PqcKeyPair {
    /// Generate a fresh keypair from OS entropy. See [`generate_keypair`].
    pub fn generate(level: DilithiumLevel) -> Result<Self> {
        generate_keypair(level)
    }

    /// Derive a deterministic keypair from a 32-byte seed via FIPS 204
    /// `ML-DSA.KeyGen_internal(ξ)`. See [`from_seed`].
    ///
    /// This is the canonical entry point for HD wallet derivation, KES
    /// `evolve()` (ADR-005), and stealth one-time spend key recovery.
    pub fn from_seed(level: DilithiumLevel, seed: &[u8; 32]) -> Result<Self> {
        from_seed(level, seed)
    }
}

/// ML-DSA detached signature.
#[derive(Clone, PartialEq, Eq)]
pub struct PqcSignature {
    level: DilithiumLevel,
    bytes: Vec<u8>,
}

impl PqcSignature {
    /// Construct from raw bytes. ML-DSA detached signatures are fixed-size
    /// per parameter set (FIPS 204); we validate exact length.
    pub fn from_bytes(level: DilithiumLevel, bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > level.signature_bytes() {
            return Err(CryptoError::InvalidSize {
                expected: level.signature_bytes(),
                actual: bytes.len(),
            });
        }
        Ok(Self { level, bytes })
    }

    /// Raw signature bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The parameter set this signature targets.
    #[must_use]
    pub fn level(&self) -> DilithiumLevel {
        self.level
    }
}

impl core::fmt::Debug for PqcSignature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PqcSignature({:?}, {} bytes)",
            self.level,
            self.bytes.len()
        )
    }
}

// ============================================================================
// Operations
// ============================================================================

/// Helper: copy a 32-byte slice into ml-dsa's `B32` (= `Array<u8, U32>`).
fn b32_from_seed(seed: &[u8; 32]) -> B32 {
    let mut xi = B32::default();
    xi.copy_from_slice(seed);
    xi
}

/// Generate a fresh ML-DSA keypair at the given security level using OS entropy.
pub fn generate_keypair(level: DilithiumLevel) -> Result<PqcKeyPair> {
    let (pk_bytes, sk_bytes) = match level {
        DilithiumLevel::Level2 => {
            let kp = <MlDsa44 as KeyGen>::key_gen(&mut OsRng);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
        DilithiumLevel::Level3 => {
            let kp = <MlDsa65 as KeyGen>::key_gen(&mut OsRng);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
        DilithiumLevel::Level5 => {
            let kp = <MlDsa87 as KeyGen>::key_gen(&mut OsRng);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
    };
    Ok(PqcKeyPair {
        public: PqcPublicKey::from_bytes(level, pk_bytes)?,
        secret: PqcSecretKey::from_bytes(level, sk_bytes)?,
    })
}

/// Derive a deterministic ML-DSA keypair from a 32-byte seed.
///
/// Implements FIPS 204 §6.1 `KeyGen_internal(ξ)` via `ml-dsa`'s
/// `<MlDsaP as KeyGen>::key_gen_internal(&B32)`. Same `(level, seed)` always
/// produces the same `(pk, sk)`.
///
/// **C-04 (REOPENED 2026-05-07) is now CLOSED 2026-05-07** — see ADR-006.
///
/// This is the entry point for:
///   - Wallet HD spend-key derivation (`qv_wallet::hd::derive_spend_key`)
///   - KES per-period leaf key derivation (`qv_crypto::kes`)
///   - Stealth one-time spend key recovery (`qv_privacy::stealth`)
///   - Miner cold key from operator seed (`qv_miner::keys::ColdKeyPair::from_seed`)
pub fn from_seed(level: DilithiumLevel, seed: &[u8; 32]) -> Result<PqcKeyPair> {
    let xi = b32_from_seed(seed);
    let (pk_bytes, sk_bytes) = match level {
        DilithiumLevel::Level2 => {
            let kp = <MlDsa44 as KeyGen>::key_gen_internal(&xi);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
        DilithiumLevel::Level3 => {
            let kp = <MlDsa65 as KeyGen>::key_gen_internal(&xi);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
        DilithiumLevel::Level5 => {
            let kp = <MlDsa87 as KeyGen>::key_gen_internal(&xi);
            (
                kp.verifying_key().encode().as_slice().to_vec(),
                kp.signing_key().encode().as_slice().to_vec(),
            )
        }
    };
    Ok(PqcKeyPair {
        public: PqcPublicKey::from_bytes(level, pk_bytes)?,
        secret: PqcSecretKey::from_bytes(level, sk_bytes)?,
    })
}

/// Produce a detached signature over `message` using `secret`.
///
/// Uses FIPS 204 deterministic signing (empty context) via the standard
/// `signature::Signer` trait. Returns `Err(CryptoError::MalformedPublicKey)`
/// if the secret-key bytes don't decode to a valid `MlDsaSigningKey`.
pub fn sign(secret: &PqcSecretKey, message: &[u8]) -> Result<PqcSignature> {
    let sig_bytes = match secret.level {
        DilithiumLevel::Level2 => sign_level::<MlDsa44>(secret.expose_secret(), message)?,
        DilithiumLevel::Level3 => sign_level::<MlDsa65>(secret.expose_secret(), message)?,
        DilithiumLevel::Level5 => sign_level::<MlDsa87>(secret.expose_secret(), message)?,
    };
    PqcSignature::from_bytes(secret.level, sig_bytes)
}

/// Generic per-level signing helper. Decodes the secret-key bytes, signs the
/// message with `Signer::sign` (panics on internal failure — vanishingly rare
/// for FIPS 204 deterministic signing of valid keys), and returns encoded
/// signature bytes.
fn sign_level<P>(sk_bytes: &[u8], message: &[u8]) -> Result<Vec<u8>>
where
    P: ml_dsa::MlDsaParams,
    MlDsaSigningKey<P>: Signer<MlDsaSignature<P>>,
{
    let enc =
        EncodedSigningKey::<P>::try_from(sk_bytes).map_err(|_| CryptoError::MalformedPublicKey)?;
    let sk = MlDsaSigningKey::<P>::decode(&enc);
    let sig: MlDsaSignature<P> = sk.sign(message);
    Ok(sig.encode().as_slice().to_vec())
}

/// Verify a detached signature.
///
/// Returns `Ok(true)` on a valid signature, `Ok(false)` on a well-formed
/// but invalid signature, and `Err(...)` on structurally malformed input
/// (wrong key length, level mismatch, undecodable signature).
pub fn verify(public: &PqcPublicKey, message: &[u8], signature: &PqcSignature) -> Result<bool> {
    if public.level != signature.level {
        return Err(CryptoError::Other(
            "public key and signature target different parameter sets".into(),
        ));
    }
    let verdict = match public.level {
        DilithiumLevel::Level2 => verify_level::<MlDsa44>(
            public.as_bytes(),
            message,
            signature.as_bytes(),
            public.level,
        )?,
        DilithiumLevel::Level3 => verify_level::<MlDsa65>(
            public.as_bytes(),
            message,
            signature.as_bytes(),
            public.level,
        )?,
        DilithiumLevel::Level5 => verify_level::<MlDsa87>(
            public.as_bytes(),
            message,
            signature.as_bytes(),
            public.level,
        )?,
    };
    Ok(verdict)
}

/// Generic per-level verifying helper.
fn verify_level<P>(
    pk_bytes: &[u8],
    message: &[u8],
    sig_bytes: &[u8],
    level: DilithiumLevel,
) -> Result<bool>
where
    P: ml_dsa::MlDsaParams,
    MlDsaVerifyingKey<P>: Verifier<MlDsaSignature<P>>,
{
    let pk_enc = EncodedVerifyingKey::<P>::try_from(pk_bytes)
        .map_err(|_| CryptoError::MalformedPublicKey)?;
    let pk = MlDsaVerifyingKey::<P>::decode(&pk_enc);
    let sig_enc =
        EncodedSignature::<P>::try_from(sig_bytes).map_err(|_| CryptoError::InvalidSize {
            expected: level.signature_bytes(),
            actual: sig_bytes.len(),
        })?;
    // `Signature::decode` returns Option — `None` means structurally invalid
    // signature encoding (out-of-range coefficients, etc.). Treat as
    // verification failure rather than error.
    let Some(sig) = MlDsaSignature::<P>::decode(&sig_enc) else {
        return Ok(false);
    };
    Ok(pk.verify(message, &sig).is_ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn keypair_sizes_match_level_spec() {
        for level in [
            DilithiumLevel::Level2,
            DilithiumLevel::Level3,
            DilithiumLevel::Level5,
        ] {
            let kp = generate_keypair(level).expect("keygen");
            assert_eq!(kp.public.as_bytes().len(), level.public_key_bytes());
            assert_eq!(kp.secret.expose_secret().len(), level.secret_key_bytes());
        }
    }

    #[test]
    fn fips204_size_invariants() {
        // Spike-verified FIPS 204 sizes for ML-DSA-65; pin them so a future
        // ml-dsa minor bump that changes the wire format is caught here.
        assert_eq!(DilithiumLevel::Level3.public_key_bytes(), 1952);
        assert_eq!(DilithiumLevel::Level3.secret_key_bytes(), 4032);
        assert_eq!(DilithiumLevel::Level3.signature_bytes(), 3309);
    }

    #[test]
    fn sign_and_verify_roundtrip_level3() {
        let kp = generate_keypair(DilithiumLevel::Level3).unwrap();
        let msg = b"Transfer: Alice -> Bob, 100 QV";
        let sig = sign(&kp.secret, msg).unwrap();
        assert!(verify(&kp.public, msg, &sig).unwrap());
    }

    #[test]
    fn tampered_message_fails_verification() {
        let kp = generate_keypair(DilithiumLevel::Level3).unwrap();
        let sig = sign(&kp.secret, b"original message").unwrap();
        assert!(!verify(&kp.public, b"tampered message", &sig).unwrap());
    }

    #[test]
    fn wrong_public_key_fails_verification() {
        let kp_a = generate_keypair(DilithiumLevel::Level3).unwrap();
        let kp_b = generate_keypair(DilithiumLevel::Level3).unwrap();
        let sig = sign(&kp_a.secret, b"pay Bob").unwrap();
        assert!(!verify(&kp_b.public, b"pay Bob", &sig).unwrap());
    }

    #[test]
    fn all_levels_roundtrip() {
        for level in [
            DilithiumLevel::Level2,
            DilithiumLevel::Level3,
            DilithiumLevel::Level5,
        ] {
            let kp = generate_keypair(level).unwrap();
            let sig = sign(&kp.secret, b"level test").unwrap();
            assert!(
                verify(&kp.public, b"level test", &sig).unwrap(),
                "{level:?}"
            );
            assert_eq!(sig.level(), level);
        }
    }

    #[test]
    fn level_mismatch_is_error() {
        let kp = generate_keypair(DilithiumLevel::Level3).unwrap();
        let sig = sign(&kp.secret, b"m").unwrap();

        // Hand-craft a public key claiming Level5 but carrying Level3 bytes
        // padded to Level5 size — must not verify as Level5.
        let mut padded = kp.public.as_bytes().to_vec();
        padded.resize(DilithiumLevel::Level5.public_key_bytes(), 0);
        let fake = PqcPublicKey::from_bytes(DilithiumLevel::Level5, padded).unwrap();

        let err = verify(&fake, b"m", &sig).unwrap_err();
        match err {
            CryptoError::Other(_) => {} // expected: level mismatch
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn pubkey_from_bytes_rejects_wrong_size() {
        let too_short = vec![0u8; 10];
        let err = PqcPublicKey::from_bytes(DilithiumLevel::Level3, too_short).unwrap_err();
        matches!(err, CryptoError::InvalidSize { .. });
    }

    // ========================================================================
    // Seeded keygen — `from_seed` (envanter ID C-04 + C-06 — CLOSED via ADR-006)
    // ========================================================================

    #[test]
    fn from_seed_is_deterministic() {
        let seed = [0xAAu8; 32];
        let kp1 = from_seed(DilithiumLevel::Level3, &seed).expect("seeded keygen");
        let kp2 = from_seed(DilithiumLevel::Level3, &seed).expect("seeded keygen");
        assert_eq!(kp1.public.as_bytes(), kp2.public.as_bytes());
        assert_eq!(kp1.secret.expose_secret(), kp2.secret.expose_secret());
    }

    #[test]
    fn from_seed_different_seeds_differ() {
        let kp1 = from_seed(DilithiumLevel::Level3, &[0xAAu8; 32]).unwrap();
        let kp2 = from_seed(DilithiumLevel::Level3, &[0xBBu8; 32]).unwrap();
        assert_ne!(kp1.public.as_bytes(), kp2.public.as_bytes());
    }

    #[test]
    fn from_seed_signed_message_verifies_with_derived_pk() {
        let seed = [42u8; 32];
        let kp = from_seed(DilithiumLevel::Level3, &seed).unwrap();
        let msg = b"derived-key signing test";
        let sig = sign(&kp.secret, msg).unwrap();
        assert!(verify(&kp.public, msg, &sig).unwrap());
    }

    #[test]
    fn from_seed_all_levels_roundtrip() {
        let seed = [7u8; 32];
        for level in [
            DilithiumLevel::Level2,
            DilithiumLevel::Level3,
            DilithiumLevel::Level5,
        ] {
            let kp = from_seed(level, &seed).unwrap();
            assert_eq!(kp.public.as_bytes().len(), level.public_key_bytes());
            assert_eq!(kp.secret.expose_secret().len(), level.secret_key_bytes());
            let sig = sign(&kp.secret, b"x").unwrap();
            assert!(verify(&kp.public, b"x", &sig).unwrap(), "{level:?}");
        }
    }
}
