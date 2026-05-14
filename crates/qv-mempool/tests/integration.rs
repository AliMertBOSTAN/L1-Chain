//! Integration tests for `qv-mempool`.
//!
//! Cross-module scenarios: clear pool → ordering → batch, encrypted pool →
//! threshold decrypt → ordering, slashing evidence workflow.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use qv_core::{
    Amount, BlockHash, Epoch, Hash256, OutPoint, Script, Transaction, TxId, TxInput, TxOutput,
};
use qv_mempool::batcher::{self, OrderIntent, PoolState, SlashingEvidence, SwapDirection};
use qv_mempool::clear::{ClearPool, ClearPoolConfig, MempoolEntry};
use qv_mempool::encrypted::{
    DecryptionShare, EncryptedPool, EncryptedPoolConfig, EncryptedTx, MockThresholdDecryptor,
};
use qv_mempool::ordering::{self, OrderKey};
use qv_mempool::MempoolError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_tx(marker: u8) -> (Transaction, TxId) {
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(
            TxId::from_bytes([marker; 32]),
            0,
        ))],
        vec![TxOutput::new(
            Amount::from_smallest_units(100),
            Script::new(vec![marker]),
        )],
    );
    let id = tx.id().unwrap();
    (tx, id)
}

fn entry(marker: u8, fee: u64, size: usize) -> MempoolEntry {
    let (tx, id) = make_tx(marker);
    MempoolEntry::new(tx, id, Amount::from_smallest_units(fee), size)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// 1) Clear pool → get_batch → ordering verification
// ---------------------------------------------------------------------------

#[test]
fn clear_pool_to_ordered_batch() {
    let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

    // Add 5 transactions with varying fees
    for (i, fee) in [50u64, 200, 100, 300, 150].iter().enumerate() {
        let marker = (i + 1) as u8;
        pool.add(entry(marker, *fee, 200)).unwrap();
    }

    let batch = pool.get_batch(5);
    assert_eq!(batch.len(), 5);

    // Build OrderKeys and verify canonical sort
    let mut keys: Vec<OrderKey> = batch
        .iter()
        .map(|e| OrderKey::new(e.fee_density, e.added_at * 1000, e.tx_id))
        .collect();

    ordering::deterministic_sort(&mut keys);
    assert!(ordering::verify_order(&keys));

    // First key should have highest fee density
    assert!(keys[0].fee_density() >= keys[1].fee_density());
}

// ---------------------------------------------------------------------------
// 2) Clear pool double-spend across different adds
// ---------------------------------------------------------------------------

#[test]
fn clear_pool_prevents_cross_tx_double_spend() {
    let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

    // tx1 spends OutPoint([10;32], 0)
    let (tx1, id1) = make_tx(10);
    pool.add(MempoolEntry::new(
        tx1,
        id1,
        Amount::from_smallest_units(100),
        200,
    ))
    .unwrap();

    // tx2 also spends OutPoint([10;32], 0) — same outpoint!
    let tx2 = Transaction::new(
        vec![TxInput::new(OutPoint::new(TxId::from_bytes([10; 32]), 0))],
        vec![TxOutput::new(
            Amount::from_smallest_units(50),
            Script::new(vec![99]),
        )],
    );
    let id2 = tx2.id().unwrap();

    let err = pool
        .add(MempoolEntry::new(
            tx2,
            id2,
            Amount::from_smallest_units(200),
            200,
        ))
        .unwrap_err();
    assert!(matches!(err, MempoolError::DoubleSpend { .. }));
}

// ---------------------------------------------------------------------------
// 3) Encrypted pool → decrypt → verify cleartext matches
// ---------------------------------------------------------------------------

#[test]
fn encrypted_pool_decrypt_roundtrip() {
    let epoch = Epoch::from(1);
    let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), epoch);
    let decryptor = MockThresholdDecryptor::new(2, 3);

    let key = vec![0x42; 32];
    let plaintext = b"quantumvault-transaction-payload";

    let encrypted_body: Vec<u8> = plaintext
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();

    let etx = EncryptedTx {
        id: TxId::from_bytes([0xAA; 32]),
        kem_ciphertext: vec![0; 32],
        encrypted_body,
        target_epoch: epoch,
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

// ---------------------------------------------------------------------------
// 4) Encrypted pool epoch advancement
// ---------------------------------------------------------------------------

#[test]
fn encrypted_pool_epoch_lifecycle() {
    let mut pool = EncryptedPool::new(EncryptedPoolConfig::default(), Epoch::from(1));

    // Add txs for epoch 1
    for i in 1..=5u8 {
        let etx = EncryptedTx {
            id: TxId::from_bytes([i; 32]),
            kem_ciphertext: vec![i; 32],
            encrypted_body: vec![i; 64],
            target_epoch: Epoch::from(1),
            received_at: now_secs(),
        };
        pool.add(etx).unwrap();
    }
    assert_eq!(pool.len(), 5);

    // Advance to epoch 2 — flushes all
    pool.advance_epoch(Epoch::from(2));
    assert!(pool.is_empty());

    // Epoch 1 tx now rejected
    let old_etx = EncryptedTx {
        id: TxId::from_bytes([99; 32]),
        kem_ciphertext: vec![0; 32],
        encrypted_body: vec![0; 64],
        target_epoch: Epoch::from(1),
        received_at: now_secs(),
    };
    assert!(matches!(
        pool.add(old_etx).unwrap_err(),
        MempoolError::WrongEpoch { .. }
    ));
}

// ---------------------------------------------------------------------------
// 5) AMM batch: multi-order execution
// ---------------------------------------------------------------------------

#[test]
fn amm_batch_multi_order_execution() {
    let pool_id = Hash256::from_bytes([0xDD; 32]);
    let pool = PoolState {
        pool_id,
        reserve_a: Amount::from_smallest_units(100_000),
        reserve_b: Amount::from_smallest_units(100_000),
    };

    let mut orders = vec![
        OrderIntent {
            order_tx_id: TxId::from_bytes([1; 32]),
            pool_id,
            direction: SwapDirection::AtoB,
            offer_amount: Amount::from_smallest_units(1000),
            min_receive: Amount::from_smallest_units(1),
            fee_density: 100,
            timestamp_ms: 1000,
        },
        OrderIntent {
            order_tx_id: TxId::from_bytes([2; 32]),
            pool_id,
            direction: SwapDirection::BtoA,
            offer_amount: Amount::from_smallest_units(500),
            min_receive: Amount::from_smallest_units(1),
            fee_density: 200,
            timestamp_ms: 2000,
        },
    ];

    let result = batcher::build_amm_batch(&pool, &mut orders).unwrap();
    assert_eq!(result.matched_orders.len(), 2);
    // Higher fee density order should be first
    assert_eq!(result.matched_orders[0], TxId::from_bytes([2; 32]));
}

// ---------------------------------------------------------------------------
// 6) AMM batch: constant product invariant holds
// ---------------------------------------------------------------------------

#[test]
fn amm_batch_invariant_holds() {
    let pool_id = Hash256::from_bytes([0xEE; 32]);
    let ra = 50_000u64;
    let rb = 50_000u64;
    let pool = PoolState {
        pool_id,
        reserve_a: Amount::from_smallest_units(ra),
        reserve_b: Amount::from_smallest_units(rb),
    };

    let mut orders = vec![OrderIntent {
        order_tx_id: TxId::from_bytes([1; 32]),
        pool_id,
        direction: SwapDirection::AtoB,
        offer_amount: Amount::from_smallest_units(5000),
        min_receive: Amount::from_smallest_units(1),
        fee_density: 100,
        timestamp_ms: 1000,
    }];

    let result = batcher::build_amm_batch(&pool, &mut orders).unwrap();
    assert_eq!(result.matched_orders.len(), 1);

    // x * y >= k (with fee, k should increase or stay same)
    let k_before = (ra as u128) * (rb as u128);
    let k_after = (result.new_reserve_a.0 as u128) * (result.new_reserve_b.0 as u128);
    assert!(
        k_after >= k_before,
        "invariant violated: {k_after} < {k_before}"
    );
}

// ---------------------------------------------------------------------------
// 7) Slashing evidence: valid vs invalid
// ---------------------------------------------------------------------------

#[test]
fn slashing_evidence_workflow() {
    let canonical = vec![
        TxId::from_bytes([1; 32]),
        TxId::from_bytes([2; 32]),
        TxId::from_bytes([3; 32]),
    ];
    let actual = vec![
        TxId::from_bytes([2; 32]),
        TxId::from_bytes([1; 32]),
        TxId::from_bytes([3; 32]),
    ];

    let evidence = SlashingEvidence {
        block_hash: BlockHash::from_bytes([0xFF; 32]),
        slot: 42,
        canonical_order: canonical,
        actual_order: actual,
        producer_key_hash: Hash256::from_bytes([0xAA; 32]),
    };

    assert!(evidence.is_valid());
}

// ---------------------------------------------------------------------------
// 8) Clear pool capacity eviction preserves high-fee txs
// ---------------------------------------------------------------------------

#[test]
fn capacity_eviction_keeps_highest_fees() {
    let config = ClearPoolConfig {
        max_tx_count: 5,
        max_pool_bytes: 10_000_000,
        min_fee: 0,
        max_age_secs: 3600,
    };
    let mut pool = ClearPool::new(config);

    // Add 5 txs
    for i in 1..=5u8 {
        pool.add(entry(i, (i as u64) * 100, 200)).unwrap();
    }

    // Add a 6th with high fee — should evict the lowest-fee
    pool.add(entry(6, 600, 200)).unwrap();
    assert_eq!(pool.len(), 5);

    // Entry with fee=100 (marker=1) should have been evicted
    let (_, id1) = make_tx(1);
    assert!(!pool.contains(&id1));

    // Entry with fee=600 (marker=6) should be present
    let (_, id6) = make_tx(6);
    assert!(pool.contains(&id6));
}

// ---------------------------------------------------------------------------
// 9) Ordering: large sort is deterministic across runs
// ---------------------------------------------------------------------------

#[test]
fn ordering_deterministic_across_runs() {
    let mut keys1: Vec<OrderKey> = (0..100u8)
        .map(|i| OrderKey::new(i as u64 * 10, (100 - i) as u64, TxId::from_bytes([i; 32])))
        .collect();

    let mut keys2 = keys1.clone();

    ordering::deterministic_sort(&mut keys1);
    ordering::deterministic_sort(&mut keys2);

    assert_eq!(keys1, keys2);
    assert!(ordering::verify_order(&keys1));
}

// ---------------------------------------------------------------------------
// 10) Full pipeline: clear pool → batch → order → verify
// ---------------------------------------------------------------------------

#[test]
fn full_clear_pipeline() {
    let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

    // Add 10 transactions
    for i in 1..=10u8 {
        pool.add(entry(i, (i as u64) * 50, 200)).unwrap();
    }

    // Get batch
    let batch = pool.get_batch(10);
    assert_eq!(batch.len(), 10);

    // Build OrderKeys
    let mut keys: Vec<OrderKey> = batch
        .iter()
        .map(|e| OrderKey::new(e.fee_density, e.added_at * 1000, e.tx_id))
        .collect();

    ordering::deterministic_sort(&mut keys);
    assert!(ordering::verify_order(&keys));

    // Highest fee tx should be first
    let (_, id10) = make_tx(10);
    assert_eq!(keys[0].tx_id(), id10);
}

// ---------------------------------------------------------------------------
// 11) Remove confirmed cleans up dependency tracking
// ---------------------------------------------------------------------------

#[test]
fn remove_confirmed_cleans_dependencies() {
    let mut pool = ClearPool::new(ClearPoolConfig::ephemeral());

    let e1 = entry(40, 100, 200);
    let e2 = entry(41, 200, 200);
    let op1 = e1.tx.inputs[0].prev_output;
    let op2 = e2.tx.inputs[0].prev_output;

    pool.add(e1).unwrap();
    pool.add(e2).unwrap();
    assert!(pool.is_spent(&op1));
    assert!(pool.is_spent(&op2));

    let mut spent = std::collections::BTreeSet::new();
    spent.insert(op1);

    pool.remove_confirmed(&spent);
    assert!(!pool.is_spent(&op1));
    assert!(pool.is_spent(&op2)); // only op1 was confirmed
}

// ---------------------------------------------------------------------------
// 12) Encrypted + decrypted → ordering integration
// ---------------------------------------------------------------------------

#[test]
fn encrypted_to_ordering_pipeline() {
    let epoch = Epoch::from(5);
    let mut enc_pool = EncryptedPool::new(EncryptedPoolConfig::default(), epoch);
    let decryptor = MockThresholdDecryptor::new(1, 1);
    let key = vec![0x55; 16];

    // Encrypt 3 "transactions" (just marker bytes for testing)
    for i in 1..=3u8 {
        let plaintext = [i; 32];
        let encrypted_body: Vec<u8> = plaintext
            .iter()
            .enumerate()
            .map(|(j, b)| b ^ key[j % key.len()])
            .collect();

        let etx = EncryptedTx {
            id: TxId::from_bytes([i; 32]),
            kem_ciphertext: vec![0; 32],
            encrypted_body,
            target_epoch: epoch,
            received_at: now_secs(),
        };
        enc_pool.add(etx).unwrap();
    }

    let shares = vec![DecryptionShare {
        member_index: 0,
        share_bytes: key,
    }];

    let decrypted = enc_pool.decrypt_batch(&decryptor, &shares).unwrap();
    assert_eq!(decrypted.len(), 3);

    // Build ordering keys from decrypted results
    let mut keys: Vec<OrderKey> = decrypted
        .iter()
        .enumerate()
        .map(|(i, (tx_id, _))| OrderKey::new((i as u64 + 1) * 100, 1000, *tx_id))
        .collect();

    ordering::deterministic_sort(&mut keys);
    assert!(ordering::verify_order(&keys));
}
