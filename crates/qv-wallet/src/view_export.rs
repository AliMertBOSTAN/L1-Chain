//! View-key export & import for **audit-only stealth scanning**
//! (ADR-011 / `docs/ABSTRACT.md` privacy matrix → "View key audit" row).
//!
//! A wallet owner can dump their hybrid view keypair plus their Dilithium
//! spend **public** key into a `.qvview` file and hand it to a trusted
//! auditor (accountant, regulator, exchange compliance). With this file
//! the auditor can:
//!
//! - call `qv_scanStealth` against the node and see every incoming
//!   stealth payment to the wallet,
//! - decode amounts from the on-chain `TxOutput`,
//! - reconstruct the wallet's full balance history.
//!
//! What the auditor **cannot** do:
//!
//! - spend any UTXO — that requires the Dilithium **spend secret**,
//!   which is **never** in a `.qvview` file,
//! - sign on behalf of the wallet for governance / staking,
//! - learn the mnemonic.
//!
//! This module only handles the file format; the actual scanning happens
//! via [`crate::rpc_client::RpcClient::scan_stealth_with_view_key`] which
//! consumes the imported keypair without ever touching a keystore.
//!
//! # On-disk format
//!
//! ```json
//! {
//!   "format": "qvview-v1",
//!   "label": "Alice — audit (2026 Q2)",
//!   "kyber_level": 3,
//!   "dilithium_level": 3,
//!   "x25519_pk_hex": "...",
//!   "x25519_sk_hex": "...",
//!   "kyber_pk_hex":  "...",
//!   "kyber_sk_hex":  "...",
//!   "spend_pk_hex":  "...",
//!   "created_at": 1716405000
//! }
//! ```
//!
//! Mirrors the wire format used by [`qv-node`'s `StealthViewKey`] RPC
//! payload so the file can be shipped to the auditor's RPC client
//! verbatim.

use crate::{WalletError, WalletResult};
use qv_crypto::{DilithiumLevel, HybridKeyPair, KyberLevel, PqcPublicKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Current file-format identifier.
pub const QVVIEW_FORMAT_V1: &str = "qvview-v1";

/// JSON payload of a `.qvview` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewKeyExport {
    /// Format tag — always `"qvview-v1"` for files this module writes.
    pub format: String,
    /// Optional human-readable label, e.g. "Alice — Q2 audit".
    #[serde(default)]
    pub label: Option<String>,
    /// Kyber parameter set (1, 3, or 5).
    pub kyber_level: u8,
    /// Dilithium parameter set used by the spend public key.
    pub dilithium_level: u8,
    /// X25519 public key (32 bytes), hex.
    pub x25519_pk_hex: String,
    /// X25519 secret key (32 bytes), hex.
    pub x25519_sk_hex: String,
    /// Kyber public key, hex.
    pub kyber_pk_hex: String,
    /// Kyber secret key, hex.
    pub kyber_sk_hex: String,
    /// Dilithium spend public key, hex.
    pub spend_pk_hex: String,
    /// Unix seconds at file creation.
    #[serde(default)]
    pub created_at: u64,
}

impl ViewKeyExport {
    /// Snapshot a live hybrid view keypair + Dilithium spend public key
    /// into the wire/file form.
    pub fn from_keys(
        view_kp: &HybridKeyPair,
        spend_pk: &PqcPublicKey,
        label: Option<String>,
    ) -> Self {
        let kyber_level = match view_kp.public.level {
            KyberLevel::Level1 => 1,
            KyberLevel::Level3 => 3,
            KyberLevel::Level5 => 5,
        };
        let dilithium_level = match spend_pk.level() {
            DilithiumLevel::Level2 => 2,
            DilithiumLevel::Level3 => 3,
            DilithiumLevel::Level5 => 5,
        };
        Self {
            format: QVVIEW_FORMAT_V1.to_string(),
            label,
            kyber_level,
            dilithium_level,
            x25519_pk_hex: hex::encode(view_kp.public.x25519),
            x25519_sk_hex: hex::encode(view_kp.x25519_secret_bytes()),
            kyber_pk_hex: hex::encode(&view_kp.public.kyber),
            kyber_sk_hex: hex::encode(view_kp.kyber_secret_bytes()),
            spend_pk_hex: hex::encode(spend_pk.as_bytes()),
            created_at: now_unix_secs(),
        }
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> WalletResult<String> {
        serde_json::to_string_pretty(self).map_err(WalletError::Json)
    }

    /// Parse a `.qvview` JSON blob.
    pub fn from_json(s: &str) -> WalletResult<Self> {
        let v: Self = serde_json::from_str(s).map_err(WalletError::Json)?;
        if v.format != QVVIEW_FORMAT_V1 {
            return Err(WalletError::InvalidArg(format!(
                "unsupported qvview format `{}` (expected `{}`)",
                v.format, QVVIEW_FORMAT_V1
            )));
        }
        Ok(v)
    }

    /// Write to disk.
    pub fn save(&self, path: &Path) -> WalletResult<()> {
        let s = self.to_json()?;
        std::fs::write(path, s).map_err(WalletError::Io)
    }

    /// Load and validate from disk.
    pub fn load(path: &Path) -> WalletResult<Self> {
        let s = std::fs::read_to_string(path).map_err(WalletError::Io)?;
        Self::from_json(&s)
    }

    /// Rebuild a usable `(HybridKeyPair, PqcPublicKey)` pair. Validates
    /// byte lengths against the declared parameter levels.
    pub fn into_keys(self) -> WalletResult<(HybridKeyPair, PqcPublicKey)> {
        let kyber_level = match self.kyber_level {
            1 => KyberLevel::Level1,
            3 => KyberLevel::Level3,
            5 => KyberLevel::Level5,
            other => {
                return Err(WalletError::InvalidArg(format!(
                    "unknown Kyber level: {other}"
                )));
            }
        };
        let dilithium_level = match self.dilithium_level {
            2 => DilithiumLevel::Level2,
            3 => DilithiumLevel::Level3,
            5 => DilithiumLevel::Level5,
            other => {
                return Err(WalletError::InvalidArg(format!(
                    "unknown Dilithium level: {other}"
                )));
            }
        };

        let x25519_pk_bytes =
            hex::decode(&self.x25519_pk_hex).map_err(|e| WalletError::InvalidArg(format!("x25519_pk_hex: {e}")))?;
        let x25519_pk: [u8; 32] = x25519_pk_bytes.as_slice().try_into().map_err(|_| {
            WalletError::InvalidArg(format!(
                "x25519_pk_hex must decode to 32 bytes (got {})",
                x25519_pk_bytes.len()
            ))
        })?;
        let x25519_sk = hex::decode(&self.x25519_sk_hex)
            .map_err(|e| WalletError::InvalidArg(format!("x25519_sk_hex: {e}")))?;
        let kyber_pk =
            hex::decode(&self.kyber_pk_hex).map_err(|e| WalletError::InvalidArg(format!("kyber_pk_hex: {e}")))?;
        let kyber_sk =
            hex::decode(&self.kyber_sk_hex).map_err(|e| WalletError::InvalidArg(format!("kyber_sk_hex: {e}")))?;

        let view_kp =
            HybridKeyPair::from_raw_parts(kyber_level, x25519_pk, x25519_sk, kyber_pk, kyber_sk)
                .map_err(|e| WalletError::Crypto(format!("view keypair: {e}")))?;

        let spend_pk_bytes = hex::decode(&self.spend_pk_hex)
            .map_err(|e| WalletError::InvalidArg(format!("spend_pk_hex: {e}")))?;
        let spend_pk = PqcPublicKey::from_bytes(dilithium_level, spend_pk_bytes)
            .map_err(|e| WalletError::Crypto(format!("spend_pk: {e}")))?;

        Ok((view_kp, spend_pk))
    }
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_privacy::StealthKeys;

    fn fresh_keys() -> StealthKeys {
        StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).unwrap()
    }

    #[test]
    fn export_roundtrip_through_json_and_disk() {
        let keys = fresh_keys();
        let exp = ViewKeyExport::from_keys(
            &keys.view_kp,
            &keys.spend_kp.public,
            Some("alice — Q2".into()),
        );
        let json = exp.to_json().unwrap();
        let back = ViewKeyExport::from_json(&json).unwrap();
        assert_eq!(back.kyber_level, 3);
        assert_eq!(back.dilithium_level, 3);
        assert_eq!(back.label.as_deref(), Some("alice — Q2"));

        // Disk roundtrip too.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alice.qvview");
        exp.save(&path).unwrap();
        let loaded = ViewKeyExport::load(&path).unwrap();
        assert_eq!(loaded.x25519_pk_hex, exp.x25519_pk_hex);
    }

    #[test]
    fn into_keys_reconstructs_a_functional_view_keypair() {
        // Audit invariant — the exported view key must be able to
        // decapsulate a fresh KEM ciphertext targeting the original
        // public key, exactly like the owner's wallet would.
        let keys = fresh_keys();
        let exp = ViewKeyExport::from_keys(&keys.view_kp, &keys.spend_kp.public, None);
        let (view_kp, spend_pk) = exp.into_keys().unwrap();

        // Encapsulate against the ORIGINAL public key, decapsulate with
        // the imported one — secrets must match byte-for-byte.
        let (ct, ss_orig) = qv_crypto::encapsulate_hybrid(&keys.view_kp.public).unwrap();
        let ss_audit = qv_crypto::decapsulate_hybrid(&view_kp, &ct).unwrap();
        assert_eq!(ss_orig, ss_audit);
        assert_eq!(spend_pk.as_bytes(), keys.spend_kp.public.as_bytes());
    }

    #[test]
    fn from_json_rejects_unknown_format() {
        let mut exp = ViewKeyExport::from_keys(
            &fresh_keys().view_kp,
            &fresh_keys().spend_kp.public,
            None,
        );
        exp.format = "qvview-v999".into();
        let json = exp.to_json().unwrap();
        let err = ViewKeyExport::from_json(&json).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn into_keys_rejects_unknown_kyber_level() {
        let mut exp = ViewKeyExport::from_keys(
            &fresh_keys().view_kp,
            &fresh_keys().spend_kp.public,
            None,
        );
        exp.kyber_level = 9;
        let err = exp.into_keys().unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }
}
