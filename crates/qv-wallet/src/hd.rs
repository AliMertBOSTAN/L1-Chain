//! HD derivation for stealth keys from a seed.
//!
//! Uses HKDF-SHA3-256 to derive per-account entropy, then generates
//! Dilithium and hybrid Kyber keypairs from that entropy.
use crate::{WalletError, WalletResult};
use qv_crypto::{sha3_256, DilithiumLevel, KyberLevel};
use qv_privacy::StealthKeys;

/// Well-known **devnet test mnemonic** (BIP-39, 24 words, checksum-valid).
///
/// This is the standard "abandon … art" test vector used widely across
/// PQC / EVM tooling. On `--network devnet`, the node's genesis allocates
/// funds to spend keys derived from this mnemonic; a fresh wallet that
/// imports the same phrase will therefore see those funds via
/// `qv_scanP2pkh` immediately (see ADR-011 / "plain-p2pkh köprüsü").
///
/// **Never use on mainnet** — the secret is public.
pub const DEVNET_TEST_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

pub trait SeedDeriver: Send + Sync {
    fn derive_account(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<StealthKeys>;
}

/// Default HD derivation using SHA3-256 KDF.
#[derive(Clone, Debug)]
pub struct DefaultSeedDeriver {
    kyber_level: KyberLevel,
    dilithium_level: DilithiumLevel,
}

impl DefaultSeedDeriver {
    /// Create a new deriver with specified security levels.
    #[must_use]
    pub fn new(kyber_level: KyberLevel, dilithium_level: DilithiumLevel) -> Self {
        Self {
            kyber_level,
            dilithium_level,
        }
    }

    /// Use default levels (Kyber Level3, Dilithium Level3).
    #[must_use]
    pub fn default_levels() -> Self {
        Self {
            kyber_level: KyberLevel::default(),
            dilithium_level: DilithiumLevel::default(),
        }
    }

    /// Derive the spend key (Dilithium) for an account.
    ///
    /// Deterministic per (seed, account_idx, dilithium_level): the path
    /// `"QuantumVault-Spend-v1" || seed || account_idx (big-endian u32)` is
    /// hashed with SHA3-256 to obtain the 32-byte ξ that FIPS 204 KeyGen
    /// requires (ADR-006 / envanter C-04 closed via ml-dsa 0.0.4).
    ///
    /// **Public** so external callers (most importantly the devnet genesis
    /// builder in `qv-node`) can derive the same spend keys without going
    /// through the heavier `derive_account` path, which also derives a
    /// view key (the view key uses OS entropy until C-05 closes).
    pub fn derive_spend_key(
        &self,
        seed: &[u8; 64],
        account_idx: u32,
    ) -> WalletResult<qv_crypto::PqcKeyPair> {
        // Path: "QuantumVault-Spend-v1" || seed || account_idx (big-endian u32)
        let mut input = Vec::with_capacity(64 + 22 + 4);
        input.extend_from_slice(b"QuantumVault-Spend-v1");
        input.extend_from_slice(seed);
        input.extend_from_slice(&account_idx.to_be_bytes());

        // SHA3-256 collapses the path into the 32-byte ξ that FIPS 204 KeyGen needs.
        let xi: [u8; 32] = sha3_256(&input);

        qv_crypto::from_seed_pqc(self.dilithium_level, &xi)
            .map_err(|e| WalletError::Crypto(e.to_string()))
    }

    /// Derive the view key (hybrid X25519 + Kyber) for an account.
    ///
    /// **Currently NOT fully deterministic** — `qv_crypto::generate_hybrid_keypair`
    /// uses OS entropy because the hybrid KEM crate (Kyber portion via
    /// `pqcrypto-kyber`) doesn't yet expose a seeded API. Tracked under new
    /// envanter ID **C-05** (qv-crypto Hybrid KEM seeded keygen). Until then,
    /// view keys are NOT reproducible from a wallet seed; the user must
    /// back up the view key separately or accept the operational risk.
    fn derive_view_key(
        &self,
        seed: &[u8; 64],
        account_idx: u32,
    ) -> WalletResult<qv_crypto::HybridKeyPair> {
        // Construct the derivation path: "view" || seed || account_idx (big-endian u32)
        let mut input = Vec::with_capacity(64 + 21 + 4);
        input.extend_from_slice(b"QuantumVault-View-v1");
        input.extend_from_slice(seed);
        input.extend_from_slice(&account_idx.to_be_bytes());

        let entropy_hash = sha3_256(&input);
        let _entropy = entropy_hash; // C-05 önkoşulu: seeded hybrid KEM keygen.

        qv_crypto::generate_hybrid_keypair(self.kyber_level)
            .map_err(|e| WalletError::Crypto(e.to_string()))
    }
}

impl SeedDeriver for DefaultSeedDeriver {
    fn derive_account(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<StealthKeys> {
        let view_kp = self.derive_view_key(seed, account_idx)?;
        let spend_kp = self.derive_spend_key(seed, account_idx)?;

        Ok(StealthKeys { view_kp, spend_kp })
    }
}

impl DefaultSeedDeriver {
    /// Generate a fresh hybrid (Kyber + X25519) view keypair using OS
    /// entropy. Exposed publicly so the keystore upgrade flow (envanter
    /// C-05) can persist it once and reuse on every unlock.
    ///
    /// **Not** deterministic from the wallet seed — Kyber doesn't expose
    /// a seeded API in our `pqcrypto-kyber` version. View keys are
    /// therefore persisted in the keystore alongside the mnemonic.
    pub fn generate_fresh_view_keypair(&self) -> WalletResult<qv_crypto::HybridKeyPair> {
        qv_crypto::generate_hybrid_keypair(self.kyber_level)
            .map_err(|e| WalletError::Crypto(e.to_string()))
    }

    /// Derive an account combining a deterministic spend key (from
    /// `seed`+`account_idx`) with a **caller-supplied** view keypair.
    ///
    /// Use this on every unlock so the persisted view key is reused
    /// instead of regenerated.
    pub fn derive_account_with_view(
        &self,
        seed: &[u8; 64],
        account_idx: u32,
        view_kp: qv_crypto::HybridKeyPair,
    ) -> WalletResult<StealthKeys> {
        let spend_kp = self.derive_spend_key(seed, account_idx)?;
        Ok(StealthKeys { view_kp, spend_kp })
    }
}
