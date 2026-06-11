//! Per-opcode gas cost table and metering.
//!
//! Every script execution has a finite gas budget. Each opcode consumes a
//! known number of gas units *before* it executes. Execution halts with
//! [`ScriptError::OutOfGas`](super::ScriptError) when the budget is exhausted.
//!
//! # Cost rationale
//!
//! | Tier     | Cost | Opcodes                                      |
//! |----------|------|----------------------------------------------|
//! | Trivial  |    1 | `NOP`, `OP_0`, `OP_1`, `DROP`, constants     |
//! | Cheap    |    2 | Stack ops, comparisons, logic, control flow   |
//! | Medium   |    5 | Arithmetic (wrapping, no alloc)               |
//! | Data     |   10 | `CAT`, `SLICE`, `LEN`, PUSH, introspection   |
//! | Hash     |   50 | `HASH_SHA3`, `HASH_BLAKE3`                   |
//! | Sig      |  500 | `CHECKSIG_PQC`                               |
//! | MultiSig | 2000 | `CHECKMULTISIG_PQC` (base; +500 per key)     |
//! | Covenant |   20 | `ASSERT_OUTPUT_SCRIPT_HASH`, `ASSERT_*`      |

use crate::opcode::OpCode;

/// Gas units consumed by a single opcode execution.
///
/// For `CheckMultiSigPqc` this is the *base* cost; the interpreter adds
/// 500 units per public key involved.
#[must_use]
pub const fn gas_cost(op: OpCode) -> u64 {
    match op {
        // ---- Trivial (1) ----
        OpCode::Op0 | OpCode::Op1 | OpCode::Nop | OpCode::Drop => 1,

        // ---- Cheap (2) ----
        OpCode::Dup
        | OpCode::Swap
        | OpCode::Over
        | OpCode::Rot
        | OpCode::Dup2
        | OpCode::Pick
        | OpCode::Roll
        | OpCode::Eq
        | OpCode::Neq
        | OpCode::Lt
        | OpCode::Gt
        | OpCode::Le
        | OpCode::Ge
        | OpCode::Not
        | OpCode::And
        | OpCode::Or
        | OpCode::If
        | OpCode::Else
        | OpCode::EndIf
        | OpCode::Verify
        | OpCode::Return => 2,

        // ---- Medium (5) ----
        OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::Mod
        | OpCode::Neg
        | OpCode::Abs
        | OpCode::Min
        | OpCode::Max => 5,

        // ---- Wide arithmetic / wide comparison (15) ----
        // u128 product + 16-byte LE encode, or 16-byte LE compare —
        // each does a bit more work than 64-bit arithmetic but is still
        // far cheaper than a hash.
        OpCode::MulU128 | OpCode::GeU128 => 15,

        // ---- Data / push / introspection (10) ----
        OpCode::Push1
        | OpCode::Push2
        | OpCode::Push4
        | OpCode::PushInt
        | OpCode::Cat
        | OpCode::Slice
        | OpCode::Len
        | OpCode::BytesToInt
        | OpCode::ReadInputValue
        | OpCode::ReadOutputValue
        | OpCode::ReadOutputScript
        | OpCode::ReadOutputDatum
        | OpCode::ReadInputDatum
        | OpCode::SelfScriptHash
        | OpCode::TxHash
        | OpCode::SlotNumber
        | OpCode::InputCount
        | OpCode::OutputCount
        | OpCode::TxFee
        | OpCode::SigHash => 10,

        // ---- Covenant assertions (20) ----
        OpCode::AssertOutputScriptHash | OpCode::AssertDatumHash | OpCode::AssertValue => 20,

        // ---- Hashing (50) ----
        OpCode::HashSha3 | OpCode::HashBlake3 => 50,

        // ---- Signature verification (500) ----
        OpCode::CheckSigPqc => 500,

        // ---- Multi-sig base (2000) ----
        OpCode::CheckMultiSigPqc => 2000,
    }
}

/// Default maximum gas budget for a single script execution.
pub const DEFAULT_GAS_LIMIT: u64 = 100_000;

/// Additional gas charged per public key in `CHECKMULTISIG_PQC`.
pub const MULTISIG_PER_KEY_COST: u64 = 500;

/// Gas meter: tracks remaining budget during script execution.
#[derive(Debug, Clone)]
pub struct GasMeter {
    /// Gas units remaining.
    remaining: u64,
    /// Total gas consumed so far.
    consumed: u64,
}

impl GasMeter {
    /// Create a new meter with the given budget.
    #[must_use]
    pub const fn new(limit: u64) -> Self {
        Self {
            remaining: limit,
            consumed: 0,
        }
    }

    /// Create a meter with the [`DEFAULT_GAS_LIMIT`].
    #[must_use]
    pub const fn default_limit() -> Self {
        Self::new(DEFAULT_GAS_LIMIT)
    }

    /// Try to consume `amount` gas units. Returns `true` if there was
    /// enough budget, `false` (and does not deduct) if not.
    pub fn consume(&mut self, amount: u64) -> bool {
        if self.remaining >= amount {
            self.remaining = self.remaining.wrapping_sub(amount);
            self.consumed = self.consumed.wrapping_add(amount);
            true
        } else {
            false
        }
    }

    /// Charge gas for the given opcode. Returns `false` on exhaustion.
    pub fn charge(&mut self, op: OpCode) -> bool {
        self.consume(gas_cost(op))
    }

    /// Gas units still available.
    #[must_use]
    pub const fn remaining(&self) -> u64 {
        self.remaining
    }

    /// Total gas units consumed since creation.
    #[must_use]
    pub const fn consumed(&self) -> u64 {
        self.consumed
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn trivial_opcodes_cost_one() {
        assert_eq!(gas_cost(OpCode::Op0), 1);
        assert_eq!(gas_cost(OpCode::Op1), 1);
        assert_eq!(gas_cost(OpCode::Nop), 1);
        assert_eq!(gas_cost(OpCode::Drop), 1);
    }

    #[test]
    fn crypto_opcodes_are_expensive() {
        assert_eq!(gas_cost(OpCode::CheckSigPqc), 500);
        assert_eq!(gas_cost(OpCode::CheckMultiSigPqc), 2000);
        assert!(gas_cost(OpCode::HashSha3) > gas_cost(OpCode::Add));
    }

    #[test]
    fn meter_deducts_correctly() {
        let mut m = GasMeter::new(100);
        assert!(m.consume(60));
        assert_eq!(m.remaining(), 40);
        assert_eq!(m.consumed(), 60);
        assert!(m.consume(40));
        assert_eq!(m.remaining(), 0);
        assert!(!m.consume(1));
        assert_eq!(m.consumed(), 100);
    }

    #[test]
    fn meter_charge_opcode() {
        let mut m = GasMeter::new(10);
        assert!(m.charge(OpCode::Op0)); // 1
        assert!(m.charge(OpCode::Add)); // 5
        assert_eq!(m.consumed(), 6);
        assert!(!m.charge(OpCode::Add)); // need 5, have 4
        assert_eq!(m.consumed(), 6); // unchanged
    }

    #[test]
    fn default_limit_is_100k() {
        let m = GasMeter::default_limit();
        assert_eq!(m.remaining(), DEFAULT_GAS_LIMIT);
        assert_eq!(m.consumed(), 0);
    }
}
