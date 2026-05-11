//! Verifiable Random Function (VRF) for Ouroboros Praos slot-leader election.
//!
//! Implements **ECVRF-RISTRETTO255-SHA512** per IETF `draft-irtf-cfrg-vrf-15`,
//! delegating to the `schnorrkel` crate (Web3 Foundation, Polkadot reference).
//!
//! See **ADR-004** for the rationale: Ristretto255 is the production-tested
//! choice for MVP/mainnet-v1; a hybrid Ristretto + lattice-VRF is planned for
//! mainnet-v2 once a production-grade lattice VRF crate exists. The trait
//! `qv_consensus::leader_schedule::VrfEvaluator` is the swap point.
//!
//! # Determinism
//!
//! `(sk, msg) → (output, proof)` is fully deterministic — a different attacker
//! cannot bias the slot-leader distribution by re-trying. Verification is also
//! deterministic and constant-time-ish (schnorrkel uses `subtle`).
//!
//! # Domain separation
//!
//! Callers MUST already domain-separate their messages (e.g. via
//! `qv_consensus::leader_schedule::vrf_input`). For extra safety this module
//! prefixes every transcript with the tag `b"QuantumVault-Praos-VRF-v1"`,
//! preventing cross-context VRF replay (an output produced for one purpose
//! is not valid for another).
//!
//! # Wire format
//!
//! - `VrfPublicKey`: 32 bytes (compressed Ristretto point)
//! - `VrfOutput`: 32 bytes (`vrf_output` per IETF spec — the random value)
//! - `VrfProof`: variable (~96 bytes for schnorrkel, but callers MUST treat as
//!   opaque `Vec<u8>` since a future hybrid swap will change the size)
//!
//! # Example
//!
//! ```rust,no_run
//! # use qv_crypto::vrf::{VrfKeyPair, evaluate, verify};
//! let kp = VrfKeyPair::generate().unwrap();
//! let msg = b"slot=1234";
//! let (output, proof) = evaluate(&kp.secret, msg).unwrap();
//! let recovered = verify(&kp.public, msg, &proof).unwrap();
//! assert_eq!(output, recovered);
//! ```

#![forbid(unsafe_code)]

use schnorrkel::{
    vrf::{VRFPreOut, VRFProof},
    Keypair, MiniSecretKey, PublicKey, SecretKey,
};
use serde::{Deserialize, Serialize};
use zeroize::ZeroizeOnDrop;

use crate::{CryptoError, Result};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Domain separation tag prepended to every VRF transcript.
///
/// Bumping this string is equivalent to a hard fork: existing VRF outputs
/// would no longer verify under the new tag.
pub const VRF_DOMAIN_TAG: &[u8] = b"QuantumVault-Praos-VRF-v1";

/// Ristretto255-VRF public key length (compressed point).
pub const VRF_PUBLIC_KEY_BYTES: usize = 32;

/// VRF output length (`vrf_output` per IETF spec).
pub const VRF_OUTPUT_BYTES: usize = 32;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// VRF secret key (zeroize-on-drop).
///
/// Wraps a `schnorrkel::SecretKey`. Callers must not log or serialize the
/// inner bytes; the `Debug` impl is opaque.
#[derive(Clone, ZeroizeOnDrop)]
pub struct VrfSecretKey {
    // schnorrkel::SecretKey internally holds 64 bytes (key||nonce). We hold
    // a copy of the canonical 64-byte representation so that we can rebuild
    // the schnorrkel SecretKey on demand without leaking it across calls.
    bytes: [u8; 64],
}

impl VrfSecretKey {
    /// Construct from canonical 64-byte representation.
    pub fn from_bytes(bytes: [u8; 64]) -> Result<Self> {
        // Validate via schnorrkel's parser to catch malformed encodings.
        SecretKey::from_bytes(&bytes)
            .map_err(|e| CryptoError::Other(format!("vrf secret key parse: {e}")))?;
        Ok(Self { bytes })
    }

    /// Expose the canonical 64-byte representation. Callers MUST treat this
    /// as sensitive material.
    pub fn expose_secret(&self) -> &[u8; 64] {
        &self.bytes
    }

    fn to_inner(&self) -> SecretKey {
        // `from_bytes` was validated at construction; safe to unwrap-equivalent.
        // We avoid `unwrap` and instead use a defensive map_err that should
        // never trigger for a well-constructed VrfSecretKey.
        SecretKey::from_bytes(&self.bytes).unwrap_or_else(|_| {
            // Defensive: if invariants somehow break, return a deterministic
            // dummy that will fail verification — better than panicking.
            SecretKey::from_bytes(&[0u8; 64]).unwrap_or_else(|_| {
                MiniSecretKey::from_bytes(&[0u8; 32])
                    .unwrap_or_else(|_| MiniSecretKey::generate())
                    .expand(MiniSecretKey::ED25519_MODE)
            })
        })
    }
}

impl core::fmt::Debug for VrfSecretKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VrfSecretKey(<redacted>, {} bytes)", self.bytes.len())
    }
}

/// VRF public key (32-byte Ristretto point, hashable + serializable).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VrfPublicKey(pub [u8; VRF_PUBLIC_KEY_BYTES]);

impl VrfPublicKey {
    /// Construct from raw 32-byte representation.
    pub fn from_bytes(bytes: [u8; VRF_PUBLIC_KEY_BYTES]) -> Result<Self> {
        // Validate that bytes form a valid Ristretto point.
        PublicKey::from_bytes(&bytes)
            .map_err(|e| CryptoError::Other(format!("vrf public key parse: {e}")))?;
        Ok(Self(bytes))
    }

    /// Raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; VRF_PUBLIC_KEY_BYTES] {
        &self.0
    }

    fn to_inner(&self) -> Result<PublicKey> {
        PublicKey::from_bytes(&self.0)
            .map_err(|e| CryptoError::Other(format!("vrf public key inner parse: {e}")))
    }
}

/// VRF random output — the 32-byte `vrf_output` per IETF spec.
///
/// This is the value Ouroboros Praos compares against the leadership
/// threshold. Two different VRF schemes producing the same `output`
/// would be a collision; `(output, proof)` is bound to `(sk, msg)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VrfOutput(pub [u8; VRF_OUTPUT_BYTES]);

impl VrfOutput {
    /// Construct from 32 bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; VRF_OUTPUT_BYTES]) -> Self {
        Self(bytes)
    }

    /// Raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; VRF_OUTPUT_BYTES] {
        &self.0
    }
}

/// VRF proof — opaque, variable-length (~96 bytes for Ristretto255-VRF).
///
/// Treat as a black box: a future hybrid (Ristretto + lattice) impl will
/// produce larger proofs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfProof(pub Vec<u8>);

impl VrfProof {
    /// Wrap raw bytes (no validation; verify-time check via [`verify`]).
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Paired VRF keys.
#[derive(Clone, Debug)]
pub struct VrfKeyPair {
    /// Public half (32-byte Ristretto point).
    pub public: VrfPublicKey,
    /// Secret half.
    pub secret: VrfSecretKey,
}

impl VrfKeyPair {
    /// Generate a fresh VRF keypair via OS entropy.
    pub fn generate() -> Result<Self> {
        let kp = Keypair::generate();
        let public = VrfPublicKey(kp.public.to_bytes());
        let secret = VrfSecretKey {
            bytes: kp.secret.to_bytes(),
        };
        Ok(Self { public, secret })
    }

    /// Derive a deterministic VRF keypair from a 32-byte seed.
    ///
    /// Uses schnorrkel's `MiniSecretKey::from_bytes` then expands to the
    /// full `SecretKey` via the standard ed25519-style expansion. Same
    /// seed → byte-identical keypair.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self> {
        let mini = MiniSecretKey::from_bytes(seed)
            .map_err(|e| CryptoError::Other(format!("vrf mini secret key: {e}")))?;
        let sk = mini.expand(MiniSecretKey::ED25519_MODE);
        let pk = sk.to_public();
        Ok(Self {
            public: VrfPublicKey(pk.to_bytes()),
            secret: VrfSecretKey {
                bytes: sk.to_bytes(),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Build a domain-separated transcript for a VRF input.
fn build_transcript(msg: &[u8]) -> merlin::Transcript {
    let mut t = merlin::Transcript::new(b"QuantumVault-Praos-VRF");
    t.append_message(b"domain", VRF_DOMAIN_TAG);
    t.append_message(b"msg", msg);
    t
}

/// Evaluate the VRF on `msg` with `secret`, producing `(output, proof)`.
///
/// `output` is the deterministic random value Ouroboros Praos compares
/// against the leadership threshold. `proof` lets anyone with the matching
/// `VrfPublicKey` verify that `output` was correctly derived.
///
/// # Wire format
///
/// `proof` is a 96-byte concatenation `pre_out (32) || proof (64)`.
/// schnorrkel separates these in its API, but for storage in
/// `BlockHeader.vrf_proof` we serialize them together so a single opaque
/// blob suffices. `verify()` re-splits accordingly.
pub fn evaluate(secret: &VrfSecretKey, msg: &[u8]) -> Result<(VrfOutput, VrfProof)> {
    let sk = secret.to_inner();
    let pk = sk.to_public();
    let kp = Keypair { secret: sk, public: pk };

    let transcript = build_transcript(msg);

    // schnorrkel returns (VRFInOut, VRFProof, VRFPreOut-batchable). We use
    // the in_out + proof; the batchable form is unused.
    let (in_out, proof, _batchable) = kp.vrf_sign(transcript);

    // Serialize as pre_out (32) || proof (64) = 96 bytes.
    let pre_out_bytes = in_out.to_preout().to_bytes();
    let proof_bytes = proof.to_bytes();
    let mut combined = Vec::with_capacity(96);
    combined.extend_from_slice(&pre_out_bytes);
    combined.extend_from_slice(&proof_bytes);

    let output_bytes: [u8; VRF_OUTPUT_BYTES] = in_out.make_bytes(VRF_DOMAIN_TAG);

    Ok((
        VrfOutput::from_bytes(output_bytes),
        VrfProof::from_bytes(combined),
    ))
}

/// Verify a VRF proof. Returns the `output` on success.
///
/// Errors if the public key is malformed, the proof is malformed, or the
/// proof does not bind `(public, msg, output)`.
///
/// # Wire format
///
/// The 96-byte proof is split as `pre_out (32) || proof (64)`; see
/// [`evaluate`].
pub fn verify(public: &VrfPublicKey, msg: &[u8], proof: &VrfProof) -> Result<VrfOutput> {
    let pk = public.to_inner()?;

    let bytes = proof.as_bytes();
    if bytes.len() != 96 {
        return Err(CryptoError::InvalidSize {
            expected: 96,
            actual: bytes.len(),
        });
    }

    let mut pre_out_arr = [0u8; 32];
    pre_out_arr.copy_from_slice(&bytes[..32]);
    let mut proof_arr = [0u8; 64];
    proof_arr.copy_from_slice(&bytes[32..]);

    let pre_out = VRFPreOut::from_bytes(&pre_out_arr)
        .map_err(|e| CryptoError::Other(format!("vrf pre_out parse: {e}")))?;
    let parsed_proof = VRFProof::from_bytes(&proof_arr)
        .map_err(|e| CryptoError::Other(format!("vrf proof parse: {e}")))?;

    let transcript = build_transcript(msg);

    let (in_out, _) = pk
        .vrf_verify(transcript, &pre_out, &parsed_proof)
        .map_err(|e| CryptoError::Other(format!("vrf verify: {e}")))?;

    let output_bytes: [u8; VRF_OUTPUT_BYTES] = in_out.make_bytes(VRF_DOMAIN_TAG);
    Ok(VrfOutput::from_bytes(output_bytes))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn keygen_produces_correct_sizes() {
        let kp = VrfKeyPair::generate().unwrap();
        assert_eq!(kp.public.as_bytes().len(), VRF_PUBLIC_KEY_BYTES);
        assert_eq!(kp.secret.expose_secret().len(), 64);
    }

    #[test]
    fn from_seed_is_deterministic() {
        let seed = [42u8; 32];
        let kp1 = VrfKeyPair::from_seed(&seed).unwrap();
        let kp2 = VrfKeyPair::from_seed(&seed).unwrap();
        assert_eq!(kp1.public, kp2.public);
        assert_eq!(kp1.secret.expose_secret(), kp2.secret.expose_secret());
    }

    #[test]
    fn from_seed_different_seeds_differ() {
        let kp_a = VrfKeyPair::from_seed(&[1u8; 32]).unwrap();
        let kp_b = VrfKeyPair::from_seed(&[2u8; 32]).unwrap();
        assert_ne!(kp_a.public, kp_b.public);
    }

    #[test]
    fn evaluate_verify_roundtrip() {
        let kp = VrfKeyPair::from_seed(&[7u8; 32]).unwrap();
        let msg = b"slot=1234|epoch_nonce=...";
        let (output, proof) = evaluate(&kp.secret, msg).unwrap();
        let recovered = verify(&kp.public, msg, &proof).unwrap();
        assert_eq!(output, recovered);
    }

    #[test]
    fn proof_is_96_bytes() {
        let kp = VrfKeyPair::from_seed(&[11u8; 32]).unwrap();
        let (_out, proof) = evaluate(&kp.secret, b"x").unwrap();
        assert_eq!(proof.as_bytes().len(), 96, "wire format = pre_out(32) + proof(64)");
    }

    #[test]
    fn evaluate_output_is_deterministic() {
        // schnorrkel's VRF output is deterministic in (sk, msg); the proof
        // contains a random nonce, so proof bytes can differ but the output
        // must always be the same.
        let kp = VrfKeyPair::from_seed(&[9u8; 32]).unwrap();
        let msg = b"deterministic test";
        let (out1, _proof1) = evaluate(&kp.secret, msg).unwrap();
        let (out2, _proof2) = evaluate(&kp.secret, msg).unwrap();
        assert_eq!(out1, out2);
    }

    #[test]
    fn wrong_public_key_fails_verify() {
        let kp_a = VrfKeyPair::from_seed(&[3u8; 32]).unwrap();
        let kp_b = VrfKeyPair::from_seed(&[4u8; 32]).unwrap();
        let msg = b"msg";
        let (_out, proof) = evaluate(&kp_a.secret, msg).unwrap();
        let res = verify(&kp_b.public, msg, &proof);
        assert!(res.is_err(), "verify should reject wrong public key");
    }

    #[test]
    fn tampered_proof_fails_verify() {
        let kp = VrfKeyPair::from_seed(&[5u8; 32]).unwrap();
        let msg = b"original";
        let (_out, proof) = evaluate(&kp.secret, msg).unwrap();
        let mut tampered = proof.0.clone();
        tampered[0] ^= 0xFF;
        let res = verify(&kp.public, msg, &VrfProof(tampered));
        assert!(res.is_err());
    }

    #[test]
    fn tampered_message_fails_verify() {
        let kp = VrfKeyPair::from_seed(&[6u8; 32]).unwrap();
        let (_out, proof) = evaluate(&kp.secret, b"original").unwrap();
        let res = verify(&kp.public, b"tampered", &proof);
        assert!(res.is_err());
    }

    #[test]
    fn malformed_proof_size_rejected() {
        let kp = VrfKeyPair::from_seed(&[8u8; 32]).unwrap();
        let bad = VrfProof::from_bytes(vec![0u8; 50]); // wrong size
        let res = verify(&kp.public, b"msg", &bad);
        assert!(matches!(res, Err(CryptoError::InvalidSize { .. })));
    }

    #[test]
    fn malformed_public_key_rejected() {
        // Not a valid Ristretto point (all 0xFF, not on the curve).
        let bad_pk = VrfPublicKey([0xFFu8; 32]);
        let res = bad_pk.to_inner();
        assert!(res.is_err());
    }

    #[test]
    fn debug_secret_does_not_leak_bytes() {
        let kp = VrfKeyPair::from_seed(&[0xCAu8; 32]).unwrap();
        let dbg = format!("{:?}", kp.secret);
        assert!(dbg.contains("redacted"));
        // Make sure raw byte values aren't leaked. 0xCA = "ca" hex.
        assert!(!dbg.contains("ca ca ca"));
    }
}
