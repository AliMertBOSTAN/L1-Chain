//! Encrypted mempool — threshold Kyber decryption (ADR-003).
//!
//! Transactions enter the mempool encrypted under a **per-epoch committee
//! public key** generated via distributed key generation (DKG).  The slot
//! leader and a t-of-n committee collectively decrypt the batch at block
//! proposal time, preventing any single validator from front-running.
//!
//! # Design (Option 3 — Kyber Distributed KEM)
//!
//! 1. **Epoch start**: committee runs DKG → combined Kyber public key.
//! 2. **User**: encrypts tx with combined pk → `EncryptedTx`.
//! 3. **Gossip**: encrypted blob propagated, not the cleartext.
//! 4. **Slot leader**: collects shares from t-of-n committee, recovers
//!    per-tx shared secrets, decrypts batch.
//! 5. **Block**: includes decrypted txs in deterministic order.
//!
//! The real threshold Kyber DKG and share combining are **behind traits**
//! (`ThresholdDecryptor`) so production crypto can be swapped in without
//! changing pool logic.  A `MockThresholdDecryptor` enables testing.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use qv_core::{Epoch, TxId};
use qv_crypto::sha3_256;
use qv_crypto::threshold::{
    DecryptionShare as CryptoDecryptionShare, DkgThresholdDecryptor,
    ThresholdDecryptor as CryptoThresholdDecryptor, ThresholdPublicKey,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::MempoolError;

// ---------------------------------------------------------------------------
// Encrypted transaction wrapper
// ---------------------------------------------------------------------------

/// An encrypted transaction blob as received over the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedTx {
    /// Unique identifier — SHA3-256 of the ciphertext.
    pub id: TxId,
    /// The Kyber ciphertext (encapsulated shared secret).
    pub kem_ciphertext: Vec<u8>,
    /// The symmetrically encrypted transaction body (AES-256-GCM or similar).
    pub encrypted_body: Vec<u8>,
    /// Epoch this transaction was encrypted for.
    pub target_epoch: Epoch,
    /// Unix-epoch seconds when first observed.
    pub received_at: u64,
}

impl EncryptedTx {
    /// Wire size estimate.
    #[must_use]
    pub fn estimated_size(&self) -> usize {
        self.kem_ciphertext.len() + self.encrypted_body.len() + 40
    }
}

// ---------------------------------------------------------------------------
// Decryption share
// ---------------------------------------------------------------------------

/// A share of the threshold decryption contributed by one committee member.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecryptionShare {
    /// Which committee member produced this share (0-indexed).
    pub member_index: u32,
    /// The decryption share bytes (scheme-specific).
    pub share_bytes: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Threshold decryptor trait
// ---------------------------------------------------------------------------

/// Trait abstracting threshold decryption so the encrypted pool can be tested
/// without real Kyber DKG.
pub trait ThresholdDecryptor: Send + Sync {
    /// Attempt to decrypt an `EncryptedTx` given at least `t` shares.
    ///
    /// Returns the cleartext transaction bytes on success.
    fn decrypt(
        &self,
        encrypted_tx: &EncryptedTx,
        shares: &[DecryptionShare],
    ) -> Result<Vec<u8>, MempoolError>;

    /// The minimum number of shares (threshold) required to decrypt.
    fn threshold(&self) -> u32;

    /// The total committee size.
    fn committee_size(&self) -> u32;
}

/// Mock decryptor for tests — XORs the encrypted_body with the first share's bytes.
#[derive(Clone, Debug)]
pub struct MockThresholdDecryptor {
    /// Threshold `t`.
    pub t: u32,
    /// Committee size `n`.
    pub n: u32,
}

impl MockThresholdDecryptor {
    /// Create a mock with given t, n.
    #[must_use]
    pub fn new(t: u32, n: u32) -> Self {
        Self { t, n }
    }
}

impl ThresholdDecryptor for MockThresholdDecryptor {
    fn decrypt(
        &self,
        encrypted_tx: &EncryptedTx,
        shares: &[DecryptionShare],
    ) -> Result<Vec<u8>, MempoolError> {
        let share_count = u32::try_from(shares.len())
            .map_err(|_| MempoolError::Decryption("share count overflow".to_owned()))?;

        if share_count < self.t {
            return Err(MempoolError::InsufficientShares {
                got: share_count,
                need: self.t,
            });
        }

        // Mock: XOR the body with the first share's bytes (cycled).
        let key = &shares[0].share_bytes;
        if key.is_empty() {
            return Err(MempoolError::Decryption("empty share".to_owned()));
        }

        let plaintext: Vec<u8> = encrypted_tx
            .encrypted_body
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % key.len()])
            .collect();

        Ok(plaintext)
    }

    fn threshold(&self) -> u32 {
        self.t
    }

    fn committee_size(&self) -> u32 {
        self.n
    }
}

// ---------------------------------------------------------------------------
// DKG-backed envelope encryption (MP-01, ADR-003)
// ---------------------------------------------------------------------------
//
// Real threshold-encrypted mempool transaction layout — replaces the mock
// XOR scheme. Hybrid envelope: a 32-byte AES-256-GCM session key is wrapped
// under the committee's threshold public key using the ElGamal-style
// DKG decryptor in `qv-crypto::threshold`; the actual transaction body is
// encrypted under that session key with AES-256-GCM.
//
//   kem_ciphertext = ElGamal( committee_pk, aes_key, randomness )  // 64 B
//                  = C1 (32B) || C2 (32B)
//   encrypted_body = nonce (12B) || AES-256-GCM( aes_key, nonce, tx_body )
//
// Decryption requires t committee members to each contribute a share of
// `C1^{s_i}` (via `create_envelope_share` below); the slot leader combines
// the shares to recover `pk^r`, derives the mask `H(pk^r)`, XORs C2 with
// the mask to recover `aes_key`, then AES-GCM-decrypts `encrypted_body`.

/// Size of the ElGamal-style envelope ciphertext: `C1 (32B) || C2 (32B)`.
pub const ENVELOPE_CIPHERTEXT_BYTES: usize = 64;
/// AES-GCM nonce size (96 bits).
pub const AES_NONCE_BYTES: usize = 12;
/// AES-256 key size.
pub const AES_KEY_BYTES: usize = 32;

/// Encrypt a transaction body under the committee's threshold public key.
///
/// Returns `(kem_ciphertext, encrypted_body)` ready to plug into an
/// [`EncryptedTx`]. The fresh AES key is derived from `aes_key_seed`
/// (typically `OsRng`-generated bytes); the ElGamal randomness from
/// `randomness` (also fresh, must never be reused for the same committee
/// public key — randomness reuse leaks the AES key).
///
/// The randomness inputs are kept caller-controlled so the function stays
/// deterministic, testable, and unaffected by RNG choices. Production
/// senders should pass `OsRng`-filled buffers.
pub fn encrypt_envelope(
    committee_pk: &ThresholdPublicKey,
    tx_body: &[u8],
    aes_key_seed: &[u8; AES_KEY_BYTES],
    randomness: &[u8; AES_KEY_BYTES],
    nonce: &[u8; AES_NONCE_BYTES],
) -> Result<(Vec<u8>, Vec<u8>), MempoolError> {
    // 1. ElGamal-wrap the AES key under the committee pk.
    let kem_64 = DkgThresholdDecryptor::encrypt(committee_pk, aes_key_seed, randomness);

    // 2. AES-256-GCM the body with the AES key.
    let cipher = Aes256Gcm::new_from_slice(aes_key_seed)
        .map_err(|e| MempoolError::Decryption(format!("AES key init: {e}")))?;
    let n = Nonce::from_slice(nonce);
    let ct = cipher
        .encrypt(n, tx_body)
        .map_err(|e| MempoolError::Decryption(format!("AES-GCM encrypt: {e}")))?;

    // 3. Wire format: nonce || aes_gcm_ciphertext+tag.
    let mut encrypted_body = Vec::with_capacity(AES_NONCE_BYTES + ct.len());
    encrypted_body.extend_from_slice(nonce);
    encrypted_body.extend_from_slice(&ct);

    Ok((kem_64.to_vec(), encrypted_body))
}

/// Convenience wrapper around [`encrypt_envelope`] that draws AES key seed,
/// ElGamal randomness, and AES-GCM nonce from `OsRng`. The caller never
/// sees the secret material, so it can't leak it accidentally.
pub fn encrypt_envelope_random(
    committee_pk: &ThresholdPublicKey,
    tx_body: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), MempoolError> {
    let mut aes_key = [0u8; AES_KEY_BYTES];
    let mut randomness = [0u8; AES_KEY_BYTES];
    let mut nonce = [0u8; AES_NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut aes_key);
    rand::rngs::OsRng.fill_bytes(&mut randomness);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    encrypt_envelope(committee_pk, tx_body, &aes_key, &randomness, &nonce)
}

/// Produce this committee member's decryption share for an `EncryptedTx`.
///
/// Each participant runs this exactly once per encrypted tx and gossips
/// the share. The slot leader collects ≥ `t` shares and feeds them into
/// [`DkgEnvelopeDecryptor::decrypt`] to recover the cleartext body.
pub fn create_envelope_share(
    participant_id: u32,
    participant_key_share: &[u8; AES_KEY_BYTES],
    encrypted_tx: &EncryptedTx,
    threshold: u32,
) -> Result<DecryptionShare, MempoolError> {
    if encrypted_tx.kem_ciphertext.len() != ENVELOPE_CIPHERTEXT_BYTES {
        return Err(MempoolError::Decryption(format!(
            "kem_ciphertext must be {ENVELOPE_CIPHERTEXT_BYTES} bytes, got {}",
            encrypted_tx.kem_ciphertext.len()
        )));
    }
    let dec = DkgThresholdDecryptor::new(participant_id, *participant_key_share, threshold);
    let crypto_share = dec
        .create_decryption_share(&encrypted_tx.kem_ciphertext)
        .map_err(|e| MempoolError::Decryption(format!("create share: {e}")))?;
    Ok(DecryptionShare {
        member_index: crypto_share.participant_id,
        share_bytes: crypto_share.share_data,
    })
}

/// Production decryptor — combines DKG-derived threshold shares to recover
/// the per-tx AES-256-GCM session key and decrypt the body.
///
/// Holds no secret state; only the `(t, n)` parameters. The actual share
/// generation happens on each committee member's node and arrives via
/// gossip as opaque bytes.
#[derive(Clone, Debug)]
pub struct DkgEnvelopeDecryptor {
    /// Threshold `t`.
    pub t: u32,
    /// Committee size `n`.
    pub n: u32,
}

impl DkgEnvelopeDecryptor {
    /// Create a new envelope decryptor with the given `(t, n)`.
    #[must_use]
    pub fn new(t: u32, n: u32) -> Self {
        Self { t, n }
    }
}

impl ThresholdDecryptor for DkgEnvelopeDecryptor {
    fn decrypt(
        &self,
        encrypted_tx: &EncryptedTx,
        shares: &[DecryptionShare],
    ) -> Result<Vec<u8>, MempoolError> {
        // 1. Enough shares?
        let got = u32::try_from(shares.len())
            .map_err(|_| MempoolError::Decryption("share count overflow".to_owned()))?;
        if got < self.t {
            return Err(MempoolError::InsufficientShares { got, need: self.t });
        }

        // 2. Convert mempool DecryptionShare → qv-crypto DecryptionShare.
        let crypto_shares: Vec<CryptoDecryptionShare> = shares
            .iter()
            .map(|s| CryptoDecryptionShare::new(s.member_index, s.share_bytes.clone()))
            .collect();

        // 3. Validate the envelope layout.
        if encrypted_tx.kem_ciphertext.len() != ENVELOPE_CIPHERTEXT_BYTES {
            return Err(MempoolError::Decryption(format!(
                "kem_ciphertext must be {ENVELOPE_CIPHERTEXT_BYTES} bytes, got {}",
                encrypted_tx.kem_ciphertext.len()
            )));
        }
        if encrypted_tx.encrypted_body.len() < AES_NONCE_BYTES {
            return Err(MempoolError::Decryption(
                "encrypted_body shorter than AES-GCM nonce".into(),
            ));
        }

        // 4. Combine shares to recover the DH value `pk^r`. We build a
        //    throwaway DkgThresholdDecryptor just to access the
        //    `combine_shares` implementation — its `key_share` field is
        //    irrelevant for combining.
        let combiner = DkgThresholdDecryptor::new(0, [0u8; 32], self.t);
        let combined = combiner
            .combine_shares(&crypto_shares, self.t)
            .map_err(|e| MempoolError::Decryption(format!("combine: {e}")))?;
        if combined.len() < 32 {
            return Err(MempoolError::Decryption(
                "combined share shorter than 32 bytes".into(),
            ));
        }

        // 5. Mask = SHA3-256(combined), AES_key = C2 XOR mask.
        let combined_arr: [u8; 32] = {
            let mut a = [0u8; 32];
            a.copy_from_slice(&combined[..32]);
            a
        };
        let mask = sha3_256(&combined_arr);
        let mut aes_key = [0u8; AES_KEY_BYTES];
        let c2 = &encrypted_tx.kem_ciphertext[AES_KEY_BYTES..ENVELOPE_CIPHERTEXT_BYTES];
        for ((dst, c2_byte), mask_byte) in aes_key.iter_mut().zip(c2.iter()).zip(mask.iter()) {
            *dst = *c2_byte ^ *mask_byte;
        }

        // 6. AES-256-GCM decrypt the body.
        let cipher = Aes256Gcm::new_from_slice(&aes_key)
            .map_err(|e| MempoolError::Decryption(format!("AES key init: {e}")))?;
        let nonce = Nonce::from_slice(&encrypted_tx.encrypted_body[..AES_NONCE_BYTES]);
        let body_ct = &encrypted_tx.encrypted_body[AES_NONCE_BYTES..];
        cipher
            .decrypt(nonce, body_ct)
            .map_err(|e| MempoolError::Decryption(format!("AES-GCM decrypt: {e}")))
    }

    fn threshold(&self) -> u32 {
        self.t
    }

    fn committee_size(&self) -> u32 {
        self.n
    }
}

// ---------------------------------------------------------------------------
// Encrypted pool
// ---------------------------------------------------------------------------

/// Configuration for the encrypted pool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedPoolConfig {
    /// Maximum number of encrypted transactions per epoch.
    pub max_tx_count: usize,
    /// Maximum aggregate ciphertext bytes.
    pub max_pool_bytes: usize,
    /// Maximum age in seconds before an encrypted tx is dropped.
    pub max_age_secs: u64,
}

impl Default for EncryptedPoolConfig {
    fn default() -> Self {
        Self {
            max_tx_count: 50_000,
            max_pool_bytes: 128 * 1024 * 1024,
            max_age_secs: 3600,
        }
    }
}

/// Encrypted mempool — holds encrypted transaction blobs until the slot leader
/// collects threshold decryption shares and reveals the cleartext batch.
#[derive(Debug)]
pub struct EncryptedPool {
    config: EncryptedPoolConfig,
    /// Encrypted tx by id.
    by_id: HashMap<TxId, EncryptedTx>,
    /// Current epoch (only accept txs targeting this epoch).
    current_epoch: Epoch,
    /// Aggregate byte size.
    total_bytes: usize,
}

impl EncryptedPool {
    /// Create a new encrypted pool for the given epoch.
    #[must_use]
    pub fn new(config: EncryptedPoolConfig, epoch: Epoch) -> Self {
        Self {
            config,
            by_id: HashMap::new(),
            current_epoch: epoch,
            total_bytes: 0,
        }
    }

    /// Number of encrypted transactions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The current epoch this pool accepts transactions for.
    #[must_use]
    pub fn current_epoch(&self) -> Epoch {
        self.current_epoch
    }

    /// Advance to a new epoch, flushing all buffered encrypted transactions.
    pub fn advance_epoch(&mut self, new_epoch: Epoch) {
        self.by_id.clear();
        self.total_bytes = 0;
        self.current_epoch = new_epoch;
    }

    /// Add an encrypted transaction.
    pub fn add(&mut self, etx: EncryptedTx) -> Result<TxId, MempoolError> {
        if etx.target_epoch != self.current_epoch {
            return Err(MempoolError::WrongEpoch {
                got: etx.target_epoch,
                expected: self.current_epoch,
            });
        }

        if self.by_id.contains_key(&etx.id) {
            return Err(MempoolError::DuplicateTx(etx.id));
        }

        let etx_size = etx.estimated_size();

        if self.by_id.len() >= self.config.max_tx_count {
            return Err(MempoolError::PoolFull);
        }
        if self.total_bytes.saturating_add(etx_size) > self.config.max_pool_bytes {
            return Err(MempoolError::PoolFull);
        }

        let id = etx.id;
        self.total_bytes = self.total_bytes.saturating_add(etx_size);
        self.by_id.insert(id, etx);
        Ok(id)
    }

    /// Get an encrypted transaction by id.
    #[must_use]
    pub fn get(&self, id: &TxId) -> Option<&EncryptedTx> {
        self.by_id.get(id)
    }

    /// Remove an encrypted transaction by id.
    pub fn remove(&mut self, id: &TxId) -> Option<EncryptedTx> {
        let etx = self.by_id.remove(id)?;
        self.total_bytes = self.total_bytes.saturating_sub(etx.estimated_size());
        Some(etx)
    }

    /// Drain all encrypted transactions (for batch decryption).
    pub fn drain_all(&mut self) -> Vec<EncryptedTx> {
        self.total_bytes = 0;
        self.by_id.drain().map(|(_, v)| v).collect()
    }

    /// Evict expired entries.  Returns the count evicted.
    pub fn evict_expired(&mut self) -> usize {
        let cutoff = now_secs().saturating_sub(self.config.max_age_secs);
        let expired: Vec<TxId> = self
            .by_id
            .values()
            .filter(|e| e.received_at < cutoff)
            .map(|e| e.id)
            .collect();

        let count = expired.len();
        for id in expired {
            self.remove(&id);
        }
        count
    }

    /// Decrypt all buffered transactions using the provided threshold decryptor
    /// and shares.  Returns the cleartext bytes per `TxId`.
    ///
    /// Transactions that fail decryption are skipped (logged, not fatal).
    pub fn decrypt_batch<D: ThresholdDecryptor>(
        &mut self,
        decryptor: &D,
        shares: &[DecryptionShare],
    ) -> Result<Vec<(TxId, Vec<u8>)>, MempoolError> {
        let all = self.drain_all();
        let mut results = Vec::with_capacity(all.len());

        for etx in &all {
            match decryptor.decrypt(etx, shares) {
                Ok(plaintext) => {
                    results.push((etx.id, plaintext));
                }
                Err(e) => {
                    tracing::warn!(
                        tx_id = ?etx.id,
                        error = %e,
                        "failed to decrypt encrypted tx — skipping"
                    );
                }
            }
        }

        Ok(results)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use qv_core::{Epoch, TxId};

    use super::*;

    fn make_etx(marker: u8, epoch: Epoch) -> EncryptedTx {
        let body = vec![marker; 64];
        let id = TxId::from_bytes([marker; 32]);
        EncryptedTx {
            id,
            kem_ciphertext: vec![marker; 32],
            encrypted_body: body,
            target_epoch: epoch,
            received_at: now_secs(),
        }
    }

    #[test]
    fn add_and_get() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        let etx = make_etx(1, Epoch::from(1));
        let id = etx.id;

        pool.add(etx).unwrap();
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&id).is_some());
    }

    #[test]
    fn wrong_epoch_rejected() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        let etx = make_etx(2, Epoch::from(2)); // wrong epoch

        let err = pool.add(etx).unwrap_err();
        assert!(matches!(err, MempoolError::WrongEpoch { .. }));
    }

    #[test]
    fn duplicate_rejected() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        let etx = make_etx(3, Epoch::from(1));

        pool.add(etx.clone()).unwrap();
        let err = pool.add(etx).unwrap_err();
        assert!(matches!(err, MempoolError::DuplicateTx(_)));
    }

    #[test]
    fn advance_epoch_clears_pool() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        pool.add(make_etx(4, Epoch::from(1))).unwrap();
        assert_eq!(pool.len(), 1);

        pool.advance_epoch(Epoch::from(2));
        assert!(pool.is_empty());
        assert_eq!(pool.current_epoch(), Epoch::from(2));
    }

    #[test]
    fn decrypt_batch_with_mock() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        let decryptor = MockThresholdDecryptor::new(2, 3);

        // Encrypt: XOR body with key
        let key = vec![0xAB; 16];
        let plaintext = b"hello world 1234";
        let encrypted_body: Vec<u8> = plaintext
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % key.len()])
            .collect();

        let etx = EncryptedTx {
            id: TxId::from_bytes([5; 32]),
            kem_ciphertext: vec![0; 32],
            encrypted_body,
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };
        pool.add(etx).unwrap();

        let shares = vec![
            DecryptionShare {
                member_index: 0,
                share_bytes: key.clone(),
            },
            DecryptionShare {
                member_index: 1,
                share_bytes: key,
            },
        ];

        let results = pool.decrypt_batch(&decryptor, &shares).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(&results[0].1, plaintext);
    }

    #[test]
    fn insufficient_shares_error() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        let decryptor = MockThresholdDecryptor::new(3, 5);

        pool.add(make_etx(6, Epoch::from(1))).unwrap();

        let shares = vec![DecryptionShare {
            member_index: 0,
            share_bytes: vec![0xCC; 16],
        }];

        // Only 1 share but need 3 — decrypt_batch skips failing txs
        let results = pool.decrypt_batch(&decryptor, &shares).unwrap();
        assert!(results.is_empty()); // skipped due to insufficient shares
    }

    #[test]
    fn drain_all_empties_pool() {
        let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));
        pool.add(make_etx(7, Epoch::from(1))).unwrap();
        pool.add(make_etx(8, Epoch::from(1))).unwrap();

        let drained = pool.drain_all();
        assert_eq!(drained.len(), 2);
        assert!(pool.is_empty());
    }

    // ------------------------------------------------------------------
    // DKG-envelope (MP-01) — shape and error-path tests that DON'T need
    // a working Pedersen DKG (T-01 is still #[ignore]'d in qv-crypto).
    // The full DKG → encrypt → share → combine → decrypt cycle is
    // covered by `envelope_full_roundtrip_with_real_dkg` below, also
    // #[ignore]'d on T-01.
    // ------------------------------------------------------------------

    fn dummy_pk() -> ThresholdPublicKey {
        // A non-zero field element. We don't care that it isn't the
        // discrete log of any known secret for the shape tests; we only
        // need `encrypt_envelope` to produce well-formed bytes.
        ThresholdPublicKey::new([0xABu8; 32])
    }

    #[test]
    fn encrypt_envelope_shapes_are_well_formed() {
        let pk = dummy_pk();
        let body = b"hello threshold mempool";
        let aes_key = [0x01u8; AES_KEY_BYTES];
        let randomness = [0x02u8; 32];
        let nonce = [0x03u8; AES_NONCE_BYTES];
        let (kem, ct) = encrypt_envelope(&pk, body, &aes_key, &randomness, &nonce).unwrap();
        assert_eq!(kem.len(), ENVELOPE_CIPHERTEXT_BYTES);
        // body = 12-byte nonce + ciphertext (>= body.len()) + 16-byte tag.
        assert!(ct.len() >= AES_NONCE_BYTES + body.len() + 16);
        assert_eq!(&ct[..AES_NONCE_BYTES], &nonce);
    }

    #[test]
    fn encrypt_envelope_random_produces_distinct_outputs() {
        // Two random-RNG encryptions of the same body must differ in
        // both the kem ciphertext and the AES envelope (nonce + AES-GCM
        // randomization).
        let pk = dummy_pk();
        let body = b"same body, fresh randomness";
        let (k1, c1) = encrypt_envelope_random(&pk, body).unwrap();
        let (k2, c2) = encrypt_envelope_random(&pk, body).unwrap();
        assert_eq!(k1.len(), ENVELOPE_CIPHERTEXT_BYTES);
        assert_eq!(k2.len(), ENVELOPE_CIPHERTEXT_BYTES);
        assert_ne!(k1, k2, "kem ciphertext must rerandomize");
        assert_ne!(c1, c2, "AES envelope must rerandomize");
    }

    #[test]
    fn dkg_decryptor_rejects_insufficient_shares() {
        let pk = dummy_pk();
        let body = b"x";
        let (kem, ct) = encrypt_envelope_random(&pk, body).unwrap();
        let etx = EncryptedTx {
            id: TxId::from_bytes([9; 32]),
            kem_ciphertext: kem,
            encrypted_body: ct,
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };
        let d = DkgEnvelopeDecryptor::new(2, 3);
        let err = d.decrypt(&etx, &[]).unwrap_err();
        assert!(matches!(
            err,
            MempoolError::InsufficientShares { got: 0, need: 2 }
        ));
    }

    #[test]
    fn dkg_decryptor_rejects_malformed_envelope() {
        let mut etx = EncryptedTx {
            id: TxId::from_bytes([9; 32]),
            kem_ciphertext: vec![0u8; ENVELOPE_CIPHERTEXT_BYTES - 1], // short
            encrypted_body: vec![0u8; 64],
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };
        let d = DkgEnvelopeDecryptor::new(1, 1);
        let share = DecryptionShare {
            member_index: 1,
            share_bytes: vec![0u8; 32],
        };
        let err = d.decrypt(&etx, &[share.clone()]).unwrap_err();
        assert!(matches!(err, MempoolError::Decryption(_)));

        // Body shorter than the AES-GCM nonce — caught the same way.
        etx.kem_ciphertext = vec![0u8; ENVELOPE_CIPHERTEXT_BYTES];
        etx.encrypted_body = vec![0u8; AES_NONCE_BYTES - 1];
        let err = d.decrypt(&etx, &[share]).unwrap_err();
        assert!(matches!(err, MempoolError::Decryption(_)));
    }

    #[test]
    fn create_envelope_share_rejects_wrong_size_kem() {
        let etx = EncryptedTx {
            id: TxId::from_bytes([0; 32]),
            kem_ciphertext: vec![0u8; 16], // wrong
            encrypted_body: vec![0u8; 64],
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };
        let err = create_envelope_share(1, &[0u8; 32], &etx, 2).unwrap_err();
        assert!(matches!(err, MempoolError::Decryption(_)));
    }

    /// Full end-to-end exercise of the real DKG → encrypt → share →
    /// combine → decrypt cycle. Currently `#[ignore]` because the
    /// underlying Pedersen DKG (`qv_crypto::threshold::run_pedersen_dkg`)
    /// is gated on closing envanter **T-01** (Feldman VSS share
    /// verification). Once T-01 is fixed this test should pass without
    /// any change to the mempool side.
    #[test]
    #[ignore]
    fn envelope_full_roundtrip_with_real_dkg() {
        use qv_crypto::threshold::{run_pedersen_dkg, FeldmanVssParticipant};

        let threshold = 2u32;
        let total = 3u32;
        let participants: Vec<FeldmanVssParticipant> = (0..total)
            .map(|id| {
                let mut seed = [0u8; 32];
                seed[0] = id as u8;
                seed[4] = 0x77;
                FeldmanVssParticipant::new(id, threshold, total, &seed)
            })
            .collect();
        let dkg = run_pedersen_dkg(&participants).expect("DKG should succeed once T-01 closes");

        let body = b"transfer 100 QV to alice@stealth";
        let (kem, ct) = encrypt_envelope_random(&dkg.public_key, body).unwrap();
        let etx = EncryptedTx {
            id: TxId::from_bytes([0xEE; 32]),
            kem_ciphertext: kem,
            encrypted_body: ct,
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };

        // Each of `threshold` participants produces a share.
        let shares: Vec<DecryptionShare> = dkg
            .participant_shares
            .iter()
            .take(threshold as usize)
            .map(|s| create_envelope_share(s.index, &s.value, &etx, threshold).unwrap())
            .collect();

        let decryptor = DkgEnvelopeDecryptor::new(threshold, total);
        let plaintext = decryptor.decrypt(&etx, &shares).unwrap();
        assert_eq!(plaintext.as_slice(), body);
    }
}
