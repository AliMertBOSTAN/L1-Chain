//! Off-chain transaction builders for DeFi flows (Faz 6 / D-3, D-5).
//!
//! These helpers run **wallet-side**: they take a snapshot of on-chain
//! state (the pool UTXO) plus user inputs and assemble an *unsigned*
//! [`Transaction`] that satisfies the on-chain covenants. Signing is the
//! wallet's job — each bundle reports which inputs still need a witness.
//!
//! # Swap transaction shape (matches `qv_script::templates::amm_pool_lock`)
//!
//! | Slot      | Index | Content                                          |
//! |-----------|-------|--------------------------------------------------|
//! | input  #0 | [`POOL_INPUT_INDEX`]  | pool UTXO (covenant; **no witness needed**) |
//! | input  #1 | [`USER_INPUT_INDEX`]  | user UTXO (must be signed by the wallet)    |
//! | output #0 | [`POOL_OUTPUT_INDEX`] | new pool UTXO: same script, new canonical datum |
//! | output #1 | [`USER_OUTPUT_INDEX`] | user change/proceeds output                 |
//!
//! The pool input carries an empty witness because `amm_pool_lock`
//! validates the state transition purely via introspection opcodes.
//! Token settlement (movement of the swapped asset into dedicated user
//! token-UTXOs) is tracked in the pool datum for now; native multi-asset
//! outputs are a later Faz 6 slice (D-6+).
//!
//! # Create-pool transaction shape ([`build_create_pool_tx`], Faz 6 / D-5)
//!
//! | Slot      | Index | Content                                          |
//! |-----------|-------|--------------------------------------------------|
//! | input  #0 | [`CREATE_POOL_USER_INPUT_INDEX`] | user funding UTXO (signed)      |
//! | output #0 | [`POOL_OUTPUT_INDEX`] | brand-new pool UTXO (`amm_pool_lock` + canonical datum) |
//! | output #1 | [`USER_OUTPUT_INDEX`] | user change output                          |
//!
//! There is no covenant input — the pool does not exist yet. The pool
//! UTXO's native `value` is funded out of the user input and, by the
//! swap flow above, is **carried unchanged** through every subsequent
//! swap (`build_swap_tx` preserves `pool_value` on output #0).
//!
//! **Honest scope note (D-5).** LP shares are accounted for *only* as the
//! `lp_total` field inside the pool datum — there is **no on-chain LP
//! token**. Add/remove-liquidity spend paths are D-6+ work; note that
//! `amm_pool_lock`'s `x·y ≥ k` check would *pass* an add-liquidity-shaped
//! transition (the product grows) but can never pass a remove-liquidity
//! one (the product shrinks), so pools created here cannot have their
//! reserves withdrawn until a dedicated spend path ships.

use thiserror::Error;

use qv_core::{Amount, Datum, Hash256, OutPoint, Script, Transaction, TxInput, TxOutput, Witness};
use qv_crypto::pqc_sign;
use qv_script::templates::{
    amm_pool_lock, lending_ltv_factor, lending_pool_lock, LendingPoolScriptParams, ScriptBuilder,
    TemplateError, LENDING_ORACLE_DOMAIN_TAG,
};

use crate::amm::{
    compute_add_liquidity, compute_swap_output, AmmError, PoolDatum, PoolState, SwapDirection,
};
use crate::lending::{LendingError, LendingPoolDatum};

// ============================================================================
// Conventions
// ============================================================================

/// The pool UTXO is always input #0 of a swap transaction.
pub const POOL_INPUT_INDEX: usize = 0;
/// The successor pool UTXO is always output #0.
pub const POOL_OUTPUT_INDEX: usize = 0;
/// The user's funding UTXO is input #1.
pub const USER_INPUT_INDEX: usize = 1;
/// The user's change/proceeds output is output #1.
pub const USER_OUTPUT_INDEX: usize = 1;
/// In a **create-pool** transaction the user's funding UTXO is input #0
/// (there is no covenant input — the pool does not exist yet).
pub const CREATE_POOL_USER_INPUT_INDEX: usize = 0;

// ============================================================================
// Errors
// ============================================================================

/// Errors from off-chain DeFi transaction building.
#[derive(Debug, Clone, Error)]
pub enum TxBuildError {
    /// `compute_swap_output` returned `None` (zero reserve, zero input,
    /// or intermediate overflow).
    #[error("swap output computation failed (zero reserve / zero input / overflow)")]
    SwapComputation,

    /// The computed output is below the caller's slippage floor.
    #[error("slippage exceeded: minimum receive {min_receive}, computed {amount_out}")]
    Slippage {
        /// Caller-requested minimum output.
        min_receive: u64,
        /// What the pool would actually pay out.
        amount_out: u64,
    },

    /// The user's funding input cannot cover the transaction fee.
    #[error("user input value {input_value} cannot cover the transaction fee {fee}")]
    InsufficientInput {
        /// Value of the user's funding UTXO.
        input_value: u64,
        /// Requested transaction fee.
        fee: u64,
    },

    /// The user's funding input cannot cover the new pool UTXO's native
    /// value plus the transaction fee (create-pool flow).
    #[error(
        "user input value {input_value} cannot cover pool value {pool_value} + fee {fee}"
    )]
    InsufficientFunding {
        /// Value of the user's funding UTXO.
        input_value: u64,
        /// Native value to lock into the new pool UTXO.
        pool_value: u64,
        /// Requested transaction fee.
        fee: u64,
    },

    /// Underlying AMM math / datum error.
    #[error(transparent)]
    Amm(#[from] AmmError),

    /// Underlying lending math / datum error.
    #[error(transparent)]
    Lending(#[from] LendingError),

    /// Script template generation error (e.g. ltv_max_bps = 0).
    #[error(transparent)]
    Template(#[from] TemplateError),

    /// Oracle price signing failed (PQC layer).
    #[error("oracle price signing failed: {0}")]
    OracleSign(String),

    /// The signed price belongs to a different pool.
    #[error("signed price is for a different pool")]
    PriceForWrongPool,

    /// The post-transition state fails the on-chain collateral check
    /// `debt · factor ≤ collateral · price_scaled` — the covenant would
    /// reject this transaction, so it is refused at build time.
    #[error(
        "collateral check failed: debt {debt} * factor {factor} > \
         collateral {collateral} * price {price_scaled}"
    )]
    CollateralShortfall {
        /// Post-transition total debt.
        debt: u64,
        /// Post-transition total collateral.
        collateral: u64,
        /// Oracle price (×`LENDING_PRICE_SCALE`).
        price_scaled: u64,
        /// LTV factor baked into the script (see `lending_ltv_factor`).
        factor: u64,
    },

    /// A zero amount was requested.
    #[error("zero amount")]
    ZeroAmount,
}

// ============================================================================
// Request / bundle types
// ============================================================================

/// Everything `build_swap_tx` needs to know, gathered by the wallet.
#[derive(Debug, Clone)]
pub struct SwapRequest {
    /// Outpoint of the live pool UTXO being consumed.
    pub pool_outpoint: OutPoint,
    /// Current (pre-swap) pool datum, decoded from the pool UTXO.
    pub pool_datum: PoolDatum,
    /// Native value carried by the pool UTXO (preserved in the successor).
    pub pool_value: Amount,
    /// Outpoint of the user's funding UTXO.
    pub user_outpoint: OutPoint,
    /// Native value of the user's funding UTXO.
    pub user_input_value: Amount,
    /// Locking script for the user's change/proceeds output.
    pub user_locking_script: Script,
    /// Which token the user is selling into the pool.
    pub direction: SwapDirection,
    /// Amount of the input token the user sells (smallest units).
    pub amount_in: u64,
    /// Slippage floor: minimum acceptable `amount_out` (0 = no floor).
    pub min_receive: u64,
    /// Transaction fee paid to the slot leader (native units).
    pub tx_fee: Amount,
}

/// The product of [`build_swap_tx`]: an unsigned transaction plus the
/// metadata the wallet needs to finish and broadcast it.
#[derive(Debug, Clone)]
pub struct SwapTxBundle {
    /// Unsigned transaction (all witnesses empty).
    pub tx: Transaction,
    /// The post-swap pool datum (already attached to output #0 in
    /// canonical encoding; provided decoded for display/bookkeeping).
    pub new_pool_datum: PoolDatum,
    /// Output amount the user receives (smallest units of the out-token).
    pub amount_out: u64,
    /// Swap fee retained by the pool (smallest units of the in-token).
    pub pool_fee_paid: u64,
    /// Indices into `tx.inputs` that the wallet must sign. The pool input
    /// is **not** listed — its covenant needs no witness.
    pub inputs_to_sign: Vec<usize>,
}

// ============================================================================
// build_swap_tx
// ============================================================================

/// Build an unsigned constant-product swap transaction against an AMM
/// pool UTXO locked by `amm_pool_lock`.
///
/// Computes `amount_out` with [`compute_swap_output`], applies the reserve
/// transition (full `amount_in` enters the pool — the fee stays in the
/// reserves, so the product grows and the on-chain `x·y ≥ k` check is
/// satisfied), regenerates the pool locking script from the datum's token
/// ids + fee, and assembles the transaction in the documented shape.
///
/// The returned transaction is unsigned: the wallet must witness the
/// inputs listed in [`SwapTxBundle::inputs_to_sign`] (the user input;
/// the pool input stays witness-less) and may then broadcast.
pub fn build_swap_tx(req: &SwapRequest) -> Result<SwapTxBundle, TxBuildError> {
    req.pool_datum.validate()?;

    let (reserve_in, reserve_out) = match req.direction {
        SwapDirection::AtoB => (req.pool_datum.reserve_a, req.pool_datum.reserve_b),
        SwapDirection::BtoA => (req.pool_datum.reserve_b, req.pool_datum.reserve_a),
    };

    let (amount_out, pool_fee_paid) =
        compute_swap_output(reserve_in, reserve_out, req.amount_in, req.pool_datum.fee_bps)
            .ok_or(TxBuildError::SwapComputation)?;

    if amount_out < req.min_receive {
        return Err(TxBuildError::Slippage {
            min_receive: req.min_receive,
            amount_out,
        });
    }

    // User change: funding input minus the network fee.
    let input_value = req.user_input_value.as_u64();
    let fee = req.tx_fee.as_u64();
    let change = input_value
        .checked_sub(fee)
        .ok_or(TxBuildError::InsufficientInput { input_value, fee })?;

    // Pool script is fully determined by (token_a, token_b, fee_bps) —
    // regenerate it so output #0 hashes identically to the input script
    // (the covenant's ASSERT_OUTPUT_SCRIPT_HASH demands this).
    let pool_script = Script::new(amm_pool_lock(
        req.pool_datum.token_a_id.as_bytes(),
        req.pool_datum.token_b_id.as_bytes(),
        req.pool_datum.fee_bps,
    ));

    // Apply the reserve transition (checked arithmetic inside).
    let pool_id = Hash256::from_bytes(pool_script.hash().to_bytes());
    let mut pool_state = PoolState::new(pool_id, req.pool_datum.clone());
    pool_state.apply_swap(req.direction, req.amount_in, amount_out)?;
    let new_pool_datum = pool_state.datum;

    let pool_output = TxOutput::new(req.pool_value, pool_script)
        .with_datum(Datum::new(new_pool_datum.to_canonical_bytes()));
    let user_output = TxOutput::new(
        Amount::from_smallest_units(change),
        req.user_locking_script.clone(),
    );

    let tx = Transaction::new(
        vec![
            TxInput::new(req.pool_outpoint), // covenant input — empty witness
            TxInput::new(req.user_outpoint), // wallet signs this one
        ],
        vec![pool_output, user_output],
    )
    .with_fee(req.tx_fee);

    Ok(SwapTxBundle {
        tx,
        new_pool_datum,
        amount_out,
        pool_fee_paid,
        inputs_to_sign: vec![USER_INPUT_INDEX],
    })
}

// ============================================================================
// build_create_pool_tx (Faz 6 / D-5 — pool bootstrap)
// ============================================================================

/// Everything [`build_create_pool_tx`] needs to know, gathered by the wallet.
#[derive(Debug, Clone)]
pub struct CreatePoolRequest {
    /// Token A identifier baked into the pool script and datum.
    pub token_a_id: Hash256,
    /// Token B identifier baked into the pool script and datum.
    pub token_b_id: Hash256,
    /// Swap fee in basis points (0..=10000) baked into script and datum.
    pub fee_bps: u16,
    /// Initial reserve of token A (smallest units, datum-level accounting).
    pub reserve_a: u64,
    /// Initial reserve of token B (smallest units, datum-level accounting).
    pub reserve_b: u64,
    /// Native value to lock into the pool UTXO. `build_swap_tx` carries
    /// this through every subsequent swap unchanged.
    pub pool_value: Amount,
    /// Outpoint of the user's funding UTXO.
    pub user_outpoint: OutPoint,
    /// Native value of the user's funding UTXO.
    pub user_input_value: Amount,
    /// Locking script for the user's change output.
    pub user_locking_script: Script,
    /// Transaction fee paid to the slot leader (native units).
    pub tx_fee: Amount,
}

/// The product of [`build_create_pool_tx`]: an unsigned transaction plus
/// the metadata the wallet needs to finish and broadcast it.
#[derive(Debug, Clone)]
pub struct CreatePoolTxBundle {
    /// Unsigned transaction (all witnesses empty).
    pub tx: Transaction,
    /// The genesis pool datum attached to output #0 (decoded copy for
    /// display/bookkeeping; the output carries the canonical encoding).
    pub pool_datum: PoolDatum,
    /// LP shares credited to the creator — **datum-level accounting
    /// only** (`lp_total` field); no on-chain LP token exists yet (D-6+).
    pub lp_total: u64,
    /// Indices into `tx.inputs` that the wallet must sign — always
    /// `[CREATE_POOL_USER_INPUT_INDEX]` for this shape.
    pub inputs_to_sign: Vec<usize>,
}

/// Build the unsigned transaction that bootstraps a brand-new AMM pool
/// UTXO locked by `amm_pool_lock` (Faz 6 / D-5).
///
/// The genesis LP total is computed through
/// [`compute_add_liquidity`]'s **empty-pool path** —
/// `lp_total = ⌊sqrt(reserve_a · reserve_b)⌋` — so the bootstrap and any
/// future add-liquidity flow can never drift in their accounting.
///
/// Value accounting (consistent with [`build_swap_tx`]):
/// `change = user_input_value − pool_value − tx_fee`. The pool UTXO's
/// native `value` is funded here once and preserved verbatim by every
/// subsequent swap.
///
/// # Honest scope (D-5)
///
/// LP shares live **only** in the datum's `lp_total` field — there is no
/// on-chain LP token, and no add/remove-liquidity spend path yet (D-6+).
/// `amm_pool_lock`'s `x·y ≥ k` invariant would let an add-liquidity-shaped
/// transition through (product grows) but blocks any remove-liquidity
/// transition (product shrinks), so reserves locked here cannot be
/// withdrawn until a dedicated spend path ships.
pub fn build_create_pool_tx(
    req: &CreatePoolRequest,
) -> Result<CreatePoolTxBundle, TxBuildError> {
    // Genesis liquidity via the canonical empty-pool path. Rejects zero
    // reserves (InsufficientLiquidity) and overflowing products.
    let (lp_issued, seeded) =
        compute_add_liquidity(0, 0, 0, req.reserve_a, req.reserve_b)?;

    let pool_datum = PoolDatum {
        token_a_id: req.token_a_id,
        token_b_id: req.token_b_id,
        reserve_a: seeded.reserve_a,
        reserve_b: seeded.reserve_b,
        lp_total: seeded.lp_total,
        fee_bps: req.fee_bps,
    };
    pool_datum.validate()?;

    // User change: funding input minus pool value minus the network fee.
    let input_value = req.user_input_value.as_u64();
    let pool_value = req.pool_value.as_u64();
    let fee = req.tx_fee.as_u64();
    let change = pool_value
        .checked_add(fee)
        .and_then(|outflow| input_value.checked_sub(outflow))
        .ok_or(TxBuildError::InsufficientFunding {
            input_value,
            pool_value,
            fee,
        })?;

    // The pool script is fully determined by (token_a, token_b, fee_bps);
    // every later swap regenerates these exact bytes from the datum.
    let pool_script = Script::new(amm_pool_lock(
        req.token_a_id.as_bytes(),
        req.token_b_id.as_bytes(),
        req.fee_bps,
    ));

    let pool_output = TxOutput::new(req.pool_value, pool_script)
        .with_datum(Datum::new(pool_datum.to_canonical_bytes()));
    let user_output = TxOutput::new(
        Amount::from_smallest_units(change),
        req.user_locking_script.clone(),
    );

    let tx = Transaction::new(
        vec![TxInput::new(req.user_outpoint)], // wallet signs this one
        vec![pool_output, user_output],
    )
    .with_fee(req.tx_fee);

    Ok(CreatePoolTxBundle {
        tx,
        pool_datum,
        lp_total: lp_issued,
        inputs_to_sign: vec![CREATE_POOL_USER_INPUT_INDEX],
    })
}

// ============================================================================
// Lending (Faz 6 / D-6, ADR-013) — oracle price + four spend-path builders
// ============================================================================

/// A price statement signed by the lending oracle operator (ADR-013 §3).
///
/// The signed message is
/// `LENDING_ORACLE_DOMAIN_TAG ‖ pool_id ‖ price_scaled(u64 LE) ‖ slot(u64 LE)`
/// — see [`oracle_price_message`]. The `lending_pool_lock` covenant
/// rebuilds this message from witness data and verifies the ML-DSA
/// signature against the oracle pubkey hash baked into the script.
#[derive(Debug, Clone)]
pub struct OracleSignedPrice {
    /// Pool this price is bound to (cross-pool replay is impossible —
    /// the pool id is part of the signed message AND of the script).
    pub pool_id: Hash256,
    /// Price in fixed-point: debt smallest-units per collateral
    /// smallest-unit, multiplied by `LENDING_PRICE_SCALE` (10^6).
    pub price_scaled: u64,
    /// Slot at which the oracle observed this price. The covenant
    /// enforces `slot ≤ current_slot ≤ slot + max_price_age_slots`.
    pub slot: u64,
    /// Raw oracle public key bytes (carried in the witness; the script
    /// only embeds its SHA3-256 hash).
    pub oracle_pubkey: Vec<u8>,
    /// Raw ML-DSA signature bytes over [`oracle_price_message`].
    pub signature: Vec<u8>,
}

/// Compose the exact byte sequence the lending oracle signs and the
/// `lending_pool_lock` script rebuilds:
/// `LENDING_ORACLE_DOMAIN_TAG ‖ pool_id ‖ price_scaled(LE) ‖ slot(LE)`.
#[must_use]
pub fn oracle_price_message(pool_id: &Hash256, price_scaled: u64, slot: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(LENDING_ORACLE_DOMAIN_TAG.len().wrapping_add(48));
    msg.extend_from_slice(LENDING_ORACLE_DOMAIN_TAG);
    msg.extend_from_slice(pool_id.as_bytes());
    msg.extend_from_slice(&price_scaled.to_le_bytes());
    msg.extend_from_slice(&slot.to_le_bytes());
    msg
}

/// **Oracle-operator side**: produce a signed price statement with a real
/// ML-DSA signature (`qv_crypto::pqc_sign`). The wallet passes the result
/// to [`build_lending_borrow_tx`] / [`build_lending_withdraw_tx`].
pub fn sign_oracle_price(
    secret: &pqc_sign::PqcSecretKey,
    public: &pqc_sign::PqcPublicKey,
    pool_id: Hash256,
    price_scaled: u64,
    slot: u64,
) -> Result<OracleSignedPrice, TxBuildError> {
    let msg = oracle_price_message(&pool_id, price_scaled, slot);
    let sig = pqc_sign::sign(secret, &msg).map_err(|e| TxBuildError::OracleSign(e.to_string()))?;
    Ok(OracleSignedPrice {
        pool_id,
        price_scaled,
        slot,
        oracle_pubkey: public.as_bytes().to_vec(),
        signature: sig.as_bytes().to_vec(),
    })
}

/// Everything the four lending builders need to know about the pool and
/// the user's funding input, gathered by the wallet.
///
/// Transaction shape (identical to the swap convention):
///
/// | Slot      | Index | Content                                            |
/// |-----------|-------|----------------------------------------------------|
/// | input  #0 | [`POOL_INPUT_INDEX`]  | lending pool UTXO (covenant; witness = spend-path data) |
/// | input  #1 | [`USER_INPUT_INDEX`]  | user UTXO (must be signed by the wallet)   |
/// | output #0 | [`POOL_OUTPUT_INDEX`] | new pool UTXO: same script, new canonical datum |
/// | output #1 | [`USER_OUTPUT_INDEX`] | user change output                         |
///
/// Unlike the AMM pool input, the lending pool input **does** carry a
/// witness (the spend-path selector, and for borrow/withdraw the signed
/// price). The sighash excludes witnesses (ADR-012), so attaching it
/// never invalidates the user's signature.
#[derive(Debug, Clone)]
pub struct LendingRequestCore {
    /// Outpoint of the live lending pool UTXO being consumed.
    pub pool_outpoint: OutPoint,
    /// Current (pre-transition) pool datum, decoded from the pool UTXO.
    pub pool_datum: LendingPoolDatum,
    /// Native value carried by the pool UTXO (preserved in the successor;
    /// the covenant rejects any decrease).
    pub pool_value: Amount,
    /// SHA3-256 of the oracle public key baked into the pool script.
    pub oracle_pk_hash: [u8; 32],
    /// Freshness window (slots) baked into the pool script.
    pub max_price_age_slots: u64,
    /// Outpoint of the user's funding UTXO.
    pub user_outpoint: OutPoint,
    /// Native value of the user's funding UTXO.
    pub user_input_value: Amount,
    /// Locking script for the user's change output.
    pub user_locking_script: Script,
    /// Transaction fee paid to the slot leader (native units).
    pub tx_fee: Amount,
}

/// The product of a lending builder: an unsigned transaction (pool
/// witness already attached) plus the metadata the wallet needs to
/// finish and broadcast it.
#[derive(Debug, Clone)]
pub struct LendingTxBundle {
    /// Transaction with the pool input's covenant witness attached; the
    /// user input is still unsigned.
    pub tx: Transaction,
    /// The post-transition pool datum (attached to output #0 in canonical
    /// encoding; provided decoded for display/bookkeeping).
    pub new_pool_datum: LendingPoolDatum,
    /// Indices into `tx.inputs` that the wallet must sign — always
    /// `[USER_INPUT_INDEX]` for this shape.
    pub inputs_to_sign: Vec<usize>,
}

/// Regenerate the canonical `lending_pool_lock` script bytes from the
/// pool datum's pinned fields plus the oracle parameters. Every builder
/// uses this so output #0 hashes identically to the input script (the
/// covenant's script-continuity check demands it).
fn lending_pool_script(
    datum: &LendingPoolDatum,
    oracle_pk_hash: &[u8; 32],
    max_price_age_slots: u64,
) -> Result<Vec<u8>, TxBuildError> {
    let params = LendingPoolScriptParams {
        base_rate_bps: datum.base_rate_bps,
        slope_bps: datum.slope_bps,
        ltv_max_bps: datum.ltv_max_bps,
        liquidation_threshold_bps: datum.liquidation_threshold_bps,
        liquidation_bonus_bps: datum.liquidation_bonus_bps,
    };
    Ok(lending_pool_lock(
        datum.pool_id.as_bytes(),
        datum.collateral_token_id.as_bytes(),
        datum.debt_token_id.as_bytes(),
        &params,
        oracle_pk_hash,
        max_price_age_slots,
    )?)
}

/// Witness for the deposit/repay path: just the `0` branch selector.
fn lending_path0_witness() -> Vec<u8> {
    ScriptBuilder::new().push_int(0).build()
}

/// Witness for the borrow/withdraw path (bottom→top):
/// `<sig> <pubkey> <price_scaled LE> <slot LE> <1>`.
fn lending_path1_witness(price: &OracleSignedPrice) -> Vec<u8> {
    ScriptBuilder::new()
        .push_bytes(&price.signature)
        .push_bytes(&price.oracle_pubkey)
        .push_bytes(&price.price_scaled.to_le_bytes())
        .push_bytes(&price.slot.to_le_bytes())
        .push_int(1)
        .build()
}

/// Wallet-side mirror of the on-chain collateral check (ADR-013 §2):
/// `debt · K ≤ collateral · price_scaled`. Fails early so an
/// under-collateralized transaction is never even assembled.
fn check_lending_collateral(
    debt: u64,
    collateral: u64,
    price_scaled: u64,
    ltv_max_bps: u16,
) -> Result<(), TxBuildError> {
    let factor = lending_ltv_factor(ltv_max_bps)?;
    let lhs = u128::from(debt).wrapping_mul(u128::from(factor)); // < 2^98
    let rhs = u128::from(collateral).wrapping_mul(u128::from(price_scaled)); // < 2^128
    if lhs > rhs {
        return Err(TxBuildError::CollateralShortfall {
            debt,
            collateral,
            price_scaled,
            factor,
        });
    }
    Ok(())
}

/// Assemble the common lending transaction shape around a validated
/// datum transition and a ready-made pool witness.
fn assemble_lending_tx(
    req: &LendingRequestCore,
    new_pool_datum: LendingPoolDatum,
    pool_witness: Vec<u8>,
) -> Result<LendingTxBundle, TxBuildError> {
    new_pool_datum.validate()?;

    // User change: funding input minus the network fee.
    let input_value = req.user_input_value.as_u64();
    let fee = req.tx_fee.as_u64();
    let change = input_value
        .checked_sub(fee)
        .ok_or(TxBuildError::InsufficientInput { input_value, fee })?;

    let script_bytes =
        lending_pool_script(&req.pool_datum, &req.oracle_pk_hash, req.max_price_age_slots)?;

    let pool_output = TxOutput::new(req.pool_value, Script::new(script_bytes))
        .with_datum(Datum::new(new_pool_datum.to_canonical_bytes()));
    let user_output = TxOutput::new(
        Amount::from_smallest_units(change),
        req.user_locking_script.clone(),
    );

    let tx = Transaction::new(
        vec![
            // Covenant input: witness selects the spend path (and carries
            // the signed price on the borrow/withdraw path).
            TxInput::new(req.pool_outpoint).with_witness(Witness::new(pool_witness)),
            TxInput::new(req.user_outpoint), // wallet signs this one
        ],
        vec![pool_output, user_output],
    )
    .with_fee(req.tx_fee);

    Ok(LendingTxBundle {
        tx,
        new_pool_datum,
        inputs_to_sign: vec![USER_INPUT_INDEX],
    })
}

/// Build an unsigned **deposit** transaction: `total_collateral` grows by
/// `deposit_amount`, `total_debt` is unchanged. Price-less spend path.
///
/// **Honest scope (D-6, same as AMM D-3/D-5):** collateral movement is
/// datum-level accounting; native multi-asset settlement is a later
/// Faz 6 slice. The pool UTXO's native value is carried unchanged.
pub fn build_lending_deposit_tx(
    req: &LendingRequestCore,
    deposit_amount: u64,
) -> Result<LendingTxBundle, TxBuildError> {
    if deposit_amount == 0 {
        return Err(TxBuildError::ZeroAmount);
    }
    let mut new_datum = req.pool_datum.clone();
    new_datum.total_collateral = new_datum
        .total_collateral
        .checked_add(deposit_amount)
        .ok_or(TxBuildError::Lending(LendingError::Overflow))?;
    assemble_lending_tx(req, new_datum, lending_path0_witness())
}

/// Build an unsigned **repay** transaction: `total_debt` shrinks by
/// `min(repay_amount, total_debt)` (over-repaying is capped, mirroring
/// `lending::repay`), `total_collateral` is unchanged. Price-less path.
pub fn build_lending_repay_tx(
    req: &LendingRequestCore,
    repay_amount: u64,
) -> Result<LendingTxBundle, TxBuildError> {
    if repay_amount == 0 {
        return Err(TxBuildError::ZeroAmount);
    }
    if req.pool_datum.total_debt == 0 {
        return Err(TxBuildError::Lending(LendingError::NoDebt));
    }
    let repaid = core::cmp::min(repay_amount, req.pool_datum.total_debt);
    let mut new_datum = req.pool_datum.clone();
    new_datum.total_debt = new_datum
        .total_debt
        .checked_sub(repaid)
        .ok_or(TxBuildError::Lending(LendingError::Underflow))?;
    assemble_lending_tx(req, new_datum, lending_path0_witness())
}

/// Build an unsigned **borrow** transaction: `total_debt` grows by
/// `borrow_amount`. Requires a fresh [`OracleSignedPrice`]; the on-chain
/// collateral check is mirrored here so an under-collateralized borrow
/// fails at build time instead of at consensus.
pub fn build_lending_borrow_tx(
    req: &LendingRequestCore,
    borrow_amount: u64,
    price: &OracleSignedPrice,
) -> Result<LendingTxBundle, TxBuildError> {
    if borrow_amount == 0 {
        return Err(TxBuildError::ZeroAmount);
    }
    if price.pool_id != req.pool_datum.pool_id {
        return Err(TxBuildError::PriceForWrongPool);
    }
    let mut new_datum = req.pool_datum.clone();
    new_datum.total_debt = new_datum
        .total_debt
        .checked_add(borrow_amount)
        .ok_or(TxBuildError::Lending(LendingError::Overflow))?;
    check_lending_collateral(
        new_datum.total_debt,
        new_datum.total_collateral,
        price.price_scaled,
        new_datum.ltv_max_bps,
    )?;
    assemble_lending_tx(req, new_datum, lending_path1_witness(price))
}

/// Build an unsigned **withdraw** transaction: `total_collateral` shrinks
/// by `withdraw_amount`. Requires a fresh [`OracleSignedPrice`]; the
/// post-withdraw state must still satisfy the collateral check.
pub fn build_lending_withdraw_tx(
    req: &LendingRequestCore,
    withdraw_amount: u64,
    price: &OracleSignedPrice,
) -> Result<LendingTxBundle, TxBuildError> {
    if withdraw_amount == 0 {
        return Err(TxBuildError::ZeroAmount);
    }
    if price.pool_id != req.pool_datum.pool_id {
        return Err(TxBuildError::PriceForWrongPool);
    }
    let mut new_datum = req.pool_datum.clone();
    new_datum.total_collateral = new_datum
        .total_collateral
        .checked_sub(withdraw_amount)
        .ok_or(TxBuildError::Lending(LendingError::Underflow))?;
    check_lending_collateral(
        new_datum.total_debt,
        new_datum.total_collateral,
        price.price_scaled,
        new_datum.ltv_max_bps,
    )?;
    assemble_lending_tx(req, new_datum, lending_path1_witness(price))
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
    use qv_core::{Slot, TxId};
    use qv_script::{validate_script, ScriptError};

    fn token_a() -> Hash256 {
        Hash256::from_bytes([0xA1; 32])
    }

    fn token_b() -> Hash256 {
        Hash256::from_bytes([0xB2; 32])
    }

    fn pool_datum() -> PoolDatum {
        PoolDatum {
            token_a_id: token_a(),
            token_b_id: token_b(),
            reserve_a: 10_000,
            reserve_b: 10_000,
            lp_total: 5_000,
            fee_bps: 30,
        }
    }

    fn swap_request() -> SwapRequest {
        SwapRequest {
            pool_outpoint: OutPoint::new(TxId::from_bytes([9; 32]), 0),
            pool_datum: pool_datum(),
            pool_value: Amount::from(1_000),
            user_outpoint: OutPoint::new(TxId::from_bytes([8; 32]), 1),
            user_input_value: Amount::from(500),
            user_locking_script: Script::new(vec![0x01]), // OP_1
            direction: SwapDirection::AtoB,
            amount_in: 1_000,
            min_receive: 900,
            tx_fee: Amount::from(10),
        }
    }

    /// Resolved prevouts for the built tx: pool UTXO (script + canonical
    /// datum) at index 0, user UTXO at index 1.
    fn resolved_inputs(req: &SwapRequest) -> Vec<TxOutput> {
        let pool_script = Script::new(amm_pool_lock(
            req.pool_datum.token_a_id.as_bytes(),
            req.pool_datum.token_b_id.as_bytes(),
            req.pool_datum.fee_bps,
        ));
        vec![
            TxOutput::new(req.pool_value, pool_script)
                .with_datum(Datum::new(req.pool_datum.to_canonical_bytes())),
            TxOutput::new(req.user_input_value, req.user_locking_script.clone()),
        ]
    }

    #[test]
    fn build_swap_tx_basic_amounts() {
        let req = swap_request();
        let bundle = build_swap_tx(&req).unwrap();

        // fee = 1000*30/10000 = 3; net = 997; out = 10000*997/10997 = 906.
        assert_eq!(bundle.pool_fee_paid, 3);
        assert_eq!(bundle.amount_out, 906);
        assert_eq!(bundle.new_pool_datum.reserve_a, 11_000);
        assert_eq!(bundle.new_pool_datum.reserve_b, 9_094);
        assert_eq!(bundle.new_pool_datum.lp_total, 5_000);
        assert_eq!(bundle.new_pool_datum.fee_bps, 30);

        // Invariant must grow (fee retained in pool).
        assert!(bundle.new_pool_datum.invariant() >= req.pool_datum.invariant());

        // Transaction shape.
        assert_eq!(bundle.tx.inputs.len(), 2);
        assert_eq!(bundle.tx.outputs.len(), 2);
        assert_eq!(bundle.tx.inputs[POOL_INPUT_INDEX].prev_output, req.pool_outpoint);
        assert_eq!(bundle.tx.inputs[USER_INPUT_INDEX].prev_output, req.user_outpoint);
        assert!(bundle.tx.inputs[POOL_INPUT_INDEX].witness.is_empty());
        assert!(bundle.tx.inputs[USER_INPUT_INDEX].witness.is_empty()); // unsigned
        assert_eq!(bundle.tx.outputs[POOL_OUTPUT_INDEX].value, req.pool_value);
        assert_eq!(
            bundle.tx.outputs[USER_OUTPUT_INDEX].value,
            Amount::from(490) // 500 input - 10 fee
        );
        assert_eq!(bundle.tx.fee, req.tx_fee);
        assert_eq!(bundle.inputs_to_sign, vec![USER_INPUT_INDEX]);

        // Output datum is the canonical encoding of the new datum.
        let datum_bytes = bundle.tx.outputs[POOL_OUTPUT_INDEX]
            .datum
            .as_ref()
            .unwrap()
            .as_bytes()
            .to_vec();
        assert_eq!(
            PoolDatum::from_canonical_bytes(&datum_bytes).unwrap(),
            bundle.new_pool_datum
        );
        // Structural sanity.
        bundle.tx.validate_structure().unwrap();
    }

    #[test]
    fn build_swap_tx_btoa_direction() {
        let mut req = swap_request();
        req.direction = SwapDirection::BtoA;
        req.min_receive = 0;
        let bundle = build_swap_tx(&req).unwrap();
        // Symmetric reserves → same numbers, mirrored sides.
        assert_eq!(bundle.amount_out, 906);
        assert_eq!(bundle.new_pool_datum.reserve_b, 11_000);
        assert_eq!(bundle.new_pool_datum.reserve_a, 9_094);
    }

    #[test]
    fn built_swap_tx_satisfies_pool_covenant_end_to_end() {
        // The full loop: helper-built tx → script VM validates the pool
        // input's amm_pool_lock covenant with an EMPTY witness.
        let req = swap_request();
        let bundle = build_swap_tx(&req).unwrap();
        let resolved = resolved_inputs(&req);

        let pool_script = &resolved[POOL_INPUT_INDEX].locking_script;
        // The helper must regenerate byte-identical script bytes.
        assert_eq!(
            pool_script,
            &bundle.tx.outputs[POOL_OUTPUT_INDEX].locking_script
        );

        let result = validate_script(pool_script, &[], &bundle.tx, &resolved, Slot::from(1))
            .unwrap();
        assert!(
            result.success,
            "helper-built swap tx must satisfy the on-chain pool covenant"
        );
    }

    #[test]
    fn tampered_reserves_fail_covenant() {
        // Take a valid bundle, then rewrite the new datum to steal from
        // the pool — the covenant must reject the transaction.
        let req = swap_request();
        let bundle = build_swap_tx(&req).unwrap();
        let resolved = resolved_inputs(&req);

        let mut stolen = bundle.new_pool_datum.clone();
        stolen.reserve_a = 9_000; // product shrinks below k
        stolen.reserve_b = 10_000;
        let mut tx = bundle.tx;
        tx.outputs[POOL_OUTPUT_INDEX].datum = Some(Datum::new(stolen.to_canonical_bytes()));

        let pool_script = &resolved[POOL_INPUT_INDEX].locking_script;
        let err = validate_script(pool_script, &[], &tx, &resolved, Slot::from(1)).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn redirected_pool_script_fails_covenant() {
        let req = swap_request();
        let bundle = build_swap_tx(&req).unwrap();
        let resolved = resolved_inputs(&req);

        let mut tx = bundle.tx;
        tx.outputs[POOL_OUTPUT_INDEX].locking_script = Script::new(vec![0x01]); // attacker p2-anyone
        let pool_script = &resolved[POOL_INPUT_INDEX].locking_script;
        let err = validate_script(pool_script, &[], &tx, &resolved, Slot::from(1)).unwrap_err();
        assert!(matches!(err, ScriptError::CovenantFailed(_)));
    }

    #[test]
    fn slippage_floor_rejected() {
        let mut req = swap_request();
        req.min_receive = 907; // actual out is 906
        let err = build_swap_tx(&req).unwrap_err();
        assert!(matches!(
            err,
            TxBuildError::Slippage {
                min_receive: 907,
                amount_out: 906
            }
        ));
    }

    #[test]
    fn insufficient_user_input_rejected() {
        let mut req = swap_request();
        req.user_input_value = Amount::from(5); // fee is 10
        let err = build_swap_tx(&req).unwrap_err();
        assert!(matches!(
            err,
            TxBuildError::InsufficientInput {
                input_value: 5,
                fee: 10
            }
        ));
    }

    #[test]
    fn zero_reserve_pool_rejected() {
        let mut req = swap_request();
        req.pool_datum.reserve_a = 0;
        let err = build_swap_tx(&req).unwrap_err();
        assert!(matches!(err, TxBuildError::SwapComputation));
    }

    #[test]
    fn invalid_fee_rejected_via_amm_error() {
        let mut req = swap_request();
        req.pool_datum.fee_bps = 10_001;
        let err = build_swap_tx(&req).unwrap_err();
        assert!(matches!(err, TxBuildError::Amm(AmmError::InvalidFee { .. })));
    }

    // ------------------------------------------------------------------
    // build_create_pool_tx (Faz 6 / D-5)
    // ------------------------------------------------------------------

    fn create_pool_request() -> CreatePoolRequest {
        CreatePoolRequest {
            token_a_id: token_a(),
            token_b_id: token_b(),
            fee_bps: 30,
            reserve_a: 10_000,
            reserve_b: 10_000,
            pool_value: Amount::from(1_000),
            user_outpoint: OutPoint::new(TxId::from_bytes([7; 32]), 2),
            user_input_value: Amount::from(5_000),
            user_locking_script: Script::new(vec![0x01]), // OP_1
            tx_fee: Amount::from(10),
        }
    }

    #[test]
    fn build_create_pool_tx_shape_and_amounts() {
        let req = create_pool_request();
        let bundle = build_create_pool_tx(&req).unwrap();

        // lp_total = sqrt(10_000 * 10_000) = 10_000 — the same number the
        // canonical empty-pool add-liquidity path produces.
        let (expected_lp, expected_datum) =
            compute_add_liquidity(0, 0, 0, req.reserve_a, req.reserve_b).unwrap();
        assert_eq!(bundle.lp_total, 10_000);
        assert_eq!(bundle.lp_total, expected_lp);
        assert_eq!(bundle.pool_datum.lp_total, expected_datum.lp_total);
        assert_eq!(bundle.pool_datum.reserve_a, 10_000);
        assert_eq!(bundle.pool_datum.reserve_b, 10_000);
        assert_eq!(bundle.pool_datum.token_a_id, token_a());
        assert_eq!(bundle.pool_datum.token_b_id, token_b());
        assert_eq!(bundle.pool_datum.fee_bps, 30);

        // Shape: one signed input, pool output #0, change output #1.
        assert_eq!(bundle.tx.inputs.len(), 1);
        assert_eq!(bundle.tx.outputs.len(), 2);
        assert_eq!(
            bundle.tx.inputs[CREATE_POOL_USER_INPUT_INDEX].prev_output,
            req.user_outpoint
        );
        assert!(bundle.tx.inputs[CREATE_POOL_USER_INPUT_INDEX]
            .witness
            .is_empty()); // unsigned
        assert_eq!(bundle.inputs_to_sign, vec![CREATE_POOL_USER_INPUT_INDEX]);
        assert_eq!(bundle.tx.outputs[POOL_OUTPUT_INDEX].value, req.pool_value);
        // change = 5000 - 1000 (pool value) - 10 (fee) = 3990
        assert_eq!(
            bundle.tx.outputs[USER_OUTPUT_INDEX].value,
            Amount::from(3_990)
        );
        assert_eq!(bundle.tx.fee, req.tx_fee);
        bundle.tx.validate_structure().unwrap();

        // Output #0 carries the canonical 90-byte datum encoding.
        let datum_bytes = bundle.tx.outputs[POOL_OUTPUT_INDEX]
            .datum
            .as_ref()
            .unwrap()
            .as_bytes()
            .to_vec();
        assert_eq!(datum_bytes.len(), PoolDatum::CANONICAL_LEN);
        assert_eq!(
            PoolDatum::from_canonical_bytes(&datum_bytes).unwrap(),
            bundle.pool_datum
        );

        // Output #0 is locked by the canonical amm_pool_lock bytes.
        let expected_script = amm_pool_lock(
            req.token_a_id.as_bytes(),
            req.token_b_id.as_bytes(),
            req.fee_bps,
        );
        assert_eq!(
            bundle.tx.outputs[POOL_OUTPUT_INDEX]
                .locking_script
                .as_bytes(),
            expected_script.as_slice()
        );
    }

    #[test]
    fn created_pool_is_spendable_by_swap_covenant_end_to_end() {
        // Full loop: bootstrap the pool, then build a swap that spends the
        // created pool UTXO — the on-chain covenant must accept it.
        let create_req = create_pool_request();
        let create_bundle = build_create_pool_tx(&create_req).unwrap();
        let create_tx_id = create_bundle.tx.id().unwrap();

        let pool_outpoint = OutPoint::new(create_tx_id, POOL_OUTPUT_INDEX as u32);
        let swap_req = SwapRequest {
            pool_outpoint,
            pool_datum: create_bundle.pool_datum.clone(),
            pool_value: create_bundle.tx.outputs[POOL_OUTPUT_INDEX].value,
            user_outpoint: OutPoint::new(TxId::from_bytes([8; 32]), 1),
            user_input_value: Amount::from(500),
            user_locking_script: Script::new(vec![0x01]),
            direction: SwapDirection::AtoB,
            amount_in: 1_000,
            min_receive: 900,
            tx_fee: Amount::from(10),
        };
        let swap_bundle = build_swap_tx(&swap_req).unwrap();

        // Resolved prevouts: the *created* pool output, then the user UTXO.
        let resolved = vec![
            create_bundle.tx.outputs[POOL_OUTPUT_INDEX].clone(),
            TxOutput::new(swap_req.user_input_value, swap_req.user_locking_script.clone()),
        ];
        let pool_script = &create_bundle.tx.outputs[POOL_OUTPUT_INDEX].locking_script;
        let result =
            validate_script(pool_script, &[], &swap_bundle.tx, &resolved, Slot::from(1))
                .unwrap();
        assert!(
            result.success,
            "a swap against the freshly created pool must satisfy the covenant"
        );
    }

    #[test]
    fn create_pool_lp_total_matches_sqrt_for_unbalanced_reserves() {
        let mut req = create_pool_request();
        req.reserve_a = 4_000_000;
        req.reserve_b = 1_000;
        let bundle = build_create_pool_tx(&req).unwrap();
        // sqrt(4_000_000 * 1_000) = sqrt(4e9) = 63_245 (floor).
        assert_eq!(bundle.lp_total, 63_245);
        assert_eq!(bundle.pool_datum.lp_total, 63_245);
    }

    #[test]
    fn create_pool_insufficient_funding_rejected() {
        let mut req = create_pool_request();
        req.user_input_value = Amount::from(1_005); // need 1000 + 10
        let err = build_create_pool_tx(&req).unwrap_err();
        assert!(matches!(
            err,
            TxBuildError::InsufficientFunding {
                input_value: 1_005,
                pool_value: 1_000,
                fee: 10
            }
        ));
    }

    #[test]
    fn create_pool_zero_reserve_rejected() {
        for (a, b) in [(0u64, 1_000u64), (1_000, 0), (0, 0)] {
            let mut req = create_pool_request();
            req.reserve_a = a;
            req.reserve_b = b;
            let err = build_create_pool_tx(&req).unwrap_err();
            assert!(
                matches!(err, TxBuildError::Amm(AmmError::InsufficientLiquidity { .. })),
                "reserves ({a}, {b}) must be rejected"
            );
        }
    }

    #[test]
    fn create_pool_invalid_fee_bps_rejected() {
        let mut req = create_pool_request();
        req.fee_bps = 10_001;
        let err = build_create_pool_tx(&req).unwrap_err();
        assert!(matches!(
            err,
            TxBuildError::Amm(AmmError::InvalidFee { fee_bps: 10_001 })
        ));
    }

    #[test]
    fn create_pool_exact_funding_yields_zero_change() {
        let mut req = create_pool_request();
        req.user_input_value = Amount::from(1_010); // exactly pool + fee
        let bundle = build_create_pool_tx(&req).unwrap();
        assert_eq!(bundle.tx.outputs[USER_OUTPUT_INDEX].value, Amount::from(0));
        bundle.tx.validate_structure().unwrap();
    }

    // ------------------------------------------------------------------
    // Lending builders (Faz 6 / D-6, ADR-013)
    // ------------------------------------------------------------------

    use qv_core::Witness;
    use qv_crypto::{sha3_256, DilithiumLevel};

    const ORACLE_MAX_AGE: u64 = 10;
    /// Price slot baked into the test oracle statement.
    const PRICE_SLOT: u64 = 95;
    /// Slot at which the lending test transactions are validated
    /// (age = 5 ≤ ORACLE_MAX_AGE).
    const VALIDATION_SLOT: u64 = 100;
    /// Price 1.0 collateral→debt, ×10^6.
    const PRICE_ONE: u64 = 1_000_000;

    fn oracle_keypair() -> pqc_sign::PqcKeyPair {
        pqc_sign::generate_keypair(DilithiumLevel::Level3).unwrap()
    }

    fn lending_pool_datum() -> LendingPoolDatum {
        LendingPoolDatum {
            pool_id: Hash256::from_bytes([0x1D; 32]),
            collateral_token_id: Hash256::from_bytes([0xC0; 32]),
            debt_token_id: Hash256::from_bytes([0xDB; 32]),
            total_collateral: 1_000_000,
            total_debt: 100_000,
            base_rate_bps: 100,
            slope_bps: 5_000,
            ltv_max_bps: 7_500,
            liquidation_threshold_bps: 8_000,
            liquidation_bonus_bps: 1_000,
            interest_multiplier_q64: 1u128 << 64,
            last_accrual_slot: 0,
        }
    }

    fn lending_request(oracle_kp: &pqc_sign::PqcKeyPair) -> LendingRequestCore {
        LendingRequestCore {
            pool_outpoint: OutPoint::new(TxId::from_bytes([0x11; 32]), 0),
            pool_datum: lending_pool_datum(),
            pool_value: Amount::from(1_000),
            oracle_pk_hash: sha3_256(oracle_kp.public.as_bytes()),
            max_price_age_slots: ORACLE_MAX_AGE,
            user_outpoint: OutPoint::new(TxId::from_bytes([0x22; 32]), 1),
            user_input_value: Amount::from(500),
            user_locking_script: Script::new(vec![0x01]), // OP_1
            tx_fee: Amount::from(10),
        }
    }

    fn signed_price(kp: &pqc_sign::PqcKeyPair, price_scaled: u64, slot: u64) -> OracleSignedPrice {
        sign_oracle_price(
            &kp.secret,
            &kp.public,
            lending_pool_datum().pool_id,
            price_scaled,
            slot,
        )
        .unwrap()
    }

    /// Resolved prevouts for a built lending tx: pool UTXO (script +
    /// canonical old datum) at index 0, user UTXO at index 1.
    fn lending_resolved_inputs(req: &LendingRequestCore) -> Vec<TxOutput> {
        let script = lending_pool_script(
            &req.pool_datum,
            &req.oracle_pk_hash,
            req.max_price_age_slots,
        )
        .unwrap();
        vec![
            TxOutput::new(req.pool_value, Script::new(script))
                .with_datum(Datum::new(req.pool_datum.to_canonical_bytes())),
            TxOutput::new(req.user_input_value, req.user_locking_script.clone()),
        ]
    }

    /// Run the pool covenant over a built bundle exactly the way the
    /// ledger would: pool locking script + the pool input's own witness.
    fn validate_lending_bundle(
        req: &LendingRequestCore,
        bundle: &LendingTxBundle,
    ) -> Result<qv_script::ExecResult, ScriptError> {
        let resolved = lending_resolved_inputs(req);
        let pool_script = resolved[POOL_INPUT_INDEX].locking_script.clone();
        validate_script(
            &pool_script,
            bundle.tx.inputs[POOL_INPUT_INDEX].witness.as_bytes(),
            &bundle.tx,
            &resolved,
            Slot::from(VALIDATION_SLOT),
        )
    }

    #[test]
    fn build_lending_deposit_tx_shape_and_covenant_e2e() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let bundle = build_lending_deposit_tx(&req, 250_000).unwrap();

        // Datum transition.
        assert_eq!(bundle.new_pool_datum.total_collateral, 1_250_000);
        assert_eq!(bundle.new_pool_datum.total_debt, 100_000);
        // Frozen interest region untouched.
        assert_eq!(bundle.new_pool_datum.interest_multiplier_q64, 1u128 << 64);

        // Transaction shape.
        assert_eq!(bundle.tx.inputs.len(), 2);
        assert_eq!(bundle.tx.outputs.len(), 2);
        assert_eq!(
            bundle.tx.inputs[POOL_INPUT_INDEX].prev_output,
            req.pool_outpoint
        );
        assert!(!bundle.tx.inputs[POOL_INPUT_INDEX].witness.is_empty()); // path selector
        assert!(bundle.tx.inputs[USER_INPUT_INDEX].witness.is_empty()); // unsigned
        assert_eq!(bundle.inputs_to_sign, vec![USER_INPUT_INDEX]);
        assert_eq!(bundle.tx.outputs[POOL_OUTPUT_INDEX].value, req.pool_value);
        assert_eq!(
            bundle.tx.outputs[USER_OUTPUT_INDEX].value,
            Amount::from(490) // 500 − 10 fee
        );
        bundle.tx.validate_structure().unwrap();

        // Output datum is the canonical 146-byte encoding.
        let datum_bytes = bundle.tx.outputs[POOL_OUTPUT_INDEX]
            .datum
            .as_ref()
            .unwrap()
            .as_bytes()
            .to_vec();
        assert_eq!(datum_bytes.len(), LendingPoolDatum::CANONICAL_LEN);
        assert_eq!(
            LendingPoolDatum::from_canonical_bytes(&datum_bytes).unwrap(),
            bundle.new_pool_datum
        );

        // The covenant accepts the helper-built transaction.
        let result = validate_lending_bundle(&req, &bundle).unwrap();
        assert!(result.success, "deposit must satisfy the pool covenant");
    }

    #[test]
    fn build_lending_repay_tx_covenant_e2e_and_cap() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        // Over-repay 250k against 100k debt — capped at the debt.
        let bundle = build_lending_repay_tx(&req, 250_000).unwrap();
        assert_eq!(bundle.new_pool_datum.total_debt, 0);
        assert_eq!(bundle.new_pool_datum.total_collateral, 1_000_000);
        let result = validate_lending_bundle(&req, &bundle).unwrap();
        assert!(result.success, "repay must satisfy the pool covenant");
    }

    #[test]
    fn build_lending_borrow_tx_covenant_e2e() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);
        // 100k existing + 400k new = 500k ≤ 75% of 1M at price 1.0.
        let bundle = build_lending_borrow_tx(&req, 400_000, &price).unwrap();
        assert_eq!(bundle.new_pool_datum.total_debt, 500_000);
        let result = validate_lending_bundle(&req, &bundle).unwrap();
        assert!(
            result.success,
            "collateralized borrow must satisfy the pool covenant"
        );
    }

    #[test]
    fn build_lending_withdraw_tx_covenant_e2e() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);
        // Collateral 1M → 800k with debt 100k: comfortably safe.
        let bundle = build_lending_withdraw_tx(&req, 200_000, &price).unwrap();
        assert_eq!(bundle.new_pool_datum.total_collateral, 800_000);
        let result = validate_lending_bundle(&req, &bundle).unwrap();
        assert!(result.success, "safe withdraw must satisfy the covenant");
    }

    #[test]
    fn lending_borrow_undercollateralized_rejected_at_build_time() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);
        // 100k + 700k = 800k > 75% of 1M.
        let err = build_lending_borrow_tx(&req, 700_000, &price).unwrap_err();
        assert!(matches!(err, TxBuildError::CollateralShortfall { .. }));
    }

    #[test]
    fn lending_borrow_undercollateralized_rejected_by_covenant() {
        // Bypass the helper's pre-check: build a valid borrow, then
        // tamper the output datum to inflate the debt. The on-chain
        // covenant must reject it.
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);
        let bundle = build_lending_borrow_tx(&req, 400_000, &price).unwrap();

        let mut greedy = bundle.new_pool_datum.clone();
        greedy.total_debt = 900_000; // way past the LTV limit
        let mut tx = bundle.tx.clone();
        tx.outputs[POOL_OUTPUT_INDEX].datum = Some(Datum::new(greedy.to_canonical_bytes()));
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: greedy,
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_stale_price_rejected_by_covenant() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        // Signed at slot 50; validation runs at slot 100 → age 50 > 10.
        let price = signed_price(&kp, PRICE_ONE, 50);
        let bundle = build_lending_borrow_tx(&req, 400_000, &price).unwrap();
        let err = validate_lending_bundle(&req, &bundle).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_wrong_oracle_signature_rejected_by_covenant() {
        let real_oracle = oracle_keypair();
        let imposter = oracle_keypair();
        // The pool script trusts the real oracle's pubkey hash…
        let req = lending_request(&real_oracle);
        // …but the price statement is signed by the imposter.
        let price = signed_price(&imposter, PRICE_ONE, PRICE_SLOT);
        let bundle = build_lending_borrow_tx(&req, 400_000, &price).unwrap();
        let err = validate_lending_bundle(&req, &bundle).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_datum_param_tamper_rejected_by_covenant() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let bundle = build_lending_deposit_tx(&req, 250_000).unwrap();

        // Loosen the pinned ltv_max in the successor datum.
        let mut loose = bundle.new_pool_datum.clone();
        loose.ltv_max_bps = 9_999;
        let mut tx = bundle.tx.clone();
        tx.outputs[POOL_OUTPUT_INDEX].datum = Some(Datum::new(loose.to_canonical_bytes()));
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: loose,
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_interest_block_tamper_rejected_by_covenant() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let bundle = build_lending_deposit_tx(&req, 250_000).unwrap();

        // The interest region is frozen by the v1 covenant (ADR-013 §4).
        let mut inflated = bundle.new_pool_datum.clone();
        inflated.interest_multiplier_q64 = u128::MAX;
        let mut tx = bundle.tx.clone();
        tx.outputs[POOL_OUTPUT_INDEX].datum = Some(Datum::new(inflated.to_canonical_bytes()));
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: inflated,
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_redirected_pool_script_rejected_by_covenant() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let bundle = build_lending_deposit_tx(&req, 250_000).unwrap();

        let mut tx = bundle.tx.clone();
        tx.outputs[POOL_OUTPUT_INDEX].locking_script = Script::new(vec![0x01]); // attacker
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: bundle.new_pool_datum.clone(),
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert!(matches!(err, ScriptError::CovenantFailed(_)));
    }

    #[test]
    fn lending_pool_value_drain_rejected_by_covenant() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let bundle = build_lending_deposit_tx(&req, 250_000).unwrap();

        let mut tx = bundle.tx.clone();
        tx.outputs[POOL_OUTPUT_INDEX].value = Amount::from(1); // drain 1000 → 1
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: bundle.new_pool_datum.clone(),
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_witness_path_swap_rejected_by_covenant() {
        // A withdraw-shaped transition smuggled through the price-less
        // path: rebuild the deposit-path witness onto a withdraw tx.
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);
        let bundle = build_lending_withdraw_tx(&req, 200_000, &price).unwrap();

        let mut tx = bundle.tx.clone();
        tx.inputs[POOL_INPUT_INDEX].witness = Witness::new(lending_path0_witness());
        let tampered = LendingTxBundle {
            tx,
            new_pool_datum: bundle.new_pool_datum.clone(),
            inputs_to_sign: bundle.inputs_to_sign.clone(),
        };
        let err = validate_lending_bundle(&req, &tampered).unwrap_err();
        assert_eq!(err, ScriptError::VerifyFailed);
    }

    #[test]
    fn lending_builder_input_validation_errors() {
        let kp = oracle_keypair();
        let req = lending_request(&kp);
        let price = signed_price(&kp, PRICE_ONE, PRICE_SLOT);

        // Zero amounts.
        assert!(matches!(
            build_lending_deposit_tx(&req, 0).unwrap_err(),
            TxBuildError::ZeroAmount
        ));
        assert!(matches!(
            build_lending_borrow_tx(&req, 0, &price).unwrap_err(),
            TxBuildError::ZeroAmount
        ));

        // Repay with no outstanding debt.
        let mut no_debt = req.clone();
        no_debt.pool_datum.total_debt = 0;
        assert!(matches!(
            build_lending_repay_tx(&no_debt, 1_000).unwrap_err(),
            TxBuildError::Lending(LendingError::NoDebt)
        ));

        // Price bound to a different pool.
        let mut foreign = price.clone();
        foreign.pool_id = Hash256::from_bytes([0xEE; 32]);
        assert!(matches!(
            build_lending_borrow_tx(&req, 100_000, &foreign).unwrap_err(),
            TxBuildError::PriceForWrongPool
        ));

        // Withdraw more collateral than exists.
        assert!(matches!(
            build_lending_withdraw_tx(&req, 2_000_000, &price).unwrap_err(),
            TxBuildError::Lending(LendingError::Underflow)
        ));

        // User input cannot cover the fee.
        let mut poor = req.clone();
        poor.user_input_value = Amount::from(5);
        assert!(matches!(
            build_lending_deposit_tx(&poor, 100).unwrap_err(),
            TxBuildError::InsufficientInput {
                input_value: 5,
                fee: 10
            }
        ));
    }

    #[test]
    fn oracle_price_message_layout() {
        let pool_id = Hash256::from_bytes([0x1D; 32]);
        let msg = oracle_price_message(&pool_id, 7, 9);
        let tag = qv_script::templates::LENDING_ORACLE_DOMAIN_TAG;
        assert_eq!(&msg[..tag.len()], tag);
        assert_eq!(&msg[tag.len()..tag.len() + 32], pool_id.as_bytes());
        assert_eq!(
            &msg[tag.len() + 32..tag.len() + 40],
            &7_u64.to_le_bytes()
        );
        assert_eq!(&msg[tag.len() + 40..], &9_u64.to_le_bytes());
    }
}
