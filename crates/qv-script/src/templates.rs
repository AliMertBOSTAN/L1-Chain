//! Standard script templates and the [`ScriptBuilder`] fluent API.
//!
//! Templates are pre-built script bytecodes for common patterns:
//!
//! - [`p2pkh_pqc`] — Pay-to-Public-Key-Hash (PQC). The most common
//!   single-owner locking script.
//! - [`multisig_pqc`] — M-of-N PQC multi-signature.
//! - [`amm_swap`] — Constant-product AMM invariant checker (covenant
//!   skeleton; superseded by [`amm_pool_lock`]).
//! - [`amm_pool_lock`] — full constant-product AMM pool covenant:
//!   `x·y ≥ k` invariant, token-id pinning, script continuity.
//! - [`lending_repay`] — Basic lending repayment covenant.
//!
//! The [`ScriptBuilder`] lets you compose arbitrary scripts programmatically.

use thiserror::Error;

use qv_crypto::sha3_256;

use crate::opcode::{encode_instructions, Instruction, OpCode};

// ============================================================================
// Template errors
// ============================================================================

/// Errors arising while *generating* a script template (not executing it).
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum TemplateError {
    /// `ltv_max_bps` must be in `1..=10_000`: 0 would make the collateral
    /// factor division by zero (a pool that can never lend should simply
    /// not expose a borrow path), >10_000 is a nonsensical LTV.
    #[error("ltv_max_bps must be in 1..=10000, got {0}")]
    InvalidLtv(u16),

    /// `max_price_age_slots` must fit in an `i64` script integer.
    #[error("max_price_age_slots {0} does not fit in a script integer")]
    MaxAgeTooLarge(u64),
}

// ============================================================================
// ScriptBuilder — fluent API for composing scripts
// ============================================================================

/// Fluent builder for Script VM bytecodes.
///
/// # Example
///
/// ```ignore
/// let script = ScriptBuilder::new()
///     .push_bytes(&pubkey_hash)
///     .op(OpCode::CheckSigPqc)
///     .op(OpCode::Verify)
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ScriptBuilder {
    instructions: Vec<Instruction>,
}

impl ScriptBuilder {
    /// Create an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a simple (data-less) opcode.
    #[must_use]
    pub fn op(mut self, op: OpCode) -> Self {
        self.instructions.push(Instruction::simple(op));
        self
    }

    /// Push an integer constant onto the stack.
    #[must_use]
    pub fn push_int(mut self, n: i64) -> Self {
        match n {
            0 => self.instructions.push(Instruction::simple(OpCode::Op0)),
            1 => self.instructions.push(Instruction::simple(OpCode::Op1)),
            _ => self.instructions.push(Instruction::push_int(n)),
        }
        self
    }

    /// Push arbitrary bytes onto the stack.
    #[must_use]
    pub fn push_bytes(mut self, data: &[u8]) -> Self {
        self.instructions
            .push(Instruction::push_bytes(data.to_vec()));
        self
    }

    /// Append a raw pre-built instruction.
    #[must_use]
    pub fn instruction(mut self, instr: Instruction) -> Self {
        self.instructions.push(instr);
        self
    }

    /// Encode to raw script bytes.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        encode_instructions(&self.instructions)
    }

    /// Return the instruction list (useful for inspection / testing).
    #[must_use]
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
}

// ============================================================================
// Template: p2pkh_pqc
// ============================================================================

/// Create a **Pay-to-Public-Key-Hash (PQC)** locking script.
///
/// Locking script logic:
/// ```text
/// <sig> <pubkey>                     ← witness (pushed by spender)
/// DUP HASH_SHA3                      ← hash the pubkey on top
/// PUSH <expected_pk_hash>            ← push the expected hash
/// EQ VERIFY                          ← check they match
/// SIG_HASH ROT ROT                   ← push sighash, reorder to (msg sig pubkey)
/// CHECKSIG_PQC                       ← verify signature over the sighash
/// ```
///
/// The witness pushes only `(signature, pubkey)`. The message the signature
/// commits to is **not** carried in the witness — the script derives it from
/// the transaction itself via the `SIG_HASH` opcode (ADR-012). `SIG_HASH` is
/// the witness-excluded transaction hash, so it binds the signature to *this*
/// transaction and closes the in-flight witness-replay vulnerability where a
/// mempool witness could be lifted onto an attacker's transaction.
#[must_use]
pub fn p2pkh_pqc(pubkey_hash: &[u8; 32]) -> Vec<u8> {
    ScriptBuilder::new()
        // Stack at entry: ... sig pubkey
        .op(OpCode::Dup)           // ... sig pubkey pubkey
        .op(OpCode::HashSha3)      // ... sig pubkey hash(pubkey)
        .push_bytes(pubkey_hash)   // ... sig pubkey hash(pk) expected_hash
        .op(OpCode::Eq)            // ... sig pubkey (hash==expected)
        .op(OpCode::Verify)        // ... sig pubkey   (fail if false)
        .op(OpCode::SigHash)       // ... sig pubkey sighash
        .op(OpCode::Rot)           // ... pubkey sighash sig
        .op(OpCode::Rot)           // ... sighash sig pubkey
        .op(OpCode::CheckSigPqc)   // ... result
        .build()
}

/// Compute the pubkey hash used in [`p2pkh_pqc`] locking scripts.
///
/// This is simply `SHA3-256(raw_pubkey_bytes)`.
#[must_use]
pub fn pubkey_hash(pubkey_bytes: &[u8]) -> [u8; 32] {
    sha3_256(pubkey_bytes)
}

// ============================================================================
// Template: stealth_p2pkh (ADR-011)
// ============================================================================

/// Domain separator for the stealth one-time commitment.
///
/// MUST byte-for-byte match `qv_privacy::stealth`'s `STEALTH_KDF_TAG`
/// (used by `compute_onetime_pk_hash`). See ADR-011. A future cleanup
/// should hoist this constant into `qv-core` so the two crates cannot drift.
const STEALTH_KDF_TAG: &[u8] = b"QuantumVault-Stealth-v1";

/// Create a **stealth pay-to-public-key-hash** locking script (ADR-011).
///
/// Locks an output to a stealth one-time commitment. Only the recipient —
/// who can decapsulate the output's KEM ciphertext with their view key to
/// recover `shared_secret` — can satisfy it.
///
/// The witness must push, bottom-to-top:
/// ```text
/// <signature> <spend_pubkey> <shared_secret>
/// ```
///
/// The script verifies:
/// 1. `onetime_pk_hash == SHA3-256(STEALTH_KDF_TAG || shared_secret || spend_pubkey)`
/// 2. `CHECKSIG_PQC(spend_pubkey, signature, sighash)` — the signature is
///    bound to *this* transaction via the `SIG_HASH` opcode (ADR-012).
///    `SIG_HASH` excludes input witnesses, so it is non-circular (unlike
///    `TX_HASH`, which would hash the witness that carries the signature).
#[must_use]
pub fn stealth_p2pkh(onetime_pk_hash: &[u8; 32]) -> Vec<u8> {
    ScriptBuilder::new()
        // Witness on entry:        sig spend_pk shared_secret
        .op(OpCode::Over) //         sig spend_pk shared_secret spend_pk
        .op(OpCode::Cat) //          sig spend_pk (shared_secret||spend_pk)
        .push_bytes(STEALTH_KDF_TAG) // sig spend_pk (ss||pk) TAG
        .op(OpCode::Swap) //         sig spend_pk TAG (ss||pk)
        .op(OpCode::Cat) //          sig spend_pk (TAG||ss||pk)
        .op(OpCode::HashSha3) //     sig spend_pk H
        .push_bytes(onetime_pk_hash) // sig spend_pk H commitment
        .op(OpCode::Eq) //           sig spend_pk (H==commitment)
        .op(OpCode::Verify) //       sig spend_pk          (fail if mismatch)
        .op(OpCode::SigHash) //      sig spend_pk sighash
        .op(OpCode::Rot) //          spend_pk sighash sig
        .op(OpCode::Rot) //          sighash sig spend_pk
        .op(OpCode::CheckSigPqc) //  result
        .build()
}

// ============================================================================
// Template: multisig_pqc
// ============================================================================

/// Create an **M-of-N PQC multi-signature** locking script.
///
/// The witness must push:
/// ```text
/// <msg> <sig1> ... <sigM> <M> <pk1> ... <pkN> <N>
/// ```
///
/// The locking script then calls `CHECKMULTISIG_PQC`.
#[must_use]
pub fn multisig_pqc(threshold: u32, pubkey_hashes: &[[u8; 32]]) -> Vec<u8> {
    // The locking script just embeds M and N and calls CHECKMULTISIG_PQC.
    // The actual public keys and signatures come from the witness.
    // We encode: PUSH_INT(M) PUSH_INT(N) CHECKMULTISIG_PQC
    // The witness must be structured accordingly.
    ScriptBuilder::new()
        .push_int(i64::from(threshold))
        .push_int(pubkey_hashes.len() as i64)
        .op(OpCode::CheckMultiSigPqc)
        .build()
}

// ============================================================================
// Template: AMM constant-product swap
// ============================================================================

/// Create an **AMM constant-product swap** locking script.
///
/// **Covenant skeleton only — superseded by [`amm_pool_lock`]**, which
/// performs the full on-chain invariant check. Kept for backwards
/// compatibility with earlier fixtures.
///
/// This is the Shared UTXO Pattern from the architecture: the pool is a
/// single UTXO with `datum = (reserve_x, reserve_y, fee_bps)`. A swap
/// transaction must produce a new pool UTXO whose reserves satisfy:
///
/// ```text
/// x_new * y_new >= x_old * y_old
/// ```
///
/// Parameters:
/// - `pool_script_hash`: the expected script hash of the output pool UTXO
///   (ensures the pool script is preserved).
/// - `pool_output_index`: the index of the new pool UTXO in the tx outputs.
///
/// The **datum** layout (encoded as 3 × i64, LE) is:
/// - bytes 0..8   → reserve_x
/// - bytes 8..16  → reserve_y
/// - bytes 16..24 → fee_bps (not used in invariant check but preserved)
///
/// The **witness** must push the old datum bytes.
///
/// Script logic:
/// ```text
/// <old_datum>                             ← witness
/// # Read old reserves from witness datum
/// DUP 0 8 SLICE  → old_x
/// DUP 8 8 SLICE  → old_y
/// # Read new datum from output
/// PUSH(pool_index) READ_OUTPUT_DATUM
/// DUP 0 8 SLICE  → new_x
/// DUP 8 8 SLICE  → new_y
/// # Invariant: new_x * new_y >= old_x * old_y
/// new_x new_y MUL
/// old_x old_y MUL
/// GE VERIFY
/// # Covenant: output script hash preserved
/// PUSH(pool_index) PUSH(pool_script_hash) ASSERT_OUTPUT_SCRIPT_HASH
/// OP_1
/// ```
#[must_use]
pub fn amm_swap(pool_script_hash: &[u8; 32], pool_output_index: i64) -> Vec<u8> {
    ScriptBuilder::new()
        // Stack at entry: ... old_datum_bytes
        // Extract old_x (bytes 0..8)
        .op(OpCode::Dup)
        .push_int(0)
        .push_int(8)
        .op(OpCode::Slice)         // ... old_datum old_x_bytes

        // Extract old_y (bytes 8..16)
        .op(OpCode::Swap)          // ... old_x_bytes old_datum
        .op(OpCode::Dup)
        .push_int(8)
        .push_int(8)
        .op(OpCode::Slice)         // ... old_x_bytes old_datum old_y_bytes
        .op(OpCode::Rot)           // ... old_datum old_y_bytes old_x_bytes
        .op(OpCode::Rot)           // ... old_y_bytes old_x_bytes old_datum
        .op(OpCode::Drop)          // ... old_y_bytes old_x_bytes

        // Read new datum from output
        .push_int(pool_output_index)
        .op(OpCode::ReadOutputDatum) // ... old_y old_x new_datum

        // Extract new_x (bytes 0..8)
        .op(OpCode::Dup)
        .push_int(0)
        .push_int(8)
        .op(OpCode::Slice)         // ... old_y old_x new_datum new_x_bytes

        // Extract new_y (bytes 8..16)
        .op(OpCode::Swap)
        .op(OpCode::Dup)
        .push_int(8)
        .push_int(8)
        .op(OpCode::Slice)         // ... old_y old_x new_x new_datum new_y_bytes
        .op(OpCode::Swap)
        .op(OpCode::Drop)          // ... old_y old_x new_x new_y_bytes

        // Now we need to interpret byte slices as i64 for multiplication.
        // Convention: the datum encodes i64 as LE 8 bytes.
        // We'll use PUSH_INT to set up a manual decode via the stack.
        // Actually, since pop_bytes in the interpreter coerces Int→bytes
        // and pop_int coerces, we need the values as Ints.
        // Simplification: we compare the raw 8-byte LE values.
        // For the invariant check, the datum values are already i64 LE.
        // The interpreter's `pop_int` on Bytes will fail, so we need
        // a workaround: we'll just leave the bytes as-is and compare
        // the product of the byte-encoded integers.

        // Better approach: the script just verifies via ASSERT_VALUE
        // and the batcher does the actual math. For now, we produce a
        // simplified covenant-only version that checks:
        // 1. Output script hash is preserved
        // 2. A datum exists on the output
        // The full invariant check requires datum→int conversion opcodes
        // which we leave for qv-defi (Stage 9). This is the "covenant
        // skeleton" referenced in the MASTER_PLAN.

        // Clear the remaining extracted bytes
        .op(OpCode::Drop)          // drop new_y
        .op(OpCode::Drop)          // drop new_x
        .op(OpCode::Drop)          // drop old_x
        .op(OpCode::Drop)          // drop old_y

        // Covenant: output pool script hash preserved
        .push_int(pool_output_index)
        .push_bytes(pool_script_hash)
        .op(OpCode::AssertOutputScriptHash)

        // Covenant: output datum exists (non-empty)
        .push_int(pool_output_index)
        .op(OpCode::ReadOutputDatum)
        .op(OpCode::Len)
        .push_int(0)
        .op(OpCode::Gt)
        .op(OpCode::Verify)

        // Success
        .op(OpCode::Op1)
        .build()
}

// ============================================================================
// Template: AMM pool covenant (amm_pool_lock) — canonical PoolDatum layout
// ============================================================================

/// Total length of the **canonical** (script-friendly) AMM `PoolDatum`
/// encoding in bytes.
///
/// The pool datum is deliberately **not** stored as bincode: the script VM
/// extracts fields with `SLICE` at fixed offsets, so the encoding must be
/// fixed-width and independent of any serializer's internals. `qv-defi`'s
/// `PoolDatum::to_canonical_bytes()` emits exactly this layout (all integers
/// little-endian):
///
/// | Bytes   | Field        | Type         |
/// |---------|--------------|--------------|
/// | 0..32   | `token_a_id` | 32 raw bytes |
/// | 32..64  | `token_b_id` | 32 raw bytes |
/// | 64..72  | `reserve_a`  | `u64` LE     |
/// | 72..80  | `reserve_b`  | `u64` LE     |
/// | 80..88  | `lp_total`   | `u64` LE     |
/// | 88..90  | `fee_bps`    | `u16` LE     |
pub const POOL_DATUM_LEN: usize = 90;

/// Byte offset of `token_a_id` in the canonical pool datum.
pub const POOL_DATUM_TOKEN_A_OFFSET: usize = 0;
/// Byte offset of `token_b_id` in the canonical pool datum.
pub const POOL_DATUM_TOKEN_B_OFFSET: usize = 32;
/// Byte offset of `reserve_a` (u64 LE) in the canonical pool datum.
pub const POOL_DATUM_RESERVE_A_OFFSET: usize = 64;
/// Byte offset of `reserve_b` (u64 LE) in the canonical pool datum.
pub const POOL_DATUM_RESERVE_B_OFFSET: usize = 72;
/// Byte offset of `lp_total` (u64 LE) in the canonical pool datum.
pub const POOL_DATUM_LP_TOTAL_OFFSET: usize = 80;
/// Byte offset of `fee_bps` (u16 LE) in the canonical pool datum.
pub const POOL_DATUM_FEE_BPS_OFFSET: usize = 88;

/// Byte width of a token id field (32).
pub const POOL_DATUM_TOKEN_ID_LEN: usize = 32;
/// Byte width of a reserve / lp_total field (8).
pub const POOL_DATUM_RESERVE_LEN: usize = 8;
/// Byte width of the fee_bps field (2).
pub const POOL_DATUM_FEE_LEN: usize = 2;

/// Convention: the pool UTXO is always **input #0** of a pool transaction.
pub const POOL_INPUT_INDEX: i64 = 0;
/// Convention: the successor pool UTXO is always **output #0**.
pub const POOL_OUTPUT_INDEX: i64 = 0;

/// Append `<idx> <READ_*_DATUM> LEN <expected> EQ VERIFY` — asserts the
/// datum read by `read_op` at index 0 is exactly [`POOL_DATUM_LEN`] bytes.
fn assert_pool_datum_len(b: ScriptBuilder, read_op: OpCode) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX) // both conventions are index 0
        .op(read_op)
        .op(OpCode::Len)
        .push_int(POOL_DATUM_LEN as i64)
        .op(OpCode::Eq)
        .op(OpCode::Verify)
}

/// Append instructions that read the datum via `read_op`, slice
/// `expected.len()` bytes at `offset`, and `EQ VERIFY` against `expected`.
///
/// Index 0 is pushed for both directions: [`POOL_INPUT_INDEX`] and
/// [`POOL_OUTPUT_INDEX`] are both 0 by convention.
fn assert_pool_datum_field(
    b: ScriptBuilder,
    read_op: OpCode,
    offset: usize,
    expected: &[u8],
) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX)
        .op(read_op)
        .push_int(offset as i64)
        .push_int(expected.len() as i64)
        .op(OpCode::Slice)
        .push_bytes(expected)
        .op(OpCode::Eq)
        .op(OpCode::Verify)
}

/// Append instructions that read the datum via `read_op`, slice the 8-byte
/// LE reserve at `offset`, and convert it to an `Int` (u64 bit-reinterpret,
/// see `BYTES_TO_INT`) ready for `MUL_U128`.
fn push_pool_reserve(b: ScriptBuilder, read_op: OpCode, offset: usize) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX)
        .op(read_op)
        .push_int(offset as i64)
        .push_int(POOL_DATUM_RESERVE_LEN as i64)
        .op(OpCode::Slice)
        .push_int(POOL_DATUM_RESERVE_LEN as i64)
        .op(OpCode::BytesToInt)
}

/// Create the **AMM pool covenant** locking script (Faz 6 / D-2).
///
/// This is the real Shared-UTXO-Pattern pool lock: the pool lives in a
/// single UTXO whose datum is the canonical `PoolDatum` encoding (see
/// [`POOL_DATUM_LEN`] for the layout). The covenant validates the state
/// transition entirely on-chain — **no witness data is required** to spend
/// the pool UTXO; everything is read via introspection opcodes.
///
/// # Transaction shape convention
///
/// - The pool UTXO being spent is **input #0** (its datum is read with
///   `READ_INPUT_DATUM 0`).
/// - The successor pool UTXO is **output #0** (read with
///   `READ_OUTPUT_DATUM 0` / checked with `ASSERT_OUTPUT_SCRIPT_HASH 0`).
///
/// # Checks performed (in order)
///
/// 1. **Datum shape** — old and new datum are exactly 90 bytes.
/// 2. **Token-id pinning** — `token_a_id` / `token_b_id` bytes in *both*
///    the old and the new datum equal the ids baked into this script.
/// 3. **Fee pinning** — `fee_bps` in both datums equals the baked-in fee.
/// 4. **Constant-product invariant** — `new_a·new_b ≥ old_a·old_b`,
///    computed with `MUL_U128`/`GE_U128` so u64 reserves cannot overflow.
/// 5. **Script continuity** — output #0 is locked under the *same* script
///    bytes (`SELF_SCRIPT_HASH` + `ASSERT_OUTPUT_SCRIPT_HASH`). The script
///    cannot embed its own hash (self-reference), so the validator exposes
///    it via `Context::locking_script_hash` and the `SELF_SCRIPT_HASH`
///    opcode pushes it at run time.
///
/// # Fees and the invariant
///
/// `fee_bps` is intentionally **not** part of the arithmetic here: a swap
/// adds the *full* `amount_in` (fee included) to the input-side reserve and
/// removes `amount_out` computed from the net amount, so the swap fee stays
/// in the pool and the product strictly **grows** on every fee-charging
/// swap. `x·y ≥ k` therefore already enforces that the fee was paid — any
/// transition that tries to skim it shrinks the product and fails check 4.
///
/// # Known limitation
///
/// Liquidity *removal* legitimately shrinks the product and cannot satisfy
/// this covenant; pools that support remove-liquidity need a dedicated
/// spend path (Faz 6 / D-6 scope).
#[must_use]
pub fn amm_pool_lock(token_a_id: &[u8; 32], token_b_id: &[u8; 32], fee_bps: u16) -> Vec<u8> {
    let fee_le = fee_bps.to_le_bytes();
    let mut b = ScriptBuilder::new();

    // 1. Both datums must have the exact canonical shape.
    b = assert_pool_datum_len(b, OpCode::ReadInputDatum);
    b = assert_pool_datum_len(b, OpCode::ReadOutputDatum);

    // 2. Token ids pinned in the old AND the new datum (old == new follows).
    b = assert_pool_datum_field(b, OpCode::ReadInputDatum, POOL_DATUM_TOKEN_A_OFFSET, token_a_id);
    b = assert_pool_datum_field(b, OpCode::ReadOutputDatum, POOL_DATUM_TOKEN_A_OFFSET, token_a_id);
    b = assert_pool_datum_field(b, OpCode::ReadInputDatum, POOL_DATUM_TOKEN_B_OFFSET, token_b_id);
    b = assert_pool_datum_field(b, OpCode::ReadOutputDatum, POOL_DATUM_TOKEN_B_OFFSET, token_b_id);

    // 3. fee_bps pinned in the old AND the new datum.
    b = assert_pool_datum_field(b, OpCode::ReadInputDatum, POOL_DATUM_FEE_BPS_OFFSET, &fee_le);
    b = assert_pool_datum_field(b, OpCode::ReadOutputDatum, POOL_DATUM_FEE_BPS_OFFSET, &fee_le);

    // 4. Constant-product invariant: new_a*new_b >= old_a*old_b (u128 math).
    b = push_pool_reserve(b, OpCode::ReadOutputDatum, POOL_DATUM_RESERVE_A_OFFSET);
    b = push_pool_reserve(b, OpCode::ReadOutputDatum, POOL_DATUM_RESERVE_B_OFFSET);
    b = b.op(OpCode::MulU128); // new product (16-byte LE)
    b = push_pool_reserve(b, OpCode::ReadInputDatum, POOL_DATUM_RESERVE_A_OFFSET);
    b = push_pool_reserve(b, OpCode::ReadInputDatum, POOL_DATUM_RESERVE_B_OFFSET);
    b = b.op(OpCode::MulU128); // old product (16-byte LE)
    b = b.op(OpCode::GeU128).op(OpCode::Verify); // new >= old

    // 5. Script continuity: output #0 keeps this exact locking script.
    b.push_int(POOL_OUTPUT_INDEX)
        .op(OpCode::SelfScriptHash)
        .op(OpCode::AssertOutputScriptHash)
        // Success marker.
        .op(OpCode::Op1)
        .build()
}

// ============================================================================
// Template: lending repayment covenant
// ============================================================================

/// Create a **lending repayment covenant** locking script.
///
/// Ensures that a repayment output exists at `repay_output_index` with at
/// least `min_repay_amount` value, and the pool script hash is preserved.
#[must_use]
pub fn lending_repay(
    pool_script_hash: &[u8; 32],
    pool_output_index: i64,
    min_repay_amount: i64,
) -> Vec<u8> {
    ScriptBuilder::new()
        // Check repayment amount
        .push_int(pool_output_index)
        .op(OpCode::ReadOutputValue)
        .push_int(min_repay_amount)
        .op(OpCode::Ge)
        .op(OpCode::Verify)
        // Check script hash preserved
        .push_int(pool_output_index)
        .push_bytes(pool_script_hash)
        .op(OpCode::AssertOutputScriptHash)
        // Success
        .op(OpCode::Op1)
        .build()
}

// ============================================================================
// Template: lending pool covenant (lending_pool_lock) — ADR-013
// ============================================================================

/// Total length of the **canonical** (script-friendly) `LendingPoolDatum`
/// encoding in bytes. Like the AMM [`POOL_DATUM_LEN`], the layout is
/// fixed-width little-endian so the covenant can `SLICE` fields at fixed
/// offsets; `qv-defi`'s `LendingPoolDatum::to_canonical_bytes()` emits
/// exactly this layout:
///
/// | Bytes    | Field                       | Type         |
/// |----------|-----------------------------|--------------|
/// | 0..32    | `pool_id`                   | 32 raw bytes |
/// | 32..64   | `collateral_token_id`       | 32 raw bytes |
/// | 64..96   | `debt_token_id`             | 32 raw bytes |
/// | 96..104  | `total_collateral`          | `u64` LE     |
/// | 104..112 | `total_debt`                | `u64` LE     |
/// | 112..114 | `base_rate_bps`             | `u16` LE     |
/// | 114..116 | `slope_bps`                 | `u16` LE     |
/// | 116..118 | `ltv_max_bps`               | `u16` LE     |
/// | 118..120 | `liquidation_threshold_bps` | `u16` LE     |
/// | 120..122 | `liquidation_bonus_bps`     | `u16` LE     |
/// | 122..138 | `interest_multiplier_q64`   | `u128` LE    |
/// | 138..146 | `last_accrual_slot`         | `u64` LE     |
pub const LENDING_DATUM_LEN: usize = 146;

/// Byte offset of `pool_id` in the canonical lending datum.
pub const LENDING_DATUM_POOL_ID_OFFSET: usize = 0;
/// Byte offset of `collateral_token_id`.
pub const LENDING_DATUM_COLLATERAL_TOKEN_OFFSET: usize = 32;
/// Byte offset of `debt_token_id`.
pub const LENDING_DATUM_DEBT_TOKEN_OFFSET: usize = 64;
/// Byte offset of `total_collateral` (u64 LE).
pub const LENDING_DATUM_TOTAL_COLLATERAL_OFFSET: usize = 96;
/// Byte offset of `total_debt` (u64 LE).
pub const LENDING_DATUM_TOTAL_DEBT_OFFSET: usize = 104;
/// Byte offset of `base_rate_bps` (u16 LE).
pub const LENDING_DATUM_BASE_RATE_OFFSET: usize = 112;
/// Byte offset of `slope_bps` (u16 LE).
pub const LENDING_DATUM_SLOPE_OFFSET: usize = 114;
/// Byte offset of `ltv_max_bps` (u16 LE).
pub const LENDING_DATUM_LTV_MAX_OFFSET: usize = 116;
/// Byte offset of `liquidation_threshold_bps` (u16 LE).
pub const LENDING_DATUM_LIQ_THRESHOLD_OFFSET: usize = 118;
/// Byte offset of `liquidation_bonus_bps` (u16 LE).
pub const LENDING_DATUM_LIQ_BONUS_OFFSET: usize = 120;
/// Byte offset of `interest_multiplier_q64` (u128 LE).
pub const LENDING_DATUM_INTEREST_MULTIPLIER_OFFSET: usize = 122;
/// Byte offset of `last_accrual_slot` (u64 LE).
pub const LENDING_DATUM_LAST_ACCRUAL_SLOT_OFFSET: usize = 138;

/// Width of the identity region pinned by the covenant: `pool_id` +
/// `collateral_token_id` + `debt_token_id` (bytes `0..96`).
pub const LENDING_DATUM_IDS_LEN: usize = 96;
/// Width of the risk-parameter region pinned by the covenant: the five
/// `u16` bps fields (bytes `112..122`).
pub const LENDING_DATUM_PARAMS_LEN: usize = 10;
/// Offset of the risk-parameter region.
pub const LENDING_DATUM_PARAMS_OFFSET: usize = LENDING_DATUM_BASE_RATE_OFFSET;
/// Width of the interest region **frozen** by the v1 covenant:
/// `interest_multiplier_q64` + `last_accrual_slot` (bytes `122..146`).
/// See ADR-013 §4: on-chain accrual needs 128-bit add/wide-mul opcodes;
/// until those exist the only sound enforceable rule is constancy.
pub const LENDING_DATUM_INTEREST_BLOCK_LEN: usize = 24;
/// Offset of the frozen interest region.
pub const LENDING_DATUM_INTEREST_BLOCK_OFFSET: usize = LENDING_DATUM_INTEREST_MULTIPLIER_OFFSET;

/// Domain separator for the oracle-signed price message (ADR-013 §3).
///
/// The oracle operator signs `TAG ‖ pool_id ‖ price_scaled(u64 LE) ‖
/// slot(u64 LE)` with ML-DSA. The covenant script rebuilds this message
/// from witness data (`CAT`) — the `TAG ‖ pool_id` prefix is baked into
/// the script, so a price signed for one pool can never be replayed
/// against another. `qv-defi::tx_helpers::oracle_price_message` MUST
/// compose the message with this exact constant.
pub const LENDING_ORACLE_DOMAIN_TAG: &[u8] = b"QuantumVault-Lending-Oracle-v1";

/// Fixed-point scale of the oracle price carried in the witness.
///
/// `price_scaled = price · 10^6` where `price` is "debt smallest-units
/// per collateral smallest-unit". A `u64` `price_scaled` therefore covers
/// prices up to ~1.8·10¹³ with 6 decimal digits of precision.
pub const LENDING_PRICE_SCALE: u64 = 1_000_000;

/// Risk parameters baked into a [`lending_pool_lock`] script and pinned
/// (byte-equal) in both the old and the new datum's `112..122` region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LendingPoolScriptParams {
    /// Base interest rate in basis points.
    pub base_rate_bps: u16,
    /// Rate slope per utilization, in basis points.
    pub slope_bps: u16,
    /// Maximum loan-to-value in basis points (`1..=10_000`).
    pub ltv_max_bps: u16,
    /// Liquidation threshold in basis points.
    pub liquidation_threshold_bps: u16,
    /// Liquidation bonus in basis points.
    pub liquidation_bonus_bps: u16,
}

impl LendingPoolScriptParams {
    /// Encode the five bps fields as the canonical 10-byte LE region
    /// (datum bytes `112..122`).
    #[must_use]
    pub fn to_le_bytes(&self) -> [u8; LENDING_DATUM_PARAMS_LEN] {
        let mut out = [0u8; LENDING_DATUM_PARAMS_LEN];
        out[0..2].copy_from_slice(&self.base_rate_bps.to_le_bytes());
        out[2..4].copy_from_slice(&self.slope_bps.to_le_bytes());
        out[4..6].copy_from_slice(&self.ltv_max_bps.to_le_bytes());
        out[6..8].copy_from_slice(&self.liquidation_threshold_bps.to_le_bytes());
        out[8..10].copy_from_slice(&self.liquidation_bonus_bps.to_le_bytes());
        out
    }
}

/// Compute the collateral factor `K = ceil(10_000 · PRICE_SCALE / ltv)`
/// baked into the borrow/withdraw path of [`lending_pool_lock`].
///
/// The on-chain collateral check is the division-free two-factor form of
/// `debt ≤ collateral · price · ltv` (ADR-013 §2):
///
/// ```text
/// total_debt_new · K  ≤  total_collateral_new · price_scaled
/// ```
///
/// Rounding `K` **up** makes the check strictly conservative (borrower-
/// adverse, pool-safe). Overflow analysis: `K ≤ 10¹⁰ < 2³⁴`, so
/// `debt · K < 2⁹⁸` and `collateral · price_scaled < 2¹²⁸` — both sides
/// are exact in `MUL_U128`'s u128 output.
pub fn lending_ltv_factor(ltv_max_bps: u16) -> Result<u64, TemplateError> {
    if ltv_max_bps == 0 || ltv_max_bps > 10_000 {
        return Err(TemplateError::InvalidLtv(ltv_max_bps));
    }
    // 10_000 · 10^6 = 10^10 — fits u64 with headroom for the ceil add.
    let numerator: u64 = 10_000 * LENDING_PRICE_SCALE;
    // Divisor is in 1..=10_000 (validated above) — div_ceil cannot panic.
    Ok(numerator.div_ceil(u64::from(ltv_max_bps)))
}

/// Append a balanced check: `datum(read_op, index 0)[offset..offset+len]
/// == expected` → `EQ VERIFY`.
fn assert_lending_datum_region(
    b: ScriptBuilder,
    read_op: OpCode,
    offset: usize,
    expected: &[u8],
) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX) // pool input AND output are both index 0
        .op(read_op)
        .push_int(offset as i64)
        .push_int(expected.len() as i64)
        .op(OpCode::Slice)
        .push_bytes(expected)
        .op(OpCode::Eq)
        .op(OpCode::Verify)
}

/// Append: read datum via `read_op`, slice the 8-byte LE field at
/// `offset`, zero-extend to 16 bytes (`CAT` with 8 zero bytes) so it can
/// be compared with `GE_U128` over the **full** u64 range (no i64
/// sign-reinterpretation pitfalls).
fn push_lending_u64_as_u128(b: ScriptBuilder, read_op: OpCode, offset: usize) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX)
        .op(read_op)
        .push_int(offset as i64)
        .push_int(8)
        .op(OpCode::Slice)
        .push_bytes(&[0u8; 8])
        .op(OpCode::Cat)
}

/// Append: read datum via `read_op`, slice the 8-byte LE field at
/// `offset`, convert to an `Int` (u64 bit-reinterpret) for `MUL_U128`.
fn push_lending_u64_as_int(b: ScriptBuilder, read_op: OpCode, offset: usize) -> ScriptBuilder {
    b.push_int(POOL_INPUT_INDEX)
        .op(read_op)
        .push_int(offset as i64)
        .push_int(8)
        .op(OpCode::Slice)
        .push_int(8)
        .op(OpCode::BytesToInt)
}

/// Create the **lending pool covenant** locking script (Faz 6 / D-6,
/// ADR-013).
///
/// The pool lives in a single UTXO whose datum is the canonical
/// 146-byte `LendingPoolDatum` encoding ([`LENDING_DATUM_LEN`]). The
/// pool UTXO being spent is **input #0**; the successor pool UTXO is
/// **output #0** (same convention as [`amm_pool_lock`]).
///
/// # Spend paths (selected by the witness top-of-stack flag)
///
/// - **Path 0 — deposit / repay** (witness: `OP_0`): no price needed.
///   Enforces `total_collateral_new ≥ total_collateral_old` and
///   `total_debt_new ≤ total_debt_old` — transitions that can only
///   improve pool health.
/// - **Path 1 — borrow / withdraw** (witness, bottom→top:
///   `<oracle_sig> <oracle_pubkey> <price_scaled u64-LE> <price_slot
///   u64-LE> OP_1`): verifies, in order, price freshness
///   (`price_slot ≤ current_slot` and `current_slot − price_slot ≤
///   max_price_age_slots`), the division-free collateral check
///   `total_debt_new · K ≤ total_collateral_new · price_scaled` (see
///   [`lending_ltv_factor`]), and a **real ML-DSA signature** over
///   `LENDING_ORACLE_DOMAIN_TAG ‖ pool_id ‖ price ‖ slot` against the
///   baked-in `oracle_pk_hash` (`CHECKSIG_PQC`).
///
/// # Common checks (both paths)
///
/// 1. Old and new datum are exactly 146 bytes.
/// 2. Identity region (`0..96`) and risk-parameter region (`112..122`)
///    pinned to the script's baked-in bytes in **both** datums.
/// 3. Interest region (`122..146`) **frozen**: old slice == new slice
///    (ADR-013 §4 — no in-script accrual in v1, but no tampering either).
/// 4. Pool native value cannot decrease: `output0.value ≥ input0.value`.
/// 5. Script continuity: `SELF_SCRIPT_HASH` + `ASSERT_OUTPUT_SCRIPT_HASH`.
///
/// # Honest limits (v1)
///
/// Single oracle key (centralisation trade-off, ADR-013 §3); aggregate
/// (pool-level) LTV only — per-position enforcement and liquidation need
/// position UTXOs (v2, ADR-013 §5); collateral/debt movements are
/// datum-level accounting (native multi-asset settlement is a later
/// Faz 6 slice).
pub fn lending_pool_lock(
    pool_id: &[u8; 32],
    collateral_token_id: &[u8; 32],
    debt_token_id: &[u8; 32],
    params: &LendingPoolScriptParams,
    oracle_pk_hash: &[u8; 32],
    max_price_age_slots: u64,
) -> Result<Vec<u8>, TemplateError> {
    let k = lending_ltv_factor(params.ltv_max_bps)?;
    // k ≤ 10^10 < 2^63 — the cast cannot wrap (guarded by the line above).
    #[allow(clippy::cast_possible_wrap)]
    let k_i64 = k as i64;
    let max_age = i64::try_from(max_price_age_slots)
        .map_err(|_| TemplateError::MaxAgeTooLarge(max_price_age_slots))?;

    // Identity region constant: pool_id ‖ collateral_token ‖ debt_token.
    let mut ids = Vec::with_capacity(LENDING_DATUM_IDS_LEN);
    ids.extend_from_slice(pool_id);
    ids.extend_from_slice(collateral_token_id);
    ids.extend_from_slice(debt_token_id);
    let params_le = params.to_le_bytes();

    // Oracle message prefix baked into the script: TAG ‖ pool_id.
    let mut oracle_prefix =
        Vec::with_capacity(LENDING_ORACLE_DOMAIN_TAG.len().wrapping_add(32));
    oracle_prefix.extend_from_slice(LENDING_ORACLE_DOMAIN_TAG);
    oracle_prefix.extend_from_slice(pool_id);

    let mut b = ScriptBuilder::new();

    // ---- Common checks (balanced — run with the witness still on stack) ----

    // 1. Both datums must have the exact canonical shape (146 bytes).
    for read_op in [OpCode::ReadInputDatum, OpCode::ReadOutputDatum] {
        b = b
            .push_int(POOL_INPUT_INDEX)
            .op(read_op)
            .op(OpCode::Len)
            .push_int(LENDING_DATUM_LEN as i64)
            .op(OpCode::Eq)
            .op(OpCode::Verify);
    }

    // 2. Identity + risk-parameter regions pinned in old AND new datum.
    for read_op in [OpCode::ReadInputDatum, OpCode::ReadOutputDatum] {
        b = assert_lending_datum_region(b, read_op, LENDING_DATUM_POOL_ID_OFFSET, &ids);
        b = assert_lending_datum_region(b, read_op, LENDING_DATUM_PARAMS_OFFSET, &params_le);
    }

    // 3. Interest region frozen: old slice == new slice (24 bytes).
    b = b
        .push_int(POOL_INPUT_INDEX)
        .op(OpCode::ReadInputDatum)
        .push_int(LENDING_DATUM_INTEREST_BLOCK_OFFSET as i64)
        .push_int(LENDING_DATUM_INTEREST_BLOCK_LEN as i64)
        .op(OpCode::Slice)
        .push_int(POOL_OUTPUT_INDEX)
        .op(OpCode::ReadOutputDatum)
        .push_int(LENDING_DATUM_INTEREST_BLOCK_OFFSET as i64)
        .push_int(LENDING_DATUM_INTEREST_BLOCK_LEN as i64)
        .op(OpCode::Slice)
        .op(OpCode::Eq)
        .op(OpCode::Verify);

    // 4. Pool native value cannot decrease: output0.value ≥ input0.value.
    //    (Amounts are < 2^63 — 21M fixed supply — so Int compare is exact.)
    b = b
        .push_int(POOL_OUTPUT_INDEX)
        .op(OpCode::ReadOutputValue)
        .push_int(POOL_INPUT_INDEX)
        .op(OpCode::ReadInputValue)
        .op(OpCode::Ge)
        .op(OpCode::Verify);

    // ---- Branch on the witness selector ----
    b = b.op(OpCode::If);

    // ======== Path 1: borrow / withdraw ========
    // Stack (bottom→top): sig pk price slot
    b = b
        // price_int (keep the raw bytes for the signed message)
        .op(OpCode::Over) //              sig pk price slot price
        .push_int(8)
        .op(OpCode::BytesToInt) //        sig pk price slot price_int
        // slot_int (keep the raw bytes for the signed message)
        .op(OpCode::Over) //              … price_int slot
        .push_int(8)
        .op(OpCode::BytesToInt) //        … price_int slot_int
        // Freshness 1: price_slot ≤ current_slot.
        .op(OpCode::Dup)
        .op(OpCode::SlotNumber)
        .op(OpCode::Le)
        .op(OpCode::Verify) //            … price_int slot_int
        // Freshness 2: current_slot − price_slot ≤ max_age.
        .op(OpCode::SlotNumber)
        .op(OpCode::Swap)
        .op(OpCode::Sub) //               … price_int age
        .push_int(max_age)
        .op(OpCode::Le)
        .op(OpCode::Verify); //           sig pk price slot price_int

    // Collateral check: debt_new · K ≤ collateral_new · price_scaled.
    b = push_lending_u64_as_int(b, OpCode::ReadOutputDatum, LENDING_DATUM_TOTAL_DEBT_OFFSET);
    b = b.push_int(k_i64).op(OpCode::MulU128); // … price_int lhs(16B)
    b = push_lending_u64_as_int(
        b,
        OpCode::ReadOutputDatum,
        LENDING_DATUM_TOTAL_COLLATERAL_OFFSET,
    ); //                                        … price_int lhs coll_int
    b = b
        .op(OpCode::Rot) //                      … lhs coll_int price_int
        .op(OpCode::MulU128) //                  … lhs rhs(16B)
        .op(OpCode::Swap) //                     … rhs lhs
        .op(OpCode::GeU128) //                   rhs ≥ lhs
        .op(OpCode::Verify); //                  sig pk price slot

    // Rebuild the signed message: TAG ‖ pool_id ‖ price ‖ slot.
    b = b
        .op(OpCode::Swap) //              sig pk slot price
        .push_bytes(&oracle_prefix) //    sig pk slot price prefix
        .op(OpCode::Swap)
        .op(OpCode::Cat) //               sig pk slot (prefix‖price)
        .op(OpCode::Swap)
        .op(OpCode::Cat); //              sig pk msg

    // Oracle pubkey binding + real PQC signature verification.
    b = b
        .op(OpCode::Over) //              sig pk msg pk
        .op(OpCode::HashSha3) //          sig pk msg h
        .push_bytes(oracle_pk_hash) //    sig pk msg h expected
        .op(OpCode::Eq)
        .op(OpCode::Verify) //            sig pk msg
        .op(OpCode::Rot) //               pk msg sig
        .op(OpCode::Rot) //               msg sig pk
        .op(OpCode::CheckSigPqc)
        .op(OpCode::Verify); //           (empty)

    // ======== Path 0: deposit / repay ========
    b = b.op(OpCode::Else);

    // total_collateral_new ≥ total_collateral_old (zero-extended GE_U128).
    b = push_lending_u64_as_u128(
        b,
        OpCode::ReadOutputDatum,
        LENDING_DATUM_TOTAL_COLLATERAL_OFFSET,
    );
    b = push_lending_u64_as_u128(
        b,
        OpCode::ReadInputDatum,
        LENDING_DATUM_TOTAL_COLLATERAL_OFFSET,
    );
    b = b.op(OpCode::GeU128).op(OpCode::Verify);

    // total_debt_old ≥ total_debt_new.
    b = push_lending_u64_as_u128(b, OpCode::ReadInputDatum, LENDING_DATUM_TOTAL_DEBT_OFFSET);
    b = push_lending_u64_as_u128(b, OpCode::ReadOutputDatum, LENDING_DATUM_TOTAL_DEBT_OFFSET);
    b = b.op(OpCode::GeU128).op(OpCode::Verify);

    b = b.op(OpCode::EndIf);

    // 5. Script continuity: output #0 keeps this exact locking script.
    Ok(b
        .push_int(POOL_OUTPUT_INDEX)
        .op(OpCode::SelfScriptHash)
        .op(OpCode::AssertOutputScriptHash)
        // Success marker.
        .op(OpCode::Op1)
        .build())
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
    use crate::interpreter::{execute, Context};
    use crate::opcode::decode_script;
    use qv_core::{
        Amount, Datum, OutPoint, Script as CoreScript, Slot, Transaction, TxId, TxInput, TxOutput,
    };

    #[test]
    fn p2pkh_script_decompiles_correctly() {
        let pk_hash = [0xAB; 32];
        let script_bytes = p2pkh_pqc(&pk_hash);
        let instrs = decode_script(&script_bytes).unwrap();
        // DUP, HASH_SHA3, PUSH(32 bytes), EQ, VERIFY, SIG_HASH, ROT, ROT, CHECKSIG_PQC
        assert_eq!(instrs.len(), 9);
        assert_eq!(instrs[0].op, OpCode::Dup);
        assert_eq!(instrs[1].op, OpCode::HashSha3);
        assert_eq!(instrs[2].op, OpCode::Push1);
        assert_eq!(instrs[2].data, pk_hash.to_vec());
        assert_eq!(instrs[3].op, OpCode::Eq);
        assert_eq!(instrs[4].op, OpCode::Verify);
        assert_eq!(instrs[5].op, OpCode::SigHash);
        assert_eq!(instrs[6].op, OpCode::Rot);
        assert_eq!(instrs[7].op, OpCode::Rot);
        assert_eq!(instrs[8].op, OpCode::CheckSigPqc);
    }

    #[test]
    fn p2pkh_spendable_with_sighash_witness() {
        use qv_crypto::{pqc_sign, DilithiumLevel};

        // Owner keypair; the locking script commits to SHA3-256(pubkey).
        let kp = pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap();
        let pubkey = kp.public.as_bytes().to_vec();
        let pk_hash = pubkey_hash(&pubkey);
        let script_bytes = p2pkh_pqc(&pk_hash);

        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(7));

        // Sign the witness-excluded sighash — the script pulls it via SIG_HASH.
        let sig = pqc_sign::sign(&kp.secret, &ctx.sighash).unwrap();

        // Witness: <sig> <pubkey>  (bottom -> top). No message carried.
        let mut witness_and_script = ScriptBuilder::new()
            .push_bytes(sig.as_bytes())
            .push_bytes(&pubkey)
            .build();
        witness_and_script.extend_from_slice(&script_bytes);

        let mut gas = GasMeter::new(10_000_000);
        let result = execute(&witness_and_script, &ctx, &mut gas).unwrap();
        assert!(
            result.success,
            "p2pkh output must be spendable with a sighash-bound signature"
        );
    }

    #[test]
    fn p2pkh_rejects_signature_for_other_tx() {
        use qv_crypto::{pqc_sign, DilithiumLevel};

        let kp = pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap();
        let pubkey = kp.public.as_bytes().to_vec();
        let pk_hash = pubkey_hash(&pubkey);
        let script_bytes = p2pkh_pqc(&pk_hash);

        // The legitimate transaction the owner intends to sign.
        let real_tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let real_ctx = Context::new(real_tx, resolved.clone(), Slot::from(7));
        let sig = pqc_sign::sign(&kp.secret, &real_ctx.sighash).unwrap();

        // Attacker reuses <sig> <pubkey> on a DIFFERENT tx (outputs redirected).
        let evil_tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::new(vec![0xEE]))],
        );
        let evil_ctx = Context::new(evil_tx, resolved, Slot::from(7));
        assert_ne!(
            real_ctx.sighash, evil_ctx.sighash,
            "redirecting outputs must change the sighash"
        );

        let mut witness_and_script = ScriptBuilder::new()
            .push_bytes(sig.as_bytes())
            .push_bytes(&pubkey)
            .build();
        witness_and_script.extend_from_slice(&script_bytes);

        let mut gas = GasMeter::new(10_000_000);
        let result = execute(&witness_and_script, &evil_ctx, &mut gas).unwrap();
        assert!(
            !result.success,
            "a signature bound to another tx's sighash must not validate here"
        );
    }

    #[test]
    fn stealth_p2pkh_decompiles_correctly() {
        let onetime = [0xCD; 32];
        let instrs = decode_script(&stealth_p2pkh(&onetime)).unwrap();
        // OVER CAT PUSH(tag) SWAP CAT HASH_SHA3 PUSH(hash) EQ VERIFY SIG_HASH ROT ROT CHECKSIG_PQC
        assert_eq!(instrs.len(), 13);
        assert_eq!(instrs[0].op, OpCode::Over);
        assert_eq!(instrs[1].op, OpCode::Cat);
        assert_eq!(instrs[2].op, OpCode::Push1);
        assert_eq!(instrs[2].data, b"QuantumVault-Stealth-v1".to_vec());
        assert_eq!(instrs[3].op, OpCode::Swap);
        assert_eq!(instrs[4].op, OpCode::Cat);
        assert_eq!(instrs[5].op, OpCode::HashSha3);
        assert_eq!(instrs[6].op, OpCode::Push1);
        assert_eq!(instrs[6].data, onetime.to_vec());
        assert_eq!(instrs[7].op, OpCode::Eq);
        assert_eq!(instrs[8].op, OpCode::Verify);
        assert_eq!(instrs[9].op, OpCode::SigHash);
        assert_eq!(instrs[10].op, OpCode::Rot);
        assert_eq!(instrs[11].op, OpCode::Rot);
        assert_eq!(instrs[12].op, OpCode::CheckSigPqc);
    }

    #[test]
    fn stealth_p2pkh_spendable_with_correct_witness() {
        use qv_crypto::{pqc_sign, DilithiumLevel};

        // Recipient's static spend keypair + an arbitrary shared secret.
        let kp = pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap();
        let spend_pk = kp.public.as_bytes().to_vec();
        let shared_secret = vec![0x5Au8; 32];

        // Commitment exactly as qv-privacy's compute_onetime_pk_hash builds it:
        // SHA3-256(STEALTH_KDF_TAG || shared_secret || spend_pk).
        let mut preimage = b"QuantumVault-Stealth-v1".to_vec();
        preimage.extend_from_slice(&shared_secret);
        preimage.extend_from_slice(&spend_pk);
        let onetime_pk_hash = sha3_256(&preimage);

        let script_bytes = stealth_p2pkh(&onetime_pk_hash);

        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(7));

        // Sign the witness-excluded sighash — the script pulls it via SIG_HASH.
        let sig = pqc_sign::sign(&kp.secret, &ctx.sighash).unwrap();

        // Witness: <sig> <spend_pk> <shared_secret>  (bottom -> top).
        let mut witness_and_script = ScriptBuilder::new()
            .push_bytes(sig.as_bytes())
            .push_bytes(&spend_pk)
            .push_bytes(&shared_secret)
            .build();
        witness_and_script.extend_from_slice(&script_bytes);

        let mut gas = GasMeter::new(10_000_000);
        let result = execute(&witness_and_script, &ctx, &mut gas).unwrap();
        assert!(
            result.success,
            "stealth output must be spendable with the correct witness"
        );
    }

    #[test]
    fn stealth_p2pkh_rejects_wrong_shared_secret() {
        let spend_pk = vec![0xABu8; 96];
        let correct_ss = vec![0x5Au8; 32];

        let mut preimage = b"QuantumVault-Stealth-v1".to_vec();
        preimage.extend_from_slice(&correct_ss);
        preimage.extend_from_slice(&spend_pk);
        let onetime_pk_hash = sha3_256(&preimage);
        let script_bytes = stealth_p2pkh(&onetime_pk_hash);

        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(100), CoreScript::default())],
        );
        let resolved = vec![TxOutput::new(Amount::from(100), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(7));

        // Witness carries the WRONG shared secret -> commitment check fails.
        let mut witness_and_script = ScriptBuilder::new()
            .push_bytes(&[0u8; 8]) // dummy signature
            .push_bytes(&spend_pk)
            .push_bytes(&[0x99u8; 32]) // wrong shared secret
            .build();
        witness_and_script.extend_from_slice(&script_bytes);

        let mut gas = GasMeter::new(10_000_000);
        let err = execute(&witness_and_script, &ctx, &mut gas).unwrap_err();
        assert!(matches!(err, crate::interpreter::ScriptError::VerifyFailed));
    }

    #[test]
    fn multisig_script_decompiles_correctly() {
        let pk_hashes = [[1u8; 32], [2u8; 32], [3u8; 32]];
        let script_bytes = multisig_pqc(2, &pk_hashes);
        let instrs = decode_script(&script_bytes).unwrap();
        // PUSH_INT(2), PUSH_INT(3), CHECKMULTISIG_PQC
        assert_eq!(instrs.len(), 3);
        assert_eq!(instrs[0].op, OpCode::PushInt);
        assert_eq!(instrs[1].op, OpCode::PushInt);
        assert_eq!(instrs[2].op, OpCode::CheckMultiSigPqc);
    }

    #[test]
    fn script_builder_push_int_shortcuts() {
        let b = ScriptBuilder::new()
            .push_int(0)
            .push_int(1)
            .push_int(42)
            .build();
        let instrs = decode_script(&b).unwrap();
        assert_eq!(instrs[0].op, OpCode::Op0);
        assert_eq!(instrs[1].op, OpCode::Op1);
        assert_eq!(instrs[2].op, OpCode::PushInt);
    }

    #[test]
    fn lending_repay_covenant_validates() {
        let pool_script = vec![0xDD; 10];
        let pool_script_hash = sha3_256(&pool_script);
        let script_bytes = lending_repay(&pool_script_hash, 0, 500);

        // Build a tx where output 0 has value=500 and the correct script
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(
                Amount::from(500),
                CoreScript::new(pool_script),
            )],
        );
        let resolved = vec![TxOutput::new(Amount::from(600), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(100));
        let mut gas = GasMeter::new(100_000);
        let result = execute(&script_bytes, &ctx, &mut gas).unwrap();
        assert!(result.success);
    }

    #[test]
    fn lending_repay_rejects_low_value() {
        let pool_script = vec![0xDD; 10];
        let pool_script_hash = sha3_256(&pool_script);
        let script_bytes = lending_repay(&pool_script_hash, 0, 500);

        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(
                Amount::from(499), // less than 500
                CoreScript::new(pool_script),
            )],
        );
        let resolved = vec![TxOutput::new(Amount::from(600), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(100));
        let mut gas = GasMeter::new(100_000);
        let err = execute(&script_bytes, &ctx, &mut gas).unwrap_err();
        assert!(matches!(err, crate::interpreter::ScriptError::VerifyFailed));
    }

    #[test]
    fn amm_swap_covenant_validates_script_hash() {
        let pool_script = vec![0xEE; 20];
        let pool_script_hash = sha3_256(&pool_script);

        // Datum: 3 × i64 LE
        let mut datum = Vec::new();
        datum.extend_from_slice(&1000_i64.to_le_bytes()); // reserve_x
        datum.extend_from_slice(&2000_i64.to_le_bytes()); // reserve_y
        datum.extend_from_slice(&30_i64.to_le_bytes()); // fee_bps

        let script_bytes = amm_swap(&pool_script_hash, 0);

        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![TxOutput::new(Amount::from(1), CoreScript::new(pool_script))
                .with_datum(Datum::new(datum.clone()))],
        );

        // Witness pushes old_datum onto the stack before the locking script
        let mut witness_and_script = ScriptBuilder::new().push_bytes(&datum).build();
        witness_and_script.extend_from_slice(&script_bytes);

        let resolved = vec![TxOutput::new(Amount::from(1), CoreScript::default())];
        let ctx = Context::new(tx, resolved, Slot::from(50));
        let mut gas = GasMeter::new(100_000);
        let result = execute(&witness_and_script, &ctx, &mut gas).unwrap();
        assert!(result.success);
    }

    // ------------------------------------------------------------------
    // amm_pool_lock (Faz 6 / D-2) — full constant-product pool covenant
    // ------------------------------------------------------------------

    use crate::interpreter::ScriptError;
    use crate::script::validate_script;

    const TOKEN_A: [u8; 32] = [0xA1; 32];
    const TOKEN_B: [u8; 32] = [0xB2; 32];
    const POOL_FEE_BPS: u16 = 30;

    /// Build a canonical 90-byte pool datum (mirrors
    /// `qv_defi::PoolDatum::to_canonical_bytes`).
    fn pool_datum_bytes(
        token_a: &[u8; 32],
        token_b: &[u8; 32],
        reserve_a: u64,
        reserve_b: u64,
        lp_total: u64,
        fee_bps: u16,
    ) -> Vec<u8> {
        let mut d = Vec::with_capacity(POOL_DATUM_LEN);
        d.extend_from_slice(token_a);
        d.extend_from_slice(token_b);
        d.extend_from_slice(&reserve_a.to_le_bytes());
        d.extend_from_slice(&reserve_b.to_le_bytes());
        d.extend_from_slice(&lp_total.to_le_bytes());
        d.extend_from_slice(&fee_bps.to_le_bytes());
        assert_eq!(d.len(), POOL_DATUM_LEN);
        d
    }

    /// Standard old datum: reserves (10_000, 10_000), lp 5_000, fee 30.
    fn old_pool_datum() -> Vec<u8> {
        pool_datum_bytes(&TOKEN_A, &TOKEN_B, 10_000, 10_000, 5_000, POOL_FEE_BPS)
    }

    /// Build the pool spending fixture: a tx whose input #0 consumes the
    /// pool UTXO (locked by `amm_pool_lock`, carrying `old_datum`) and
    /// whose output #0 recreates the pool with `new_datum`.
    fn pool_swap_fixture(
        old_datum: Vec<u8>,
        new_datum: Vec<u8>,
    ) -> (Vec<u8>, Transaction, Vec<TxOutput>) {
        let script_bytes = amm_pool_lock(&TOKEN_A, &TOKEN_B, POOL_FEE_BPS);
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([1; 32]), 0))],
            vec![
                TxOutput::new(Amount::from(1_000), CoreScript::new(script_bytes.clone()))
                    .with_datum(Datum::new(new_datum)),
            ],
        );
        let resolved = vec![
            TxOutput::new(Amount::from(1_000), CoreScript::new(script_bytes.clone()))
                .with_datum(Datum::new(old_datum)),
        ];
        (script_bytes, tx, resolved)
    }

    #[test]
    fn pool_datum_layout_constants_are_contiguous() {
        assert_eq!(POOL_DATUM_TOKEN_A_OFFSET, 0);
        assert_eq!(
            POOL_DATUM_TOKEN_B_OFFSET,
            POOL_DATUM_TOKEN_A_OFFSET + POOL_DATUM_TOKEN_ID_LEN
        );
        assert_eq!(
            POOL_DATUM_RESERVE_A_OFFSET,
            POOL_DATUM_TOKEN_B_OFFSET + POOL_DATUM_TOKEN_ID_LEN
        );
        assert_eq!(
            POOL_DATUM_RESERVE_B_OFFSET,
            POOL_DATUM_RESERVE_A_OFFSET + POOL_DATUM_RESERVE_LEN
        );
        assert_eq!(
            POOL_DATUM_LP_TOTAL_OFFSET,
            POOL_DATUM_RESERVE_B_OFFSET + POOL_DATUM_RESERVE_LEN
        );
        assert_eq!(
            POOL_DATUM_FEE_BPS_OFFSET,
            POOL_DATUM_LP_TOTAL_OFFSET + POOL_DATUM_RESERVE_LEN
        );
        assert_eq!(
            POOL_DATUM_LEN,
            POOL_DATUM_FEE_BPS_OFFSET + POOL_DATUM_FEE_LEN
        );
    }

    #[test]
    fn amm_pool_lock_embeds_fee_bps_little_endian() {
        let instrs = decode_script(&amm_pool_lock(&TOKEN_A, &TOKEN_B, 30)).unwrap();
        // fee_bps = 30 → LE bytes [0x1E, 0x00] must appear as a push.
        assert!(
            instrs
                .iter()
                .any(|i| i.op == OpCode::Push1 && i.data == vec![0x1E, 0x00]),
            "fee_bps LE push missing"
        );
        // Token ids must be pushed verbatim.
        assert!(instrs.iter().any(|i| i.data == TOKEN_A.to_vec()));
        assert!(instrs.iter().any(|i| i.data == TOKEN_B.to_vec()));
        // Script continuity opcodes present.
        assert!(instrs.iter().any(|i| i.op == OpCode::SelfScriptHash));
        assert!(instrs
            .iter()
            .any(|i| i.op == OpCode::AssertOutputScriptHash));
    }

    #[test]
    fn amm_pool_lock_accepts_valid_swap() {
        // Swap 1000 A in (fee 30 bps): new reserves (11_000, 9_094).
        // 11_000 * 9_094 = 100_034_000 >= 100_000_000 = 10_000 * 10_000.
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, 11_000, 9_094, 5_000, POOL_FEE_BPS);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        // No witness needed: the covenant validates purely via introspection.
        let result = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap();
        assert!(result.success, "valid swap must satisfy the pool covenant");
    }

    #[test]
    fn amm_pool_lock_accepts_no_op_transition() {
        // new == old → product equal → GE holds (boundary case).
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), old_pool_datum());
        let result = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap();
        assert!(result.success);
    }

    #[test]
    fn amm_pool_lock_rejects_reserve_theft() {
        // Attacker removes 1000 A without paying: product 90M < 100M.
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, 9_000, 10_000, 5_000, POOL_FEE_BPS);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_rejects_token_id_change() {
        // Same reserves (invariant holds) but token_b swapped for a fake.
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &[0xEE; 32], 10_000, 10_000, 5_000, POOL_FEE_BPS);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_rejects_fee_change() {
        // Reserves fine, but the new datum tries to zero out the fee.
        let new_datum = pool_datum_bytes(&TOKEN_A, &TOKEN_B, 10_000, 10_000, 5_000, 0);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_rejects_script_change() {
        // Valid datum transition, but the pool funds get re-locked under a
        // different (attacker) script → script-continuity covenant fails.
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, 11_000, 9_094, 5_000, POOL_FEE_BPS);
        let (script_bytes, mut tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        tx.outputs[0].locking_script = CoreScript::new(vec![OpCode::Op1.to_byte()]);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert!(matches!(err, ScriptError::CovenantFailed(_)));
    }

    #[test]
    fn amm_pool_lock_rejects_malformed_datum_length() {
        // A truncated 89-byte new datum must fail the shape check.
        let mut short = old_pool_datum();
        short.pop();
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), short);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_rejects_missing_output_datum() {
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, 11_000, 9_094, 5_000, POOL_FEE_BPS);
        let (script_bytes, mut tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        tx.outputs[0].datum = None; // datum stripped → LEN 0 ≠ 90
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_rejects_huge_reserve_overflow_trick() {
        // u64 reserves near MAX: the u128 product math must not wrap into
        // a small value that sneaks past GE. old = (u64::MAX, u64::MAX);
        // attacker tries (1, 1) — must fail.
        let old =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, u64::MAX, u64::MAX, 5_000, POOL_FEE_BPS);
        let new = pool_datum_bytes(&TOKEN_A, &TOKEN_B, 1, 1, 5_000, POOL_FEE_BPS);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old, new);
        let err = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn amm_pool_lock_runs_within_default_gas_limit() {
        let new_datum =
            pool_datum_bytes(&TOKEN_A, &TOKEN_B, 11_000, 9_094, 5_000, POOL_FEE_BPS);
        let (script_bytes, tx, resolved) = pool_swap_fixture(old_pool_datum(), new_datum);
        let result = validate_script(
            &CoreScript::new(script_bytes),
            &[],
            &tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap();
        assert!(result.success);
        assert!(
            result.gas_used < crate::gas::DEFAULT_GAS_LIMIT / 10,
            "pool covenant should be cheap; used {} gas",
            result.gas_used
        );
    }

    // ------------------------------------------------------------------
    // lending_pool_lock (Faz 6 / D-6, ADR-013)
    // ------------------------------------------------------------------

    use qv_crypto::{pqc_sign, DilithiumLevel};

    const LENDING_POOL_ID: [u8; 32] = [0x1D; 32];
    const LENDING_COLL_TOKEN: [u8; 32] = [0xC0; 32];
    const LENDING_DEBT_TOKEN: [u8; 32] = [0xDB; 32];
    const ORACLE_MAX_AGE: u64 = 10;

    fn lending_params() -> LendingPoolScriptParams {
        LendingPoolScriptParams {
            base_rate_bps: 100,
            slope_bps: 5_000,
            ltv_max_bps: 7_500,
            liquidation_threshold_bps: 8_000,
            liquidation_bonus_bps: 1_000,
        }
    }

    /// Canonical 146-byte lending datum (mirrors
    /// `qv_defi::LendingPoolDatum::to_canonical_bytes`).
    fn lending_datum_bytes_full(
        collateral: u64,
        debt: u64,
        params: &LendingPoolScriptParams,
        multiplier_q64: u128,
        last_accrual_slot: u64,
    ) -> Vec<u8> {
        let mut d = Vec::with_capacity(LENDING_DATUM_LEN);
        d.extend_from_slice(&LENDING_POOL_ID);
        d.extend_from_slice(&LENDING_COLL_TOKEN);
        d.extend_from_slice(&LENDING_DEBT_TOKEN);
        d.extend_from_slice(&collateral.to_le_bytes());
        d.extend_from_slice(&debt.to_le_bytes());
        d.extend_from_slice(&params.to_le_bytes());
        d.extend_from_slice(&multiplier_q64.to_le_bytes());
        d.extend_from_slice(&last_accrual_slot.to_le_bytes());
        assert_eq!(d.len(), LENDING_DATUM_LEN);
        d
    }

    fn lending_datum_bytes(collateral: u64, debt: u64) -> Vec<u8> {
        lending_datum_bytes_full(collateral, debt, &lending_params(), 1u128 << 64, 0)
    }

    fn lending_script(oracle_pk_hash: &[u8; 32]) -> Vec<u8> {
        lending_pool_lock(
            &LENDING_POOL_ID,
            &LENDING_COLL_TOKEN,
            &LENDING_DEBT_TOKEN,
            &lending_params(),
            oracle_pk_hash,
            ORACLE_MAX_AGE,
        )
        .unwrap()
    }

    /// Pool spending fixture: input #0 consumes the pool UTXO (old datum),
    /// output #0 recreates it (new datum). Both carry `pool_value` = 1000
    /// unless overridden.
    fn lending_fixture(
        script_bytes: &[u8],
        old_datum: Vec<u8>,
        new_datum: Vec<u8>,
    ) -> (Transaction, Vec<TxOutput>) {
        let tx = Transaction::new(
            vec![TxInput::new(OutPoint::new(TxId::from_bytes([2; 32]), 0))],
            vec![
                TxOutput::new(Amount::from(1_000), CoreScript::new(script_bytes.to_vec()))
                    .with_datum(Datum::new(new_datum)),
            ],
        );
        let resolved = vec![
            TxOutput::new(Amount::from(1_000), CoreScript::new(script_bytes.to_vec()))
                .with_datum(Datum::new(old_datum)),
        ];
        (tx, resolved)
    }

    /// Witness for the deposit/repay path: just the `0` selector.
    fn lending_path0_witness() -> Vec<u8> {
        ScriptBuilder::new().push_int(0).build()
    }

    /// `TAG ‖ pool_id ‖ price ‖ slot` — must match the script's rebuild.
    fn oracle_msg(pool_id: &[u8; 32], price_scaled: u64, slot: u64) -> Vec<u8> {
        let mut m = LENDING_ORACLE_DOMAIN_TAG.to_vec();
        m.extend_from_slice(pool_id);
        m.extend_from_slice(&price_scaled.to_le_bytes());
        m.extend_from_slice(&slot.to_le_bytes());
        m
    }

    /// Witness for the borrow/withdraw path: a real ML-DSA signature over
    /// the oracle message, plus the price/slot bytes and the `1` selector.
    fn lending_path1_witness(
        kp: &pqc_sign::PqcKeyPair,
        pool_id: &[u8; 32],
        price_scaled: u64,
        slot: u64,
    ) -> Vec<u8> {
        let msg = oracle_msg(pool_id, price_scaled, slot);
        let sig = pqc_sign::sign(&kp.secret, &msg).unwrap();
        ScriptBuilder::new()
            .push_bytes(sig.as_bytes())
            .push_bytes(kp.public.as_bytes())
            .push_bytes(&price_scaled.to_le_bytes())
            .push_bytes(&slot.to_le_bytes())
            .push_int(1)
            .build()
    }

    fn oracle_keypair() -> pqc_sign::PqcKeyPair {
        pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap()
    }

    fn oracle_pk_hash_of(kp: &pqc_sign::PqcKeyPair) -> [u8; 32] {
        sha3_256(kp.public.as_bytes())
    }

    #[test]
    fn lending_datum_layout_constants_are_contiguous() {
        assert_eq!(LENDING_DATUM_POOL_ID_OFFSET, 0);
        assert_eq!(LENDING_DATUM_COLLATERAL_TOKEN_OFFSET, 32);
        assert_eq!(LENDING_DATUM_DEBT_TOKEN_OFFSET, 64);
        assert_eq!(LENDING_DATUM_TOTAL_COLLATERAL_OFFSET, 96);
        assert_eq!(LENDING_DATUM_TOTAL_DEBT_OFFSET, 104);
        assert_eq!(LENDING_DATUM_BASE_RATE_OFFSET, 112);
        assert_eq!(LENDING_DATUM_SLOPE_OFFSET, 114);
        assert_eq!(LENDING_DATUM_LTV_MAX_OFFSET, 116);
        assert_eq!(LENDING_DATUM_LIQ_THRESHOLD_OFFSET, 118);
        assert_eq!(LENDING_DATUM_LIQ_BONUS_OFFSET, 120);
        assert_eq!(LENDING_DATUM_INTEREST_MULTIPLIER_OFFSET, 122);
        assert_eq!(LENDING_DATUM_LAST_ACCRUAL_SLOT_OFFSET, 138);
        assert_eq!(
            LENDING_DATUM_LEN,
            LENDING_DATUM_LAST_ACCRUAL_SLOT_OFFSET + 8
        );
        assert_eq!(
            LENDING_DATUM_PARAMS_OFFSET + LENDING_DATUM_PARAMS_LEN,
            LENDING_DATUM_INTEREST_BLOCK_OFFSET
        );
        assert_eq!(
            LENDING_DATUM_INTEREST_BLOCK_OFFSET + LENDING_DATUM_INTEREST_BLOCK_LEN,
            LENDING_DATUM_LEN
        );
    }

    #[test]
    fn lending_ltv_factor_values() {
        // ceil(10^10 / 7500) = 1_333_334 (7500·1_333_333 < 10^10).
        assert_eq!(lending_ltv_factor(7_500).unwrap(), 1_333_334);
        // ltv = 100% → exactly PRICE_SCALE · 1.
        assert_eq!(lending_ltv_factor(10_000).unwrap(), 1_000_000);
        assert_eq!(
            lending_ltv_factor(0),
            Err(TemplateError::InvalidLtv(0))
        );
        assert_eq!(
            lending_ltv_factor(10_001),
            Err(TemplateError::InvalidLtv(10_001))
        );
    }

    #[test]
    fn lending_pool_lock_accepts_deposit_path0() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Deposit: collateral 1M → 1.2M, debt unchanged.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(1_200_000, 500_000),
        );
        let result = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap();
        assert!(result.success, "deposit must pass the price-less path");
    }

    #[test]
    fn lending_pool_lock_accepts_repay_path0() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Repay: debt 500k → 200k, collateral unchanged.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(1_000_000, 200_000),
        );
        let result = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap();
        assert!(result.success, "repay must pass the price-less path");
    }

    #[test]
    fn lending_pool_lock_path0_rejects_collateral_decrease() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Withdraw disguised as path 0 — must be rejected.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(900_000, 0),
        );
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_path0_rejects_debt_increase() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Borrow disguised as path 0 — must be rejected.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 100_000),
        );
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_accepts_borrow_with_signed_price() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Borrow 500k against 1M collateral at price 1.0 (scaled 10^6):
        // lhs = 500_000·1_333_334 = 6.667e11 ≤ rhs = 1_000_000·10^6 = 1e12.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 95);
        let result = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100), // age = 5 ≤ 10
        )
        .unwrap();
        assert!(result.success, "collateralized borrow must validate");
    }

    #[test]
    fn lending_pool_lock_rejects_undercollateralized_borrow() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Borrow 800k: lhs = 800_000·1_333_334 ≈ 1.067e12 > rhs = 1e12.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 800_000),
        );
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 95);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_accepts_withdraw_with_signed_price() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Withdraw 200k: collateral 1M → 800k with debt 500k:
        // lhs = 6.667e11 ≤ rhs = 800_000·10^6 = 8e11.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(800_000, 500_000),
        );
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 95);
        let result = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap();
        assert!(result.success, "safe withdraw must validate");
    }

    #[test]
    fn lending_pool_lock_rejects_unsafe_withdraw() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Withdraw to 600k with debt 500k: rhs = 6e11 < lhs = 6.667e11.
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(600_000, 500_000),
        );
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 95);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_stale_price() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        // Price signed at slot 50, validated at slot 100: age 50 > 10.
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 50);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_future_dated_price() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        // Price slot 150 > current slot 100.
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 150);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_wrong_oracle_key() {
        let real_oracle = oracle_keypair();
        let imposter = oracle_keypair();
        // Script trusts the real oracle's pubkey hash.
        let script = lending_script(&oracle_pk_hash_of(&real_oracle));
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        // The imposter signs a perfectly valid message with its own key.
        let witness = lending_path1_witness(&imposter, &LENDING_POOL_ID, 1_000_000, 95);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_tampered_price_bytes() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        // Oracle signed price 0.5; attacker swaps in 1.0 in the witness.
        // The script rebuilds the message from the witness bytes, so the
        // signature no longer verifies.
        let signed_msg = oracle_msg(&LENDING_POOL_ID, 500_000, 95);
        let sig = pqc_sign::sign(&kp.secret, &signed_msg).unwrap();
        let witness = ScriptBuilder::new()
            .push_bytes(sig.as_bytes())
            .push_bytes(kp.public.as_bytes())
            .push_bytes(&1_000_000_u64.to_le_bytes()) // tampered price
            .push_bytes(&95_u64.to_le_bytes())
            .push_int(1)
            .build();
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_cross_pool_price_replay() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        // Signature is valid — but for a DIFFERENT pool id. The script's
        // baked-in `TAG ‖ pool_id` prefix makes the rebuilt message differ.
        let witness = lending_path1_witness(&kp, &[0xEE; 32], 1_000_000, 95);
        let err = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_param_tamper() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // New datum claims ltv_max = 99.99% — params region is pinned.
        let mut loose = lending_params();
        loose.ltv_max_bps = 9_999;
        let new_datum = lending_datum_bytes_full(1_000_000, 500_000, &loose, 1u128 << 64, 0);
        let (tx, resolved) =
            lending_fixture(&script, lending_datum_bytes(1_000_000, 500_000), new_datum);
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_interest_block_tamper() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // Attacker inflates the interest multiplier — frozen region.
        let new_datum = lending_datum_bytes_full(
            1_200_000,
            500_000,
            &lending_params(),
            u128::MAX, // inflated multiplier
            0,
        );
        let (tx, resolved) =
            lending_fixture(&script, lending_datum_bytes(1_000_000, 500_000), new_datum);
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_script_change() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (mut tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(1_200_000, 500_000),
        );
        // Valid datum transition, but funds re-locked under attacker script.
        tx.outputs[0].locking_script = CoreScript::new(vec![OpCode::Op1.to_byte()]);
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert!(matches!(err, ScriptError::CovenantFailed(_)));
    }

    #[test]
    fn lending_pool_lock_rejects_pool_value_decrease() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let (mut tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 500_000),
            lending_datum_bytes(1_200_000, 500_000),
        );
        // Datum transition fine, but the successor pool UTXO's native
        // value is drained (1000 → 1).
        tx.outputs[0].value = Amount::from(1);
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_rejects_malformed_datum_length() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        let mut short = lending_datum_bytes(1_000_000, 500_000);
        short.pop(); // 145 bytes
        let (tx, resolved) =
            lending_fixture(&script, lending_datum_bytes(1_000_000, 500_000), short);
        let err = validate_script(
            &CoreScript::new(script),
            &lending_path0_witness(),
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_pool_lock_within_gas_and_size_limits() {
        let kp = oracle_keypair();
        let script = lending_script(&oracle_pk_hash_of(&kp));
        // ADR-013 §6: script ≈ 1.1 KB, far below MAX_SCRIPT_SIZE (16 KB);
        // witness + script must also decode within the same limit.
        assert!(
            script.len() < 2_048,
            "lending covenant unexpectedly large: {} bytes",
            script.len()
        );
        let witness = lending_path1_witness(&kp, &LENDING_POOL_ID, 1_000_000, 95);
        assert!(
            script.len() + witness.len() < OpCode::MAX_SCRIPT_SIZE,
            "witness + script must fit the decode limit; got {}",
            script.len() + witness.len()
        );

        let (tx, resolved) = lending_fixture(
            &script,
            lending_datum_bytes(1_000_000, 0),
            lending_datum_bytes(1_000_000, 500_000),
        );
        let result = validate_script(
            &CoreScript::new(script),
            &witness,
            &tx,
            &resolved,
            Slot::from(100),
        )
        .unwrap();
        assert!(result.success);
        assert!(
            result.gas_used < crate::gas::DEFAULT_GAS_LIMIT / 10,
            "lending covenant should be cheap; used {} gas",
            result.gas_used
        );
    }

    #[test]
    fn lending_pool_lock_rejects_invalid_build_params() {
        let mut params = lending_params();
        params.ltv_max_bps = 0;
        assert_eq!(
            lending_pool_lock(
                &LENDING_POOL_ID,
                &LENDING_COLL_TOKEN,
                &LENDING_DEBT_TOKEN,
                &params,
                &[0u8; 32],
                ORACLE_MAX_AGE,
            ),
            Err(TemplateError::InvalidLtv(0))
        );
        assert_eq!(
            lending_pool_lock(
                &LENDING_POOL_ID,
                &LENDING_COLL_TOKEN,
                &LENDING_DEBT_TOKEN,
                &lending_params(),
                &[0u8; 32],
                u64::MAX,
            ),
            Err(TemplateError::MaxAgeTooLarge(u64::MAX))
        );
    }
}
