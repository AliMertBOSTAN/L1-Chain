//! High-level script compilation, decompilation, and validation entry point.
//!
//! This module bridges `qv-core`'s opaque [`Script`](qv_core::Script) type
//! with the `qv-script` VM. It provides:
//!
//! - [`validate_script`] — the single function that the ledger layer calls
//!   to check whether a UTXO may be spent. This is the **only** public
//!   entry point that the consensus / validation pipeline needs.
//! - [`disassemble`] — human-readable script decompilation for debugging.
//! - [`compile`] — encode a list of instructions into raw script bytes.

use qv_core::{Script as CoreScript, Slot, Transaction, TxOutput};

use crate::gas::{GasMeter, DEFAULT_GAS_LIMIT};
use crate::interpreter::{execute, Context, ExecResult, ScriptError};
use crate::opcode::{decode_script, encode_instructions, Instruction};

// ============================================================================
// Script validation — THE entry point for L1
// ============================================================================

/// Validate a locking script in the context of a spending transaction.
///
/// This is the **only** function that the ledger validation pipeline calls
/// on `qv-script`. It assembles the execution environment and runs the
/// interpreter with the default gas limit.
///
/// # Arguments
///
/// - `locking_script` — the script from the UTXO being spent.
/// - `witness_bytes` — the serialised witness data from the spending input.
///   These bytes are prepended to the locking script (witness runs first,
///   then the locking script evaluates).
/// - `tx` — the transaction that is trying to spend the UTXO.
/// - `resolved_inputs` — the `TxOutput`s that each input references
///   (same order as `tx.inputs`).
/// - `current_slot` — the slot in which this block is being produced.
///
/// # Returns
///
/// - `Ok(ExecResult)` with `success = true` if the script validates.
/// - `Ok(ExecResult)` with `success = false` if the script terminated
///   normally but the final stack is empty or falsy.
/// - `Err(ScriptError)` for fatal execution errors (out of gas, stack
///   overflow, bad opcode, etc.).
pub fn validate_script(
    locking_script: &CoreScript,
    witness_bytes: &[u8],
    tx: &Transaction,
    resolved_inputs: &[TxOutput],
    current_slot: Slot,
) -> Result<ExecResult, ScriptError> {
    validate_script_with_gas(
        locking_script,
        witness_bytes,
        tx,
        resolved_inputs,
        current_slot,
        DEFAULT_GAS_LIMIT,
    )
}

/// Like [`validate_script`] but with a custom gas limit.
pub fn validate_script_with_gas(
    locking_script: &CoreScript,
    witness_bytes: &[u8],
    tx: &Transaction,
    resolved_inputs: &[TxOutput],
    current_slot: Slot,
    gas_limit: u64,
) -> Result<ExecResult, ScriptError> {
    // Concatenate witness + locking script (witness executes first).
    let mut full_script =
        Vec::with_capacity(witness_bytes.len().wrapping_add(locking_script.len()));
    full_script.extend_from_slice(witness_bytes);
    full_script.extend_from_slice(locking_script.as_bytes());

    let ctx = Context::new(tx.clone(), resolved_inputs.to_vec(), current_slot);
    let mut gas = GasMeter::new(gas_limit);

    execute(&full_script, &ctx, &mut gas)
}

// ============================================================================
// Disassemble
// ============================================================================

/// Disassemble raw script bytes into a human-readable string.
///
/// Each instruction is printed on its own line with its mnemonic and any
/// inline data. Unknown bytes produce an `ERROR: ...` line.
pub fn disassemble(script_bytes: &[u8]) -> String {
    match decode_script(script_bytes) {
        Ok(instrs) => {
            let mut out = String::new();
            for (i, instr) in instrs.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(instr.op.mnemonic());
                if !instr.data.is_empty() {
                    out.push_str(" 0x");
                    for b in &instr.data {
                        out.push_str(&format!("{b:02x}"));
                    }
                }
            }
            out
        }
        Err(e) => format!("ERROR: {e}"),
    }
}

/// Compile a list of instructions into raw script bytes.
///
/// This is a thin wrapper around [`encode_instructions`] for symmetry
/// with [`disassemble`].
#[must_use]
pub fn compile(instructions: &[Instruction]) -> Vec<u8> {
    encode_instructions(instructions)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use crate::opcode::OpCode;
    use crate::templates::{p2pkh_pqc, pubkey_hash, ScriptBuilder};
    use qv_core::{
        Amount, OutPoint, Script as CoreScript, Slot, Transaction, TxId, TxInput, TxOutput,
    };

    #[test]
    fn disassemble_simple() {
        let bytes = ScriptBuilder::new()
            .op(OpCode::Op1)
            .op(OpCode::Dup)
            .op(OpCode::Add)
            .op(OpCode::Verify)
            .build();
        let text = disassemble(&bytes);
        assert!(text.contains("OP_1"));
        assert!(text.contains("DUP"));
        assert!(text.contains("ADD"));
        assert!(text.contains("VERIFY"));
    }

    #[test]
    fn disassemble_with_push_data() {
        let bytes = ScriptBuilder::new().push_bytes(&[0xDE, 0xAD]).build();
        let text = disassemble(&bytes);
        assert!(text.contains("PUSH1"));
        assert!(text.contains("0xdead"));
    }

    #[test]
    fn compile_roundtrip() {
        let instrs = vec![
            Instruction::simple(OpCode::Op1),
            Instruction::push_int(42),
            Instruction::simple(OpCode::Add),
        ];
        let bytes = compile(&instrs);
        let decoded = decode_script(&bytes).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].op, OpCode::Op1);
        assert_eq!(decoded[1].op, OpCode::PushInt);
        assert_eq!(decoded[2].op, OpCode::Add);
    }

    #[test]
    fn validate_simple_script_succeeds() {
        // Locking script: OP_1 (always valid)
        let locking = CoreScript::new(vec![OpCode::Op1.to_byte()]);
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let result = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
        assert!(result.success);
    }

    #[test]
    fn validate_op0_fails() {
        let locking = CoreScript::new(vec![OpCode::Op0.to_byte()]);
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let result = validate_script(&locking, &[], &tx, &resolved, Slot::from(0)).unwrap();
        assert!(!result.success);
    }

    #[test]
    fn validate_with_witness() {
        // Witness pushes 1, locking script duplicates and adds → 2 (truthy)
        let witness = ScriptBuilder::new().op(OpCode::Op1).build();
        let locking = CoreScript::new(ScriptBuilder::new().op(OpCode::Dup).op(OpCode::Add).build());
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let result = validate_script(&locking, &witness, &tx, &resolved, Slot::from(0)).unwrap();
        assert!(result.success);
    }
}
