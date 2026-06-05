//! Per-output **selective disclosure** (ADR-011 — privacy matrix
//! "Selective disclosure" row).
//!
//! A view-key export ([`crate::view_export`]) hands the auditor scanning
//! authority for **all** future incoming payments. Sometimes a wallet
//! owner wants to prove a single specific payment instead — e.g. "I
//! received the 50 000-unit invoice from Bob on tx 0xabc:0" without
//! exposing the rest of their incoming stream.
//!
//! That's what a `.qvdisclose` file is. It bundles:
//!
//! - the [`qv_privacy::view_key::DisclosureProof`] (shared_secret +
//!   optional amount + optional blinding factor + binding hash),
//! - the stealth-output components needed to verify it
//!   (kem_ciphertext + view_tag + kyber_level + onetime_pk_hash),
//! - the Dilithium spend public key (so the verifier can re-derive
//!   `compute_onetime_pk_hash`),
//! - the outpoint (`tx_id:index`) so the verifier can cross-check
//!   on-chain that the disclosed UTXO actually exists, and the amount,
//! - the file format tag so future versions stay distinguishable.
//!
//! The file is **fully self-contained** — verification needs nothing
//! beyond the file plus the verifier's own crypto code. (Optionally the
//! verifier can also query `qv_getUtxo` on the node to confirm the
//! outpoint is canonical.)
//!
//! # Privacy trade-off
//!
//! Multiple `.qvdisclose` files from the same wallet share the same
//! `spend_pk_hex`. A verifier seeing more than one CAN link them to
//! the same wallet — this matches the spend-time linkability noted in
//! ADR-011. If full per-output unlinkability is required, only ever
//! create ONE disclosure per spend public key (i.e. per wallet HD
//! account).

use crate::{WalletError, WalletResult};
use qv_crypto::{DilithiumLevel, HashDigest, KyberLevel, PqcPublicKey, SharedSecret};
use qv_privacy::confidential::MockCommitter;
use qv_privacy::stealth::StealthOutput;
use qv_privacy::view_key::DisclosureProof;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// File-format identifier.
pub const QVDISCLOSE_FORMAT_V1: &str = "qvdisclose-v1";

/// JSON payload of a `.qvdisclose` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisclosureFile {
    /// Format tag — always `"qvdisclose-v1"` for files this module writes.
    pub format: String,
    /// Optional human-readable label, e.g. "Invoice #42 / Acme Corp".
    #[serde(default)]
    pub label: Option<String>,

    // ---- Outpoint (informational + verifier cross-check) ----
    /// Hex-encoded `TxId` of the funding transaction.
    pub tx_id_hex: String,
    /// Output index inside the funding transaction.
    pub output_index: u32,

    // ---- Stealth-output payload (needed to verify the proof) ----
    /// Kyber parameter set (1, 3, or 5).
    pub kyber_level: u8,
    /// Dilithium parameter set used by the spend pk.
    pub dilithium_level: u8,
    /// Hybrid-KEM ciphertext that wrapped the shared secret (hex).
    pub kem_ciphertext_hex: String,
    /// 1-byte view tag from the on-chain `StealthInfo` (hex, 2 chars).
    pub view_tag_hex: String,
    /// One-time PK hash committed to by the locking script (hex).
    pub onetime_pk_hash_hex: String,
    /// Dilithium spend public key of the wallet (hex).
    pub spend_pk_hex: String,

    // ---- DisclosureProof contents ----
    /// Recovered shared secret (32 bytes, hex). Lets the verifier
    /// recompute the view tag + one-time PK hash and confirm ownership.
    pub shared_secret_hex: String,
    /// Disclosed plaintext amount (smallest units), if any.
    #[serde(default)]
    pub disclosed_amount: Option<u64>,
    /// Disclosed blinding factor (32 bytes, hex), if confidential
    /// amounts are in use and the discloser opted to open the commitment.
    #[serde(default)]
    pub disclosed_blinding_hex: Option<String>,
    /// Binding hash from [`DisclosureProof::binding_hash`] (hex).
    pub binding_hash_hex: String,

    /// Unix seconds at file creation.
    #[serde(default)]
    pub created_at: u64,
}

impl DisclosureFile {
    /// Pretty-printed JSON.
    pub fn to_json(&self) -> WalletResult<String> {
        serde_json::to_string_pretty(self).map_err(WalletError::Json)
    }

    /// Parse a `.qvdisclose` JSON blob and tag-check the version.
    pub fn from_json(s: &str) -> WalletResult<Self> {
        let v: Self = serde_json::from_str(s).map_err(WalletError::Json)?;
        if v.format != QVDISCLOSE_FORMAT_V1 {
            return Err(WalletError::InvalidArg(format!(
                "unsupported qvdisclose format `{}` (expected `{}`)",
                v.format, QVDISCLOSE_FORMAT_V1
            )));
        }
        Ok(v)
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> WalletResult<()> {
        std::fs::write(path, self.to_json()?).map_err(WalletError::Io)
    }

    /// Load and tag-check from disk.
    pub fn load(path: &Path) -> WalletResult<Self> {
        let s = std::fs::read_to_string(path).map_err(WalletError::Io)?;
        Self::from_json(&s)
    }

    /// Verify the disclosure file against its own embedded data.
    ///
    /// Returns `Ok(true)` iff every cross-check passes:
    ///
    /// 1. `kyber_level` and `dilithium_level` are recognised.
    /// 2. `spend_pk_hex` decodes to a valid Dilithium public key.
    /// 3. `view_tag_hex` is exactly one byte.
    /// 4. `onetime_pk_hash_hex` is exactly 32 bytes.
    /// 5. `shared_secret_hex` is 32 bytes; recomputing the view tag from
    ///    it matches `view_tag_hex`.
    /// 6. Recomputing `compute_onetime_pk_hash(shared_secret, spend_pk)`
    ///    matches `onetime_pk_hash_hex`.
    /// 7. The embedded binding hash matches what `DisclosureProof::verify`
    ///    derives from the (possibly-disclosed) amount + blinding.
    ///
    /// **What this does NOT check**: that the outpoint actually exists
    /// on-chain with the disclosed value. For that, query
    /// `qv_getUtxo(tx_id:output_index)` and compare its `value` to
    /// [`Self::disclosed_amount`].
    pub fn verify_self_contained(&self) -> WalletResult<bool> {
        // --- 1. Decode levels.
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

        // --- 2. Decode all the hex fields.
        let spend_pk_bytes = hex::decode(&self.spend_pk_hex)
            .map_err(|e| WalletError::InvalidArg(format!("spend_pk_hex: {e}")))?;
        let spend_pk = PqcPublicKey::from_bytes(dilithium_level, spend_pk_bytes)
            .map_err(|e| WalletError::Crypto(format!("spend_pk: {e}")))?;

        let view_tag_bytes = hex::decode(&self.view_tag_hex)
            .map_err(|e| WalletError::InvalidArg(format!("view_tag_hex: {e}")))?;
        if view_tag_bytes.len() != 1 {
            return Err(WalletError::InvalidArg(format!(
                "view_tag must be 1 byte (got {})",
                view_tag_bytes.len()
            )));
        }
        let view_tag = view_tag_bytes[0];

        let onetime_pk_hash_bytes = hex::decode(&self.onetime_pk_hash_hex)
            .map_err(|e| WalletError::InvalidArg(format!("onetime_pk_hash_hex: {e}")))?;
        let onetime_pk_hash: HashDigest =
            onetime_pk_hash_bytes.as_slice().try_into().map_err(|_| {
                WalletError::InvalidArg(format!(
                    "onetime_pk_hash must be 32 bytes (got {})",
                    onetime_pk_hash_bytes.len()
                ))
            })?;

        let shared_secret_bytes = hex::decode(&self.shared_secret_hex)
            .map_err(|e| WalletError::InvalidArg(format!("shared_secret_hex: {e}")))?;
        let shared_secret_arr: [u8; 32] =
            shared_secret_bytes.as_slice().try_into().map_err(|_| {
                WalletError::InvalidArg(format!(
                    "shared_secret must be 32 bytes (got {})",
                    shared_secret_bytes.len()
                ))
            })?;

        let kem_ciphertext = hex::decode(&self.kem_ciphertext_hex)
            .map_err(|e| WalletError::InvalidArg(format!("kem_ciphertext_hex: {e}")))?;

        let binding_hash_bytes = hex::decode(&self.binding_hash_hex)
            .map_err(|e| WalletError::InvalidArg(format!("binding_hash_hex: {e}")))?;
        let binding_hash: HashDigest = binding_hash_bytes.as_slice().try_into().map_err(|_| {
            WalletError::InvalidArg(format!(
                "binding_hash must be 32 bytes (got {})",
                binding_hash_bytes.len()
            ))
        })?;

        let disclosed_blinding_bytes = self
            .disclosed_blinding_hex
            .as_ref()
            .map(hex::decode)
            .transpose()
            .map_err(|e| WalletError::InvalidArg(format!("disclosed_blinding_hex: {e}")))?;

        // --- 3. Build the qv-privacy structs and delegate to the canonical
        //         verifier.
        let stealth_output = StealthOutput {
            kem_ciphertext,
            kyber_level: self.kyber_level,
            view_tag,
            onetime_pk_hash,
        };

        let blinding = disclosed_blinding_bytes
            .as_ref()
            .map(|b| -> WalletResult<[u8; 32]> {
                b.as_slice().try_into().map_err(|_| {
                    WalletError::InvalidArg("disclosed_blinding must be 32 bytes".into())
                })
            })
            .transpose()?
            .map(qv_privacy::confidential::BlindingFactor);

        let proof = DisclosureProof {
            shared_secret_bytes: shared_secret_arr,
            disclosed_amount: self.disclosed_amount,
            disclosed_blinding: blinding.as_ref().map(|b| *b.as_bytes()),
            binding_hash,
        };

        // We never carry an explicit Pedersen commitment in a
        // .qvdisclose file (qv-privacy::confidential is still mock —
        // envanter P-01). Pass `None` for the commitment; the verifier
        // exercises checks 1-3 above (view tag, pk hash, binding hash)
        // without entering the commitment-opening branch.
        let _ = kyber_level; // suppress unused (Hybrid level is read above).
        let committer = MockCommitter::new();
        proof
            .verify(&stealth_output, &spend_pk, None, &committer)
            .map_err(|e| WalletError::Privacy(e.to_string()))
    }
}

/// Builder helper — bundle every field needed by the verifier into a
/// `DisclosureFile`. Called from the CLI and HTTP handlers after
/// `scan_stealth` finds the UTXO and the user picks an amount.
#[allow(clippy::too_many_arguments)]
pub fn create_disclosure(
    tx_id_hex: &str,
    output_index: u32,
    kyber_level: KyberLevel,
    dilithium_level: DilithiumLevel,
    kem_ciphertext_hex: &str,
    view_tag: u8,
    onetime_pk_hash: &HashDigest,
    spend_pk: &PqcPublicKey,
    shared_secret: &SharedSecret,
    disclosed_amount: Option<u64>,
    label: Option<String>,
) -> WalletResult<DisclosureFile> {
    let kyber_level_u8 = match kyber_level {
        KyberLevel::Level1 => 1,
        KyberLevel::Level3 => 3,
        KyberLevel::Level5 => 5,
    };
    let dilithium_level_u8 = match dilithium_level {
        DilithiumLevel::Level2 => 2,
        DilithiumLevel::Level3 => 3,
        DilithiumLevel::Level5 => 5,
    };

    // Build the canonical proof — its `binding_hash` ties everything
    // together; recomputing it in `verify_self_contained` is what makes
    // the file tamper-evident.
    let proof = DisclosureProof::create(shared_secret, onetime_pk_hash, disclosed_amount, None);

    Ok(DisclosureFile {
        format: QVDISCLOSE_FORMAT_V1.to_string(),
        label,
        tx_id_hex: tx_id_hex.to_string(),
        output_index,
        kyber_level: kyber_level_u8,
        dilithium_level: dilithium_level_u8,
        kem_ciphertext_hex: kem_ciphertext_hex.to_string(),
        view_tag_hex: hex::encode([view_tag]),
        onetime_pk_hash_hex: hex::encode(onetime_pk_hash),
        spend_pk_hex: hex::encode(spend_pk.as_bytes()),
        shared_secret_hex: hex::encode(shared_secret.as_bytes()),
        disclosed_amount,
        disclosed_blinding_hex: None,
        binding_hash_hex: hex::encode(proof.binding_hash),
        created_at: now_unix_secs(),
    })
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
    use qv_privacy::stealth::{create_stealth_output, scan_output, StealthKeys};

    fn alice() -> StealthKeys {
        StealthKeys::generate(KyberLevel::Level3, DilithiumLevel::Level3).unwrap()
    }

    /// Full sender → recipient → discloser → auditor cycle.
    #[test]
    fn disclosure_file_roundtrip_verifies_with_amount() {
        let alice = alice();
        let addr = alice.address();

        // Bob (sender) creates a stealth output to Alice.
        let (output, _sender_ss) = create_stealth_output(&addr).unwrap();
        // Alice scans her own.
        let scan = scan_output(&alice, &output).unwrap().expect("our output");

        // Alice discloses (amount = 1000) on outpoint deadbeef...:0.
        let tx_id_hex = "ab".repeat(32);
        let file = create_disclosure(
            &tx_id_hex,
            0,
            KyberLevel::Level3,
            DilithiumLevel::Level3,
            &hex::encode(&output.kem_ciphertext),
            output.view_tag,
            &output.onetime_pk_hash,
            &alice.spend_kp.public,
            &scan.shared_secret,
            Some(1000),
            Some("test invoice".into()),
        )
        .unwrap();

        // JSON + disk roundtrip.
        let json = file.to_json().unwrap();
        let back = DisclosureFile::from_json(&json).unwrap();
        assert_eq!(back.tx_id_hex, tx_id_hex);
        assert_eq!(back.output_index, 0);
        assert_eq!(back.disclosed_amount, Some(1000));
        assert_eq!(back.label.as_deref(), Some("test invoice"));

        // Self-contained verification passes.
        assert!(back.verify_self_contained().unwrap());
    }

    #[test]
    fn disclosure_file_roundtrip_verifies_without_amount() {
        let alice = alice();
        let (output, _) = create_stealth_output(&alice.address()).unwrap();
        let scan = scan_output(&alice, &output).unwrap().unwrap();

        let file = create_disclosure(
            &hex::encode([0xCDu8; 32]),
            7,
            KyberLevel::Level3,
            DilithiumLevel::Level3,
            &hex::encode(&output.kem_ciphertext),
            output.view_tag,
            &output.onetime_pk_hash,
            &alice.spend_kp.public,
            &scan.shared_secret,
            None,
            None,
        )
        .unwrap();
        assert!(file.verify_self_contained().unwrap());
    }

    #[test]
    fn disclosure_file_rejects_tampered_shared_secret() {
        let alice = alice();
        let (output, _) = create_stealth_output(&alice.address()).unwrap();
        let scan = scan_output(&alice, &output).unwrap().unwrap();

        let mut file = create_disclosure(
            &hex::encode([0u8; 32]),
            0,
            KyberLevel::Level3,
            DilithiumLevel::Level3,
            &hex::encode(&output.kem_ciphertext),
            output.view_tag,
            &output.onetime_pk_hash,
            &alice.spend_kp.public,
            &scan.shared_secret,
            Some(42),
            None,
        )
        .unwrap();

        // Tamper the shared_secret to a random value.
        file.shared_secret_hex = hex::encode([0x99u8; 32]);
        assert!(!file.verify_self_contained().unwrap());
    }

    #[test]
    fn disclosure_file_rejects_unknown_format() {
        let alice = alice();
        let (output, _) = create_stealth_output(&alice.address()).unwrap();
        let scan = scan_output(&alice, &output).unwrap().unwrap();

        let mut file = create_disclosure(
            &hex::encode([0u8; 32]),
            0,
            KyberLevel::Level3,
            DilithiumLevel::Level3,
            &hex::encode(&output.kem_ciphertext),
            output.view_tag,
            &output.onetime_pk_hash,
            &alice.spend_kp.public,
            &scan.shared_secret,
            None,
            None,
        )
        .unwrap();
        file.format = "qvdisclose-v999".into();
        let json = serde_json::to_string(&file).unwrap();
        let err = DisclosureFile::from_json(&json).unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }
}
