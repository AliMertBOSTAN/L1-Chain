//! Criterion benchmarks for `qv-core`.
//!
//! Run with:
//! ```bash
//! cargo bench -p qv-core
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use qv_core::{
    merkle_root_of, Amount, Block, BlockHash, BlockHeader, Height, MerkleRoot, OutPoint,
    Script as CoreScript, Slot, Timestamp, Transaction, TxId, TxInput, TxOutput, UtxoCommitment,
    BLOCK_VERSION,
};

// ---------------------------------------------------------------------------
// Helper: create a simple transaction
// ---------------------------------------------------------------------------

fn create_test_tx(index: u32) -> Transaction {
    let tx_id = TxId::from_bytes({
        let mut bytes = [0u8; 32];
        bytes[0] = (index & 0xFF) as u8;
        bytes[1] = ((index >> 8) & 0xFF) as u8;
        bytes
    });

    let input = TxInput::new(OutPoint::new(tx_id, 0));
    let output = TxOutput::new(Amount::from(1000), CoreScript::default());

    Transaction::new(vec![input], vec![output])
}

// ---------------------------------------------------------------------------
// Benchmark: merkle_root_of with varying numbers of transactions
// ---------------------------------------------------------------------------

fn bench_merkle_root(c: &mut Criterion) {
    let sizes = [1usize, 10, 100, 1000];

    let mut group = c.benchmark_group("merkle_root_of");
    for &n in &sizes {
        let txs: Vec<Transaction> = (0..n as u32).map(create_test_tx).collect();
        let ids: Vec<TxId> = txs
            .iter()
            .map(|tx| tx.id().expect("compute txid"))
            .collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_txs", n)),
            &ids,
            |b, ids| {
                b.iter(|| merkle_root_of(black_box(ids)));
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: Transaction serialization (bincode)
// ---------------------------------------------------------------------------

fn bench_transaction_serialization(c: &mut Criterion) {
    c.bench_function("transaction_canonical_bytes", |b| {
        let tx = create_test_tx(0);

        b.iter(|| {
            black_box(bincode::serialize(black_box(&tx)).expect("serialize"));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmark: Block::validate_structure (10-tx block)
// ---------------------------------------------------------------------------

fn bench_block_validate_structure(c: &mut Criterion) {
    c.bench_function("block_validate_structure_10tx", |b| {
        // Create a block with 10 transactions
        let transactions: Vec<Transaction> = (0..10).map(create_test_tx).collect();

        let header = BlockHeader {
            version: BLOCK_VERSION,
            prev_hash: BlockHash::ZERO,
            height: Height::from(1),
            slot: Slot::from(1),
            timestamp: Timestamp::from_unix_secs(1000),
            merkle_root: MerkleRoot::ZERO, // Will be recomputed
            utxo_commitment: UtxoCommitment::ZERO,
            vrf_proof: vec![],
            kes_sig: vec![],
            producer_key_hash: [0u8; 32].into(),
        };

        let mut block = Block::new(header, transactions);

        // Compute the correct merkle root
        let merkle = block.compute_merkle_root().expect("merkle");
        block.header.merkle_root = merkle;

        b.iter(|| {
            black_box(&block).validate_structure().expect("validate");
        });
    });
}

criterion_group!(
    benches,
    bench_merkle_root,
    bench_transaction_serialization,
    bench_block_validate_structure
);
criterion_main!(benches);
