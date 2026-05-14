//! HD derivation for stealth keys from a seed.
//!
//! Uses HKDF-SHA3-256 to derive per-account entropy, then generates
//! Dilithium and hybrid Kyber keypairs from that entropy.
use crate::{WalletError, WalletResult};
use qv_crypto::{sha3_256, DilithiumLevel, KyberLevel};
use qv_privacy::StealthKeys;

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
    /// **Status (envanter C-04, REOPENED 2026-05-07):** would be deterministic
    /// once `qv_crypto::from_seed_pqc` is wired against `ml-dsa = "0.0.4"`
    /// (see envanter C-06). For now, this calls the stub which returns
    /// `Err` — wallet `init` will surface that error to the user. Keystore
    /// save / mnemonic flow still works; only the address derivation step
    /// fails until C-06 closes.
    fn derive_spend_key(
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
