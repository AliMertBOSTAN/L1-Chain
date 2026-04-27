//! Integration tests for qv-script.
//!
//! These tests exercise the full pipeline: template → encode → decode →
//! execute → validate, crossing module boundaries.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_wrap,
    clippy::integer_division
)]

use qv_core::{
    Amount, Datum, OutPoint, Script as CoreScript, Slot, Transaction, TxId, TxInput, TxOutput,
};
use qv_crypto::sha3_256;
use qv_script::interpreter::{execute, Context};
use qv_script::templates::{amm_swap, lending_repay, p2pkh_pqc, pubkey_hash};
use qv_script::{
    compile, decode_script, disassemble, encode_instructions, gas_cost, validate_script,
    validate_script_with_gas, ExecResult, GasMeter, Instruction, OpCode, ScriptBuilder,
    ScriptError, Value,
};

// ============================================================================
// Helpers
// ============================================================================

fn simple_tx(outputs: Vec<TxOutput>) -> Transaction {
    Transaction::new(
        vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
        outputs,
    )
}

fn make_ctx(tx: Transaction, resolved_inputs: Vec<TxOutput>, slot: u64) -> Context {
    Context::new(tx, resolved_inputs, Slot::from(slot))
}

// ============================================================================
// End-to-end: encode → decode → execute
// ============================================================================

#[test]
fn encode_decode_execute_roundtrip() {
    let instrs = vec![
        Instruction::push_int(7),
        Instruction::push_int(3),
        Instruction::simple(OpCode::Add),
        Instruction::push_int(10),
        Instruction::simple(OpCode::Eq),
    ];
    let bytes = compile(&instrs);
    let decoded = decode_script(&bytes).unwrap();
    assert_eq!(decoded.len(), 5);

    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let ctx = make_ctx(
        tx,
        vec![TxOutput::new(Amount::from(1), CoreScript::default())],
        0,
    );
    let mut gas = GasMeter::new(100_000);
    let result = execute(&bytes, &ctx, &mut gas).unwrap();
    assert!(result.success);
    assert_eq!(result.final_stack, vec![Value::Int(1)]);
}

// ============================================================================
// validate_script API
// ============================================================================

#[test]
fn validate_script_true_script() {
    let locking = CoreScript::new(vec![OpCode::Op1.to_byte()]);
    let tx = simple_tx(vec![TxOutput::new(
        Amount::from(100),
        CoreScript::default(),
    )]);
    let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
    let r = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    assert!(r.success);
}

#[test]
fn validate_script_false_script() {
    let locking = CoreScript::new(vec![OpCode::Op0.to_byte()]);
    let tx = simple_tx(vec![TxOutput::new(
        Amount::from(100),
        CoreScript::default(),
    )]);
    let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
    let r = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    assert!(!r.success);
}

#[test]
fn validate_with_witness_prepended() {
    // Witness pushes 5 and 5, locking script adds and checks == 10
    let witness = ScriptBuilder::new().push_int(5).push_int(5).build();
    let locking = CoreScript::new(
        ScriptBuilder::new()
            .op(OpCode::Add)
            .push_int(10)
            .op(OpCode::Eq)
            .build(),
    );
    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];
    let r = validate_script(&locking, &witness, &tx, &resolved, Slot::from(0)).unwrap();
    assert!(r.success);
}

// ============================================================================
// Gas tracking across execution
// ============================================================================

#[test]
fn gas_is_tracked_correctly() {
    let locking = CoreScript::new(
        ScriptBuilder::new()
            .op(OpCode::Op1)        // 1
            .op(OpCode::Dup)        // 2
            .op(OpCode::Add)        // 5
            .build(), // total = 8
    );
    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];
    let r =
        validate_script_with_gas(&locking, &[], &tx, &resolved, Slot::from(0), 100_000).unwrap();
    assert!(r.success);
    assert_eq!(r.gas_used, 8); // 1 + 2 + 5
}

#[test]
fn gas_exhaustion_with_custom_limit() {
    let locking = CoreScript::new(
        ScriptBuilder::new()
            .op(OpCode::Op1) // cost 1
            .op(OpCode::Op1) // cost 1
            .op(OpCode::Add) // cost 5
            .build(),
    );
    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];
    // Give only 4 gas: Op1(1) + Op1(1) = 2, then Add needs 5 → boom
    let err =
        validate_script_with_gas(&locking, &[], &tx, &resolved, Slot::from(0), 4).unwrap_err();
    assert_eq!(err, ScriptError::OutOfGas);
}

// ============================================================================
// Introspection across the full pipeline
// ============================================================================

#[test]
fn introspection_reads_correct_tx_data() {
    // Script checks: output[0].value == 777 AND input_count == 1 AND fee == 5
    let script_bytes = ScriptBuilder::new()
        .push_int(0)
        .op(OpCode::ReadOutputValue)
        .push_int(777)
        .op(OpCode::Eq)
        .op(OpCode::Verify)
        .op(OpCode::InputCount)
        .push_int(1)
        .op(OpCode::Eq)
        .op(OpCode::Verify)
        .op(OpCode::TxFee)
        .push_int(5)
        .op(OpCode::Eq)
        .build();

    let locking = CoreScript::new(script_bytes);
    let tx = simple_tx(vec![TxOutput::new(
        Amount::from(777),
        CoreScript::default(),
    )])
    .with_fee(Amount::from(5));
    let resolved = vec![TxOutput::new(Amount::from(800), CoreScript::default())];
    let r = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    assert!(r.success);
}

// ============================================================================
// Conditional flow
// ============================================================================

#[test]
fn nested_if_else() {
    // if true { if false { 0 } else { 42 } } else { 99 }
    let script_bytes = ScriptBuilder::new()
        .op(OpCode::Op1)     // outer condition = true
        .op(OpCode::If)
            .op(OpCode::Op0) // inner condition = false
            .op(OpCode::If)
                .push_int(0)
            .op(OpCode::Else)
                .push_int(42) // ← this should execute
            .op(OpCode::EndIf)
        .op(OpCode::Else)
            .push_int(99)
        .op(OpCode::EndIf)
        .build();

    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let ctx = make_ctx(
        tx,
        vec![TxOutput::new(Amount::from(1), CoreScript::default())],
        0,
    );
    let mut gas = GasMeter::new(100_000);
    let r = execute(&script_bytes, &ctx, &mut gas).unwrap();
    assert_eq!(r.final_stack, vec![Value::Int(42)]);
}

// ============================================================================
// Covenant template: lending_repay
// ============================================================================

#[test]
fn lending_repay_end_to_end() {
    let pool_script_raw = vec![0xAA; 16];
    let pool_hash = sha3_256(&pool_script_raw);
    let script = lending_repay(&pool_hash, 0, 1000);

    // Good tx: output 0 has value 1000 and correct script
    let tx = simple_tx(vec![TxOutput::new(
        Amount::from(1000),
        CoreScript::new(pool_script_raw.clone()),
    )]);
    let resolved = vec![TxOutput::new(Amount::from(2000), CoreScript::default())];
    let r = validate_script(
        &CoreScript::new(script.clone()),
        &[],
        &tx,
        &resolved,
        Slot::from(0),
    )
    .unwrap();
    assert!(r.success);

    // Bad tx: wrong script on output
    let tx_bad = simple_tx(vec![TxOutput::new(
        Amount::from(1000),
        CoreScript::new(vec![0xBB; 16]),
    )]);
    let r_bad = validate_script(
        &CoreScript::new(script),
        &[],
        &tx_bad,
        &resolved,
        Slot::from(0),
    );
    assert!(r_bad.is_err()); // covenant failure
}

// ============================================================================
// Disassemble round-trip check
// ============================================================================

#[test]
fn disassemble_contains_all_mnemonics() {
    let script = ScriptBuilder::new()
        .op(OpCode::Op0)
        .op(OpCode::Op1)
        .push_int(42)
        .push_bytes(&[0xDE, 0xAD])
        .op(OpCode::Dup)
        .op(OpCode::Add)
        .op(OpCode::HashSha3)
        .op(OpCode::CheckSigPqc)
        .op(OpCode::ReadOutputValue)
        .op(OpCode::AssertValue)
        .op(OpCode::Nop)
        .build();
    let text = disassemble(&script);
    assert!(text.contains("OP_0"));
    assert!(text.contains("OP_1"));
    assert!(text.contains("PUSH_INT"));
    assert!(text.contains("PUSH1"));
    assert!(text.contains("DUP"));
    assert!(text.contains("ADD"));
    assert!(text.contains("HASH_SHA3"));
    assert!(text.contains("CHECKSIG_PQC"));
    assert!(text.contains("READ_OUTPUT_VALUE"));
    assert!(text.contains("ASSERT_VALUE"));
    assert!(text.contains("NOP"));
}

// ============================================================================
// Property: deterministic execution
// ============================================================================

#[test]
fn deterministic_execution() {
    // Same script + context → same result every time
    let script = ScriptBuilder::new()
        .push_int(100)
        .push_int(200)
        .op(OpCode::Add)
        .push_int(300)
        .op(OpCode::Eq)
        .build();

    let locking = CoreScript::new(script);
    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];

    let r1 = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    let r2 = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    assert_eq!(r1.success, r2.success);
    assert_eq!(r1.gas_used, r2.gas_used);
    assert_eq!(r1.final_stack, r2.final_stack);
}

// ============================================================================
// Wrapping arithmetic safety
// ============================================================================

#[test]
fn overflow_wraps_deterministically() {
    let script = ScriptBuilder::new()
        .push_int(i64::MAX)
        .push_int(1)
        .op(OpCode::Add) // wraps to i64::MIN
        .push_int(i64::MIN)
        .op(OpCode::Eq)
        .build();

    let locking = CoreScript::new(script);
    let tx = simple_tx(vec![TxOutput::new(Amount::from(1), CoreScript::default())]);
    let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];
    let r = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
    assert!(r.success);
}
