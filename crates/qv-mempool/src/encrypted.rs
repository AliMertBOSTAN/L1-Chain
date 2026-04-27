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

use qv_core::{Epoch, TxId};
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
}
