//! Stealth-address wire encoding (ADR-011 Faz 5).
//!
//! A QuantumVault stealth address carries the recipient's hybrid view
//! public key plus their Dilithium spend public key, so a sender can
//! produce KEM-encapsulated stealth outputs to them. The on-the-wire
//! representation is:
//!
//! ```text
//!     qvst1 <hex-of-bincode-payload>
//! ```
//!
//! The payload is a `bincode`-serialized [`StealthAddressPayload`]. Because
//! Dilithium and Kyber public keys are large, the resulting hex string is a
//! few KB — that is the PQC reality and is accepted as a trade-off.
//!
//! For display / clipboard convenience, [`fingerprint`] derives a short
//! `qvfp1…` form by hashing the public-key material. The fingerprint is **not**
//! a payable address; it identifies an address but cannot be sent to.

use crate::{WalletError, WalletResult};
use qv_crypto::{DilithiumLevel, HybridPublicKey, KyberLevel, PqcPublicKey};
use qv_privacy::StealthAddress;
use serde::{Deserialize, Serialize};

/// Bech32-like prefix that tags the full payable stealth address.
pub const ADDRESS_PREFIX: &str = "qvst1";

/// Prefix for the short, non-payable fingerprint form.
pub const FINGERPRINT_PREFIX: &str = "qvfp1";

/// Serializable wire representation of a [`StealthAddress`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthAddressPayload {
    /// Kyber parameter set (1, 3, or 5).
    pub kyber_level: u8,
    /// Dilithium parameter set used by the spend key (2, 3, or 5).
    pub dilithium_level: u8,
    /// X25519 view public key (32 bytes).
    pub x25519_pk: Vec<u8>,
    /// Kyber view public key bytes.
    pub kyber_pk: Vec<u8>,
    /// Dilithium spend public key bytes.
    pub spend_pk: Vec<u8>,
}

impl StealthAddressPayload {
    /// Snapshot the publishable bytes from a live `StealthAddress`.
    #[must_use]
    pub fn from_address(addr: &StealthAddress) -> Self {
        let kyber_level = match addr.view_pk.level {
            KyberLevel::Level1 => 1,
            KyberLevel::Level3 => 3,
            KyberLevel::Level5 => 5,
        };
        let dilithium_level = match addr.spend_pk.level() {
            DilithiumLevel::Level2 => 2,
            DilithiumLevel::Level3 => 3,
            DilithiumLevel::Level5 => 5,
        };
        Self {
            kyber_level,
            dilithium_level,
            x25519_pk: addr.view_pk.x25519.to_vec(),
            kyber_pk: addr.view_pk.kyber.clone(),
            spend_pk: addr.spend_pk.as_bytes().to_vec(),
        }
    }

    /// Rebuild a usable [`StealthAddress`] from the wire payload.
    ///
    /// Validates parameter levels and byte lengths.
    pub fn into_address(self) -> WalletResult<StealthAddress> {
        let kyber_level = match self.kyber_level {
            1 => KyberLevel::Level1,
            3 => KyberLevel::Level3,
            5 => KyberLevel::Level5,
            other => {
                return Err(WalletError::InvalidArg(format!(
                    "unknown Kyber level: {other}"
                )))
            }
        };
        let dilithium_level = match self.dilithium_level {
            2 => DilithiumLevel::Level2,
            3 => DilithiumLevel::Level3,
            5 => DilithiumLevel::Level5,
            other => {
                return Err(WalletError::InvalidArg(format!(
                    "unknown Dilithium level: {other}"
                )))
            }
        };
        let x25519_pk: [u8; 32] = self.x25519_pk.as_slice().try_into().map_err(|_| {
            WalletError::InvalidArg(format!(
                "x25519_pk must be 32 bytes (got {})",
                self.x25519_pk.len()
            ))
        })?;
        let view_pk = HybridPublicKey {
            x25519: x25519_pk,
            kyber: self.kyber_pk,
            level: kyber_level,
        };
        let spend_pk = PqcPublicKey::from_bytes(dilithium_level, self.spend_pk.as_slice())
            .map_err(|e| WalletError::Crypto(format!("spend_pk parse: {e}")))?;
        Ok(StealthAddress { view_pk, spend_pk })
    }
}

/// Encode a stealth address to its on-the-wire string form (`qvst1…`).
pub fn encode_address(addr: &StealthAddress) -> WalletResult<String> {
    let payload = StealthAddressPayload::from_address(addr);
    let bytes = bincode::serialize(&payload).map_err(WalletError::Bincode)?;
    Ok(format!("{}{}", ADDRESS_PREFIX, hex::encode(bytes)))
}

/// Decode a string-form stealth address (`qvst1…`) into a usable
/// [`StealthAddress`].
pub fn decode_address(s: &str) -> WalletResult<StealthAddress> {
    let trimmed = s.strip_prefix(ADDRESS_PREFIX).ok_or_else(|| {
        WalletError::InvalidArg(format!("address must start with `{ADDRESS_PREFIX}`"))
    })?;
    let bytes =
        hex::decode(trimmed).map_err(|e| WalletError::InvalidArg(format!("hex decode: {e}")))?;
    let payload: StealthAddressPayload =
        bincode::deserialize(&bytes).map_err(WalletError::Bincode)?;
    payload.into_address()
}

/// Short, human-readable fingerprint of a stealth address.
///
/// `SHA3-256("QuantumVault-StealthAddr-v1" || spend_pk || x25519 || kyber)`,
/// truncated to 20 bytes and prefixed with `qvfp1`. Use this for UI display
/// and clipboard ergonomics; it is **not** payable.
#[must_use]
pub fn fingerprint(addr: &StealthAddress) -> String {
    use qv_crypto::sha3_256;
    let mut input = Vec::with_capacity(
        "QuantumVault-StealthAddr-v1".len()
            + addr.spend_pk.as_bytes().len()
            + addr.view_pk.x25519.len()
            + addr.view_pk.kyber.len(),
    );
    input.extend_from_slice(b"QuantumVault-StealthAddr-v1");
    input.extend_from_slice(addr.spend_pk.as_bytes());
    input.extend_from_slice(&addr.view_pk.x25519);
    input.extend_from_slice(&addr.view_pk.kyber);
    let digest = sha3_256(&input);
    let mut s = String::with_capacity(FINGERPRINT_PREFIX.len() + 40);
    s.push_str(FINGERPRINT_PREFIX);
    s.push_str(&hex::encode(digest.get(..20).unwrap_or(&digest[..])));
    s
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_privacy::StealthKeys;

    fn fresh_address() -> StealthAddress {
        StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3)
            .unwrap()
            .address()
    }

    #[test]
    fn encode_decode_roundtrip() {
        let addr = fresh_address();
        let encoded = encode_address(&addr).unwrap();
        assert!(encoded.starts_with(ADDRESS_PREFIX));
        let back = decode_address(&encoded).unwrap();
        assert_eq!(back.view_pk.x25519, addr.view_pk.x25519);
        assert_eq!(back.view_pk.kyber, addr.view_pk.kyber);
        assert_eq!(back.view_pk.level, addr.view_pk.level);
        assert_eq!(back.spend_pk.as_bytes(), addr.spend_pk.as_bytes());
        assert_eq!(back.spend_pk.level(), addr.spend_pk.level());
    }

    #[test]
    fn decode_rejects_bad_prefix() {
        let addr = fresh_address();
        let bad = encode_address(&addr).unwrap().replacen(ADDRESS_PREFIX, "qvbad1", 1);
        let err = decode_address(&bad).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn decode_rejects_bad_hex() {
        let err = decode_address("qvst1ZZZZ").unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn fingerprint_is_short_and_stable() {
        let addr = fresh_address();
        let fp1 = fingerprint(&addr);
        let fp2 = fingerprint(&addr);
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with(FINGERPRINT_PREFIX));
        // 5-char prefix + 40 hex chars
        assert_eq!(fp1.len(), FINGERPRINT_PREFIX.len() + 40);
    }

    #[test]
    fn fingerprint_changes_with_address() {
        let a = fresh_address();
        let b = fresh_address();
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
