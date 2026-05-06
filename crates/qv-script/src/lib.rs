//! # qv-script
//!
//! Covenant-capable, stack-based Script VM for QuantumVault L1.
//!
//! Scripts are **validated, never executed** in the Turing-complete sense.
//! A script answers exactly one question: *"may this UTXO be spent by this
//! transaction?"* The interpreter runs deterministically — no floats,
//! wrapping integer arithmetic, bounded gas, bounded stack depth.
//!
//! ## Modules
//!
//! - [`opcode`] — the complete opcode set (`OpCode` enum), `Value` stack
//!   element, `Instruction` type, encode/decode helpers.
//! - [`gas`] — per-opcode gas cost table and [`GasMeter`].
//! - [`interpreter`] — the stack-based VM core: [`execute`] /
//!   [`execute_instructions`], [`Context`], [`ExecResult`].
//! - [`templates`] — [`ScriptBuilder`] fluent API and standard templates
//!   (`p2pkh_pqc`, `multisig_pqc`, `amm_swap`, `lending_repay`).
//! - [`script`] — high-level entry points: [`validate_script`] (the one
//!   function the ledger calls), [`disassemble`], [`compile`].
//!
//! ## Quick start (validation pipeline)
//!
//! ```ignore
//! use qv_script::validate_script;
//!
//! let result = validate_script(
//!     &utxo.locking_script,
//!     input.witness.as_bytes(),
//!     &tx,
//!     &resolved_inputs,
//!     current_slot,
//! )?;
//! assert!(result.success);
//! ```

#![forbid(unsafe_code)]

pub mod gas;
pub mod interpreter;
pub mod opcode;
pub mod script;
pub mod templates;

// ---------------------------------------------------------------------------
// Re-exports: the stable public surface of the crate.
// ---------------------------------------------------------------------------

/// Stack value representation, opcodes, encoding/decoding; see [`opcode`] module.
pub use opcode::{decode_script, encode_instructions, Instruction, OpCode, OpcodeError, Value};

/// Per-opcode gas costs and gas metering; see [`gas`] module.
pub use gas::{gas_cost, GasMeter, DEFAULT_GAS_LIMIT, MULTISIG_PER_KEY_COST};

/// Script interpreter VM and execution context; see [`interpreter`] module.
pub use interpreter::{execute, execute_instructions, Context, ExecResult, ScriptError};

/// Standard script templates (P2PKH, multisig, AMM, lending); see [`templates`] module.
pub use templates::{amm_swap, lending_repay, multisig_pqc, p2pkh_pqc, pubkey_hash, ScriptBuilder};

/// High-level script validation and compilation; see [`script`] module.
pub use script::{compile, disassemble, validate_script, validate_script_with_gas};

// ---------------------------------------------------------------------------
// Aggregate error
// ---------------------------------------------------------------------------

use thiserror::Error;

/// Aggregate error for the `qv-script` crate.
///
/// Merges opcode-level and interpreter-level errors into a single type
/// for consumers that just want to propagate "script failed".
///
/// # Examples
///
/// ```rust
/// # use qv_script::{ScriptCrateError, ScriptError};
/// fn handle_script_error(e: ScriptCrateError) {
///     match e {
///         ScriptCrateError::Opcode(op_err) => eprintln!("opcode error: {}", op_err),
///         ScriptCrateError::Exec(ScriptError::OutOfGas) => eprintln!("script exceeded gas limit"),
///         ScriptCrateError::Exec(e) => eprintln!("execution failed: {}", e),
///     }
/// }
/// ```
#[derive(Debug, Error)]
pub enum ScriptCrateError {
    /// Opcode encoding / decoding error.
    #[error(transparent)]
    Opcode(#[from] OpcodeError),

    /// Script execution error.
    #[error(transparent)]
    Exec(#[from] ScriptError),
}

/// Convenience alias.
pub type ScriptResult<T> = Result<T, ScriptCrateError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn public_surface_reachable() {
        let _: Option<OpCode> = None;
        let _: Option<Value> = None;
        let _: Option<Instruction> = None;
        let _: Option<GasMeter> = None;
        let _: Option<Context> = None;
        let _: Option<ExecResult> = None;
        let _: Option<ScriptBuilder> = None;
        let _: Option<ScriptCrateError> = None;
    }

    #[test]
    fn aggregate_error_wraps_opcode() {
        let e: ScriptCrateError = OpcodeError::UnknownOpcode(0xFE).into();
        assert!(matches!(e, ScriptCrateError::Opcode(_)));
    }

    #[test]
    fn aggregate_error_wraps_script() {
        let e: ScriptCrateError = ScriptError::OutOfGas.into();
        assert!(matches!(e, ScriptCrateError::Exec(_)));
    }
}
