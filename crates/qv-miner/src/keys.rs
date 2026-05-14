//! Key management for the stake pool operator.
//!
//! Operators manage three key pairs:
//! - **VRF key**: Ristretto255-VRF, used for slot leader election (per ADR-004).
//! - **KES key**: Sum-KES on Dilithium L3, used to sign block headers (per ADR-005).
//! - **Cold key**: Dilithium Level 3 (`qv_crypto`), used for pool registration
//!   and other rare critical operations.
//!
//! All three are now backed by real `qv_crypto` primitives (envanter
//! M-01..M-05 closed 2026-05-06). At-rest encryption (Argon2id + AES-GCM) is
//! tracked under envanter **M-04** and lives in [`save_encrypted`] /
//! [`load_encrypted`] — the file-on-disk format is currently a plain
//! bincode envelope and **must not** be used for production keys until
//! M-04 lands.

use crate::{MinerError, MinerResult};
use std::path::Path;

// ---------------------------------------------------------------------------
// VRF key — Ristretto255-VRF (qv_crypto::vrf)
// ---------------------------------------------------------------------------

/// Operator VRF keypair (Ristretto255).
///
/// Wraps `qv_crypto::VrfKeyPair`. The 32-byte public key is what the operator
/// registers on-chain via `StakePool.vrf_key`.
#[derive(Clone, Debug)]
pub struct VrfKeyPair {
    inner: qv_crypto::VrfKeyPair,
}

impl VrfKeyPair {
    /// Generate from OS entropy.
    pub fn generate() -> MinerResult<Self> {
        let inner = qv_crypto::VrfKeyPair::generate()
            .map_err(|e| MinerError::KeyGeneration(format!("vrf: {e}")))?;
        Ok(Self { inner })
    }

    /// Derive deterministically from a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> MinerResult<Self> {
        let inner = qv_crypto::VrfKeyPair::from_seed(seed)
            .map_err(|e| MinerError::KeyGeneration(format!("vrf from_seed: {e}")))?;
        Ok(Self { inner })
    }

    /// Public-key bytes (32 bytes; on-chain `StakePool.vrf_key`).
    pub fn public_bytes(&self) -> &[u8] {
        self.inner.public.as_bytes()
    }

    /// Convert into a `RistrettoVrfEvaluator` consumable by qv-consensus.
    pub fn into_evaluator(self) -> qv_consensus::RistrettoVrfEvaluator {
        qv_consensus::RistrettoVrfEvaluator::new(self.inner)
    }

    /// Borrow the underlying `qv_crypto::VrfKeyPair`.
    pub fn as_inner(&self) -> &qv_crypto::VrfKeyPair {
        &self.inner
    }
}

// ---------------------------------------------------------------------------
// KES key — Sum-KES on Dilithium L3 (qv_crypto::kes)
// ---------------------------------------------------------------------------

/// Operator KES keypair (Sum-KES on Dilithium, depth 11, N=2048 periods).
///
/// Wraps `qv_crypto::KesPublicKey` + `qv_crypto::KesSecretKey`. The 32-byte
/// `pk_root` is what the operator registers on-chain via `StakePool.kes_key`.
///
/// **Intentionally NOT `Clone`.** `KesSecretKey` owns leaf seeds that are
/// zeroized on drop; cloning would either duplicate that secret material
/// (forward-security violation) or silently produce different keys. If you
/// need shared access to a single `KesKeyPair` across tasks, wrap it in
/// `Arc<Mutex<...>>` (see qv-node `slot_ticker::with_kes_signing`).
pub struct KesKeyPair {
    pk: qv_crypto::KesPublicKey,
    sk: qv_crypto::KesSecretKey,
}

impl core::fmt::Debug for KesKeyPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "KesKeyPair(pk={:02x?}…, period={}, exhausted={})",
            &self.pk.as_bytes()[..4],
            self.sk.period(),
            self.sk.is_exhausted()
        )
    }
}

impl KesKeyPair {
    /// Generate from OS entropy: a 32-byte master seed is sampled and the
    /// full leaf-seed table is pre-derived.
    pub fn generate() -> MinerResult<Self> {
        // Sample 32 bytes from the OS.
        use rand::RngCore;
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        Self::from_seed(&seed)
    }

    /// Generate deterministically from a 32-byte master seed.
    pub fn from_seed(master_seed: &[u8; 32]) -> MinerResult<Self> {
        let (pk, sk) = qv_crypto::kes_generate(master_seed)
            .map_err(|e| MinerError::KeyGeneration(format!("kes: {e}")))?;
        Ok(Self { pk, sk })
    }

    /// 32-byte Merkle-root public key.
    pub fn public_bytes(&self) -> &[u8] {
        self.pk.as_bytes()
    }

    /// Borrow the typed `qv_crypto::KesPublicKey` for verification flows.
    pub fn public_key(&self) -> qv_crypto::KesPublicKey {
        self.pk
    }

    /// Currently-active KES period.
    pub fn period(&self) -> u32 {
        self.sk.period()
    }

    /// Sign `message` at the current period.
    pub fn sign(&self, message: &[u8]) -> MinerResult<qv_crypto::KesSignature> {
        qv_crypto::kes_sign(&self.sk, message)
            .map_err(|e| MinerError::SigningFailed(format!("kes sign: {e}")))
    }

    /// Advance to the next period; zeroizes the just-consumed leaf seed.
    pub fn evolve_to_next_period(&mut self) -> MinerResult<()> {
        qv_crypto::kes_evolve(&mut self.sk)
            .map_err(|e| MinerError::SigningFailed(format!("kes evolve: {e}")))
    }

    /// Borrow the secret state (used internally for sign/evolve).
    pub fn as_secret(&self) -> &qv_crypto::KesSecretKey {
        &self.sk
    }
}

// ---------------------------------------------------------------------------
// Cold key — Dilithium L3 (qv_crypto::pqc_sign)
// ---------------------------------------------------------------------------

/// Operator cold (Dilithium) keypair for pool registration and rare ops.
#[derive(Clone, Debug)]
pub struct ColdKeyPair {
    inner: qv_crypto::PqcKeyPair,
}

impl ColdKeyPair {
    /// Generate from OS entropy.
    pub fn generate() -> MinerResult<Self> {
        let inner = qv_crypto::generate_pqc_keypair(qv_crypto::DilithiumLevel::Level3)
            .map_err(|e| MinerError::KeyGeneration(format!("cold: {e}")))?;
        Ok(Self { inner })
    }

    /// Derive deterministically from a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> MinerResult<Self> {
        let inner = qv_crypto::from_seed_pqc(qv_crypto::DilithiumLevel::Level3, seed)
            .map_err(|e| MinerError::KeyGeneration(format!("cold from_seed: {e}")))?;
        Ok(Self { inner })
    }

    /// Public-key bytes (~1952 bytes for Dilithium L3).
    pub fn public_bytes(&self) -> &[u8] {
        self.inner.public.as_bytes()
    }

    /// Sign `message` with the cold key.
    pub fn sign(&self, message: &[u8]) -> MinerResult<qv_crypto::PqcSignature> {
        qv_crypto::sign_pqc(&self.inner.secret, message)
            .map_err(|e| MinerError::SigningFailed(format!("cold sign: {e}")))
    }

    /// Borrow the underlying typed keypair.
    pub fn as_inner(&self) -> &qv_crypto::PqcKeyPair {
        &self.inner
    }
}

// ---------------------------------------------------------------------------
// Bundle — operator keys
// ---------------------------------------------------------------------------

/// All operator keys, derived from a single 32-byte master seed.
///
/// `master_seed` is held in-memory so [`save_encrypted`] can persist it
/// after the operator generates keys via [`generate`] or restores them via
/// [`from_seed`] / [`load_encrypted`]. It is the **only** secret needed to
/// reconstruct the full `(vrf, kes, cold)` triple.
///
/// [`save_encrypted`]: OperatorKeys::save_encrypted
/// [`generate`]: OperatorKeys::generate
/// [`from_seed`]: OperatorKeys::from_seed
/// [`load_encrypted`]: OperatorKeys::load_encrypted
#[derive(Debug)]
pub struct OperatorKeys {
    /// 32-byte entropy from which `(vrf, kes, cold)` are deterministically
    /// derived. Persisted under Argon2id+AES-GCM encryption (envanter M-04).
    pub master_seed: [u8; 32],
    /// VRF key pair (Ristretto255-VRF, schnorrkel; ADR-004).
    pub vrf: VrfKeyPair,
    /// KES key pair (Sum-KES on Dilithium L3, depth 11; ADR-005).
    pub kes: KesKeyPair,
    /// Cold (Dilithium L3) key pair, FIPS 204 ML-DSA-65 (ADR-006).
    pub cold: ColdKeyPair,
}

impl OperatorKeys {
    /// Generate a fresh set of operator keys from OS entropy.
    ///
    /// Internally samples a random 32-byte master seed and delegates to
    /// [`from_seed`]; the seed is retained in `self.master_seed` so the
    /// keys can be re-encrypted to disk via [`save_encrypted`].
    ///
    /// [`from_seed`]: OperatorKeys::from_seed
    /// [`save_encrypted`]: OperatorKeys::save_encrypted
    pub fn generate() -> MinerResult<Self> {
        use rand::RngCore;
        let mut master = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut master);
        Self::from_seed(&master)
    }

    /// Derive all three keys deterministically from a 32-byte master seed.
    ///
    /// Each subkey uses a domain-separated child seed:
    ///   `vrf  = SHA3-256(master || "vrf")`
    ///   `kes  = SHA3-256(master || "kes")`
    ///   `cold = SHA3-256(master || "cold")`
    pub fn from_seed(master: &[u8; 32]) -> MinerResult<Self> {
        let derive = |tag: &[u8]| -> [u8; 32] {
            let mut input = Vec::with_capacity(32 + tag.len());
            input.extend_from_slice(master);
            input.extend_from_slice(tag);
            qv_crypto::sha3_256(&input)
        };

        Ok(Self {
            master_seed: *master,
            vrf: VrfKeyPair::from_seed(&derive(b"vrf"))?,
            kes: KesKeyPair::from_seed(&derive(b"kes"))?,
            cold: ColdKeyPair::from_seed(&derive(b"cold"))?,
        })
    }

    /// Decrypt a keystore at `path` using `password` and reconstruct all keys.
    ///
    /// The on-disk envelope holds `(master_seed, kes_period)` under
    /// Argon2id + AES-256-GCM (see [`crate::keystore`] for format details).
    /// After deriving keys via `from_seed`, the KES key is evolved
    /// `kes_period` times to restore the forward-secure pointer.
    pub fn load_encrypted(path: &Path, password: &str) -> MinerResult<Self> {
        let plaintext = crate::keystore::load(path, password)?;
        let mut keys = Self::from_seed(&plaintext.master_seed)?;

        // Replay forward evolution to restore the KES rotation state. Each
        // evolve advances by exactly one period; total cost is O(period)
        // hashes (cheap) — the heavy work was the initial 2048-leaf gen
        // inside `from_seed`.
        for _ in 0..plaintext.kes_period {
            keys.kes.evolve_to_next_period()?;
        }
        Ok(keys)
    }

    /// Encrypt and persist the master seed + current KES period to `path`.
    ///
    /// Argon2id derives a 32-byte key from `password`; AES-256-GCM seals the
    /// payload with a fresh per-save random salt + nonce. Wrong-password
    /// loads fail loudly via the GCM tag check.
    pub fn save_encrypted(&self, path: &Path, password: &str) -> MinerResult<()> {
        let plaintext = crate::keystore::OperatorKeystorePlaintext {
            version: 1,
            master_seed: self.master_seed,
            kes_period: self.kes.period(),
        };
        crate::keystore::save(path, &plaintext, password)
    }

    /// Advance the KES key one period (forward security: zeroize the
    /// just-consumed leaf seed).
    pub async fn rotate_kes(&mut self) -> MinerResult<()> {
        self.kes.evolve_to_next_period()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn vrf_key_generate() {
        let key = VrfKeyPair::generate().unwrap();
        assert_eq!(key.public_bytes().len(), 32);
    }

    #[test]
    fn vrf_key_from_seed_is_deterministic() {
        let k1 = VrfKeyPair::from_seed(&[42u8; 32]).unwrap();
        let k2 = VrfKeyPair::from_seed(&[42u8; 32]).unwrap();
        assert_eq!(k1.public_bytes(), k2.public_bytes());
    }

    #[test]
    #[ignore] // KES generation is ~2 s; run via `cargo test -- --ignored`.
    fn kes_key_generate_and_period() {
        let key = KesKeyPair::generate().unwrap();
        assert_eq!(key.public_bytes().len(), 32);
        assert_eq!(key.period(), 0);
    }

    #[test]
    #[ignore]
    fn kes_key_evolve_advances_period() {
        let mut key = KesKeyPair::from_seed(&[1u8; 32]).unwrap();
        assert_eq!(key.period(), 0);
        key.evolve_to_next_period().unwrap();
        assert_eq!(key.period(), 1);
        key.evolve_to_next_period().unwrap();
        assert_eq!(key.period(), 2);
    }

    #[test]
    #[ignore]
    fn kes_sign_verify_roundtrip() {
        let key = KesKeyPair::from_seed(&[2u8; 32]).unwrap();
        let pk = key.public_key();
        let sig = key.sign(b"block header bytes").unwrap();
        let valid = qv_crypto::kes_verify(&pk, &sig, b"block header bytes").unwrap();
        assert!(valid);
    }

    #[test]
    fn cold_key_generate() {
        let key = ColdKeyPair::generate().unwrap();
        // Dilithium Level 3 public key is 1952 bytes (per FIPS 204).
        assert_eq!(key.public_bytes().len(), 1952);
    }

    /// C-04 + C-06 closed via ADR-006 (ml-dsa swap, 2026-05-07) — runs by default.
    /// Pure Dilithium-only path (no KES); fast (~ms).
    #[test]
    fn cold_key_from_seed_is_deterministic() {
        let k1 = ColdKeyPair::from_seed(&[7u8; 32]).unwrap();
        let k2 = ColdKeyPair::from_seed(&[7u8; 32]).unwrap();
        assert_eq!(k1.public_bytes(), k2.public_bytes());
    }

    /// C-04 + C-06 closed via ADR-006 — runs by default. Fast Dilithium path.
    #[test]
    fn cold_key_sign_verify_roundtrip() {
        let key = ColdKeyPair::from_seed(&[8u8; 32]).unwrap();
        let sig = key.sign(b"register pool tx").unwrap();
        let valid =
            qv_crypto::verify_pqc(&key.as_inner().public, b"register pool tx", &sig).unwrap();
        assert!(valid);
    }

    /// `OperatorKeys::from_seed` invokes the full KES leaf-tree generation
    /// (~2s for the depth-11 / 2048-leaf Sum-KES). C-04/C-06 dependency was
    /// closed via ADR-006; this test now ignores purely on performance grounds.
    /// Run via `cargo test -- --ignored` when KES correctness is the focus.
    #[test]
    #[ignore]
    fn operator_keys_from_seed_is_deterministic() {
        let master = [0xAB_u8; 32];
        let k1 = OperatorKeys::from_seed(&master).unwrap();
        let k2 = OperatorKeys::from_seed(&master).unwrap();
        assert_eq!(k1.vrf.public_bytes(), k2.vrf.public_bytes());
        assert_eq!(k1.kes.public_bytes(), k2.kes.public_bytes());
        assert_eq!(k1.cold.public_bytes(), k2.cold.public_bytes());
    }

    /// M-04 closed 2026-05-07: master-seed level keystore round-trips.
    ///
    /// Marked `#[ignore]` because save+load each invoke the full KES
    /// leaf-tree generation (`from_seed` → `kes_generate`, ~2s). End-to-end
    /// correctness is exercised here when run with `--ignored`.
    #[test]
    #[ignore]
    fn keystore_save_load_roundtrip_preserves_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("operator.keystore");

        let original = OperatorKeys::from_seed(&[0x42u8; 32]).unwrap();
        let vrf_pk = original.vrf.public_bytes().to_vec();
        let kes_pk = original.kes.public_bytes().to_vec();
        let cold_pk = original.cold.public_bytes().to_vec();
        let master_seed = original.master_seed;

        original.save_encrypted(&path, "correct horse").unwrap();
        let loaded = OperatorKeys::load_encrypted(&path, "correct horse").unwrap();

        assert_eq!(loaded.master_seed, master_seed);
        assert_eq!(loaded.vrf.public_bytes(), vrf_pk.as_slice());
        assert_eq!(loaded.kes.public_bytes(), kes_pk.as_slice());
        assert_eq!(loaded.cold.public_bytes(), cold_pk.as_slice());
    }

    /// Wrong password must NOT decrypt — relies on AES-GCM tag mismatch.
    /// This test is fast: the save path runs `from_seed` (slow) but load
    /// fails before `from_seed` is called. Keep it slow-marked as a courtesy
    /// because of the save half.
    #[test]
    #[ignore]
    fn keystore_wrong_password_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("operator.keystore");

        let original = OperatorKeys::from_seed(&[0x99u8; 32]).unwrap();
        original.save_encrypted(&path, "right").unwrap();

        let res = OperatorKeys::load_encrypted(&path, "wrong");
        assert!(res.is_err(), "wrong password must not decrypt");
    }
}
