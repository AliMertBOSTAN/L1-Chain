//! Cryptographic hash functions.
//!
//! Two algorithms are supported:
//!
//! - **SHA3-256** — `sha3` crate, Keccak-f\[1600\] based, NIST FIPS 202.
//!   Conservative choice, hardware support on many platforms.
//! - **BLAKE3**   — `blake3` crate, tree-hashing, highly parallel.
//!   Typically 3–10× faster than SHA3-256 for long inputs.
//!
//! Both produce a 32-byte digest.
//!
//! # Collision resistance under quantum adversaries
//!
//! Grover's algorithm yields at most a quadratic speed-up for pre-image
//! search, so a 256-bit digest still affords ≥ 128-bit post-quantum
//! security against classical collisions. This is sufficient for our
//! blockchain primitives (transaction ids, Merkle trees, commitment hashes).

use sha3::{Digest, Sha3_256};

use crate::{CryptoError, Result};

/// Fixed-size cryptographic digest.
pub type HashDigest = [u8; 32];

/// Hash algorithm selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    /// NIST SHA3-256 (FIPS 202).
    Sha3_256,
    /// BLAKE3 (32-byte output, default mode).
    Blake3,
}

// ============================================================================
// One-shot helpers
// ============================================================================

/// Hash the given byte slice with the specified algorithm.
pub fn hash(algorithm: HashAlgorithm, data: &[u8]) -> HashDigest {
    match algorithm {
        HashAlgorithm::Sha3_256 => sha3_256(data),
        HashAlgorithm::Blake3 => blake3(data),
    }
}

/// SHA3-256 one-shot hash.
#[must_use]
pub fn sha3_256(data: &[u8]) -> HashDigest {
    let mut h = Sha3_256::new();
    h.update(data);
    h.finalize().into()
}

/// BLAKE3 one-shot hash.
#[must_use]
pub fn blake3(data: &[u8]) -> HashDigest {
    blake3::hash(data).into()
}

/// Double-hash: `H(H(data))`. Common building block for Merkle trees
/// (prevents second-preimage attacks on tree structure).
#[must_use]
pub fn double_hash(algorithm: HashAlgorithm, data: &[u8]) -> HashDigest {
    let first = hash(algorithm, data);
    hash(algorithm, &first)
}

/// Double SHA3-256.
#[must_use]
pub fn double_sha3_256(data: &[u8]) -> HashDigest {
    double_hash(HashAlgorithm::Sha3_256, data)
}

/// Double BLAKE3.
#[must_use]
pub fn double_blake3(data: &[u8]) -> HashDigest {
    double_hash(HashAlgorithm::Blake3, data)
}

// ============================================================================
// Streaming hasher
// ============================================================================

/// Stateful hasher for inputs that arrive in chunks.
///
/// Usage:
///
/// ```
/// use qv_crypto::{Hasher, HashAlgorithm};
///
/// let mut h = Hasher::new(HashAlgorithm::Sha3_256);
/// h.update(b"hello ");
/// h.update(b"world");
/// let digest = h.finalize();
/// assert_eq!(digest.len(), 32);
/// ```
pub enum Hasher {
    /// SHA3-256 backing state.
    Sha3(Sha3_256),
    /// BLAKE3 backing state.
    Blake3(blake3::Hasher),
}

impl Hasher {
    /// Create a new streaming hasher.
    #[must_use]
    pub fn new(algorithm: HashAlgorithm) -> Self {
        match algorithm {
            HashAlgorithm::Sha3_256 => Self::Sha3(Sha3_256::new()),
            HashAlgorithm::Blake3 => Self::Blake3(blake3::Hasher::new()),
        }
    }

    /// Feed more bytes into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha3(h) => Digest::update(h, data),
            Self::Blake3(h) => {
                h.update(data);
            }
        }
    }

    /// Consume the hasher and return the final digest.
    #[must_use]
    pub fn finalize(self) -> HashDigest {
        match self {
            Self::Sha3(h) => h.finalize().into(),
            Self::Blake3(h) => h.finalize().into(),
        }
    }

    /// Compute a digest without consuming the hasher (useful for streaming
    /// commitments that need to be checked repeatedly).
    #[must_use]
    pub fn finalize_cloned(&self) -> HashDigest {
        match self {
            Self::Sha3(h) => h.clone().finalize().into(),
            Self::Blake3(h) => h.finalize().into(),
        }
    }
}

// ============================================================================
// Fallible variant for callers that prefer a Result surface
// ============================================================================

/// Same as [`hash`] but returns a [`Result`] so that callers using the
/// crate-wide error type can bubble up uniformly. Today this never errors;
/// the signature exists so that future backends (e.g. hardware accelerators
/// that may fail) can be plugged in without API breakage.
pub fn try_hash(algorithm: HashAlgorithm, data: &[u8]) -> Result<HashDigest> {
    let out = hash(algorithm, data);
    if out.iter().all(|&b| b == 0) && !data.is_empty() {
        // Defensive: an all-zero digest on non-empty input implies a bug.
        return Err(CryptoError::HashFailed);
    }
    Ok(out)
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

    // ------------------------------------------------------------------
    // NIST SHA3-256 Known-Answer Tests (FIPS 202)
    // ------------------------------------------------------------------

    #[test]
    fn sha3_256_empty_nist_kat() {
        // SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a
        let d = sha3_256(b"");
        assert_eq!(
            hex::encode(d),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );
    }

    #[test]
    fn sha3_256_abc_nist_kat() {
        // SHA3-256("abc") = 3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532
        let d = sha3_256(b"abc");
        assert_eq!(
            hex::encode(d),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
    }

    #[test]
    fn determinism_same_input_same_digest() {
        let a = sha3_256(b"QuantumVault");
        let b = sha3_256(b"QuantumVault");
        assert_eq!(a, b);
        let x = blake3(b"QuantumVault");
        let y = blake3(b"QuantumVault");
        assert_eq!(x, y);
    }

    #[test]
    fn avalanche_single_byte_flip() {
        let a = sha3_256(b"hello");
        let b = sha3_256(b"hellp"); // single character change
        assert_ne!(a, b);
        let x = blake3(b"hello");
        let y = blake3(b"hellp");
        assert_ne!(x, y);
    }

    #[test]
    fn streaming_matches_one_shot_sha3() {
        let msg = b"The quick brown fox jumps over the lazy dog";
        let one_shot = sha3_256(msg);

        let mut h = Hasher::new(HashAlgorithm::Sha3_256);
        h.update(&msg[..10]);
        h.update(&msg[10..]);
        let streamed = h.finalize();
        assert_eq!(one_shot, streamed);
    }

    #[test]
    fn streaming_matches_one_shot_blake3() {
        let msg = b"The quick brown fox jumps over the lazy dog";
        let one_shot = blake3(msg);

        let mut h = Hasher::new(HashAlgorithm::Blake3);
        h.update(&msg[..10]);
        h.update(&msg[10..]);
        let streamed = h.finalize();
        assert_eq!(one_shot, streamed);
    }

    #[test]
    fn double_hash_equals_hash_of_hash() {
        let msg = b"merkle node";
        let dbl = double_sha3_256(msg);
        let step = sha3_256(&sha3_256(msg));
        assert_eq!(dbl, step);
    }

    #[test]
    fn large_input_one_mib() {
        let big = vec![0x42u8; 1024 * 1024];
        let d_sha = sha3_256(&big);
        let d_b3 = blake3(&big);
        // Non-trivial sanity: outputs differ across algorithms.
        assert_ne!(d_sha, d_b3);
    }

    #[test]
    fn try_hash_succeeds_for_normal_input() {
        let r = try_hash(HashAlgorithm::Sha3_256, b"hello").unwrap();
        assert_eq!(r, sha3_256(b"hello"));
    }

    #[test]
    fn finalize_cloned_is_independent() {
        let mut h = Hasher::new(HashAlgorithm::Sha3_256);
        h.update(b"prefix");
        let snapshot = h.finalize_cloned();
        h.update(b"-more");
        let full = h.finalize();
        assert_ne!(snapshot, full);
    }
}
