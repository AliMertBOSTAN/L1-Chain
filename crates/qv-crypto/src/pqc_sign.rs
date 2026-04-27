//! Post-quantum digital signatures (Dilithium / ML-DSA).
//!
//! Dispatches at runtime on [`DilithiumLevel`] because each parameter set
//! is exposed as a distinct concrete type by the `pqcrypto-dilithium`
//! crate. Internally we always carry raw byte vectors and delegate to the
//! typed API only inside the per-level match arms.
//!
//! # Security levels (NIST FIPS 204, ML-DSA)
//!
//! | Variant | Classical sec. | PQ sec. | pk bytes | sk bytes | sig bytes |
//! |---------|---------------:|--------:|---------:|---------:|----------:|
//! | Level 2 | 128            | 128     | 1312     | 2528     | 2420      |
//! | Level 3 | 192            | 192     | 1952     | 4000     | 3293      |
//! | Level 5 | 256            | 256     | 2592     | 4864     | 4595      |
//!
//! The pqcrypto-dilithium crate's sizes match these.

use pqcrypto_dilithium::{dilithium2, dilithium3, dilithium5};
use pqcrypto_traits::sign::{DetachedSignature, PublicKey, SecretKey};

use crate::{CryptoError, Result, SecureBytes};

/// Dilithium / ML-DSA security level.
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
    /// Public key length in bytes.
    #[must_use]
    pub const fn public_key_bytes(self) -> usize {
        match self {
            Self::Level2 => dilithium2::public_key_bytes(),
            Self::Level3 => dilithium3::public_key_bytes(),
            Self::Level5 => dilithium5::public_key_bytes(),
        }
    }

    /// Secret key length in bytes.
    #[must_use]
    pub const fn secret_key_bytes(self) -> usize {
        match self {
            Self::Level2 => dilithium2::secret_key_bytes(),
            Self::Level3 => dilithium3::secret_key_bytes(),
            Self::Level5 => dilithium5::secret_key_bytes(),
        }
    }

    /// Detached signature length in bytes.
    #[must_use]
    pub const fn signature_bytes(self) -> usize {
        match self {
            Self::Level2 => dilithium2::signature_bytes(),
            Self::Level3 => dilithium3::signature_bytes(),
            Self::Level5 => dilithium5::signature_bytes(),
        }
    }
}

// ============================================================================
// Typed wrappers — keep byte buffers behind opaque newtypes
// ============================================================================

/// Dilithium public key.
#[derive(Clone, PartialEq, Eq)]
pub struct PqcPublicKey {
    level: DilithiumLevel,
    bytes: Vec<u8>,
}

impl PqcPublicKey {
    /// Construct from raw bytes, validating length against the expected
    /// parameter-set size.
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

/// Dilithium secret key. Stored inside a [`SecureBytes`] so it zeroes on drop.
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
    /// Callers **must** treat the return value as sensitive and avoid
    /// copying it into non-zeroizing storage.
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

/// Paired Dilithium keys.
#[derive(Clone, Debug)]
pub struct PqcKeyPair {
    /// Public half.
    pub public: PqcPublicKey,
    /// Secret half.
    pub secret: PqcSecretKey,
}

/// Dilithium detached signature.
#[derive(Clone, PartialEq, Eq)]
pub struct PqcSignature {
    level: DilithiumLevel,
    bytes: Vec<u8>,
}

impl PqcSignature {
    /// Construct from raw bytes. Unlike keys, signature length is an
    /// upper bound — the actual signature may be shorter in some variants.
    /// We validate that the length fits.
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

/// Generate a fresh Dilithium keypair at the given security level.
pub fn generate_keypair(level: DilithiumLevel) -> Result<PqcKeyPair> {
    let (pk_bytes, sk_bytes) = match level {
        DilithiumLevel::Level2 => {
            let (pk, sk) = dilithium2::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        DilithiumLevel::Level3 => {
            let (pk, sk) = dilithium3::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        DilithiumLevel::Level5 => {
            let (pk, sk) = dilithium5::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
    };
    Ok(PqcKeyPair {
        public: PqcPublicKey::from_bytes(level, pk_bytes)?,
        secret: PqcSecretKey::from_bytes(level, sk_bytes)?,
    })
}

/// Produce a detached signature over `message` using `secret`.
pub fn sign(secret: &PqcSecretKey, message: &[u8]) -> Result<PqcSignature> {
    let sig_bytes = match secret.level {
        DilithiumLevel::Level2 => {
            let sk = dilithium2::SecretKey::from_bytes(secret.expose_secret())
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let sig = dilithium2::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        DilithiumLevel::Level3 => {
            let sk = dilithium3::SecretKey::from_bytes(secret.expose_secret())
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let sig = dilithium3::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        DilithiumLevel::Level5 => {
            let sk = dilithium5::SecretKey::from_bytes(secret.expose_secret())
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let sig = dilithium5::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
    };
    PqcSignature::from_bytes(secret.level, sig_bytes)
}

/// Verify a detached signature.
///
/// Returns `Ok(true)` on a valid signature, `Ok(false)` on a
/// well-formed but invalid signature, and `Err(...)` on structurally
/// malformed input.
pub fn verify(public: &PqcPublicKey, message: &[u8], signature: &PqcSignature) -> Result<bool> {
    if public.level != signature.level {
        return Err(CryptoError::Other(
            "public key and signature target different parameter sets".into(),
        ));
    }
    let verdict =
        match public.level {
            DilithiumLevel::Level2 => {
                let pk = dilithium2::PublicKey::from_bytes(public.as_bytes())
                    .map_err(|_| CryptoError::MalformedPublicKey)?;
                let sig = dilithium2::DetachedSignature::from_bytes(signature.as_bytes()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: public.level.signature_bytes(),
                        actual: signature.as_bytes().len(),
                    },
                )?;
                dilithium2::verify_detached_signature(&sig, message, &pk).is_ok()
            }
            DilithiumLevel::Level3 => {
                let pk = dilithium3::PublicKey::from_bytes(public.as_bytes())
                    .map_err(|_| CryptoError::MalformedPublicKey)?;
                let sig = dilithium3::DetachedSignature::from_bytes(signature.as_bytes()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: public.level.signature_bytes(),
                        actual: signature.as_bytes().len(),
                    },
                )?;
                dilithium3::verify_detached_signature(&sig, message, &pk).is_ok()
            }
            DilithiumLevel::Level5 => {
                let pk = dilithium5::PublicKey::from_bytes(public.as_bytes())
                    .map_err(|_| CryptoError::MalformedPublicKey)?;
                let sig = dilithium5::DetachedSignature::from_bytes(signature.as_bytes()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: public.level.signature_bytes(),
                        actual: signature.as_bytes().len(),
                    },
                )?;
                dilithium5::verify_detached_signature(&sig, message, &pk).is_ok()
            }
        };
    Ok(verdict)
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
}
