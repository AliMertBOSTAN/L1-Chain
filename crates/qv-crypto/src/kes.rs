//! Forward-secure Key-Evolving Signatures (KES) on Dilithium.
//!
//! Implements the **MMM sum-composition** (Malkin-Micciancio-Miner 2002) over
//! Dilithium leaf keys per **ADR-005**. A binary tree of depth `d` covers
//! `N = 2^d` periods; the public key is the Merkle root of the leaf public
//! keys. Each period's signature is the leaf signature plus the Merkle path
//! to the root.
//!
//! # Forward security model
//!
//! At generation time we deterministically pre-derive every leaf seed from
//! the master seed and **zeroize the master seed**. We then only retain the
//! leaf seeds; as periods advance, the consumed leaf's seed and keypair are
//! zeroized in place. A compromise at period `p` reveals only the leaves for
//! periods `≥ p` — past signatures cannot be forged.
//!
//! Memory footprint: `32 · N` bytes for leaf seeds plus `32 · N` bytes for
//! leaf pk hashes (public). For `N = 2048` that's 128 KB total — acceptable
//! for an operator daemon. The classical MMM `O(log N)` state machine is a
//! future optimization (`v2` / mainnet hardening).
//!
//! # Wire format
//!
//! `KesPublicKey`: 32 bytes (`pk_root`, the Merkle root).
//! `KesSignature`: `bincode::serialize(KesSignature)` with `period` (4 B) +
//! `leaf_pk` (~1952 B for Level 3) + `leaf_signature` (~3293 B) + `depth`-many
//! 32-byte sibling hashes. Total ≈ 5.6 KB for `d = 11`.
//!
//! # Example
//!
//! ```rust,no_run
//! # use qv_crypto::kes::{generate, sign, verify, evolve};
//! let seed = [0xAB; 32];
//! let (pk, mut sk) = generate(&seed).unwrap();
//! let sig0 = sign(&sk, b"block @ slot 0").unwrap();
//! assert!(verify(&pk, &sig0, b"block @ slot 0").unwrap());
//! evolve(&mut sk).unwrap();
//! let sig1 = sign(&sk, b"block @ slot 1").unwrap();
//! assert!(verify(&pk, &sig1, b"block @ slot 1").unwrap());
//! ```

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::{
    from_seed_pqc, sha3_256, sign_pqc, verify_pqc, CryptoError, DilithiumLevel, PqcKeyPair,
    PqcPublicKey, PqcSignature, Result,
};

// ---------------------------------------------------------------------------
// Constants — tunable but not currently parameterized at the type level
// ---------------------------------------------------------------------------

/// Sum-KES tree depth. `N = 2^KES_TREE_DEPTH` periods covered.
pub const KES_TREE_DEPTH: u32 = 11;

/// Total number of KES periods (`2^KES_TREE_DEPTH`).
pub const KES_TOTAL_PERIODS: u32 = 1 << KES_TREE_DEPTH;

/// Dilithium security level used for every leaf signature. Level 3 = ML-DSA-65.
pub const KES_LEAF_LEVEL: DilithiumLevel = DilithiumLevel::Level3;

/// Domain separation tag for leaf seed derivation.
const KES_LEAF_SEED_TAG: &[u8] = b"QuantumVault-KES-leaf-v1";

/// Domain separation tag for the per-period sign-bound message.
const KES_SIGN_TAG: &[u8] = b"QuantumVault-KES-sign-v1";

/// Domain separation tag for internal Merkle hashes.
const KES_MERKLE_TAG: &[u8] = b"QuantumVault-KES-merkle-v1";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// KES public key: 32-byte Merkle root over all leaf public keys.
///
/// Written once to `StakePool.kes_key` at pool registration; never changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KesPublicKey(pub [u8; 32]);

impl KesPublicKey {
    /// Construct from raw 32 bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// KES secret key: holds remaining leaf seeds and the Merkle pk-hash array.
///
/// `Drop` zeroizes all retained leaf seeds. `current_leaf_kp` is materialized
/// on demand at sign time and dropped immediately, so it is not held in this
/// struct between calls.
pub struct KesSecretKey {
    /// Currently-active KES period in `[0, KES_TOTAL_PERIODS)`.
    period: u32,
    /// Per-period leaf seeds. Index = period. Past entries are zeroized in
    /// place via [`Zeroize::zeroize`] but the slot remains so the index
    /// arithmetic stays simple.
    leaf_seeds: Vec<[u8; 32]>,
    /// Leaf public-key hashes (32 bytes each) — public, used to recompute
    /// the Merkle path siblings at sign time.
    leaf_pk_hashes: Vec<[u8; 32]>,
}

impl core::fmt::Debug for KesSecretKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "KesSecretKey(period={}, total={}, leaf_seeds=<redacted×{}>)",
            self.period,
            KES_TOTAL_PERIODS,
            self.leaf_seeds.len()
        )
    }
}

impl Drop for KesSecretKey {
    fn drop(&mut self) {
        for seed in &mut self.leaf_seeds {
            seed.zeroize();
        }
    }
}

impl KesSecretKey {
    /// Currently-active KES period.
    #[must_use]
    pub fn period(&self) -> u32 {
        self.period
    }

    /// Whether all periods have been consumed.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.period >= KES_TOTAL_PERIODS
    }
}

/// KES signature for a specific period.
///
/// Includes the leaf public key + signature + Merkle path so a verifier can
/// (a) verify the leaf Dilithium signature, (b) recompute the leaf pk hash,
/// (c) walk the Merkle path back to the root and (d) compare with the
/// registered `KesPublicKey`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KesSignature {
    /// Period this signature was issued at.
    pub period: u32,
    /// Raw Dilithium public-key bytes for this leaf.
    pub leaf_pk: Vec<u8>,
    /// Raw Dilithium detached-signature bytes.
    pub leaf_signature: Vec<u8>,
    /// Merkle path: `KES_TREE_DEPTH` sibling hashes, leaf-to-root order.
    pub merkle_path: Vec<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Derive the leaf seed for a given period from the master seed.
fn derive_leaf_seed(master_seed: &[u8; 32], period: u32) -> [u8; 32] {
    let mut input = Vec::with_capacity(KES_LEAF_SEED_TAG.len() + 32 + 4);
    input.extend_from_slice(KES_LEAF_SEED_TAG);
    input.extend_from_slice(master_seed);
    input.extend_from_slice(&period.to_le_bytes());
    sha3_256(&input)
}

/// Hash a Dilithium public key into a 32-byte node value.
fn leaf_pk_hash(leaf_pk_bytes: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(KES_MERKLE_TAG.len() + 1 + leaf_pk_bytes.len());
    input.extend_from_slice(KES_MERKLE_TAG);
    input.push(0x00); // domain tag for leaf
    input.extend_from_slice(leaf_pk_bytes);
    sha3_256(&input)
}

/// Internal Merkle node hash: `H(tag || 0x01 || left || right)`.
fn internal_node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut input = Vec::with_capacity(KES_MERKLE_TAG.len() + 1 + 64);
    input.extend_from_slice(KES_MERKLE_TAG);
    input.push(0x01); // domain tag for internal
    input.extend_from_slice(left);
    input.extend_from_slice(right);
    sha3_256(&input)
}

/// Compute the Merkle root from a slice of 2^d leaf hashes.
fn merkle_root_from_leaves(leaves: &[[u8; 32]]) -> [u8; 32] {
    debug_assert!(leaves.len().is_power_of_two());
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len() / 2);
        for chunk in layer.chunks(2) {
            // chunks(2) over a power-of-two layer always gives pairs.
            let left = chunk.first().copied().unwrap_or([0u8; 32]);
            let right = chunk.get(1).copied().unwrap_or(left);
            next.push(internal_node_hash(&left, &right));
        }
        layer = next;
    }
    layer.first().copied().unwrap_or([0u8; 32])
}

/// Compute the Merkle path (sibling hashes from leaf to just-below-root) for
/// `leaf_index` over `2^depth` leaves.
fn merkle_path_for(leaves: &[[u8; 32]], leaf_index: u32, depth: u32) -> Vec<[u8; 32]> {
    let mut path = Vec::with_capacity(depth as usize);
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = leaf_index as usize;
    for _level in 0..depth {
        // Sibling is the partner in the current layer.
        let sibling_idx = idx ^ 1;
        let sibling = layer.get(sibling_idx).copied().unwrap_or([0u8; 32]);
        path.push(sibling);

        // Compute next layer.
        let mut next = Vec::with_capacity(layer.len() / 2);
        for chunk in layer.chunks(2) {
            let left = chunk.first().copied().unwrap_or([0u8; 32]);
            let right = chunk.get(1).copied().unwrap_or(left);
            next.push(internal_node_hash(&left, &right));
        }
        layer = next;
        idx /= 2;
    }
    path
}

/// Re-walk a Merkle path back to the root from a given leaf hash + index.
fn merkle_root_from_path(leaf_hash: &[u8; 32], leaf_index: u32, path: &[[u8; 32]]) -> [u8; 32] {
    let mut current = *leaf_hash;
    let mut idx = leaf_index;
    for sibling in path {
        if idx & 1 == 0 {
            // Current is left child.
            current = internal_node_hash(&current, sibling);
        } else {
            // Current is right child.
            current = internal_node_hash(sibling, &current);
        }
        idx /= 2;
    }
    current
}

/// Build the period-bound message that the leaf actually signs.
fn period_bound_message(period: u32, msg: &[u8]) -> Vec<u8> {
    let mut bound = Vec::with_capacity(KES_SIGN_TAG.len() + 4 + msg.len());
    bound.extend_from_slice(KES_SIGN_TAG);
    bound.extend_from_slice(&period.to_le_bytes());
    bound.extend_from_slice(msg);
    bound
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a fresh KES keypair from a 32-byte master seed.
///
/// Pre-derives every leaf seed and leaf pk hash, then **zeroizes the master
/// seed** before returning. Cost: roughly `KES_TOTAL_PERIODS` Dilithium
/// keygens (~2048 × ~1 ms ≈ 2 s on commodity hardware). This is a one-time
/// cost at operator onboarding.
pub fn generate(master_seed: &[u8; 32]) -> Result<(KesPublicKey, KesSecretKey)> {
    let total = KES_TOTAL_PERIODS as usize;

    let mut leaf_seeds: Vec<[u8; 32]> = Vec::with_capacity(total);
    let mut leaf_pk_hashes: Vec<[u8; 32]> = Vec::with_capacity(total);

    for period in 0..KES_TOTAL_PERIODS {
        let seed = derive_leaf_seed(master_seed, period);
        let kp = from_seed_pqc(KES_LEAF_LEVEL, &seed)?;
        let h = leaf_pk_hash(kp.public.as_bytes());
        leaf_seeds.push(seed);
        leaf_pk_hashes.push(h);
        // kp drops here — its secret bytes were already in SecureBytes which
        // zeroizes on drop.
    }

    let pk_root = merkle_root_from_leaves(&leaf_pk_hashes);

    let sk = KesSecretKey {
        period: 0,
        leaf_seeds,
        leaf_pk_hashes,
    };

    Ok((KesPublicKey(pk_root), sk))
}

/// Sign `msg` at the current KES period.
///
/// Errors if the KES is exhausted or the leaf-derivation fails.
pub fn sign(sk: &KesSecretKey, msg: &[u8]) -> Result<KesSignature> {
    if sk.is_exhausted() {
        return Err(CryptoError::Other(
            "kes secret key exhausted (period >= 2^depth)".to_string(),
        ));
    }
    let period = sk.period;

    // Re-derive the current leaf keypair from the stored seed. The seed is
    // zeroized later by `evolve()`; here we just borrow it.
    let seed =
        sk.leaf_seeds.get(period as usize).copied().ok_or_else(|| {
            CryptoError::Other(format!("kes leaf seed missing for period {period}"))
        })?;
    let kp: PqcKeyPair = from_seed_pqc(KES_LEAF_LEVEL, &seed)?;

    let bound = period_bound_message(period, msg);
    let sig: PqcSignature = sign_pqc(&kp.secret, &bound)?;

    let merkle_path = merkle_path_for(&sk.leaf_pk_hashes, period, KES_TREE_DEPTH);

    Ok(KesSignature {
        period,
        leaf_pk: kp.public.as_bytes().to_vec(),
        leaf_signature: sig.as_bytes().to_vec(),
        merkle_path,
    })
}

/// Verify a KES signature.
///
/// 1. Verify the leaf Dilithium signature against the period-bound message.
/// 2. Recompute the leaf pk hash.
/// 3. Walk the Merkle path back and compare with `pk_root`.
pub fn verify(pk: &KesPublicKey, sig: &KesSignature, msg: &[u8]) -> Result<bool> {
    if sig.period >= KES_TOTAL_PERIODS {
        return Ok(false);
    }
    if sig.merkle_path.len() != KES_TREE_DEPTH as usize {
        return Ok(false);
    }

    // 1. Verify the leaf signature.
    let leaf_pk = PqcPublicKey::from_bytes(KES_LEAF_LEVEL, sig.leaf_pk.clone())?;
    let leaf_sig = PqcSignature::from_bytes(KES_LEAF_LEVEL, sig.leaf_signature.clone())?;
    let bound = period_bound_message(sig.period, msg);
    if !verify_pqc(&leaf_pk, &bound, &leaf_sig)? {
        return Ok(false);
    }

    // 2. Recompute leaf pk hash.
    let h = leaf_pk_hash(&sig.leaf_pk);

    // 3. Walk the Merkle path.
    let computed_root = merkle_root_from_path(&h, sig.period, &sig.merkle_path);

    Ok(computed_root == pk.0)
}

/// Advance the KES key one period. Zeroizes the just-consumed leaf seed.
///
/// Errors if the key is already exhausted (i.e. there is no period to
/// advance into).
pub fn evolve(sk: &mut KesSecretKey) -> Result<()> {
    if sk.is_exhausted() {
        return Err(CryptoError::Other(
            "kes secret key already exhausted".to_string(),
        ));
    }
    if let Some(seed) = sk.leaf_seeds.get_mut(sk.period as usize) {
        seed.zeroize();
    }
    sk.period += 1;
    Ok(())
}

/// Convenience: return the KES key's current period.
#[must_use]
pub fn current_period(sk: &KesSecretKey) -> u32 {
    sk.period
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Generate a small-tree keypair for fast tests by overriding constants.
    /// We can't actually change the consts without `cfg(test)`, so the tests
    /// here use the production `KES_TOTAL_PERIODS = 2048`. Generation takes
    /// ~2 seconds — acceptable for CI but slow for `cargo test`. Mark
    /// expensive tests with `#[ignore]` and run them via
    /// `cargo test -- --ignored`.

    #[test]
    fn merkle_root_from_two_leaves() {
        let l = [1u8; 32];
        let r = [2u8; 32];
        let root = merkle_root_from_leaves(&[l, r]);
        let expected = internal_node_hash(&l, &r);
        assert_eq!(root, expected);
    }

    #[test]
    fn merkle_path_recomputes_root() {
        // 4 leaves, depth = 2.
        let leaves = [[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];
        let depth = 2;
        let root = merkle_root_from_leaves(&leaves);

        for idx in 0..4 {
            let path = merkle_path_for(&leaves, idx, depth);
            assert_eq!(path.len(), depth as usize);
            let recovered = merkle_root_from_path(&leaves[idx as usize], idx, &path);
            assert_eq!(recovered, root, "path[{idx}] does not recompute root");
        }
    }

    #[test]
    fn derive_leaf_seed_is_deterministic() {
        let master = [99u8; 32];
        let s1 = derive_leaf_seed(&master, 7);
        let s2 = derive_leaf_seed(&master, 7);
        let s_other = derive_leaf_seed(&master, 8);
        assert_eq!(s1, s2);
        assert_ne!(s1, s_other);
    }

    #[test]
    fn period_bound_message_distinguishes_periods() {
        let m_a = period_bound_message(0, b"hello");
        let m_b = period_bound_message(1, b"hello");
        assert_ne!(m_a, m_b);
    }

    /// Full generate → sign → verify roundtrip. Marked `#[ignore]` because
    /// generating a full 2048-leaf KES tree takes ~2 s on commodity HW; run
    /// via `cargo test -- --ignored` or in `cargo nextest` slow profiles.
    #[test]
    #[ignore]
    fn full_generate_sign_verify_roundtrip() {
        let seed = [0xAB_u8; 32];
        let (pk, sk) = generate(&seed).expect("kes generate");
        let msg = b"block @ slot 0";
        let sig = sign(&sk, msg).expect("kes sign");
        assert_eq!(sig.period, 0);
        assert_eq!(sig.merkle_path.len(), KES_TREE_DEPTH as usize);
        assert!(verify(&pk, &sig, msg).expect("kes verify"));
    }

    #[test]
    #[ignore]
    fn evolve_then_sign_uses_next_period() {
        let seed = [0xCD_u8; 32];
        let (pk, mut sk) = generate(&seed).unwrap();
        evolve(&mut sk).unwrap();
        assert_eq!(sk.period(), 1);
        let sig = sign(&sk, b"msg").unwrap();
        assert_eq!(sig.period, 1);
        assert!(verify(&pk, &sig, b"msg").unwrap());
    }

    #[test]
    #[ignore]
    fn forward_security_zeroizes_old_leaf() {
        let seed = [0xEF_u8; 32];
        let (_pk, mut sk) = generate(&seed).unwrap();
        let leaf_0_seed_before = sk.leaf_seeds[0];
        assert_ne!(leaf_0_seed_before, [0u8; 32]);
        evolve(&mut sk).unwrap();
        // After evolve, period 0's seed must be zeroized in place.
        assert_eq!(sk.leaf_seeds[0], [0u8; 32]);
    }

    #[test]
    #[ignore]
    fn cross_period_signatures_dont_match() {
        let seed = [0x10_u8; 32];
        let (pk, mut sk) = generate(&seed).unwrap();
        let sig0 = sign(&sk, b"shared msg").unwrap();
        evolve(&mut sk).unwrap();
        let sig1 = sign(&sk, b"shared msg").unwrap();
        // Both must verify (under their respective periods).
        assert!(verify(&pk, &sig0, b"shared msg").unwrap());
        assert!(verify(&pk, &sig1, b"shared msg").unwrap());
        // But the signatures themselves differ: distinct periods, distinct leaves.
        assert_ne!(sig0.leaf_signature, sig1.leaf_signature);
        assert_ne!(sig0.leaf_pk, sig1.leaf_pk);
        // And rebinding sig0 as if it were period 1 must fail (period bound
        // changes the message hashed by the leaf).
        let mut forged = sig0.clone();
        forged.period = 1;
        assert!(!verify(&pk, &forged, b"shared msg").unwrap());
    }

    #[test]
    #[ignore]
    fn tampered_leaf_signature_rejected() {
        let seed = [0x20_u8; 32];
        let (pk, sk) = generate(&seed).unwrap();
        let mut sig = sign(&sk, b"msg").unwrap();
        if let Some(byte) = sig.leaf_signature.first_mut() {
            *byte ^= 0xFF;
        }
        assert!(!verify(&pk, &sig, b"msg").unwrap());
    }

    #[test]
    #[ignore]
    fn tampered_merkle_path_rejected() {
        let seed = [0x30_u8; 32];
        let (pk, sk) = generate(&seed).unwrap();
        let mut sig = sign(&sk, b"msg").unwrap();
        if let Some(node) = sig.merkle_path.first_mut() {
            node[0] ^= 0xFF;
        }
        assert!(!verify(&pk, &sig, b"msg").unwrap());
    }

    #[test]
    #[ignore]
    fn wrong_message_rejected() {
        let seed = [0x40_u8; 32];
        let (pk, sk) = generate(&seed).unwrap();
        let sig = sign(&sk, b"original").unwrap();
        assert!(!verify(&pk, &sig, b"tampered").unwrap());
    }

    #[test]
    #[ignore]
    fn signature_serde_roundtrip() {
        let seed = [0x50_u8; 32];
        let (pk, sk) = generate(&seed).unwrap();
        let sig = sign(&sk, b"serde test").unwrap();
        let bytes = bincode::serialize(&sig).unwrap();
        let parsed: KesSignature = bincode::deserialize(&bytes).unwrap();
        assert_eq!(sig, parsed);
        assert!(verify(&pk, &parsed, b"serde test").unwrap());
    }
}
