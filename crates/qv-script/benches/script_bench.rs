//! Criterion benchmarks for `qv-script`.
//!
//! Run with:
//! ```bash
//! cargo bench -p qv-script
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use qv_core::{Amount, OutPoint, Script as CoreScript, Slot, Transaction, TxId, TxInput, TxOutput};
use qv_crypto::{generate_pqc_keypair, sign_pqc, DilithiumLevel};
use bincode;
use qv_script::{
    decode_script, p2pkh_pqc, pubkey_hash, validate_script, ScriptBuilder, OpCode,
};

// ---------------------------------------------------------------------------
// Helper: create a simple transaction with witness data
// ---------------------------------------------------------------------------

fn create_test_tx_with_witness(witness_bytes: Vec<u8>) -> (Transaction, Vec<TxOutput>) {
    // Create a simple transaction with one input
    let tx_id = TxId::from_bytes([42; 32]);
    let input = TxInput::new(OutPoint::new(tx_id, 0));

    // Create one output
    let output = TxOutput::new(Amount::from(1000), CoreScript::default());

    let tx = Transaction::new(vec![input], vec![output.clone()]);

    // Resolved inputs (the UTXO being spent)
    let resolved_inputs = vec![output];

    (tx, resolved_inputs)
}

// ---------------------------------------------------------------------------
// Benchmark: p2pkh_pqc script validation
// ---------------------------------------------------------------------------

fn bench_script_p2pkh_validation(c: &mut Criterion) {
    // Setup: generate keypair and sign once (outside the loop)
    let kp = generate_pqc_keypair(DilithiumLevel::Level3).expect("keygen");
    let pk_hash = pubkey_hash(kp.public.as_bytes());
    let locking_script = CoreScript::new(p2pkh_pqc(&pk_hash));

    // Create a test transaction (the message to sign)
    let (tx, _) = create_test_tx_with_witness(vec![]);
    let payload = bincode::serialize(&tx).expect("serialize tx");

    // Sign the payload
    let sig = sign_pqc(&kp.secret, &payload).expect("sign");

    // Build witness bytecode: push msg, sig, pubkey
    let witness_bytes = ScriptBuilder::new()
        .push_bytes(&payload)
        .push_bytes(sig.as_bytes())
        .push_bytes(kp.public.as_bytes())
        .build();

    // Resolved inputs with the correct locking script
    let resolved_inputs = vec![TxOutput::new(Amount::from(1000), locking_script.clone())];

    c.bench_function("script_p2pkh_pqc_validate", |b| {
        b.iter(|| {
            black_box(
                validate_script(
                    &locking_script,
                    black_box(&witness_bytes),
                    &tx,
                    &resolved_inputs,
                    Slot::from(0),
                )
                .expect("validate"),
            );
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmark: script decode (large script)
// ---------------------------------------------------------------------------

fn bench_script_decode(c: &mut Criterion) {
    c.bench_function("script_decode_16kb", |b| {
        // Create a large script (~16KB) by chaining many push operations
        let mut builder = ScriptBuilder::new();
        for i in 0..256 {
            let data = vec![(i % 256) as u8; 64]; // 64 bytes per iteration
            builder = builder.push_bytes(&data);
        }
        let large_script = builder.build();

        b.iter(|| {
            black_box(decode_script(black_box(&large_script)).expect("decode"));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmark: gas metering (100 simple operations)
// ---------------------------------------------------------------------------

fn bench_script_gas_metering(c: &mut Criterion) {
    c.bench_function("script_gas_100_ops", |b| {
        // Create a script with 100 simple operations
        let mut builder = ScriptBuilder::new();
        for _ in 0..100 {
            builder = builder.op(OpCode::Op1).op(OpCode::Add);
        }
        let script = CoreScript::new(builder.build());

        let (tx, resolved_inputs) = create_test_tx_with_witness(vec![]);

        b.iter(|| {
            // validate_script will meter gas as it executes
            let _ = validate_script(
                &script,
                &[],
                black_box(&tx),
                black_box(&resolved_inputs),
                Slot::from(0),
            );
        });
    });
}

criterion_group!(
    benches,
    bench_script_p2pkh_validation,
    bench_script_decode,
    bench_script_gas_metering
);
criterion_main!(benches);
