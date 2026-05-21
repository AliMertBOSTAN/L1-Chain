//! Hybrid key encapsulation: X25519 + Kyber / ML-KEM.
//!
//! Security model: an adversary must break **both** the discrete-log
//! assumption on Curve25519 *and* the module-LWE assumption behind
//! Kyber to recover the shared secret. The final secret is bound to the
//! full transcript (both public keys, both ciphertexts, the context
//! label) so the scheme is IND-CCA2 and resistant to key-reuse games.
//!
//! # Wire format
//!
//! ```text
//!   hybrid_ciphertext = eph_x25519_public_key (32 B) || kyber_ciphertext
//! ```
//!
//! The 32-byte prefix is the sender's ephemeral X25519 public key; the
//! remainder is the Kyber KEM ciphertext encapsulated against the
//! recipient's long-term Kyber public key.
//!
//! # Shared-secret derivation
//!
//! ```text
//!   SS = SHA3-256( "QuantumVault-Hybrid-KEM-v1" ||
//!                  ecdh_secret ||
//!                  kyber_ss    ||
//!                  eph_x25519_pk ||
//!                  peer_x25519_pk ||
//!                  kyber_ciphertext )
//! ```

use pqcrypto_kyber::{kyber1024, kyber512, kyber768};
use pqcrypto_traits::kem::{Ciphertext, PublicKey, SecretKey, SharedSecret as KemSharedSecret};
use rand_core::OsRng;
use sha3::{Digest, Sha3_256};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Secret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{CryptoError, Result, SecureBytes};

const X25519_PK_BYTES: usize = 32;
const SHARED_SECRET_BYTES: usize = 32;
const KDF_CONTEXT: &[u8] = b"QuantumVault-Hybrid-KEM-v1";

// ============================================================================
// Level enum + size queries
// ============================================================================

/// Kyber / ML-KEM security level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KyberLevel {
    /// ML-KEM-512 — NIST category 1 (128-bit PQ).
    Level1,
    /// ML-KEM-768 — NIST category 3 (192-bit PQ). **Default.**
    Level3,
    /// ML-KEM-1024 — NIST category 5 (256-bit PQ).
    Level5,
}

impl Default for KyberLevel {
    fn default() -> Self {
        Self::Level3
    }
}

impl KyberLevel {
    /// Kyber public key length (bytes).
    #[must_use]
    pub const fn kyber_public_key_bytes(self) -> usize {
        match self {
            Self::Level1 => kyber512::public_key_bytes(),
            Self::Level3 => kyber768::public_key_bytes(),
            Self::Level5 => kyber1024::public_key_bytes(),
        }
    }

    /// Kyber secret key length (bytes).
    #[must_use]
    pub const fn kyber_secret_key_bytes(self) -> usize {
        match self {
            Self::Level1 => kyber512::secret_key_bytes(),
            Self::Level3 => kyber768::secret_key_bytes(),
            Self::Level5 => kyber1024::secret_key_bytes(),
        }
    }

    /// Kyber ciphertext length (bytes).
    #[must_use]
    pub const fn kyber_ciphertext_bytes(self) -> usize {
        match self {
            Self::Level1 => kyber512::ciphertext_bytes(),
            Self::Level3 => kyber768::ciphertext_bytes(),
            Self::Level5 => kyber1024::ciphertext_bytes(),
        }
    }

    /// Hybrid ciphertext length: X25519 ephemeral pk (32 B) + Kyber ct.
    #[must_use]
    pub const fn hybrid_ciphertext_bytes(self) -> usize {
        X25519_PK_BYTES + self.kyber_ciphertext_bytes()
    }
}

// ============================================================================
// Public-facing types
// ============================================================================

/// Publishable half of a hybrid keypair (what a peer advertises).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HybridPublicKey {
    /// X25519 public key (always 32 bytes).
    pub x25519: [u8; X25519_PK_BYTES],
    /// Kyber / ML-KEM public key bytes.
    pub kyber: Vec<u8>,
    /// Parameter set.
    pub level: KyberLevel,
}

/// Full hybrid keypair (public + secret halves).
///
/// Secret key material lives inside [`SecureBytes`] and is zeroized on drop.
#[derive(Clone)]
pub struct HybridKeyPair {
    /// Public key (safe to share).
    pub public: HybridPublicKey,
    /// X25519 secret key bytes.
    x25519_secret: SecureBytes,
    /// Kyber secret key bytes.
    kyber_secret: SecureBytes,
}

impl HybridKeyPair {
    /// The parameter set the keypair was generated against.
    #[must_use]
    pub fn level(&self) -> KyberLevel {
        self.public.level
    }
}

impl core::fmt::Debug for HybridKeyPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HybridKeyPair")
            .field("level", &self.public.level)
            .field("public.x25519", &hex::encode(self.public.x25519))
            .field(
                "public.kyber",
                &format!("{} bytes", self.public.kyber.len()),
            )
            .field("x25519_secret", &self.x25519_secret)
            .field("kyber_secret", &self.kyber_secret)
            .finish()
    }
}

/// Wire-format hybrid ciphertext: `eph_x25519_pk || kyber_ct`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HybridCiphertext {
    /// Raw concatenated bytes.
    pub bytes: Vec<u8>,
    /// Parameter set (needed for split/decoding).
    pub level: KyberLevel,
}

impl HybridCiphertext {
    /// The ephemeral X25519 prefix (first 32 bytes).
    pub fn ephemeral_x25519(&self) -> Result<&[u8]> {
        if self.bytes.len() < X25519_PK_BYTES {
            return Err(CryptoError::MalformedCiphertext);
        }
        Ok(&self.bytes[..X25519_PK_BYTES])
    }

    /// The Kyber ciphertext tail.
    pub fn kyber_part(&self) -> Result<&[u8]> {
        let expected = self.level.hybrid_ciphertext_bytes();
        if self.bytes.len() != expected {
            return Err(CryptoError::MalformedCiphertext);
        }
        Ok(&self.bytes[X25519_PK_BYTES..])
    }
}

/// 32-byte shared secret. Zeroed on drop.
#[derive(Clone)]
pub struct SharedSecret(pub [u8; SHARED_SECRET_BYTES]);

impl SharedSecret {
    /// Expose the raw bytes — caller must treat as sensitive.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; SHARED_SECRET_BYTES] {
        &self.0
    }
}

impl Drop for SharedSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Zeroize for SharedSecret {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for SharedSecret {}

impl PartialEq for SharedSecret {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for SharedSecret {}

impl core::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SharedSecret({} bytes)", self.0.len())
    }
}

// ============================================================================
// Operations
// ============================================================================

/// Generate a fresh hybrid keypair (X25519 + Kyber).
pub fn generate_keypair(level: KyberLevel) -> Result<HybridKeyPair> {
    // --- X25519 part
    let x_sk = X25519Secret::random_from_rng(OsRng);
    let x_pk = X25519Public::from(&x_sk);
    let x_pk_bytes: [u8; X25519_PK_BYTES] = x_pk.to_bytes();
    let x_sk_bytes = SecureBytes::from_slice(x_sk.as_bytes());

    // --- Kyber part
    let (kyber_pk_bytes, kyber_sk_bytes) = match level {
        KyberLevel::Level1 => {
            let (pk, sk) = kyber512::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        KyberLevel::Level3 => {
            let (pk, sk) = kyber768::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        KyberLevel::Level5 => {
            let (pk, sk) = kyber1024::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
    };

    Ok(HybridKeyPair {
        public: HybridPublicKey {
            x25519: x_pk_bytes,
            kyber: kyber_pk_bytes,
            level,
        },
        x25519_secret: x_sk_bytes,
        kyber_secret: SecureBytes::from_vec(kyber_sk_bytes),
    })
}

/// Encapsulate a shared secret for the given peer public key.
///
/// Produces a (ciphertext, shared_secret) pair. The ciphertext is sent
/// to the peer; the shared secret can be used immediately by the
/// encapsulator.
pub fn encapsulate(peer: &HybridPublicKey) -> Result<(HybridCiphertext, SharedSecret)> {
    // --- 1. Ephemeral X25519 keypair + ECDH with peer's X25519 pk.
    let eph_sk = X25519Secret::random_from_rng(OsRng);
    let eph_pk = X25519Public::from(&eph_sk);
    let eph_pk_bytes: [u8; X25519_PK_BYTES] = eph_pk.to_bytes();

    let peer_x_pk = X25519Public::from(peer.x25519);
    let ecdh = eph_sk.diffie_hellman(&peer_x_pk);
    let ecdh_bytes = ecdh.to_bytes(); // zeroes on drop internally (dalek)

    // --- 2. Kyber encapsulation against peer's Kyber pk.
    if peer.kyber.len() != peer.level.kyber_public_key_bytes() {
        return Err(CryptoError::MalformedPublicKey);
    }

    let (kyber_ss_bytes, kyber_ct_bytes) = match peer.level {
        KyberLevel::Level1 => {
            let pk = kyber512::PublicKey::from_bytes(&peer.kyber)
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let (ss, ct) = kyber512::encapsulate(&pk);
            (ss.as_bytes().to_vec(), ct.as_bytes().to_vec())
        }
        KyberLevel::Level3 => {
            let pk = kyber768::PublicKey::from_bytes(&peer.kyber)
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let (ss, ct) = kyber768::encapsulate(&pk);
            (ss.as_bytes().to_vec(), ct.as_bytes().to_vec())
        }
        KyberLevel::Level5 => {
            let pk = kyber1024::PublicKey::from_bytes(&peer.kyber)
                .map_err(|_| CryptoError::MalformedPublicKey)?;
            let (ss, ct) = kyber1024::encapsulate(&pk);
            (ss.as_bytes().to_vec(), ct.as_bytes().to_vec())
        }
    };

    // --- 3. Combine via transcript-bound KDF.
    let ss = kdf_combine(
        &ecdh_bytes,
        &kyber_ss_bytes,
        &eph_pk_bytes,
        &peer.x25519,
        &kyber_ct_bytes,
    );

    // --- 4. Wire-format ciphertext: eph_pk || kyber_ct.
    let mut out = Vec::with_capacity(X25519_PK_BYTES + kyber_ct_bytes.len());
    out.extend_from_slice(&eph_pk_bytes);
    out.extend_from_slice(&kyber_ct_bytes);

    Ok((
        HybridCiphertext {
            bytes: out,
            level: peer.level,
        },
        ss,
    ))
}

/// Decapsulate a hybrid ciphertext with the recipient's secret keypair.
pub fn decapsulate(local: &HybridKeyPair, ciphertext: &HybridCiphertext) -> Result<SharedSecret> {
    if ciphertext.level != local.public.level {
        return Err(CryptoError::MalformedCiphertext);
    }
    if ciphertext.bytes.len() != ciphertext.level.hybrid_ciphertext_bytes() {
        return Err(CryptoError::MalformedCiphertext);
    }

    // --- 1. Split into ephemeral X25519 pk + Kyber ct.
    let eph_pk_bytes: [u8; X25519_PK_BYTES] = ciphertext.bytes[..X25519_PK_BYTES]
        .try_into()
        .map_err(|_| CryptoError::MalformedCiphertext)?;
    let kyber_ct_bytes = &ciphertext.bytes[X25519_PK_BYTES..];

    // --- 2. X25519 ECDH with local secret.
    let x_sk_array: [u8; X25519_PK_BYTES] =
        local
            .x25519_secret
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::InvalidSize {
                expected: X25519_PK_BYTES,
                actual: local.x25519_secret.len(),
            })?;
    let local_x_sk = X25519Secret::from(x_sk_array);
    let eph_pk = X25519Public::from(eph_pk_bytes);
    let ecdh = local_x_sk.diffie_hellman(&eph_pk);
    let ecdh_bytes = ecdh.to_bytes();

    // --- 3. Kyber decapsulation.
    let kyber_ss_bytes =
        match local.public.level {
            KyberLevel::Level1 => {
                let sk = kyber512::SecretKey::from_bytes(local.kyber_secret.as_slice()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: local.public.level.kyber_secret_key_bytes(),
                        actual: local.kyber_secret.len(),
                    },
                )?;
                let ct = kyber512::Ciphertext::from_bytes(kyber_ct_bytes)
                    .map_err(|_| CryptoError::MalformedCiphertext)?;
                kyber512::decapsulate(&ct, &sk).as_bytes().to_vec()
            }
            KyberLevel::Level3 => {
                let sk = kyber768::SecretKey::from_bytes(local.kyber_secret.as_slice()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: local.public.level.kyber_secret_key_bytes(),
                        actual: local.kyber_secret.len(),
                    },
                )?;
                let ct = kyber768::Ciphertext::from_bytes(kyber_ct_bytes)
                    .map_err(|_| CryptoError::MalformedCiphertext)?;
                kyber768::decapsulate(&ct, &sk).as_bytes().to_vec()
            }
            KyberLevel::Level5 => {
                let sk = kyber1024::SecretKey::from_bytes(local.kyber_secret.as_slice()).map_err(
                    |_| CryptoError::InvalidSize {
                        expected: local.public.level.kyber_secret_key_bytes(),
                        actual: local.kyber_secret.len(),
                    },
                )?;
                let ct = kyber1024::Ciphertext::from_bytes(kyber_ct_bytes)
                    .map_err(|_| CryptoError::MalformedCiphertext)?;
                kyber1024::decapsulate(&ct, &sk).as_bytes().to_vec()
            }
        };

    // --- 4. Same transcript the encapsulator hashed.
    //        Note: peer_x25519_pk here is the *recipient's* static pk.
    let ss = kdf_combine(
        &ecdh_bytes,
        &kyber_ss_bytes,
        &eph_pk_bytes,
        &local.public.x25519,
        kyber_ct_bytes,
    );
    Ok(ss)
}

// ============================================================================
// KDF: SHA3-256 of the full transcript
// ============================================================================

fn kdf_combine(
    ecdh: &[u8],
    kyber_ss: &[u8],
    eph_x25519_pk: &[u8],
    peer_x25519_pk: &[u8],
    kyber_ct: &[u8],
) -> SharedSecret {
    let mut h = Sha3_256::new();
    h.update(KDF_CONTEXT);
    h.update(ecdh);
    h.update(kyber_ss);
    h.update(eph_x25519_pk);
    h.update(peer_x25519_pk);
    h.update(kyber_ct);
    let digest: [u8; SHARED_SECRET_BYTES] = h.finalize().into();
    SharedSecret(digest)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn keypair_sizes_match_level_spec() {
        for level in [KyberLevel::Level1, KyberLevel::Level3, KyberLevel::Level5] {
            let kp = generate_keypair(level).unwrap();
            assert_eq!(kp.public.x25519.len(), X25519_PK_BYTES);
            assert_eq!(kp.public.kyber.len(), level.kyber_public_key_bytes());
            assert_eq!(kp.kyber_secret.len(), level.kyber_secret_key_bytes());
        }
    }

    #[test]
    fn encapsulate_decapsulate_roundtrip_level3() {
        let recipient = generate_keypair(KyberLevel::Level3).unwrap();
        let (ct, ss_sender) = encapsulate(&recipient.public).unwrap();
        let ss_receiver = decapsulate(&recipient, &ct).unwrap();
        assert_eq!(ss_sender, ss_receiver);
    }

    #[test]
    fn ciphertext_size_matches_spec() {
        let kp = generate_keypair(KyberLevel::Level3).unwrap();
        let (ct, _) = encapsulate(&kp.public).unwrap();
        assert_eq!(ct.bytes.len(), KyberLevel::Level3.hybrid_ciphertext_bytes());
    }

    #[test]
    fn all_levels_roundtrip() {
        for level in [KyberLevel::Level1, KyberLevel::Level3, KyberLevel::Level5] {
            let kp = generate_keypair(level).unwrap();
            let (ct, ss_send) = encapsulate(&kp.public).unwrap();
            let ss_recv = decapsulate(&kp, &ct).unwrap();
            assert_eq!(ss_send, ss_recv, "roundtrip failed at {level:?}");
        }
    }

    #[test]
    fn wrong_recipient_fails() {
        let alice = generate_keypair(KyberLevel::Level3).unwrap();
        let bob = generate_keypair(KyberLevel::Level3).unwrap();
        let (ct, ss_for_alice) = encapsulate(&alice.public).unwrap();

        // Bob attempts to decap a ciphertext intended for Alice.
        // Kyber is IND-CCA2, so decap yields a pseudo-random value, not an error.
        let ss_at_bob = decapsulate(&bob, &ct).unwrap();
        assert_ne!(ss_at_bob, ss_for_alice);
    }

    #[test]
    fn tampered_kyber_ct_changes_shared_secret() {
        let kp = generate_keypair(KyberLevel::Level3).unwrap();
        let (mut ct, ss_clean) = encapsulate(&kp.public).unwrap();
        // Flip a bit deep in the Kyber portion.
        *ct.bytes.last_mut().unwrap() ^= 0x01;
        let ss_tampered = decapsulate(&kp, &ct).unwrap();
        assert_ne!(ss_tampered, ss_clean);
    }

    #[test]
    fn malformed_short_ciphertext_rejected() {
        let kp = generate_keypair(KyberLevel::Level3).unwrap();
        let ct = HybridCiphertext {
            bytes: vec![0u8; 10],
            level: KyberLevel::Level3,
        };
        let err = decapsulate(&kp, &ct).unwrap_err();
        matches!(err, CryptoError::MalformedCiphertext);
    }

    #[test]
    fn level_mismatch_between_keypair_and_ciphertext_rejected() {
        let kp = generate_keypair(KyberLevel::Level3).unwrap();
        let ct_wrong_level = HybridCiphertext {
            bytes: vec![0u8; KyberLevel::Level5.hybrid_ciphertext_bytes()],
            level: KyberLevel::Level5,
        };
        let err = decapsulate(&kp, &ct_wrong_level).unwrap_err();
        matches!(err, CryptoError::MalformedCiphertext);
    }
}
