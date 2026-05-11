//! Integration tests for `qv-storage`.
//!
//! These tests exercise **cross-module** workflows: blocks flowing into the
//! block store, their transaction effects applied to the UTXO store, and
//! chain/epoch metadata persisted through the state store.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;

use qv_consensus::{
    ChainEntry, Delegation, PoolId, StakeDistribution, StakePool,
};
use qv_core::{
    Amount, Block, BlockHash, BlockHeader, Epoch, Hash256, Height, OutPoint, Script, Slot,
    Timestamp, Transaction, TxId, TxInput, TxOutput,
};
use qv_storage::block_store::BlockStore;
use qv_storage::kv::MemoryKvStore;
use qv_storage::state_store::{EpochSnapshot, LedgerState, StateStore};
use qv_storage::utxo_store::UtxoStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tx_output(value: u64, marker: u8) -> TxOutput {
    TxOutput::new(
        Amount::from_smallest_units(value),
        Script::new(vec![marker]),
    )
}

fn coinbase_block(height: u64, marker: u8, value: u64) -> Block {
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(
            TxId::from_bytes([marker; 32]),
            0,
        ))],
        vec![tx_output(value, marker)],
    );

    let mut header = BlockHeader::genesis_template();
    header.height = Height::from(height);
    header.prev_hash = BlockHash::from_bytes([marker; 32]);
    header.timestamp = Timestamp::from_unix_secs(height);

    let mut block = Block::new(header, vec![tx]);
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    block
}

fn transfer_block(prev_outpoint: OutPoint, marker: u8, value_out: u64, height: u64) -> Block {
    let tx = Transaction::new(
        vec![TxInput::new(prev_outpoint)],
        vec![tx_output(value_out, marker)],
    );

    let mut header = BlockHeader::genesis_template();
    header.height = Height::from(height);
    header.prev_hash = BlockHash::from_bytes([marker; 32]);
    header.timestamp = Timestamp::from_unix_secs(height);

    let mut block = Block::new(header, vec![tx]);
    block.header.merkle_root = block.compute_merkle_root().unwrap();
    block
}

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

// ---------------------------------------------------------------------------
// 1) Block store ↔ UTXO store: end-to-end block flow
// ---------------------------------------------------------------------------

#[test]
fn block_stored_then_utxo_applied() {
    let kv = MemoryKvStore::new();
    let block_store = BlockStore::new(kv.clone());
    let utxo_store = UtxoStore::new(kv.clone());

    // Seed a genesis UTXO
    let genesis_op = OutPoint::new(TxId::from_bytes([1u8; 32]), 0);
    utxo_store.insert(genesis_op, tx_output(1000, 1)).unwrap();

    // Create & persist a block that spends the genesis UTXO
    let block = transfer_block(genesis_op, 0xAA, 950, 1);
    let hash = block_store.put_block(&block).unwrap();

    // Apply the block's tx effects to the UTXO set
    utxo_store.apply_block(&block).unwrap();

    // Genesis UTXO consumed, new UTXO created
    assert!(!utxo_store.contains(&genesis_op).unwrap());
    let new_tx_id = block.transactions[0].id().unwrap();
    let new_op = OutPoint::new(new_tx_id, 0);
    assert!(utxo_store.contains(&new_op).unwrap());

    // Block is retrievable by hash and by height
    let fetched = block_store.get_block(&hash).unwrap().unwrap();
    assert_eq!(fetched, block);
    let by_height = block_store
        .get_block_by_height(Height::from(1))
        .unwrap()
        .unwrap();
    assert_eq!(by_height, block);
}

// ---------------------------------------------------------------------------
// 2) Multi-block apply then full revert
// ---------------------------------------------------------------------------

#[test]
fn multi_block_apply_and_sequential_revert() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    // Seed
    let op0 = OutPoint::new(TxId::from_bytes([1u8; 32]), 0);
    utxo_store.insert(op0, tx_output(1000, 1)).unwrap();

    // Block 1: spend op0 → create op1
    let b1 = transfer_block(op0, 0x10, 900, 1);
    utxo_store.apply_block(&b1).unwrap();
    let op1 = OutPoint::new(b1.transactions[0].id().unwrap(), 0);

    // Block 2: spend op1 → create op2
    let b2 = transfer_block(op1, 0x20, 800, 2);
    utxo_store.apply_block(&b2).unwrap();
    let op2 = OutPoint::new(b2.transactions[0].id().unwrap(), 0);

    // Block 3: spend op2 → create op3
    let b3 = transfer_block(op2, 0x30, 700, 3);
    utxo_store.apply_block(&b3).unwrap();
    let op3 = OutPoint::new(b3.transactions[0].id().unwrap(), 0);

    // Only op3 should be live
    assert_eq!(utxo_store.len().unwrap(), 1);
    assert!(utxo_store.contains(&op3).unwrap());

    // Revert in reverse order
    utxo_store.revert_block(&b3).unwrap();
    assert!(utxo_store.contains(&op2).unwrap());
    assert!(!utxo_store.contains(&op3).unwrap());

    utxo_store.revert_block(&b2).unwrap();
    assert!(utxo_store.contains(&op1).unwrap());

    utxo_store.revert_block(&b1).unwrap();
    assert!(utxo_store.contains(&op0).unwrap());
    assert_eq!(utxo_store.len().unwrap(), 1);
}

// ---------------------------------------------------------------------------
// 3) UTXO commitment root stability across apply/revert
// ---------------------------------------------------------------------------

#[test]
fn commitment_root_stable_across_apply_revert() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    let op = OutPoint::new(TxId::from_bytes([5u8; 32]), 0);
    utxo_store.insert(op, tx_output(500, 5)).unwrap();

    let root_before = utxo_store.commitment_root().unwrap();

    let block = transfer_block(op, 0x55, 490, 1);
    utxo_store.apply_block(&block).unwrap();

    let root_during = utxo_store.commitment_root().unwrap();
    assert_ne!(root_before, root_during, "apply should change the root");

    utxo_store.revert_block(&block).unwrap();

    let root_after = utxo_store.commitment_root().unwrap();
    assert_eq!(
        root_before, root_after,
        "revert must restore original root"
    );
}

// ---------------------------------------------------------------------------
// 4) Snapshot + apply more blocks + restore snapshot
// ---------------------------------------------------------------------------

#[test]
fn snapshot_survives_subsequent_mutations() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    // Seed two UTXOs
    let op_a = OutPoint::new(TxId::from_bytes([10u8; 32]), 0);
    let op_b = OutPoint::new(TxId::from_bytes([11u8; 32]), 1);
    utxo_store.insert(op_a, tx_output(100, 10)).unwrap();
    utxo_store.insert(op_b, tx_output(200, 11)).unwrap();

    let root_snapshot = utxo_store.commitment_root().unwrap();
    utxo_store.create_snapshot(b"epoch-42").unwrap();

    // Mutate heavily
    utxo_store.remove(&op_a).unwrap();
    let op_c = OutPoint::new(TxId::from_bytes([12u8; 32]), 0);
    utxo_store.insert(op_c, tx_output(300, 12)).unwrap();

    assert!(!utxo_store.contains(&op_a).unwrap());
    assert!(utxo_store.contains(&op_c).unwrap());

    // Restore
    utxo_store.restore_snapshot(b"epoch-42").unwrap();

    assert!(utxo_store.contains(&op_a).unwrap());
    assert!(utxo_store.contains(&op_b).unwrap());
    assert!(!utxo_store.contains(&op_c).unwrap());

    let root_restored = utxo_store.commitment_root().unwrap();
    assert_eq!(root_snapshot, root_restored);
}

// ---------------------------------------------------------------------------
// 5) State store: full chain lifecycle (entries + tip + ledger + epoch)
// ---------------------------------------------------------------------------

#[test]
fn state_store_full_lifecycle() {
    let kv = MemoryKvStore::new();
    let state_store = StateStore::new(kv);

    // Chain entries
    let entry1 = ChainEntry {
        hash: BlockHash::from_bytes([0x01; 32]),
        parent_hash: BlockHash::ZERO,
        height: Height::from(1),
        slot: Slot::from(1),
        producer_key_hash: Hash256::from_bytes([0xAA; 32]),
    };
    let entry2 = ChainEntry {
        hash: BlockHash::from_bytes([0x02; 32]),
        parent_hash: entry1.hash,
        height: Height::from(2),
        slot: Slot::from(3),
        producer_key_hash: Hash256::from_bytes([0xBB; 32]),
    };

    state_store.put_chain_entry(&entry1).unwrap();
    state_store.put_chain_entry(&entry2).unwrap();
    state_store.set_tip_hash(entry2.hash).unwrap();

    assert_eq!(
        state_store.get_chain_entry(&entry1.hash).unwrap().unwrap(),
        entry1
    );
    assert_eq!(state_store.get_tip_hash().unwrap(), Some(entry2.hash));

    // Ledger state
    let p = pool(7, 1_000_000);
    let delegation = Delegation {
        delegator_id: Hash256::from_bytes([0xCC; 32]),
        pool_id: p.id,
        amount: Amount::from_smallest_units(500_000),
    };
    let mut rewards = BTreeMap::new();
    rewards.insert(
        Hash256::from_bytes([0xCC; 32]),
        Amount::from_smallest_units(42),
    );

    let ledger = LedgerState {
        pools: vec![p.clone()],
        delegations: vec![delegation],
        reward_balances: rewards,
    };

    state_store.put_ledger_state(&ledger).unwrap();
    assert_eq!(
        state_store.get_ledger_state().unwrap().unwrap(),
        ledger
    );

    // Epoch snapshots
    let dist = StakeDistribution::snapshot(Epoch::from(5), &[p], &[]).unwrap();
    let snap = EpochSnapshot {
        epoch: Epoch::from(5),
        stake_distribution: dist,
        tip_hash: entry2.hash,
    };
    state_store.put_epoch_snapshot(&snap).unwrap();

    let latest = state_store.latest_epoch_snapshot().unwrap().unwrap();
    assert_eq!(latest.epoch, Epoch::from(5));
}

// ---------------------------------------------------------------------------
// 6) Block store + state store: block persistence linked to chain tip
// ---------------------------------------------------------------------------

#[test]
fn block_and_state_stores_linked_via_tip() {
    let kv = MemoryKvStore::new();
    let block_store = BlockStore::new(kv.clone());
    let state_store = StateStore::new(kv);

    let block = coinbase_block(1, 0x77, 5000);
    let hash = block_store.put_block(&block).unwrap();

    let entry = ChainEntry {
        hash,
        parent_hash: BlockHash::ZERO,
        height: Height::from(1),
        slot: Slot::from(1),
        producer_key_hash: Hash256::from_bytes([0x77; 32]),
    };
    state_store.put_chain_entry(&entry).unwrap();
    state_store.set_tip_hash(hash).unwrap();

    // Verify linkage: tip → chain entry → block
    let tip = state_store.get_tip_hash().unwrap().unwrap();
    let ce = state_store.get_chain_entry(&tip).unwrap().unwrap();
    let fetched_block = block_store.get_block(&ce.hash).unwrap().unwrap();
    assert_eq!(fetched_block, block);
}

// ---------------------------------------------------------------------------
// 7) All three stores share one KV backend (namespace isolation)
// ---------------------------------------------------------------------------

#[test]
fn stores_share_backend_without_cross_contamination() {
    let kv = MemoryKvStore::new();
    let block_store = BlockStore::new(kv.clone());
    let utxo_store = UtxoStore::new(kv.clone());
    let state_store = StateStore::new(kv);

    // Populate each store
    let block = coinbase_block(1, 0xDD, 1000);
    block_store.put_block(&block).unwrap();

    let op = OutPoint::new(TxId::from_bytes([0xDD; 32]), 0);
    utxo_store.insert(op, tx_output(1000, 0xDD)).unwrap();

    let entry = ChainEntry {
        hash: BlockHash::from_bytes([0xDD; 32]),
        parent_hash: BlockHash::ZERO,
        height: Height::from(1),
        slot: Slot::from(1),
        producer_key_hash: Hash256::ZERO,
    };
    state_store.put_chain_entry(&entry).unwrap();

    // Each store sees its own data
    assert!(block_store
        .get_block_by_height(Height::from(1))
        .unwrap()
        .is_some());
    assert!(utxo_store.contains(&op).unwrap());
    assert!(state_store
        .get_chain_entry(&BlockHash::from_bytes([0xDD; 32]))
        .unwrap()
        .is_some());

    // UTXO count is only 1 (not polluted by block/state data)
    assert_eq!(utxo_store.len().unwrap(), 1);
}

// ---------------------------------------------------------------------------
// 8) Double-spend inside a single block is rejected
// ---------------------------------------------------------------------------

#[test]
fn double_spend_inside_block_rejected() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    let op = OutPoint::new(TxId::from_bytes([0xEE; 32]), 0);
    utxo_store.insert(op, tx_output(100, 0xEE)).unwrap();

    // Build a block with two transactions both spending the same outpoint
    let tx1 = Transaction::new(
        vec![TxInput::new(op)],
        vec![tx_output(50, 1)],
    );
    let tx2 = Transaction::new(
        vec![TxInput::new(op)],
        vec![tx_output(40, 2)],
    );

    let mut header = BlockHeader::genesis_template();
    header.height = Height::from(1);
    header.prev_hash = BlockHash::from_bytes([0xEE; 32]);

    let mut block = Block::new(header, vec![tx1, tx2]);
    block.header.merkle_root = block.compute_merkle_root().unwrap();

    let result = utxo_store.apply_block(&block);
    assert!(result.is_err(), "double-spend in block must be rejected");
}

// ---------------------------------------------------------------------------
// 9) Multiple epoch snapshots — latest_epoch_snapshot ordering
// ---------------------------------------------------------------------------

#[test]
fn epoch_snapshots_ordering() {
    let kv = MemoryKvStore::new();
    let state_store = StateStore::new(kv);

    let p = pool(1, 1000);

    for epoch_num in [3u64, 1, 5, 2, 4] {
        let dist =
            StakeDistribution::snapshot(Epoch::from(epoch_num), &[p.clone()], &[]).unwrap();
        let snap = EpochSnapshot {
            epoch: Epoch::from(epoch_num),
            stake_distribution: dist,
            tip_hash: BlockHash::from_bytes([epoch_num as u8; 32]),
        };
        state_store.put_epoch_snapshot(&snap).unwrap();
    }

    let latest = state_store.latest_epoch_snapshot().unwrap().unwrap();
    assert_eq!(
        latest.epoch,
        Epoch::from(5),
        "latest must be epoch 5 regardless of insertion order"
    );

    // Also verify individual retrieval
    let snap3 = state_store
        .get_epoch_snapshot(Epoch::from(3))
        .unwrap()
        .unwrap();
    assert_eq!(snap3.epoch, Epoch::from(3));
}

// ---------------------------------------------------------------------------
// 10) Rollback to snapshot alias works identically to restore
// ---------------------------------------------------------------------------

#[test]
fn rollback_to_snapshot_same_as_restore() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    let op1 = OutPoint::new(TxId::from_bytes([0xA1; 32]), 0);
    let op2 = OutPoint::new(TxId::from_bytes([0xA2; 32]), 0);
    utxo_store.insert(op1, tx_output(100, 0xA1)).unwrap();

    utxo_store.create_snapshot(b"rollback-test").unwrap();

    utxo_store.remove(&op1).unwrap();
    utxo_store.insert(op2, tx_output(200, 0xA2)).unwrap();

    utxo_store.rollback_to_snapshot(b"rollback-test").unwrap();

    assert!(utxo_store.contains(&op1).unwrap());
    assert!(!utxo_store.contains(&op2).unwrap());
}

// ---------------------------------------------------------------------------
// 11) Intra-block chained spending (tx1 output consumed by tx2 in same block)
// ---------------------------------------------------------------------------

#[test]
fn intra_block_chained_spending() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    // Seed
    let seed_op = OutPoint::new(TxId::from_bytes([0xBB; 32]), 0);
    utxo_store.insert(seed_op, tx_output(1000, 0xBB)).unwrap();

    // tx1 spends seed → creates output A
    let tx1 = Transaction::new(
        vec![TxInput::new(seed_op)],
        vec![tx_output(900, 1)],
    );
    let tx1_id = tx1.id().unwrap();
    let op_a = OutPoint::new(tx1_id, 0);

    // tx2 spends output A (created by tx1 in same block) → creates output B
    let tx2 = Transaction::new(
        vec![TxInput::new(op_a)],
        vec![tx_output(800, 2)],
    );
    let tx2_id = tx2.id().unwrap();
    let op_b = OutPoint::new(tx2_id, 0);

    let mut header = BlockHeader::genesis_template();
    header.height = Height::from(1);
    header.prev_hash = BlockHash::from_bytes([0xBB; 32]);

    let mut block = Block::new(header, vec![tx1, tx2]);
    block.header.merkle_root = block.compute_merkle_root().unwrap();

    utxo_store.apply_block(&block).unwrap();

    // seed_op consumed, op_a consumed intra-block, only op_b survives
    assert!(!utxo_store.contains(&seed_op).unwrap());
    assert!(!utxo_store.contains(&op_a).unwrap());
    assert!(utxo_store.contains(&op_b).unwrap());

    // Revert restores only the seed
    utxo_store.revert_block(&block).unwrap();
    assert!(utxo_store.contains(&seed_op).unwrap());
    assert!(!utxo_store.contains(&op_b).unwrap());
    assert_eq!(utxo_store.len().unwrap(), 1);
}

// ---------------------------------------------------------------------------
// 12) Large batch: 100 blocks applied then fully reverted
// ---------------------------------------------------------------------------

#[test]
fn hundred_block_apply_and_full_revert() {
    let kv = MemoryKvStore::new();
    let utxo_store = UtxoStore::new(kv);

    let initial_op = OutPoint::new(TxId::from_bytes([0xFF; 32]), 0);
    utxo_store
        .insert(initial_op, tx_output(1_000_000, 0xFF))
        .unwrap();

    let mut blocks = Vec::new();
    let mut current_op = initial_op;

    for i in 1u64..=100 {
        let marker = (i & 0xFF) as u8;
        let value = 1_000_000 - i * 10;
        let block = transfer_block(current_op, marker, value, i);
        utxo_store.apply_block(&block).unwrap();

        let new_tx_id = block.transactions[0].id().unwrap();
        current_op = OutPoint::new(new_tx_id, 0);
        blocks.push(block);
    }

    assert_eq!(utxo_store.len().unwrap(), 1);
    assert!(utxo_store.contains(&current_op).unwrap());

    // Revert all 100 blocks
    for block in blocks.iter().rev() {
        utxo_store.revert_block(block).unwrap();
    }

    assert_eq!(utxo_store.len().unwrap(), 1);
    assert!(utxo_store.contains(&initial_op).unwrap());
}
