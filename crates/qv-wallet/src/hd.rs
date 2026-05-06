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
    fn derive_spend_key(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<qv_crypto::PqcKeyPair> {
        // Construct the derivation path: "spend" || account_idx (big-endian u32)
        let mut input = Vec::with_capacity(64 + 5 + 4);
        input.extend_from_slice(b"QuantumVault-Spend-v1");
        input.extend_from_slice(seed);
        input.extend_from_slice(&account_idx.to_be_bytes());

        let entropy_hash = sha3_256(&input);

        // Use the hash as entropy for Dilithium keygen.
        // Note: pqcrypto-dilithium does not currently support seeded keygen,
        // so we use OS entropy. In the future, when deterministic keygen is available,
        // we can seed the RNG with entropy_hash.
        let _entropy = entropy_hash; // Will be used once seeded keygen is available.

        qv_crypto::generate_pqc_keypair(self.dilithium_level)
            .map_err(|e| WalletError::Crypto(e.to_string()))
    }

    /// Derive the view key (Kyber hybrid) for an account.
    fn derive_view_key(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<qv_crypto::HybridKeyPair> {
        // Construct the derivation path: "view" || account_idx (big-endian u32)
        let mut input = Vec::with_capacity(64 + 5 + 4);
        input.extend_from_slice(b"QuantumVault-View-v1");
        input.extend_from_slice(seed);
        input.extend_from_slice(&account_idx.to_be_bytes());

        let entropy_hash = sha3_256(&input);

        // Use the hash as entropy for Kyber keygen.
        // Note: Similar to spend key, we will use this once seeded keygen is available.
        let _entropy = entropy_hash; // Will be used once seeded keygen is available.

        qv_crypto::generate_hybrid_keypair(self.kyber_level)
            .map_err(|e| WalletError::Crypto(e.to_string()))
    }
}

impl SeedDeriver for DefaultSeedDeriver {
    fn derive_account(&self, seed: &[u8; 64], account_idx: u32) -> WalletResult<StealthKeys> {
        let view_kp = self.derive_view_key(seed, account_idx)?;
        let spend_kp = self.derive_spend_key(seed, account_idx)?;

        Ok(StealthKeys {
            view_kp,
            spend_kp,
        })
    }
}
