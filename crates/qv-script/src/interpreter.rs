//! Stack-based Script VM interpreter.
//!
//! The interpreter takes a decoded script (a list of [`Instruction`]s), an
//! execution [`Context`] that provides transaction introspection data, and a
//! [`GasMeter`] budget. It runs every instruction deterministically and
//! reports whether the script *validated* (top-of-stack is truthy at the end)
//! or *failed*.
//!
//! # Determinism guarantees
//!
//! - No floating point. All arithmetic is wrapping `i64`.
//! - Division by zero is a script error (not a panic/UB).
//! - Stack depth is capped at [`OpCode::MAX_STACK_DEPTH`].
//! - Gas is charged *before* execution of each opcode.
//! - `IF`/`ELSE`/`ENDIF` is tracked with a nesting stack, never recursion.

use thiserror::Error;

use qv_core::{Datum, Slot, Transaction, TxOutput};
use qv_crypto::{blake3, sha3_256, verify_pqc, DilithiumLevel, PqcPublicKey, PqcSignature};

use crate::gas::{GasMeter, MULTISIG_PER_KEY_COST};
use crate::opcode::{decode_script, Instruction, OpCode, OpcodeError, Value};

// ============================================================================
// Errors
// ============================================================================

/// Errors arising during script interpretation.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ScriptError {
    /// Gas budget exhausted before the script finished.
    #[error("out of gas")]
    OutOfGas,

    /// Stack underflow: an opcode tried to pop from an empty stack.
    #[error("stack underflow")]
    StackUnderflow,

    /// Stack depth exceeded [`OpCode::MAX_STACK_DEPTH`].
    #[error("stack overflow (depth > {0})")]
    StackOverflow(usize),

    /// An arithmetic opcode received a non-integer value.
    #[error("type error: expected Int, got Bytes")]
    TypeError,

    /// Division or modulo by zero.
    #[error("division by zero")]
    DivisionByZero,

    /// An introspection opcode received an out-of-range index.
    #[error("index {index} out of range (count {count})")]
    IndexOutOfRange {
        /// The index requested.
        index: usize,
        /// The number of items available.
        count: usize,
    },

    /// `VERIFY` popped a falsy value.
    #[error("VERIFY failed")]
    VerifyFailed,

    /// `RETURN` was executed (unconditional script failure).
    #[error("RETURN executed")]
    Returned,

    /// A covenant assertion did not hold.
    #[error("covenant assertion failed: {0}")]
    CovenantFailed(&'static str),

    /// `CHECKSIG_PQC` received malformed key or signature bytes.
    #[error("malformed crypto material: {0}")]
    MalformedCrypto(String),

    /// Unbalanced `IF`/`ELSE`/`ENDIF`.
    #[error("unbalanced conditional: {0}")]
    UnbalancedConditional(&'static str),

    /// SLICE out of bounds.
    #[error("slice out of bounds")]
    SliceOutOfBounds,

    /// Opcode decoding failure (delegates to [`OpcodeError`]).
    #[error(transparent)]
    Opcode(#[from] OpcodeError),
}

// ============================================================================
// Context — transaction introspection data
// ============================================================================

/// Read-only transaction context supplied to the interpreter.
///
/// The interpreter never modifies the context. It only reads input/output
/// values, scripts, datums, the transaction hash, and the current slot for
/// introspection opcodes.
#[derive(Debug, Clone)]
pub struct Context {
    /// The transaction being validated.
    pub tx: Transaction,
    /// Resolved values of the inputs being consumed (same order as `tx.inputs`).
    /// Each entry is the `TxOutput` that the corresponding input references.
    pub resolved_inputs: Vec<TxOutput>,
    /// Current slot (for `SLOT_NUMBER` opcode).
    pub current_slot: Slot,
    /// Pre-computed transaction hash (SHA3-256 of canonical bytes).
    ///
    /// Includes input witnesses — suitable for identity/introspection, but
    /// **not** for signing (a witness is part of the signed message → cyclic).
    pub tx_hash: [u8; 32],
    /// Pre-computed signature hash: SHA3-256 of the canonical bytes with all
    /// input witnesses cleared. Witness-independent, so it is the message a
    /// signature commits to (ADR-012). Used by the `SIG_HASH` opcode.
    pub sighash: [u8; 32],
}

impl Context {
    /// Build a context from a transaction and its resolved inputs.
    ///
    /// The caller must ensure `resolved_inputs` has the same length and
    /// order as `tx.inputs`. The transaction hash is computed eagerly.
    pub fn new(tx: Transaction, resolved_inputs: Vec<TxOutput>, current_slot: Slot) -> Self {
        let tx_hash = tx
            .canonical_bytes()
            .map(|b| sha3_256(&b))
            .unwrap_or([0u8; 32]);
        let sighash = tx.sighash().unwrap_or([0u8; 32]);
        Self {
            tx,
            resolved_inputs,
            current_slot,
            tx_hash,
            sighash,
        }
    }
}

// ============================================================================
// Execution result
// ============================================================================

/// The outcome of running a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    /// `true` if the script validated (top-of-stack was truthy).
    pub success: bool,
    /// Total gas consumed.
    pub gas_used: u64,
    /// Final stack (for debugging / testing).
    pub final_stack: Vec<Value>,
}

// ============================================================================
// Interpreter
// ============================================================================

/// Run a script against the given context.
///
/// The script bytes are first decoded into [`Instruction`]s, then executed
/// one by one. Gas is charged per opcode *before* execution.
///
/// Returns [`ExecResult`] on normal termination, or [`ScriptError`] on
/// fatal conditions (gas exhaustion, stack overflow, bad opcodes, etc.).
pub fn execute(
    script_bytes: &[u8],
    ctx: &Context,
    gas: &mut GasMeter,
) -> Result<ExecResult, ScriptError> {
    let instructions = decode_script(script_bytes)?;
    execute_instructions(&instructions, ctx, gas)
}

/// Execute pre-decoded instructions.
pub fn execute_instructions(
    instructions: &[Instruction],
    ctx: &Context,
    gas: &mut GasMeter,
) -> Result<ExecResult, ScriptError> {
    let mut stack: Vec<Value> = Vec::new();

    // Conditional execution tracking.
    // `exec_stack` mirrors nested IF/ELSE/ENDIF.
    // `true` = currently executing, `false` = skipping.
    let mut exec_stack: Vec<bool> = Vec::new();

    let executing = |es: &[bool]| -> bool { es.iter().all(|b| *b) };

    let mut ip = 0;
    while ip < instructions.len() {
        let instr = &instructions[ip];
        ip = ip.wrapping_add(1);

        // ---- Gas charge ----
        if !gas.charge(instr.op) {
            return Err(ScriptError::OutOfGas);
        }

        // ---- Conditional flow handling ----
        // IF/ELSE/ENDIF must be tracked even when we're skipping.
        match instr.op {
            OpCode::If => {
                if executing(&exec_stack) {
                    let cond = pop(&mut stack)?;
                    exec_stack.push(cond.is_truthy());
                } else {
                    exec_stack.push(false);
                }
                continue;
            }
            OpCode::Else => {
                // Compute parent_exec BEFORE taking the mutable borrow on exec_stack.
                let exec_len = exec_stack.len();
                let parent_exec = exec_len < 2
                    || exec_stack[..exec_len.wrapping_sub(2)]
                        .iter()
                        .all(|b| *b)
                    // check parent (all but last)
                    && exec_stack
                        .iter()
                        .take(exec_len.wrapping_sub(1))
                        .all(|b| *b);
                let top = exec_stack
                    .last_mut()
                    .ok_or(ScriptError::UnbalancedConditional("ELSE without IF"))?;
                if parent_exec {
                    *top = !*top;
                }
                continue;
            }
            OpCode::EndIf => {
                exec_stack
                    .pop()
                    .ok_or(ScriptError::UnbalancedConditional("ENDIF without IF"))?;
                continue;
            }
            _ => {}
        }

        // Skip non-control opcodes when inside a false branch.
        if !executing(&exec_stack) {
            continue;
        }

        // ---- Execute the opcode ----
        match instr.op {
            // -- Constants --
            OpCode::Op0 => push(&mut stack, Value::Int(0))?,
            OpCode::Op1 => push(&mut stack, Value::Int(1))?,
            OpCode::Push1 | OpCode::Push2 | OpCode::Push4 => {
                push(&mut stack, Value::Bytes(instr.data.clone()))?;
            }
            OpCode::PushInt => {
                let n = i64::from_le_bytes(
                    instr.data[..8]
                        .try_into()
                        .map_err(|_| ScriptError::TypeError)?,
                );
                push(&mut stack, Value::Int(n))?;
            }

            // -- Stack manipulation --
            OpCode::Dup => {
                let a = peek(&stack)?;
                push(&mut stack, a)?;
            }
            OpCode::Swap => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, b)?;
                push(&mut stack, a)?;
            }
            OpCode::Drop => {
                pop(&mut stack)?;
            }
            OpCode::Pick => {
                let n = pop_int(&mut stack)? as usize;
                let len = stack.len();
                if n >= len {
                    return Err(ScriptError::StackUnderflow);
                }
                let val = stack[len.wrapping_sub(1).wrapping_sub(n)].clone();
                push(&mut stack, val)?;
            }
            OpCode::Roll => {
                let n = pop_int(&mut stack)? as usize;
                let len = stack.len();
                if n >= len {
                    return Err(ScriptError::StackUnderflow);
                }
                let val = stack.remove(len.wrapping_sub(1).wrapping_sub(n));
                push(&mut stack, val)?;
            }
            OpCode::Over => {
                if stack.len() < 2 {
                    return Err(ScriptError::StackUnderflow);
                }
                let val = stack[stack.len().wrapping_sub(2)].clone();
                push(&mut stack, val)?;
            }
            OpCode::Rot => {
                // a b c → b c a
                if stack.len() < 3 {
                    return Err(ScriptError::StackUnderflow);
                }
                let len = stack.len();
                let a = stack.remove(len.wrapping_sub(3));
                push(&mut stack, a)?;
            }
            OpCode::Dup2 => {
                if stack.len() < 2 {
                    return Err(ScriptError::StackUnderflow);
                }
                let a = stack[stack.len().wrapping_sub(2)].clone();
                let b = stack[stack.len().wrapping_sub(1)].clone();
                push(&mut stack, a)?;
                push(&mut stack, b)?;
            }

            // -- Arithmetic (wrapping i64) --
            OpCode::Add => bin_int_op(&mut stack, |a, b| a.wrapping_add(b))?,
            OpCode::Sub => bin_int_op(&mut stack, |a, b| a.wrapping_sub(b))?,
            OpCode::Mul => bin_int_op(&mut stack, |a, b| a.wrapping_mul(b))?,
            OpCode::Div => {
                let b = pop_int(&mut stack)?;
                let a = pop_int(&mut stack)?;
                if b == 0 {
                    return Err(ScriptError::DivisionByZero);
                }
                push(&mut stack, Value::Int(a.wrapping_div(b)))?;
            }
            OpCode::Mod => {
                let b = pop_int(&mut stack)?;
                let a = pop_int(&mut stack)?;
                if b == 0 {
                    return Err(ScriptError::DivisionByZero);
                }
                push(&mut stack, Value::Int(a.wrapping_rem(b)))?;
            }
            OpCode::Neg => {
                let a = pop_int(&mut stack)?;
                push(&mut stack, Value::Int(a.wrapping_neg()))?;
            }
            OpCode::Abs => {
                let a = pop_int(&mut stack)?;
                push(&mut stack, Value::Int(a.wrapping_abs()))?;
            }
            OpCode::Min => bin_int_op(&mut stack, core::cmp::min)?,
            OpCode::Max => bin_int_op(&mut stack, core::cmp::max)?,

            // -- Comparison --
            OpCode::Eq => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, Value::from(a == b))?;
            }
            OpCode::Neq => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, Value::from(a != b))?;
            }
            OpCode::Lt => cmp_int_op(&mut stack, |a, b| a < b)?,
            OpCode::Gt => cmp_int_op(&mut stack, |a, b| a > b)?,
            OpCode::Le => cmp_int_op(&mut stack, |a, b| a <= b)?,
            OpCode::Ge => cmp_int_op(&mut stack, |a, b| a >= b)?,
            OpCode::Not => {
                let a = pop(&mut stack)?;
                push(&mut stack, Value::from(!a.is_truthy()))?;
            }
            OpCode::And => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, Value::from(a.is_truthy() && b.is_truthy()))?;
            }
            OpCode::Or => {
                let b = pop(&mut stack)?;
                let a = pop(&mut stack)?;
                push(&mut stack, Value::from(a.is_truthy() || b.is_truthy()))?;
            }

            // -- Control flow (IF/ELSE/ENDIF handled above) --
            OpCode::Verify => {
                let a = pop(&mut stack)?;
                if !a.is_truthy() {
                    return Err(ScriptError::VerifyFailed);
                }
            }
            OpCode::Return => {
                return Err(ScriptError::Returned);
            }

            // -- Crypto --
            OpCode::CheckSigPqc => {
                // Stack: ... msg sig pubkey → ... result
                let pk_bytes = pop_bytes(&mut stack)?;
                let sig_bytes = pop_bytes(&mut stack)?;
                let msg = pop_bytes(&mut stack)?;

                let valid = checksig_pqc(&pk_bytes, &sig_bytes, &msg);
                push(&mut stack, Value::from(valid))?;
            }
            OpCode::CheckMultiSigPqc => {
                // Stack: ... m pk1..pkN sig1..sigM → ... result
                // Pop N (number of public keys) then M (threshold)
                let n_keys = pop_int(&mut stack)? as usize;
                // Charge per-key gas
                if !gas.consume(MULTISIG_PER_KEY_COST.saturating_mul(n_keys as u64)) {
                    return Err(ScriptError::OutOfGas);
                }
                let mut pks = Vec::with_capacity(n_keys);
                for _ in 0..n_keys {
                    pks.push(pop_bytes(&mut stack)?);
                }
                let m_sigs = pop_int(&mut stack)? as usize;
                let mut sigs = Vec::with_capacity(m_sigs);
                for _ in 0..m_sigs {
                    sigs.push(pop_bytes(&mut stack)?);
                }
                let msg = pop_bytes(&mut stack)?;

                let valid = checkmultisig_pqc(&pks, &sigs, &msg, m_sigs);
                push(&mut stack, Value::from(valid))?;
            }
            OpCode::HashSha3 => {
                let data = pop_bytes(&mut stack)?;
                push(&mut stack, Value::Bytes(sha3_256(&data).to_vec()))?;
            }
            OpCode::HashBlake3 => {
                let data = pop_bytes(&mut stack)?;
                push(&mut stack, Value::Bytes(blake3(&data).to_vec()))?;
            }

            // -- Introspection --
            OpCode::ReadInputValue => {
                let i = pop_int(&mut stack)? as usize;
                let count = ctx.resolved_inputs.len();
                if i >= count {
                    return Err(ScriptError::IndexOutOfRange { index: i, count });
                }
                let amount: u64 = ctx.resolved_inputs[i].value.as_u64();
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(amount as i64))?;
            }
            OpCode::ReadOutputValue => {
                let i = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if i >= count {
                    return Err(ScriptError::IndexOutOfRange { index: i, count });
                }
                let amount: u64 = ctx.tx.outputs[i].value.as_u64();
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(amount as i64))?;
            }
            OpCode::ReadOutputScript => {
                let i = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if i >= count {
                    return Err(ScriptError::IndexOutOfRange { index: i, count });
                }
                push(
                    &mut stack,
                    Value::Bytes(ctx.tx.outputs[i].locking_script.as_bytes().to_vec()),
                )?;
            }
            OpCode::ReadOutputDatum => {
                let i = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if i >= count {
                    return Err(ScriptError::IndexOutOfRange { index: i, count });
                }
                let datum_bytes = ctx.tx.outputs[i]
                    .datum
                    .as_ref()
                    .map_or_else(Vec::new, |d| d.as_bytes().to_vec());
                push(&mut stack, Value::Bytes(datum_bytes))?;
            }
            OpCode::TxHash => {
                push(&mut stack, Value::Bytes(ctx.tx_hash.to_vec()))?;
            }
            OpCode::SigHash => {
                push(&mut stack, Value::Bytes(ctx.sighash.to_vec()))?;
            }
            OpCode::SlotNumber => {
                let slot: u64 = ctx.current_slot.as_u64();
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(slot as i64))?;
            }
            OpCode::InputCount => {
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(ctx.tx.inputs.len() as i64))?;
            }
            OpCode::OutputCount => {
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(ctx.tx.outputs.len() as i64))?;
            }
            OpCode::TxFee => {
                let fee: u64 = ctx.tx.fee.as_u64();
                #[allow(clippy::cast_possible_wrap)]
                push(&mut stack, Value::Int(fee as i64))?;
            }

            // -- Covenants --
            OpCode::AssertOutputScriptHash => {
                let hash_bytes = pop_bytes(&mut stack)?;
                let idx = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if idx >= count {
                    return Err(ScriptError::IndexOutOfRange { index: idx, count });
                }
                let actual = ctx.tx.outputs[idx].locking_script.hash();
                if actual.as_bytes() != hash_bytes.as_slice() {
                    return Err(ScriptError::CovenantFailed("output script hash mismatch"));
                }
            }
            OpCode::AssertDatumHash => {
                let hash_bytes = pop_bytes(&mut stack)?;
                let idx = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if idx >= count {
                    return Err(ScriptError::IndexOutOfRange { index: idx, count });
                }
                let actual = ctx.tx.outputs[idx].datum.as_ref().map(Datum::hash);
                match actual {
                    Some(h) if h.as_bytes() == hash_bytes.as_slice() => {}
                    _ => return Err(ScriptError::CovenantFailed("datum hash mismatch")),
                }
            }
            OpCode::AssertValue => {
                let expected_amount = pop_int(&mut stack)?;
                let idx = pop_int(&mut stack)? as usize;
                let count = ctx.tx.outputs.len();
                if idx >= count {
                    return Err(ScriptError::IndexOutOfRange { index: idx, count });
                }
                let actual: u64 = ctx.tx.outputs[idx].value.as_u64();
                #[allow(clippy::cast_possible_wrap)]
                if (actual as i64) != expected_amount {
                    return Err(ScriptError::CovenantFailed("output value mismatch"));
                }
            }

            // -- Data ops --
            OpCode::Cat => {
                let b = pop_bytes(&mut stack)?;
                let a = pop_bytes(&mut stack)?;
                let mut out = a;
                out.extend_from_slice(&b);
                push(&mut stack, Value::Bytes(out))?;
            }
            OpCode::Slice => {
                let len = pop_int(&mut stack)? as usize;
                let start = pop_int(&mut stack)? as usize;
                let data = pop_bytes(&mut stack)?;
                let end = start.saturating_add(len);
                if end > data.len() {
                    return Err(ScriptError::SliceOutOfBounds);
                }
                push(&mut stack, Value::Bytes(data[start..end].to_vec()))?;
            }
            OpCode::Len => {
                let a = pop(&mut stack)?;
                let l = match &a {
                    Value::Int(_) => 8_i64,
                    Value::Bytes(b) => b.len() as i64,
                };
                push(&mut stack, Value::Int(l))?;
            }

            // -- Meta --
            OpCode::Nop => {}

            // IF/ELSE/ENDIF already handled above
            OpCode::If | OpCode::Else | OpCode::EndIf => unreachable!(),
        }
    }

    // Check balanced conditionals
    if !exec_stack.is_empty() {
        return Err(ScriptError::UnbalancedConditional("unterminated IF block"));
    }

    // Script succeeds iff the top-of-stack is truthy (or stack is empty → fail).
    let success = stack.last().is_some_and(Value::is_truthy);

    Ok(ExecResult {
        success,
        gas_used: gas.consumed(),
        final_stack: stack,
    })
}

// ============================================================================
// Stack helpers
// ============================================================================

fn push(stack: &mut Vec<Value>, val: Value) -> Result<(), ScriptError> {
    if stack.len() >= OpCode::MAX_STACK_DEPTH {
        return Err(ScriptError::StackOverflow(OpCode::MAX_STACK_DEPTH));
    }
    stack.push(val);
    Ok(())
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, ScriptError> {
    stack.pop().ok_or(ScriptError::StackUnderflow)
}

fn peek(stack: &[Value]) -> Result<Value, ScriptError> {
    stack.last().cloned().ok_or(ScriptError::StackUnderflow)
}

fn pop_int(stack: &mut Vec<Value>) -> Result<i64, ScriptError> {
    match pop(stack)? {
        Value::Int(n) => Ok(n),
        Value::Bytes(_) => Err(ScriptError::TypeError),
    }
}

fn pop_bytes(stack: &mut Vec<Value>) -> Result<Vec<u8>, ScriptError> {
    match pop(stack)? {
        Value::Bytes(b) => Ok(b),
        Value::Int(n) => Ok(n.to_le_bytes().to_vec()),
    }
}

fn bin_int_op(stack: &mut Vec<Value>, f: impl FnOnce(i64, i64) -> i64) -> Result<(), ScriptError> {
    let b = pop_int(stack)?;
    let a = pop_int(stack)?;
    push(stack, Value::Int(f(a, b)))
}

fn cmp_int_op(stack: &mut Vec<Value>, f: impl FnOnce(i64, i64) -> bool) -> Result<(), ScriptError> {
    let b = pop_int(stack)?;
    let a = pop_int(stack)?;
    push(stack, Value::from(f(a, b)))
}

// ============================================================================
// Crypto helpers
// ============================================================================

/// Verify a single PQC signature. Returns `true` on valid, `false` on
/// any error (malformed key, bad sig, etc.) — never panics.
fn checksig_pqc(pk_bytes: &[u8], sig_bytes: &[u8], msg: &[u8]) -> bool {
    // Try Level 3 first (the default), then Level 2, then Level 5.
    for level in [
        DilithiumLevel::Level3,
        DilithiumLevel::Level2,
        DilithiumLevel::Level5,
    ] {
        if let Ok(pk) = PqcPublicKey::from_bytes(level, pk_bytes) {
            if let Ok(sig) = PqcSignature::from_bytes(level, sig_bytes) {
                // verify_pqc returns Ok(true) for valid, Ok(false) for invalid,
                // Err for parameter mismatch. Only accept Ok(true).
                if matches!(verify_pqc(&pk, msg, &sig), Ok(true)) {
                    return true;
                }
            }
        }
    }
    false
}

/// M-of-N PQC multi-signature check. Each sig must match a distinct pk.
fn checkmultisig_pqc(pks: &[Vec<u8>], sigs: &[Vec<u8>], msg: &[u8], threshold: usize) -> bool {
    if threshold > pks.len() || sigs.len() < threshold {
        return false;
    }
    let mut used = vec![false; pks.len()];
    let mut valid_count = 0_usize;
    for sig in sigs {
        for (i, pk) in pks.iter().enumerate() {
            if used[i] {
                continue;
            }
            if checksig_pqc(pk, sig, msg) {
                used[i] = true;
                valid_count = valid_count.wrapping_add(1);
                break;
            }
        }
    }
    valid_count >= threshold
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
    use crate::gas::GasMeter;
    use crate::opcode::{encode_instructions, Instruction, OpCode};
    use qv_core::{
        Amount, OutPoint, Script as CoreScript, Slot, Transaction, TxId, TxInput, TxOutput,
    };

    fn dummy_ctx() -> Context {
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![
                TxOutput::new(Amount::from(100), CoreScript::new(vec![0xAA])),
                TxOutput::new(Amount::from(200), CoreScript::new(vec![0xBB])),
            ],
        )
        .with_fee(Amount::from(10));
        let resolved = vec![TxOutput::new(
            Amount::from(310),
            CoreScript::new(vec![0xCC]),
        )];
        Context::new(tx, resolved, Slot::from(42))
    }

    fn run(ops: &[Instruction]) -> Result<ExecResult, ScriptError> {
        let bytes = encode_instructions(ops);
        let ctx = dummy_ctx();
        let mut gas = GasMeter::new(100_000);
        execute(&bytes, &ctx, &mut gas)
    }

    // -- Basic stack + arithmetic --

    #[test]
    fn op1_dup_add_gives_two() {
        let result = run(&[
            Instruction::simple(OpCode::Op1),
            Instruction::simple(OpCode::Dup),
            Instruction::simple(OpCode::Add),
        ])
        .unwrap();
        assert!(result.success); // top = 2 → truthy
        assert_eq!(result.final_stack, vec![Value::Int(2)]);
    }

    #[test]
    fn push_int_and_sub() {
        let result = run(&[
            Instruction::push_int(10),
            Instruction::push_int(3),
            Instruction::simple(OpCode::Sub),
        ])
        .unwrap();
        assert_eq!(result.final_stack, vec![Value::Int(7)]);
    }

    #[test]
    fn division_by_zero() {
        let err = run(&[
            Instruction::push_int(10),
            Instruction::push_int(0),
            Instruction::simple(OpCode::Div),
        ])
        .unwrap_err();
        assert_eq!(err, ScriptError::DivisionByZero);
    }

    #[test]
    fn wrapping_arithmetic() {
        let result = run(&[
            Instruction::push_int(i64::MAX),
            Instruction::push_int(1),
            Instruction::simple(OpCode::Add),
        ])
        .unwrap();
        // i64::MAX + 1 wraps to i64::MIN
        assert_eq!(result.final_stack, vec![Value::Int(i64::MIN)]);
    }

    // -- Comparison --

    #[test]
    fn eq_and_neq() {
        let r = run(&[
            Instruction::push_int(5),
            Instruction::push_int(5),
            Instruction::simple(OpCode::Eq),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(1)]);

        let r = run(&[
            Instruction::push_int(5),
            Instruction::push_int(6),
            Instruction::simple(OpCode::Neq),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(1)]);
    }

    #[test]
    fn lt_gt() {
        let r = run(&[
            Instruction::push_int(3),
            Instruction::push_int(5),
            Instruction::simple(OpCode::Lt),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(1)]);
    }

    // -- Control flow --

    #[test]
    fn if_true_branch() {
        let result = run(&[
            Instruction::simple(OpCode::Op1), // condition = true
            Instruction::simple(OpCode::If),
            Instruction::push_int(42), // true branch
            Instruction::simple(OpCode::Else),
            Instruction::push_int(99), // false branch
            Instruction::simple(OpCode::EndIf),
        ])
        .unwrap();
        assert_eq!(result.final_stack, vec![Value::Int(42)]);
    }

    #[test]
    fn if_false_branch() {
        let result = run(&[
            Instruction::simple(OpCode::Op0), // condition = false
            Instruction::simple(OpCode::If),
            Instruction::push_int(42),
            Instruction::simple(OpCode::Else),
            Instruction::push_int(99),
            Instruction::simple(OpCode::EndIf),
        ])
        .unwrap();
        assert_eq!(result.final_stack, vec![Value::Int(99)]);
    }

    #[test]
    fn unbalanced_endif() {
        let err = run(&[Instruction::simple(OpCode::EndIf)]).unwrap_err();
        assert!(matches!(err, ScriptError::UnbalancedConditional(_)));
    }

    #[test]
    fn unterminated_if() {
        let err = run(&[
            Instruction::simple(OpCode::Op1),
            Instruction::simple(OpCode::If),
            Instruction::push_int(1),
        ])
        .unwrap_err();
        assert!(matches!(err, ScriptError::UnbalancedConditional(_)));
    }

    // -- Verify / Return --

    #[test]
    fn verify_pass() {
        let r = run(&[
            Instruction::simple(OpCode::Op1),
            Instruction::simple(OpCode::Verify),
            Instruction::simple(OpCode::Op1), // final truthy
        ])
        .unwrap();
        assert!(r.success);
    }

    #[test]
    fn verify_fail() {
        let err = run(&[
            Instruction::simple(OpCode::Op0),
            Instruction::simple(OpCode::Verify),
        ])
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn return_aborts() {
        let err = run(&[Instruction::simple(OpCode::Return)]).unwrap_err();
        assert_eq!(err, ScriptError::Returned);
    }

    // -- Introspection --

    #[test]
    fn read_output_value() {
        let r = run(&[
            Instruction::push_int(1), // output index 1
            Instruction::simple(OpCode::ReadOutputValue),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(200)]);
    }

    #[test]
    fn input_output_count() {
        let r = run(&[
            Instruction::simple(OpCode::InputCount),
            Instruction::simple(OpCode::OutputCount),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(1), Value::Int(2)]);
    }

    #[test]
    fn slot_number() {
        let r = run(&[Instruction::simple(OpCode::SlotNumber)]).unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(42)]);
    }

    #[test]
    fn tx_fee() {
        let r = run(&[Instruction::simple(OpCode::TxFee)]).unwrap();
        assert_eq!(r.final_stack, vec![Value::Int(10)]);
    }

    #[test]
    fn sighash_opcode_pushes_context_sighash() {
        let ctx = dummy_ctx();
        let bytes = encode_instructions(&[Instruction::simple(OpCode::SigHash)]);
        let mut gas = GasMeter::new(100_000);
        let r = execute(&bytes, &ctx, &mut gas).unwrap();
        assert_eq!(r.final_stack, vec![Value::Bytes(ctx.sighash.to_vec())]);
        // The sighash is witness-excluded, so for a witness-less tx it equals
        // the tx_hash; the meaningful divergence is exercised in qv-core tests.
        assert_eq!(ctx.sighash.len(), 32);
    }

    // -- Hashing --

    #[test]
    fn hash_sha3_produces_32_bytes() {
        let r = run(&[
            Instruction::push_bytes(b"hello".to_vec()),
            Instruction::simple(OpCode::HashSha3),
        ])
        .unwrap();
        match &r.final_stack[0] {
            Value::Bytes(b) => assert_eq!(b.len(), 32),
            _ => panic!("expected Bytes"),
        }
    }

    // -- Data ops --

    #[test]
    fn cat_and_len() {
        let r = run(&[
            Instruction::push_bytes(vec![1, 2]),
            Instruction::push_bytes(vec![3, 4, 5]),
            Instruction::simple(OpCode::Cat),
            Instruction::simple(OpCode::Dup),
            Instruction::simple(OpCode::Len),
        ])
        .unwrap();
        assert_eq!(
            r.final_stack,
            vec![Value::Bytes(vec![1, 2, 3, 4, 5]), Value::Int(5)]
        );
    }

    #[test]
    fn slice_ok() {
        let r = run(&[
            Instruction::push_bytes(vec![10, 20, 30, 40, 50]),
            Instruction::push_int(1), // start
            Instruction::push_int(3), // len
            Instruction::simple(OpCode::Slice),
        ])
        .unwrap();
        assert_eq!(r.final_stack, vec![Value::Bytes(vec![20, 30, 40])]);
    }

    #[test]
    fn slice_out_of_bounds() {
        let err = run(&[
            Instruction::push_bytes(vec![10, 20]),
            Instruction::push_int(0),
            Instruction::push_int(5),
            Instruction::simple(OpCode::Slice),
        ])
        .unwrap_err();
        assert_eq!(err, ScriptError::SliceOutOfBounds);
    }

    // -- Gas exhaustion --

    #[test]
    fn gas_exhaustion() {
        let bytes = encode_instructions(&[
            Instruction::simple(OpCode::Op1),
            Instruction::simple(OpCode::Op1),
            Instruction::simple(OpCode::Add),
        ]);
        let ctx = dummy_ctx();
        let mut gas = GasMeter::new(5); // 1+1+5 = 7 needed, only 5 available
        let err = execute(&bytes, &ctx, &mut gas).unwrap_err();
        assert_eq!(err, ScriptError::OutOfGas);
    }

    // -- Stack overflow --

    #[test]
    fn stack_overflow() {
        // Push 1025 items (max is 1024)
        let mut ops: Vec<Instruction> = Vec::new();
        for _ in 0..=OpCode::MAX_STACK_DEPTH {
            ops.push(Instruction::simple(OpCode::Op1));
        }
        let err = run(&ops).unwrap_err();
        assert!(matches!(err, ScriptError::StackOverflow(_)));
    }

    // -- Empty stack → not success --

    #[test]
    fn empty_script_fails() {
        let r = run(&[]).unwrap();
        assert!(!r.success);
    }

    // -- Stack manipulation: over, rot, dup2, pick, roll --

    #[test]
    fn over_copies_second() {
        let r = run(&[
            Instruction::push_int(10),
            Instruction::push_int(20),
            Instruction::simple(OpCode::Over),
        ])
        .unwrap();
        assert_eq!(
            r.final_stack,
            vec![Value::Int(10), Value::Int(20), Value::Int(10)]
        );
    }

    #[test]
    fn rot_rotates_top_three() {
        let r = run(&[
            Instruction::push_int(1),
            Instruction::push_int(2),
            Instruction::push_int(3),
            Instruction::simple(OpCode::Rot),
        ])
        .unwrap();
        // 1 2 3 → 2 3 1
        assert_eq!(
            r.final_stack,
            vec![Value::Int(2), Value::Int(3), Value::Int(1)]
        );
    }
}
