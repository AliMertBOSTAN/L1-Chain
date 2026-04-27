//! Persistent consensus/ledger state storage.

use std::collections::BTreeMap;

use qv_consensus::{ChainEntry, Delegation, StakeDistribution, StakePool};
use qv_core::{Amount, BlockHash, Epoch, Hash256};
use serde::{Deserialize, Serialize};

use crate::kv::KvStore;
use crate::{decode, encode, StorageError, StorageResult};

const CHAIN_ENTRY_PREFIX: &[u8] = b"state:chain_entry:";
const CHAIN_TIP_KEY: &[u8] = b"state:chain_tip";
const LEDGER_STATE_KEY: &[u8] = b"state:ledger";
const EPOCH_SNAPSHOT_PREFIX: &[u8] = b"state:epoch_snapshot:";

/// Persisted ledger state for consensus accounting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerState {
    /// Registered stake pools.
    pub pools: Vec<StakePool>,
    /// Current delegations.
    pub delegations: Vec<Delegation>,
    /// Accrued rewards by recipient.
    pub reward_balances: BTreeMap<Hash256, Amount>,
}

/// Epoch-bound snapshot persisted for restart/recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochSnapshot {
    /// Epoch identifier.
    pub epoch: Epoch,
    /// Frozen stake distribution used by Praos in this epoch.
    pub stake_distribution: StakeDistribution,
    /// Chain tip hash at snapshot time.
    pub tip_hash: BlockHash,
}

/// Persistent state-store facade.
#[derive(Clone, Debug)]
pub struct StateStore<S: KvStore> {
    kv: S,
}

impl<S: KvStore> StateStore<S> {
    /// Create a new state-store backed by `kv`.
    #[must_use]
    pub fn new(kv: S) -> Self {
        Self { kv }
    }

    /// Persist one chain entry by hash.
    pub fn put_chain_entry(&self, entry: &ChainEntry) -> StorageResult<()> {
        self.kv
            .put(&Self::key_chain_entry(&entry.hash), &encode(entry)?)
    }

    /// Fetch one chain entry by hash.
    pub fn get_chain_entry(&self, hash: &BlockHash) -> StorageResult<Option<ChainEntry>> {
        let Some(bytes) = self.kv.get(&Self::key_chain_entry(hash))? else {
            return Ok(None);
        };
        Ok(Some(decode::<ChainEntry>(&bytes)?))
    }

    /// Persist the current best tip hash.
    pub fn set_tip_hash(&self, hash: BlockHash) -> StorageResult<()> {
        self.kv.put(CHAIN_TIP_KEY, &hash.to_bytes())
    }

    /// Read the current best tip hash.
    pub fn get_tip_hash(&self) -> StorageResult<Option<BlockHash>> {
        let Some(bytes) = self.kv.get(CHAIN_TIP_KEY)? else {
            return Ok(None);
        };

        Ok(Some(Self::decode_block_hash(&bytes)?))
    }

    /// Persist aggregate ledger state.
    pub fn put_ledger_state(&self, state: &LedgerState) -> StorageResult<()> {
        self.kv.put(LEDGER_STATE_KEY, &encode(state)?)
    }

    /// Read aggregate ledger state.
    pub fn get_ledger_state(&self) -> StorageResult<Option<LedgerState>> {
        let Some(bytes) = self.kv.get(LEDGER_STATE_KEY)? else {
            return Ok(None);
        };
        Ok(Some(decode::<LedgerState>(&bytes)?))
    }

    /// Persist one epoch snapshot.
    pub fn put_epoch_snapshot(&self, snapshot: &EpochSnapshot) -> StorageResult<()> {
        let key = Self::key_epoch_snapshot(snapshot.epoch);
        self.kv.put(&key, &encode(snapshot)?)
    }

    /// Read one epoch snapshot.
    pub fn get_epoch_snapshot(&self, epoch: Epoch) -> StorageResult<Option<EpochSnapshot>> {
        let key = Self::key_epoch_snapshot(epoch);
        let Some(bytes) = self.kv.get(&key)? else {
            return Ok(None);
        };
        Ok(Some(decode::<EpochSnapshot>(&bytes)?))
    }

    /// Read the latest persisted epoch snapshot, if any.
    pub fn latest_epoch_snapshot(&self) -> StorageResult<Option<EpochSnapshot>> {
        let snapshots = self.kv.scan_prefix(EPOCH_SNAPSHOT_PREFIX)?;
        if snapshots.is_empty() {
            return Ok(None);
        }

        let mut best_epoch: Option<Epoch> = None;
        let mut best_snapshot: Option<EpochSnapshot> = None;

        for (key, value) in snapshots {
            let epoch = Self::epoch_from_snapshot_key(&key)?;
            let snapshot = decode::<EpochSnapshot>(&value)?;

            if best_epoch.is_none() || best_epoch.is_some_and(|e| epoch > e) {
                best_epoch = Some(epoch);
                best_snapshot = Some(snapshot);
            }
        }

        Ok(best_snapshot)
    }

    fn key_chain_entry(hash: &BlockHash) -> Vec<u8> {
        let mut key = Vec::with_capacity(CHAIN_ENTRY_PREFIX.len() + BlockHash::LEN);
        key.extend_from_slice(CHAIN_ENTRY_PREFIX);
        key.extend_from_slice(hash.as_bytes());
        key
    }

    fn key_epoch_snapshot(epoch: Epoch) -> Vec<u8> {
        let mut key = Vec::with_capacity(EPOCH_SNAPSHOT_PREFIX.len() + 8);
        key.extend_from_slice(EPOCH_SNAPSHOT_PREFIX);
        key.extend_from_slice(&epoch.as_u64().to_be_bytes());
        key
    }

    fn decode_block_hash(bytes: &[u8]) -> StorageResult<BlockHash> {
        if bytes.len() != BlockHash::LEN {
            return Err(StorageError::Corrupted("invalid block hash length"));
        }

        let mut arr = [0u8; BlockHash::LEN];
        arr.copy_from_slice(bytes);
        Ok(BlockHash::from_bytes(arr))
    }

    fn epoch_from_snapshot_key(key: &[u8]) -> StorageResult<Epoch> {
        if !key.starts_with(EPOCH_SNAPSHOT_PREFIX) {
            return Err(StorageError::Corrupted("invalid epoch snapshot key prefix"));
        }

        let raw = &key[EPOCH_SNAPSHOT_PREFIX.len()..];
        if raw.len() != 8 {
            return Err(StorageError::Corrupted("invalid epoch snapshot key length"));
        }

        let mut epoch_bytes = [0u8; 8];
        epoch_bytes.copy_from_slice(raw);
        Ok(Epoch::from(u64::from_be_bytes(epoch_bytes)))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::collections::BTreeMap;

    use qv_consensus::{ChainEntry, Delegation, PoolId, StakeDistribution, StakePool};
    use qv_core::{Amount, BlockHash, Epoch, Hash256, Height, Slot};

    use crate::kv::MemoryKvStore;
    use crate::state_store::{EpochSnapshot, LedgerState, StateStore};

    fn pool(byte: u8, pledge: u64) -> StakePool {
        let vrf_key = vec![byte; 32];
        StakePool {
            id: PoolId::from_vrf_key(&vrf_key),
            vrf_key,
            kes_key: vec![byte.wrapping_add(1); 32],
            pledge: Amount::from_smallest_units(pledge),
            margin_num: 5,
            margin_den: 100,
            fixed_cost: Amount::from_smallest_units(340_000_000),
            active: true,
        }
    }

    #[test]
    fn chain_entry_and_tip_roundtrip() {
        let store = StateStore::new(MemoryKvStore::new());
        let entry = ChainEntry {
            hash: BlockHash::from_bytes([1u8; 32]),
            parent_hash: BlockHash::ZERO,
            height: Height::from(1),
            slot: Slot::from(3),
            producer_key_hash: Hash256::from_bytes([2u8; 32]),
        };

        store.put_chain_entry(&entry).unwrap();
        let got = store.get_chain_entry(&entry.hash).unwrap().unwrap();
        assert_eq!(got, entry);

        store.set_tip_hash(entry.hash).unwrap();
        assert_eq!(store.get_tip_hash().unwrap(), Some(entry.hash));
    }

    #[test]
    fn ledger_state_roundtrip() {
        let store = StateStore::new(MemoryKvStore::new());

        let stake_pool = pool(7, 1_000_000);
        let delegation = Delegation {
            delegator_id: Hash256::from_bytes([9u8; 32]),
            pool_id: stake_pool.id,
            amount: Amount::from_smallest_units(500_000),
        };

        let mut rewards = BTreeMap::new();
        rewards.insert(
            Hash256::from_bytes([9u8; 32]),
            Amount::from_smallest_units(42),
        );

        let state = LedgerState {
            pools: vec![stake_pool],
            delegations: vec![delegation],
            reward_balances: rewards,
        };

        store.put_ledger_state(&state).unwrap();
        let back = store.get_ledger_state().unwrap().unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn epoch_snapshot_roundtrip_and_latest() {
        let store = StateStore::new(MemoryKvStore::new());

        let p1 = pool(1, 1000);
        let p2 = pool(2, 2000);

        let dist1 = StakeDistribution::snapshot(Epoch::from(1), &[p1.clone()], &[]).unwrap();
        let dist2 = StakeDistribution::snapshot(Epoch::from(2), &[p1, p2], &[]).unwrap();

        let s1 = EpochSnapshot {
            epoch: Epoch::from(1),
            stake_distribution: dist1,
            tip_hash: BlockHash::from_bytes([0x11; 32]),
        };
        let s2 = EpochSnapshot {
            epoch: Epoch::from(2),
            stake_distribution: dist2,
            tip_hash: BlockHash::from_bytes([0x22; 32]),
        };

        store.put_epoch_snapshot(&s1).unwrap();
        store.put_epoch_snapshot(&s2).unwrap();

        let back = store.get_epoch_snapshot(Epoch::from(1)).unwrap().unwrap();
        assert_eq!(back, s1);

        let latest = store.latest_epoch_snapshot().unwrap().unwrap();
        assert_eq!(latest.epoch, Epoch::from(2));
        assert_eq!(latest.tip_hash, BlockHash::from_bytes([0x22; 32]));
    }
}
