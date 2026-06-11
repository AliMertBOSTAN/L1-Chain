//! Opcode set for the QuantumVault Script VM.
//!
//! The VM is **stack-based and deterministic**: no floats, overflow wraps,
//! gas-limited. Scripts are validated (not executed in the Turing-complete
//! sense) — they answer one question: "may this UTXO be spent?"
//!
//! # Opcode categories
//!
//! | Range     | Category       | Examples                                  |
//! |-----------|----------------|-------------------------------------------|
//! | 0x00–0x0F | Constants      | `OP_0`, `OP_1`, `PUSH1`–`PUSH4`          |
//! | 0x10–0x1F | Stack          | `DUP`, `SWAP`, `DROP`, `PICK`, `ROLL`     |
//! | 0x20–0x2F | Arithmetic     | `ADD`, `SUB`, `MUL`, `DIV`, `MOD`, `NEG`  |
//! | 0x30–0x3F | Compare        | `EQ`, `NEQ`, `LT`, `GT`, `LE`, `GE`       |
//! | 0x40–0x4F | Logic/Control  | `IF`, `ELSE`, `ENDIF`, `VERIFY`, `RETURN` |
//! | 0x50–0x5F | Crypto         | `CHECKSIG_PQC`, `CHECKMULTISIG_PQC`, …    |
//! | 0x60–0x6F | Introspection  | `READ_INPUT_VALUE`, `TX_HASH`, …          |
//! | 0x70–0x7F | Covenant       | `ASSERT_OUTPUT_SCRIPT_HASH`, …            |
//! | 0x80–0x8F | Data           | `CAT`, `SLICE`, `LEN`                     |
//! | 0xFF      | Meta           | `NOP`                                     |

use core::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Errors
// ============================================================================

/// Errors arising from opcode encoding / decoding.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum OpcodeError {
    /// The byte does not map to any known opcode.
    #[error("unknown opcode byte: 0x{0:02x}")]
    UnknownOpcode(u8),

    /// A PUSH instruction requires more data bytes than remain in the script.
    #[error("unexpected end of script: need {need} bytes, have {have}")]
    UnexpectedEnd {
        /// Bytes required.
        need: usize,
        /// Bytes available.
        have: usize,
    },

    /// A PUSH instruction carries zero length (invalid).
    #[error("push length must be > 0")]
    ZeroPush,

    /// Script exceeds the maximum allowed size.
    #[error("script too large: {size} bytes (max {max})")]
    ScriptTooLarge {
        /// Observed size.
        size: usize,
        /// Maximum allowed.
        max: usize,
    },
}

// ============================================================================
// Value — the VM stack element
// ============================================================================

/// A stack element in the Script VM.
///
/// All values on the stack are either a 64-bit integer or a variable-length
/// byte vector. Arithmetic opcodes operate on `Int`; crypto and introspection
/// opcodes may produce or consume `Bytes`.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Value {
    /// A 64-bit signed integer (wrapping arithmetic, no overflow traps).
    Int(i64),
    /// An opaque byte string.
    Bytes(Vec<u8>),
}

impl Value {
    /// Interpret as an integer, or `None` if the value is `Bytes`.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            Self::Bytes(_) => None,
        }
    }

    /// Interpret as bytes, or `None` if the value is `Int`.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            Self::Int(_) => None,
        }
    }

    /// Coerce to bool: zero or empty → false, anything else → true.
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Int(n) => *n != 0,
            Self::Bytes(b) => !b.is_empty(),
        }
    }

    /// Encode the value to its canonical byte representation for hashing.
    #[must_use]
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        match self {
            Self::Int(n) => n.to_le_bytes().to_vec(),
            Self::Bytes(b) => b.clone(),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "Int({n})"),
            Self::Bytes(b) => write!(f, "Bytes({} bytes)", b.len()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Bytes(b) => write!(f, "0x{}", hex::encode(b)),
        }
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Self::Int(n)
    }
}

impl From<Vec<u8>> for Value {
    fn from(b: Vec<u8>) -> Self {
        Self::Bytes(b)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Int(i64::from(b))
    }
}

// ============================================================================
// OpCode enum
// ============================================================================

/// A single instruction in the QuantumVault Script VM.
///
/// Each variant maps 1:1 to a byte (the discriminant). Multi-byte instructions
/// (the `PUSH*` family) are followed by a length prefix and the pushed data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpCode {
    // ---- Constants (0x00–0x0F) ----
    /// Push the integer 0 onto the stack.
    Op0 = 0x00,
    /// Push the integer 1 onto the stack.
    Op1 = 0x01,
    /// Next 1 byte is the data length N; the following N bytes are pushed.
    Push1 = 0x02,
    /// Next 2 bytes (LE) are the data length N; push N bytes.
    Push2 = 0x03,
    /// Next 4 bytes (LE) are the data length N; push N bytes.
    Push4 = 0x04,
    /// Push a specific signed integer (next 8 bytes, LE i64).
    PushInt = 0x05,

    // ---- Stack manipulation (0x10–0x1F) ----
    /// Duplicate the top stack element.
    Dup = 0x10,
    /// Swap the top two stack elements.
    Swap = 0x11,
    /// Remove the top stack element.
    Drop = 0x12,
    /// Copy the element N deep to the top (N from top of stack).
    Pick = 0x13,
    /// Move the element N deep to the top (N from top of stack).
    Roll = 0x14,
    /// Duplicate the second element.
    Over = 0x15,
    /// Rotate the top 3: (a b c → b c a).
    Rot = 0x16,
    /// Duplicate the top two elements.
    Dup2 = 0x17,

    // ---- Arithmetic (0x20–0x2F) ----
    /// a b → (a + b) wrapping.
    Add = 0x20,
    /// a b → (a - b) wrapping.
    Sub = 0x21,
    /// a b → (a * b) wrapping.
    Mul = 0x22,
    /// a b → (a / b) truncated toward zero; division by zero = error.
    Div = 0x23,
    /// a b → (a % b); division by zero = error.
    Mod = 0x24,
    /// a → (-a) wrapping.
    Neg = 0x25,
    /// a → |a|.
    Abs = 0x26,
    /// a b → min(a,b).
    Min = 0x27,
    /// a b → max(a,b).
    Max = 0x28,
    /// a b → push (a as u64 * b as u64) as 16-byte LE Bytes.
    ///
    /// Treats inputs as `u64` (bit-reinterpret of `i64`), computes a
    /// `u128` product that cannot overflow, and pushes the 16-byte
    /// little-endian encoding onto the stack. Used by AMM swap covenants
    /// for the constant-product invariant `new_a*new_b ≥ old_a*old_b`
    /// where reserves are u64 and the product needs ~128 bits of room.
    /// Pair with [`OpCode::GeU128`] for the ≥ check.
    MulU128 = 0x29,

    // ---- Comparison (0x30–0x3F) ----
    /// a b → 1 if a == b, else 0.
    Eq = 0x30,
    /// a b → 1 if a ≠ b, else 0.
    Neq = 0x31,
    /// a b → 1 if a < b, else 0.
    Lt = 0x32,
    /// a b → 1 if a > b, else 0.
    Gt = 0x33,
    /// a b → 1 if a ≤ b, else 0.
    Le = 0x34,
    /// a b → 1 if a ≥ b, else 0.
    Ge = 0x35,
    /// a → 1 if a == 0, else 0.
    Not = 0x36,
    /// a b → 1 if both non-zero, else 0.
    And = 0x37,
    /// a b → 1 if either non-zero, else 0.
    Or = 0x38,
    /// a b → 1 if `a ≥ b` as little-endian unsigned 128-bit integers, else 0.
    ///
    /// Both operands must be exactly-16-byte `Bytes`. Companion to
    /// [`OpCode::MulU128`] so AMM scripts can compare two products.
    GeU128 = 0x39,

    // ---- Control flow (0x40–0x4F) ----
    /// Pop; if truthy, execute until matching `Else`/`EndIf`.
    If = 0x40,
    /// Marks the start of the false branch.
    Else = 0x41,
    /// Terminates an `If`/`Else` block.
    EndIf = 0x42,
    /// Pop and fail the script if the value is falsy.
    Verify = 0x43,
    /// Immediately fail the script (unconditional abort).
    Return = 0x44,

    // ---- Crypto (0x50–0x5F) ----
    /// Pop `(pubkey, signature, message)` → push 1 if valid PQC sig, else 0.
    CheckSigPqc = 0x50,
    /// Pop `(m, pk1..pkN, sig1..sigM)` → push 1 if M-of-N PQC sig, else 0.
    CheckMultiSigPqc = 0x51,
    /// Pop bytes → push SHA3-256 hash (32 bytes).
    HashSha3 = 0x52,
    /// Pop bytes → push BLAKE3 hash (32 bytes).
    HashBlake3 = 0x53,

    // ---- Introspection (0x60–0x6F) ----
    /// Pop index i → push the value (Amount) of input #i.
    ReadInputValue = 0x60,
    /// Pop index i → push the value (Amount) of output #i.
    ReadOutputValue = 0x61,
    /// Pop index i → push the locking script of output #i.
    ReadOutputScript = 0x62,
    /// Pop index i → push the datum of output #i (empty bytes if none).
    ReadOutputDatum = 0x63,
    /// Push the SHA3-256 hash of the current transaction.
    TxHash = 0x64,
    /// Pop index i → push the datum bytes of resolved input #i
    /// (empty bytes if the prev-output had no datum). Symmetric counterpart
    /// to [`OpCode::ReadOutputDatum`]; lets AMM / lending locking scripts
    /// read their own pre-transition state when validating an invariant
    /// (e.g. `new_a*new_b ≥ old_a*old_b` for constant-product swaps).
    ReadInputDatum = 0x6A,
    /// Push the SHA3-256 of the currently-executing locking script.
    ///
    /// The validator computes this once per script invocation (it knows
    /// the script bytes — it is executing them) and exposes it via
    /// `Context.locking_script_hash`. Used by covenant scripts to
    /// enforce **script continuity**: an AMM pool UTXO can require its
    /// successor UTXO to be locked to the same script (preventing a
    /// swap that drains the pool into a plain p2pkh UTXO) by combining
    /// `SELF_SCRIPT_HASH` with [`OpCode::AssertOutputScriptHash`].
    SelfScriptHash = 0x6B,
    /// Push the current slot number as an integer.
    SlotNumber = 0x65,
    /// Push the number of inputs.
    InputCount = 0x66,
    /// Push the number of outputs.
    OutputCount = 0x67,
    /// Push the fee paid by this transaction.
    TxFee = 0x68,
    /// Push the signature hash (witness-excluded tx hash) of this transaction.
    SigHash = 0x69,

    // ---- Covenants (0x70–0x7F) ----
    /// Pop `(index, hash)` → verify output #index has that script hash.
    AssertOutputScriptHash = 0x70,
    /// Pop `(index, hash)` → verify output #index has that datum hash.
    AssertDatumHash = 0x71,
    /// Pop `(index, amount)` → verify output #index has that value.
    AssertValue = 0x72,

    // ---- Data / byte ops (0x80–0x8F) ----
    /// Pop two byte-strings → push concatenation.
    Cat = 0x80,
    /// Pop `(bytes, start, len)` → push sub-slice.
    Slice = 0x81,
    /// Pop value → push its byte length (Int → 8, Bytes → len).
    Len = 0x82,
    /// Pop `(bytes, n)` → read first `n` bytes as little-endian
    /// unsigned integer, push as `Int`.
    ///
    /// `n` must be in `1..=8`. The byte string must have length exactly
    /// `n`. The result is a bit-reinterpret into `i64`: values with the
    /// top bit set wrap to negative — that is the intended behaviour so
    /// downstream opcodes like [`OpCode::MulU128`] can still treat them
    /// as `u64`. Used by AMM covenants to extract reserves from datum
    /// byte sequences.
    BytesToInt = 0x83,

    // ---- Meta (0xFF) ----
    /// No operation.
    Nop = 0xFF,
}

impl OpCode {
    /// Number of opcodes defined.
    pub const COUNT: usize = 63;

    /// Maximum allowed script size in bytes.
    pub const MAX_SCRIPT_SIZE: usize = 16_384;

    /// Maximum allowed stack depth.
    pub const MAX_STACK_DEPTH: usize = 1_024;

    /// Decode a single opcode byte. Returns `Err` for unrecognised bytes.
    pub fn from_byte(b: u8) -> Result<Self, OpcodeError> {
        match b {
            0x00 => Ok(Self::Op0),
            0x01 => Ok(Self::Op1),
            0x02 => Ok(Self::Push1),
            0x03 => Ok(Self::Push2),
            0x04 => Ok(Self::Push4),
            0x05 => Ok(Self::PushInt),

            0x10 => Ok(Self::Dup),
            0x11 => Ok(Self::Swap),
            0x12 => Ok(Self::Drop),
            0x13 => Ok(Self::Pick),
            0x14 => Ok(Self::Roll),
            0x15 => Ok(Self::Over),
            0x16 => Ok(Self::Rot),
            0x17 => Ok(Self::Dup2),

            0x20 => Ok(Self::Add),
            0x21 => Ok(Self::Sub),
            0x22 => Ok(Self::Mul),
            0x23 => Ok(Self::Div),
            0x24 => Ok(Self::Mod),
            0x25 => Ok(Self::Neg),
            0x26 => Ok(Self::Abs),
            0x27 => Ok(Self::Min),
            0x28 => Ok(Self::Max),
            0x29 => Ok(Self::MulU128),

            0x30 => Ok(Self::Eq),
            0x31 => Ok(Self::Neq),
            0x32 => Ok(Self::Lt),
            0x33 => Ok(Self::Gt),
            0x34 => Ok(Self::Le),
            0x35 => Ok(Self::Ge),
            0x36 => Ok(Self::Not),
            0x37 => Ok(Self::And),
            0x38 => Ok(Self::Or),
            0x39 => Ok(Self::GeU128),

            0x40 => Ok(Self::If),
            0x41 => Ok(Self::Else),
            0x42 => Ok(Self::EndIf),
            0x43 => Ok(Self::Verify),
            0x44 => Ok(Self::Return),

            0x50 => Ok(Self::CheckSigPqc),
            0x51 => Ok(Self::CheckMultiSigPqc),
            0x52 => Ok(Self::HashSha3),
            0x53 => Ok(Self::HashBlake3),

            0x60 => Ok(Self::ReadInputValue),
            0x61 => Ok(Self::ReadOutputValue),
            0x62 => Ok(Self::ReadOutputScript),
            0x63 => Ok(Self::ReadOutputDatum),
            0x64 => Ok(Self::TxHash),
            0x65 => Ok(Self::SlotNumber),
            0x66 => Ok(Self::InputCount),
            0x67 => Ok(Self::OutputCount),
            0x68 => Ok(Self::TxFee),
            0x69 => Ok(Self::SigHash),
            0x6A => Ok(Self::ReadInputDatum),
            0x6B => Ok(Self::SelfScriptHash),

            0x70 => Ok(Self::AssertOutputScriptHash),
            0x71 => Ok(Self::AssertDatumHash),
            0x72 => Ok(Self::AssertValue),

            0x80 => Ok(Self::Cat),
            0x81 => Ok(Self::Slice),
            0x82 => Ok(Self::Len),
            0x83 => Ok(Self::BytesToInt),

            0xFF => Ok(Self::Nop),

            other => Err(OpcodeError::UnknownOpcode(other)),
        }
    }

    /// Encode the opcode as a single byte.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Human-readable mnemonic for disassembly / debugging.
    #[must_use]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::Op0 => "OP_0",
            Self::Op1 => "OP_1",
            Self::Push1 => "PUSH1",
            Self::Push2 => "PUSH2",
            Self::Push4 => "PUSH4",
            Self::PushInt => "PUSH_INT",
            Self::Dup => "DUP",
            Self::Swap => "SWAP",
            Self::Drop => "DROP",
            Self::Pick => "PICK",
            Self::Roll => "ROLL",
            Self::Over => "OVER",
            Self::Rot => "ROT",
            Self::Dup2 => "DUP2",
            Self::Add => "ADD",
            Self::Sub => "SUB",
            Self::Mul => "MUL",
            Self::Div => "DIV",
            Self::Mod => "MOD",
            Self::Neg => "NEG",
            Self::Abs => "ABS",
            Self::Min => "MIN",
            Self::MulU128 => "MUL_U128",
            Self::Max => "MAX",
            Self::Eq => "EQ",
            Self::Neq => "NEQ",
            Self::Lt => "LT",
            Self::Gt => "GT",
            Self::Le => "LE",
            Self::Ge => "GE",
            Self::Not => "NOT",
            Self::And => "AND",
            Self::Or => "OR",
            Self::GeU128 => "GE_U128",
            Self::If => "IF",
            Self::Else => "ELSE",
            Self::EndIf => "ENDIF",
            Self::Verify => "VERIFY",
            Self::Return => "RETURN",
            Self::CheckSigPqc => "CHECKSIG_PQC",
            Self::CheckMultiSigPqc => "CHECKMULTISIG_PQC",
            Self::HashSha3 => "HASH_SHA3",
            Self::HashBlake3 => "HASH_BLAKE3",
            Self::ReadInputValue => "READ_INPUT_VALUE",
            Self::ReadOutputValue => "READ_OUTPUT_VALUE",
            Self::ReadOutputScript => "READ_OUTPUT_SCRIPT",
            Self::ReadOutputDatum => "READ_OUTPUT_DATUM",
            Self::TxHash => "TX_HASH",
            Self::SlotNumber => "SLOT_NUMBER",
            Self::InputCount => "INPUT_COUNT",
            Self::OutputCount => "OUTPUT_COUNT",
            Self::TxFee => "TX_FEE",
            Self::SigHash => "SIG_HASH",
            Self::ReadInputDatum => "READ_INPUT_DATUM",
            Self::SelfScriptHash => "SELF_SCRIPT_HASH",
            Self::AssertOutputScriptHash => "ASSERT_OUTPUT_SCRIPT_HASH",
            Self::AssertDatumHash => "ASSERT_DATUM_HASH",
            Self::AssertValue => "ASSERT_VALUE",
            Self::Cat => "CAT",
            Self::Slice => "SLICE",
            Self::Len => "LEN",
            Self::BytesToInt => "BYTES_TO_INT",
            Self::Nop => "NOP",
        }
    }

    /// True if the opcode is a `PUSH` variant that reads inline data.
    #[must_use]
    pub const fn is_push(self) -> bool {
        matches!(
            self,
            Self::Push1 | Self::Push2 | Self::Push4 | Self::PushInt
        )
    }
}

impl fmt::Display for OpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.mnemonic())
    }
}

// ============================================================================
// Instruction — a decoded (opcode, optional data) pair
// ============================================================================

/// A decoded instruction: an opcode plus any inline data (for `PUSH*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    /// The opcode.
    pub op: OpCode,
    /// Inline data (non-empty only for `PUSH*` and `PUSH_INT`).
    pub data: Vec<u8>,
}

impl Instruction {
    /// Create a data-less instruction.
    #[must_use]
    pub const fn simple(op: OpCode) -> Self {
        Self {
            op,
            data: Vec::new(),
        }
    }

    /// Create a push instruction from raw bytes.
    #[must_use]
    pub fn push_bytes(data: Vec<u8>) -> Self {
        let op = match data.len() {
            0..=255 => OpCode::Push1,
            256..=65535 => OpCode::Push2,
            _ => OpCode::Push4,
        };
        Self { op, data }
    }

    /// Create a PUSH_INT instruction.
    #[must_use]
    pub fn push_int(n: i64) -> Self {
        Self {
            op: OpCode::PushInt,
            data: n.to_le_bytes().to_vec(),
        }
    }

    /// Encode this instruction into its wire-format bytes.
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.op.to_byte());
        match self.op {
            OpCode::Push1 => {
                #[allow(clippy::cast_possible_truncation)]
                let len = self.data.len() as u8;
                out.push(len);
                out.extend_from_slice(&self.data);
            }
            OpCode::Push2 => {
                #[allow(clippy::cast_possible_truncation)]
                let len = self.data.len() as u16;
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&self.data);
            }
            OpCode::Push4 => {
                #[allow(clippy::cast_possible_truncation)]
                let len = self.data.len() as u32;
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&self.data);
            }
            OpCode::PushInt => {
                out.extend_from_slice(&self.data);
            }
            _ => { /* no inline data */ }
        }
    }
}

// ============================================================================
// Decode: raw bytes → Vec<Instruction>
// ============================================================================

/// Decode a raw script byte-slice into a list of [`Instruction`]s.
///
/// Returns `Err` if:
/// - an unknown opcode byte is encountered,
/// - a PUSH instruction extends past the end of the script, or
/// - the script exceeds [`OpCode::MAX_SCRIPT_SIZE`].
pub fn decode_script(script: &[u8]) -> Result<Vec<Instruction>, OpcodeError> {
    if script.len() > OpCode::MAX_SCRIPT_SIZE {
        return Err(OpcodeError::ScriptTooLarge {
            size: script.len(),
            max: OpCode::MAX_SCRIPT_SIZE,
        });
    }

    let mut instructions = Vec::new();
    let mut pos = 0;

    while pos < script.len() {
        let byte = script[pos];
        let op = OpCode::from_byte(byte)?;
        pos = pos.wrapping_add(1);

        let data = match op {
            OpCode::Push1 => {
                if pos >= script.len() {
                    return Err(OpcodeError::UnexpectedEnd { need: 1, have: 0 });
                }
                let len = script[pos] as usize;
                pos = pos.wrapping_add(1);
                if len == 0 {
                    return Err(OpcodeError::ZeroPush);
                }
                let remaining = script.len().wrapping_sub(pos);
                if remaining < len {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: len,
                        have: remaining,
                    });
                }
                let d = script[pos..pos.wrapping_add(len)].to_vec();
                pos = pos.wrapping_add(len);
                d
            }
            OpCode::Push2 => {
                let remaining = script.len().wrapping_sub(pos);
                if remaining < 2 {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: 2,
                        have: remaining,
                    });
                }
                let len = u16::from_le_bytes([script[pos], script[pos.wrapping_add(1)]]) as usize;
                pos = pos.wrapping_add(2);
                if len == 0 {
                    return Err(OpcodeError::ZeroPush);
                }
                let remaining = script.len().wrapping_sub(pos);
                if remaining < len {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: len,
                        have: remaining,
                    });
                }
                let d = script[pos..pos.wrapping_add(len)].to_vec();
                pos = pos.wrapping_add(len);
                d
            }
            OpCode::Push4 => {
                let remaining = script.len().wrapping_sub(pos);
                if remaining < 4 {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: 4,
                        have: remaining,
                    });
                }
                let len = u32::from_le_bytes([
                    script[pos],
                    script[pos.wrapping_add(1)],
                    script[pos.wrapping_add(2)],
                    script[pos.wrapping_add(3)],
                ]) as usize;
                pos = pos.wrapping_add(4);
                if len == 0 {
                    return Err(OpcodeError::ZeroPush);
                }
                let remaining = script.len().wrapping_sub(pos);
                if remaining < len {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: len,
                        have: remaining,
                    });
                }
                let d = script[pos..pos.wrapping_add(len)].to_vec();
                pos = pos.wrapping_add(len);
                d
            }
            OpCode::PushInt => {
                let remaining = script.len().wrapping_sub(pos);
                if remaining < 8 {
                    return Err(OpcodeError::UnexpectedEnd {
                        need: 8,
                        have: remaining,
                    });
                }
                let d = script[pos..pos.wrapping_add(8)].to_vec();
                pos = pos.wrapping_add(8);
                d
            }
            _ => Vec::new(),
        };

        instructions.push(Instruction { op, data });
    }

    Ok(instructions)
}

/// Encode a list of instructions back to raw bytes.
#[must_use]
pub fn encode_instructions(instructions: &[Instruction]) -> Vec<u8> {
    let mut out = Vec::new();
    for instr in instructions {
        instr.encode(&mut out);
    }
    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn opcode_byte_roundtrip() {
        for byte in 0..=0xFF_u8 {
            if let Ok(op) = OpCode::from_byte(byte) {
                assert_eq!(op.to_byte(), byte, "roundtrip failed for 0x{byte:02x}");
            }
        }
    }

    #[test]
    fn all_opcodes_have_mnemonics() {
        let known: &[OpCode] = &[
            OpCode::Op0,
            OpCode::Op1,
            OpCode::Push1,
            OpCode::Push2,
            OpCode::Push4,
            OpCode::PushInt,
            OpCode::Dup,
            OpCode::Swap,
            OpCode::Drop,
            OpCode::Pick,
            OpCode::Roll,
            OpCode::Over,
            OpCode::Rot,
            OpCode::Dup2,
            OpCode::Add,
            OpCode::Sub,
            OpCode::Mul,
            OpCode::Div,
            OpCode::Mod,
            OpCode::Neg,
            OpCode::Abs,
            OpCode::Min,
            OpCode::Max,
            OpCode::MulU128,
            OpCode::Eq,
            OpCode::Neq,
            OpCode::Lt,
            OpCode::Gt,
            OpCode::Le,
            OpCode::Ge,
            OpCode::Not,
            OpCode::And,
            OpCode::Or,
            OpCode::GeU128,
            OpCode::If,
            OpCode::Else,
            OpCode::EndIf,
            OpCode::Verify,
            OpCode::Return,
            OpCode::CheckSigPqc,
            OpCode::CheckMultiSigPqc,
            OpCode::HashSha3,
            OpCode::HashBlake3,
            OpCode::ReadInputValue,
            OpCode::ReadOutputValue,
            OpCode::ReadOutputScript,
            OpCode::ReadOutputDatum,
            OpCode::TxHash,
            OpCode::SlotNumber,
            OpCode::InputCount,
            OpCode::OutputCount,
            OpCode::TxFee,
            OpCode::SigHash,
            OpCode::ReadInputDatum,
            OpCode::SelfScriptHash,
            OpCode::AssertOutputScriptHash,
            OpCode::AssertDatumHash,
            OpCode::AssertValue,
            OpCode::Cat,
            OpCode::Slice,
            OpCode::Len,
            OpCode::BytesToInt,
            OpCode::Nop,
        ];
        assert_eq!(known.len(), OpCode::COUNT);
        for op in known {
            assert!(!op.mnemonic().is_empty());
        }
    }

    #[test]
    fn unknown_opcode_rejected() {
        assert!(matches!(
            OpCode::from_byte(0xFE),
            Err(OpcodeError::UnknownOpcode(0xFE))
        ));
    }

    #[test]
    fn push1_roundtrip() {
        let instr = Instruction::push_bytes(vec![0xDE, 0xAD]);
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        let decoded = decode_script(&buf).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].op, OpCode::Push1);
        assert_eq!(decoded[0].data, vec![0xDE, 0xAD]);
    }

    #[test]
    fn push_int_roundtrip() {
        let instr = Instruction::push_int(-42);
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        let decoded = decode_script(&buf).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].op, OpCode::PushInt);
        let val = i64::from_le_bytes(decoded[0].data[..8].try_into().unwrap());
        assert_eq!(val, -42);
    }

    #[test]
    fn simple_script_decode_encode() {
        // OP_1 DUP ADD VERIFY
        let raw = vec![0x01, 0x10, 0x20, 0x43];
        let decoded = decode_script(&raw).unwrap();
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded[0].op, OpCode::Op1);
        assert_eq!(decoded[1].op, OpCode::Dup);
        assert_eq!(decoded[2].op, OpCode::Add);
        assert_eq!(decoded[3].op, OpCode::Verify);
        assert_eq!(encode_instructions(&decoded), raw);
    }

    #[test]
    fn truncated_push_rejected() {
        // Push1 says 5 bytes follow, but only 2 remain
        let raw = vec![0x02, 0x05, 0xAA, 0xBB];
        assert!(matches!(
            decode_script(&raw),
            Err(OpcodeError::UnexpectedEnd { need: 5, have: 2 })
        ));
    }

    #[test]
    fn zero_push_rejected() {
        // Push1 with length 0
        let raw = vec![0x02, 0x00];
        assert!(matches!(decode_script(&raw), Err(OpcodeError::ZeroPush)));
    }

    #[test]
    fn script_too_large() {
        let big = vec![0xFF; OpCode::MAX_SCRIPT_SIZE + 1]; // all NOPs
        assert!(matches!(
            decode_script(&big),
            Err(OpcodeError::ScriptTooLarge { .. })
        ));
    }

    #[test]
    fn value_truthiness() {
        assert!(!Value::Int(0).is_truthy());
        assert!(Value::Int(1).is_truthy());
        assert!(Value::Int(-1).is_truthy());
        assert!(!Value::Bytes(vec![]).is_truthy());
        assert!(Value::Bytes(vec![0]).is_truthy());
    }

    #[test]
    fn value_coercions() {
        let v: Value = 42_i64.into();
        assert_eq!(v.as_int(), Some(42));
        assert_eq!(v.as_bytes(), None);

        let b: Value = vec![1, 2, 3].into();
        assert_eq!(b.as_int(), None);
        assert_eq!(b.as_bytes(), Some(&[1, 2, 3][..]));

        let t: Value = true.into();
        assert_eq!(t.as_int(), Some(1));
    }

    #[test]
    fn display_opcode() {
        assert_eq!(format!("{}", OpCode::CheckSigPqc), "CHECKSIG_PQC");
        assert_eq!(format!("{}", OpCode::Add), "ADD");
    }
}
