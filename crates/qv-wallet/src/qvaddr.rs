//! `.qvaddr` file format + QR helpers for sharing stealth addresses
//! (ADR-011 Faz 5).
//!
//! A stealth address at our default parameter set (Kyber-3 + Dilithium-3)
//! is ~3.1 KB; the hex form is ~6.4 KB. Both copy-paste and a single QR
//! code are awkward at that size:
//!
//! - **Best for desktop / file transfer:** the `.qvaddr` JSON file produced
//!   by [`Qvaddr::save`] and read back by [`Qvaddr::load`].
//! - **Best for in-person / phone-to-desktop:** a 2-part QR sequence
//!   ([`address_to_qr_parts`] / [`address_from_qr_parts`]) — each QR carries
//!   a `QVADDR1:k/N:<HEX>` payload that the receiving wallet reassembles.
//! - **Quick visual identification:** a small QR of the
//!   short fingerprint ([`fingerprint`](crate::address::fingerprint)) —
//!   *not payable*, just for "is this the right person?" checks.

use crate::address::{decode_address, encode_address, fingerprint};
use crate::{WalletError, WalletResult};
use qrcode::render::svg;
use qrcode::render::unicode;
use qrcode::{EcLevel, QrCode};
use qv_privacy::StealthAddress;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Wire format identifier embedded in every `.qvaddr` file.
pub const QVADDR_FORMAT_V1: &str = "qvaddr-v1";

/// Magic prefix on every QR payload chunk: `QVADDR1:k/N:<hex>`.
pub const QVADDR_QR_PREFIX: &str = "QVADDR1:";

/// Default number of parts when chunking a full address into QR codes.
/// Two V40-L QR codes cover any Kyber-3 + Dilithium-3 stealth address with
/// comfortable margin. Lower levels could potentially fit in one but we
/// pick the simplest universal default.
pub const DEFAULT_QR_PARTS: usize = 2;

/// Self-describing payload of a `.qvaddr` file.
///
/// Only the `address` field is required for sending — everything else is
/// metadata so the receiving wallet can label the contact, refuse mismatched
/// fingerprints, or warn about unexpected parameter levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Qvaddr {
    /// File-format tag — currently always `"qvaddr-v1"`.
    pub format: String,
    /// Optional human-readable label set by the sender (e.g. "Alice").
    #[serde(default)]
    pub label: Option<String>,
    /// Full `qvst1…` stealth address.
    pub address: String,
    /// Convenience copy of [`crate::address::fingerprint`] for quick
    /// integrity checks without re-deriving.
    pub fingerprint: String,
    /// Kyber parameter level (1, 3, or 5).
    pub kyber_level: u8,
    /// Dilithium parameter level used by the spend key.
    pub dilithium_level: u8,
    /// Unix seconds at file-creation time.
    #[serde(default)]
    pub created_at: u64,
}

impl Qvaddr {
    /// Build a `.qvaddr` payload around the given stealth address.
    pub fn from_address(addr: &StealthAddress, label: Option<String>) -> WalletResult<Self> {
        let kyber_level = match addr.view_pk.level {
            qv_crypto::KyberLevel::Level1 => 1,
            qv_crypto::KyberLevel::Level3 => 3,
            qv_crypto::KyberLevel::Level5 => 5,
        };
        let dilithium_level = match addr.spend_pk.level() {
            qv_crypto::DilithiumLevel::Level2 => 2,
            qv_crypto::DilithiumLevel::Level3 => 3,
            qv_crypto::DilithiumLevel::Level5 => 5,
        };
        Ok(Self {
            format: QVADDR_FORMAT_V1.into(),
            label,
            address: encode_address(addr)?,
            fingerprint: fingerprint(addr),
            kyber_level,
            dilithium_level,
            created_at: now_unix_secs(),
        })
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> WalletResult<String> {
        serde_json::to_string_pretty(self).map_err(WalletError::Json)
    }

    /// Parse a `.qvaddr` JSON blob.
    pub fn from_json(s: &str) -> WalletResult<Self> {
        let q: Self = serde_json::from_str(s).map_err(WalletError::Json)?;
        if q.format != QVADDR_FORMAT_V1 {
            return Err(WalletError::InvalidArg(format!(
                "unsupported qvaddr format `{}` (expected `{}`)",
                q.format, QVADDR_FORMAT_V1
            )));
        }
        // Sanity: the embedded fingerprint must match the address it claims.
        let addr = decode_address(&q.address)?;
        let actual_fp = fingerprint(&addr);
        if actual_fp != q.fingerprint {
            return Err(WalletError::InvalidArg(format!(
                "qvaddr file fingerprint mismatch (file: {}, computed: {})",
                q.fingerprint, actual_fp
            )));
        }
        Ok(q)
    }

    /// Save this payload to `path` as pretty JSON.
    pub fn save(&self, path: &Path) -> WalletResult<()> {
        let s = self.to_json()?;
        std::fs::write(path, s).map_err(WalletError::Io)
    }

    /// Load and validate a payload from `path`.
    pub fn load(path: &Path) -> WalletResult<Self> {
        let s = std::fs::read_to_string(path).map_err(WalletError::Io)?;
        Self::from_json(&s)
    }

    /// Convenience — parse the embedded address into a usable
    /// [`StealthAddress`].
    pub fn to_stealth_address(&self) -> WalletResult<StealthAddress> {
        decode_address(&self.address)
    }
}

// ---------------------------------------------------------------------------
// QR splitting / reassembly
// ---------------------------------------------------------------------------

/// Split the on-the-wire address into `parts` QR payload strings of the
/// form `QVADDR1:k/N:<UPPERCASE_HEX>`. The HEX preserves the full
/// `qvst1…` text including the prefix (so the receiver can verify it).
///
/// Uppercase is used because QR's compact "alphanumeric" mode only allows
/// `0-9 A-Z $ % * + - . / : space`; lowercase hex needs the larger "byte"
/// mode and shrinks capacity per code.
pub fn address_to_qr_parts(address: &str, parts: usize) -> WalletResult<Vec<String>> {
    if parts == 0 {
        return Err(WalletError::InvalidArg("parts must be >= 1".into()));
    }
    let upper = address.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    let chunk_len = bytes.len().div_ceil(parts);
    let mut out = Vec::with_capacity(parts);
    for k in 0..parts {
        let start = k.saturating_mul(chunk_len);
        if start >= bytes.len() {
            break;
        }
        let end = start.saturating_add(chunk_len).min(bytes.len());
        let chunk = std::str::from_utf8(&bytes[start..end])
            .map_err(|e| WalletError::InvalidArg(format!("qr chunk utf8: {e}")))?;
        out.push(format!("{}{}/{}:{}", QVADDR_QR_PREFIX, k + 1, parts, chunk));
    }
    Ok(out)
}

/// Reassemble a full address string from a vector of QR payloads emitted
/// by [`address_to_qr_parts`]. Accepts the payloads in any order.
pub fn address_from_qr_parts(parts: &[String]) -> WalletResult<String> {
    if parts.is_empty() {
        return Err(WalletError::InvalidArg("no QR parts supplied".into()));
    }

    let mut total: Option<usize> = None;
    let mut indexed: Vec<(usize, String)> = Vec::with_capacity(parts.len());

    for p in parts {
        let body = p.strip_prefix(QVADDR_QR_PREFIX).ok_or_else(|| {
            WalletError::InvalidArg(format!("not a qvaddr QR payload: {p:.20}…"))
        })?;
        let (header, hex_chunk) = body.split_once(':').ok_or_else(|| {
            WalletError::InvalidArg("qvaddr QR payload missing `:` separator".into())
        })?;
        let (idx_str, total_str) = header.split_once('/').ok_or_else(|| {
            WalletError::InvalidArg("qvaddr QR header must be `<k>/<N>`".into())
        })?;
        let idx: usize = idx_str
            .parse()
            .map_err(|e| WalletError::InvalidArg(format!("qr index: {e}")))?;
        let n: usize = total_str
            .parse()
            .map_err(|e| WalletError::InvalidArg(format!("qr total: {e}")))?;
        if idx == 0 || idx > n {
            return Err(WalletError::InvalidArg(format!(
                "qvaddr QR index {idx} out of range 1..={n}"
            )));
        }
        match total {
            None => total = Some(n),
            Some(t) if t != n => {
                return Err(WalletError::InvalidArg(format!(
                    "qvaddr QR parts disagree on total: {t} vs {n}"
                )))
            }
            _ => {}
        }
        indexed.push((idx, hex_chunk.to_string()));
    }

    let total = total.unwrap_or(0);
    if total == 0 || indexed.len() != total {
        return Err(WalletError::InvalidArg(format!(
            "expected {total} QR parts, got {}",
            indexed.len()
        )));
    }
    indexed.sort_by_key(|(k, _)| *k);

    let mut joined = String::new();
    for (k, expected) in indexed.iter().zip(1..=total) {
        if k.0 != expected {
            return Err(WalletError::InvalidArg(format!(
                "qvaddr QR part {expected} is missing"
            )));
        }
        joined.push_str(&k.1);
    }

    // Address strings are case-sensitive by construction (`qvst1` lowercase
    // prefix + lowercase hex); restore the original case.
    let lower = joined.to_ascii_lowercase();
    // Validate by attempting a decode — guards against typos.
    let _ = decode_address(&lower)?;
    Ok(lower)
}

// ---------------------------------------------------------------------------
// QR rendering
// ---------------------------------------------------------------------------

/// Render an SVG QR code for arbitrary text payload. Used by the HTTP
/// `/api/wallet/*.svg` endpoints.
pub fn render_qr_svg(payload: &str) -> WalletResult<String> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|e| WalletError::InvalidArg(format!("qr build: {e}")))?;
    Ok(code
        .render::<svg::Color<'static>>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .dark_color(svg::Color("#0b1020"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// Render a QR code for terminal display using Unicode half-block characters.
pub fn render_qr_unicode(payload: &str) -> WalletResult<String> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|e| WalletError::InvalidArg(format!("qr build: {e}")))?;
    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    use qv_crypto::{DilithiumLevel, KyberLevel};
    use qv_privacy::StealthKeys;

    fn fresh_address() -> StealthAddress {
        StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3)
            .unwrap()
            .address()
    }

    #[test]
    fn qvaddr_roundtrips_through_json() {
        let addr = fresh_address();
        let q = Qvaddr::from_address(&addr, Some("alice".into())).unwrap();
        let json = q.to_json().unwrap();
        let back = Qvaddr::from_json(&json).unwrap();
        assert_eq!(back.address, q.address);
        assert_eq!(back.fingerprint, q.fingerprint);
        assert_eq!(back.label.as_deref(), Some("alice"));
        assert_eq!(back.kyber_level, 3);
        assert_eq!(back.dilithium_level, 3);
    }

    #[test]
    fn qvaddr_rejects_tampered_fingerprint() {
        let addr = fresh_address();
        let mut q = Qvaddr::from_address(&addr, None).unwrap();
        q.fingerprint = "qvfp1deadbeef".repeat(3); // mismatched
        let json = q.to_json().unwrap();
        let err = Qvaddr::from_json(&json).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn qvaddr_save_load_roundtrip() {
        let addr = fresh_address();
        let q = Qvaddr::from_address(&addr, Some("bob".into())).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contact.qvaddr");
        q.save(&path).unwrap();
        let loaded = Qvaddr::load(&path).unwrap();
        assert_eq!(loaded.address, q.address);
        assert_eq!(loaded.label.as_deref(), Some("bob"));
    }

    #[test]
    fn qr_parts_split_and_reassemble() {
        let addr = fresh_address();
        let s = encode_address(&addr).unwrap();
        let parts = address_to_qr_parts(&s, 2).unwrap();
        assert_eq!(parts.len(), 2);
        for (k, p) in parts.iter().enumerate() {
            let header = format!("{}{}/2:", QVADDR_QR_PREFIX, k + 1);
            assert!(p.starts_with(&header), "got: {}", &p[..p.len().min(40)]);
        }
        let back = address_from_qr_parts(&parts).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn qr_parts_reassemble_out_of_order() {
        let addr = fresh_address();
        let s = encode_address(&addr).unwrap();
        let parts = address_to_qr_parts(&s, 3).unwrap();
        // Shuffle
        let shuffled = vec![parts[2].clone(), parts[0].clone(), parts[1].clone()];
        let back = address_from_qr_parts(&shuffled).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn qr_parts_detect_missing_chunk() {
        let addr = fresh_address();
        let s = encode_address(&addr).unwrap();
        let mut parts = address_to_qr_parts(&s, 3).unwrap();
        parts.remove(1); // drop the middle chunk
        let err = address_from_qr_parts(&parts).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[test]
    fn render_unicode_qr_is_non_empty() {
        let s = render_qr_unicode("QVADDR1:1/1:HELLO").unwrap();
        assert!(s.contains('\n'));
        assert!(s.len() > 100);
    }

    #[test]
    fn render_svg_qr_contains_svg_root() {
        let s = render_qr_svg("QVADDR1:1/1:HELLO").unwrap();
        assert!(s.starts_with("<?xml") || s.contains("<svg"));
    }
}
