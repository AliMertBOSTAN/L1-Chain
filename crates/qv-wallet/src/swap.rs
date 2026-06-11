//! Wallet-side AMM flow helpers (Faz 6 / D-4 swap, D-5 create-pool).
//!
//! The heavy lifting — computing the constant-product output and
//! assembling the covenant transactions — lives in
//! [`qv_defi::build_swap_tx`] / [`qv_defi::build_create_pool_tx`]. This
//! module contains:
//!
//! * thin, pure, unit-testable glue (outpoint/token parsing, pool UTXO
//!   decoding + canonical-script verification, funding coin selection,
//!   user-input signing — ADR-012 sighash, same `<sig> <pubkey>` witness
//!   shape as
//!   [`TxBuilder::sign_plain_input`](crate::tx_builder::TxBuilder::sign_plain_input)),
//! * the **shared async flows** [`execute_swap`] and
//!   [`execute_create_pool`], called by both the CLI (`qv-wallet swap` /
//!   `qv-wallet create-pool` in `main.rs`) and the HTTP API
//!   (`/api/defi/swap` / `/api/defi/create-pool` in `server.rs`) so the
//!   two surfaces can never drift. The flows build and **sign** the
//!   transaction but do not broadcast — broadcasting policy (optional
//!   `--broadcast` vs. always-on HTTP) stays with the caller.
//!
//! Keystore unlocking stays with the callers.

use std::str::FromStr;

use qv_core::{Amount, Hash256, OutPoint, Script, Transaction, TxId, Witness};
use qv_crypto::{PqcPublicKey, PqcSecretKey};
use qv_defi::{
    build_create_pool_tx, build_swap_tx, CreatePoolRequest, PoolDatum, SwapDirection,
    SwapRequest,
};
use qv_script::templates::{amm_pool_lock, p2pkh_pqc, pubkey_hash};
use qv_script::ScriptBuilder;

use crate::cli::SwapDirectionArg;
use crate::rpc_client::{P2pkhMatch, RpcClient};
use crate::{WalletError, WalletResult};

/// Map the clap-level `--direction` flag onto the qv-defi enum.
#[must_use]
pub fn direction_from_arg(arg: SwapDirectionArg) -> SwapDirection {
    match arg {
        SwapDirectionArg::AToB => SwapDirection::AtoB,
        SwapDirectionArg::BToA => SwapDirection::BtoA,
    }
}

/// Human-readable label for a swap direction (used in the CLI summary).
#[must_use]
pub fn direction_label(direction: SwapDirection) -> &'static str {
    match direction {
        SwapDirection::AtoB => "A -> B (sell token A, receive token B)",
        SwapDirection::BtoA => "B -> A (sell token B, receive token A)",
    }
}

/// Parse the HTTP-API direction string (`"a-to-b"` / `"b-to-a"`, same
/// spelling as the CLI `--direction` value-enum) into the qv-defi enum.
pub fn direction_from_str(s: &str) -> WalletResult<SwapDirection> {
    match s.trim().to_ascii_lowercase().as_str() {
        "a-to-b" => Ok(SwapDirection::AtoB),
        "b-to-a" => Ok(SwapDirection::BtoA),
        other => Err(WalletError::InvalidArg(format!(
            "direction must be `a-to-b` or `b-to-a`, got {other:?}"
        ))),
    }
}

/// Parse a 32-byte token identifier from hex (64 chars).
pub fn parse_token_id(label: &str, hex_str: &str) -> WalletResult<Hash256> {
    let bytes = hex::decode(hex_str.trim())
        .map_err(|e| WalletError::InvalidArg(format!("{label}: invalid hex: {e}")))?;
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        WalletError::InvalidArg(format!(
            "{label} must be exactly 32 bytes (64 hex chars), got {} bytes",
            bytes.len()
        ))
    })?;
    Ok(Hash256::from_bytes(arr))
}

/// Parse an outpoint in either the canonical `txid#idx` or the
/// Bitcoin-style `txid:idx` form (both accepted by `OutPoint::from_str`).
pub fn parse_outpoint(s: &str) -> WalletResult<OutPoint> {
    OutPoint::from_str(s).map_err(|e| {
        WalletError::InvalidArg(format!(
            "outpoint {s:?} must be `<txid_hex>#<idx>` or `<txid_hex>:<idx>`: {e}"
        ))
    })
}

/// Decode the pool UTXO's datum (hex of the canonical fixed-width
/// `PoolDatum` layout, as returned in `qv_getUtxo`'s `datum_hex`).
pub fn decode_pool_datum(datum_hex: &str) -> WalletResult<PoolDatum> {
    let bytes = hex::decode(datum_hex)
        .map_err(|e| WalletError::InvalidArg(format!("pool datum_hex: {e}")))?;
    PoolDatum::from_canonical_bytes(&bytes)
        .map_err(|e| WalletError::InvalidArg(format!("pool datum decode: {e}")))
}

/// Decode the pool UTXO's locking script (hex from `qv_getUtxo`'s
/// `script_hex`) and verify it is **byte-identical** to the canonical
/// `amm_pool_lock` regenerated from the datum's token ids + fee.
///
/// `build_swap_tx` regenerates the successor pool script from the datum;
/// the on-chain covenant (`ASSERT_OUTPUT_SCRIPT_HASH`) demands the exact
/// same bytes, so any mismatch here would produce an unspendable
/// transaction — better to refuse up front with a clear error.
pub fn decode_pool_script(script_hex: &str, datum: &PoolDatum) -> WalletResult<Script> {
    let bytes = hex::decode(script_hex)
        .map_err(|e| WalletError::InvalidArg(format!("pool script_hex: {e}")))?;
    let expected = amm_pool_lock(
        datum.token_a_id.as_bytes(),
        datum.token_b_id.as_bytes(),
        datum.fee_bps,
    );
    if bytes != expected {
        return Err(WalletError::InvalidArg(
            "pool UTXO's locking script is not the canonical amm_pool_lock for its datum \
             (token ids + fee_bps) — refusing to build a swap against it"
                .into(),
        ));
    }
    Ok(Script::new(bytes))
}

/// Coin selection for the swap funding input: the **smallest** plain
/// p2pkh UTXO whose value covers `needed` (ties broken by `tx_id`, then
/// `output_index`, for determinism). Returns `None` when no single UTXO
/// is sufficient — the swap shape takes exactly one user input.
#[must_use]
pub fn select_funding_utxo(utxos: &[P2pkhMatch], needed: u64) -> Option<&P2pkhMatch> {
    utxos
        .iter()
        .filter(|u| u.value >= needed)
        .min_by(|a, b| {
            a.value
                .cmp(&b.value)
                .then_with(|| a.tx_id.cmp(&b.tx_id))
                .then_with(|| a.output_index.cmp(&b.output_index))
        })
}

/// Sign the swap transaction's user input in place (ADR-012).
///
/// Same signing path as `TxBuilder::sign_plain_input`, but operating
/// directly on the [`Transaction`] returned by `build_swap_tx` so the
/// bundle's `fee` field and shape are preserved verbatim: sign the
/// witness-excluded sighash with Dilithium and attach the witness script
/// `<signature> <pubkey>` to `inputs[input_idx]`. The pool input (index 0)
/// keeps its empty witness — the covenant needs none.
pub fn sign_swap_user_input(
    tx: &mut Transaction,
    input_idx: usize,
    secret_key: &PqcSecretKey,
    public_key: &PqcPublicKey,
) -> WalletResult<()> {
    if tx.inputs.get(input_idx).is_none() {
        return Err(WalletError::TxBuilder(format!(
            "swap user input index {input_idx} out of range (count {})",
            tx.inputs.len()
        )));
    }

    // ADR-012: one transaction-wide, witness-excluded sighash.
    let sighash = tx
        .sighash()
        .map_err(|e| WalletError::TxBuilder(format!("sighash failed: {e}")))?;

    let signature = qv_crypto::sign_pqc(secret_key, &sighash)
        .map_err(|e| WalletError::Crypto(e.to_string()))?;

    let witness_script = ScriptBuilder::new()
        .push_bytes(signature.as_bytes())
        .push_bytes(public_key.as_bytes())
        .build();

    if let Some(input) = tx.inputs.get_mut(input_idx) {
        input.witness = Witness::new(witness_script);
    } else {
        return Err(WalletError::TxBuilder(format!(
            "swap user input index {input_idx} disappeared during signing"
        )));
    }
    Ok(())
}

// ============================================================================
// Shared async flows — one implementation for CLI and HTTP (Faz 6 / D-5)
// ============================================================================

/// Everything [`execute_swap`] needs besides keys and an RPC client.
#[derive(Debug, Clone)]
pub struct SwapParams {
    /// Pool UTXO outpoint string (`txid#idx` or `txid:idx`).
    pub pool: String,
    /// Which pool token the user sells.
    pub direction: SwapDirection,
    /// Amount of the input token to sell (smallest units).
    pub amount_in: u64,
    /// Slippage floor: minimum acceptable output amount (0 = no floor).
    pub min_receive: u64,
    /// Optional explicit funding UTXO (`txid#idx` / `txid:idx`); when
    /// `None` the smallest sufficient plain p2pkh UTXO is auto-selected.
    pub input: Option<String>,
    /// Optional explicit value of `input`; when `None` it is resolved
    /// via `qv_getUtxo`.
    pub input_value: Option<u64>,
    /// Network fee (smallest units).
    pub fee: u64,
}

/// The result of [`execute_swap`]: a fully **signed** transaction (hex)
/// plus the numbers the caller wants to show. Broadcasting is the
/// caller's decision.
#[derive(Debug, Clone)]
pub struct SwapOutcome {
    /// Hex-encoded bincode of the signed transaction.
    pub tx_hex: String,
    /// Local transaction id (hex).
    pub tx_id_hex: String,
    /// The consumed pool outpoint (canonical `txid#idx`).
    pub pool_outpoint: String,
    /// The funding outpoint that was spent (canonical `txid#idx`).
    pub user_outpoint: String,
    /// Value of the funding UTXO.
    pub user_input_value: u64,
    /// Change returned to the user's own p2pkh output.
    pub change: u64,
    /// Computed output amount (smallest units of the out-token).
    pub amount_out: u64,
    /// Swap fee retained by the pool reserves (in-token units).
    pub pool_fee_paid: u64,
    /// Post-swap pool datum (new reserves / lp_total / fee).
    pub new_pool_datum: PoolDatum,
    /// Size of the serialized transaction in bytes.
    pub tx_size: usize,
}

/// Resolve the user's funding input: explicit outpoint (+ optional value,
/// else `qv_getUtxo`) or auto-selection of the smallest sufficient plain
/// p2pkh UTXO via `qv_scanP2pkh`.
async fn resolve_funding(
    rpc: &RpcClient,
    user_pk_hash: &[u8; 32],
    input: Option<&str>,
    input_value: Option<u64>,
    needed: u64,
) -> WalletResult<(OutPoint, u64)> {
    match input {
        Some(s) => {
            let op = parse_outpoint(s)?;
            let value = match input_value {
                Some(v) => v,
                None => {
                    rpc.get_utxo(&op.to_string())
                        .await?
                        .ok_or_else(|| {
                            WalletError::InvalidArg(format!(
                                "funding UTXO {op} not found on the node"
                            ))
                        })?
                        .value
                }
            };
            Ok((op, value))
        }
        None => {
            let utxos = rpc.scan_p2pkh(user_pk_hash).await?;
            let pick = select_funding_utxo(&utxos, needed).ok_or_else(|| {
                WalletError::InvalidArg(format!(
                    "no single plain p2pkh UTXO covers the required {needed} units — \
                     fund the account or pass an explicit input"
                ))
            })?;
            let tx_id_bytes = hex::decode(&pick.tx_id)
                .map_err(|e| WalletError::Rpc(format!("server tx_id hex: {e}")))?;
            let tx_id_arr: [u8; 32] = tx_id_bytes
                .as_slice()
                .try_into()
                .map_err(|_| WalletError::Rpc("server tx_id must be 32 bytes".into()))?;
            Ok((
                OutPoint::new(TxId::from_bytes(tx_id_arr), pick.output_index),
                pick.value,
            ))
        }
    }
}

/// Fetch + verify the pool UTXO named by `pool`: existence, canonical
/// 90-byte datum, byte-identical `amm_pool_lock` script.
async fn fetch_verified_pool(
    rpc: &RpcClient,
    pool: &str,
) -> WalletResult<(OutPoint, PoolDatum, u64)> {
    let pool_outpoint = parse_outpoint(pool)?;
    let pool_info = rpc
        .get_utxo(&pool_outpoint.to_string())
        .await?
        .ok_or_else(|| {
            WalletError::InvalidArg(format!(
                "pool UTXO {pool_outpoint} not found on the node (never existed or already spent)"
            ))
        })?;
    let datum_hex = pool_info.datum_hex.as_deref().ok_or_else(|| {
        if pool_info.has_datum {
            WalletError::Rpc(format!(
                "node did not return datum_hex for {pool_outpoint} — qv-node predates the \
                 Faz 6 qv_getUtxo extension; upgrade the node"
            ))
        } else {
            WalletError::InvalidArg(format!(
                "UTXO {pool_outpoint} carries no datum — not an AMM pool UTXO"
            ))
        }
    })?;
    let pool_datum = decode_pool_datum(datum_hex)?;
    let script_hex = pool_info.script_hex.as_deref().ok_or_else(|| {
        WalletError::Rpc(format!(
            "node did not return script_hex for {pool_outpoint} — qv-node predates the \
             Faz 6 qv_getUtxo extension; upgrade the node"
        ))
    })?;
    // Refuse non-canonical pool scripts up front; the on-chain covenant
    // would reject the successor output anyway.
    let _pool_script = decode_pool_script(script_hex, &pool_datum)?;
    Ok((pool_outpoint, pool_datum, pool_info.value))
}

/// Build and **sign** an AMM swap transaction against a live pool UTXO.
///
/// Shared by `qv-wallet swap` (CLI) and `POST /api/defi/swap` (HTTP).
/// Steps: fetch + verify the pool UTXO (`qv_getUtxo`; canonical datum and
/// byte-identical `amm_pool_lock` script), resolve the funding input,
/// delegate the covenant assembly to [`qv_defi::build_swap_tx`], and sign
/// the user input (ADR-012 sighash). Does **not** broadcast.
pub async fn execute_swap(
    rpc: &RpcClient,
    spend_sk: &PqcSecretKey,
    spend_pk: &PqcPublicKey,
    params: &SwapParams,
) -> WalletResult<SwapOutcome> {
    if params.amount_in == 0 {
        return Err(WalletError::InvalidArg("amount must be positive".into()));
    }

    // ----- 1. Pool UTXO: outpoint → qv_getUtxo → datum + script. -----
    let (pool_outpoint, pool_datum, pool_value) =
        fetch_verified_pool(rpc, &params.pool).await?;

    // ----- 2. Funding input: explicit or auto-select (covers the fee). -----
    let user_pk_hash = pubkey_hash(spend_pk.as_bytes());
    let (user_outpoint, user_input_value) = resolve_funding(
        rpc,
        &user_pk_hash,
        params.input.as_deref(),
        params.input_value,
        params.fee,
    )
    .await?;
    let change = user_input_value.checked_sub(params.fee).ok_or_else(|| {
        WalletError::InvalidArg(format!(
            "funding input value ({user_input_value}) cannot cover the fee ({})",
            params.fee
        ))
    })?;

    // ----- 3. Build the unsigned bundle and sign the user input. -----
    let req = SwapRequest {
        pool_outpoint,
        pool_datum,
        pool_value: Amount::from_smallest_units(pool_value),
        user_outpoint,
        user_input_value: Amount::from_smallest_units(user_input_value),
        user_locking_script: Script::new(p2pkh_pqc(&user_pk_hash)),
        direction: params.direction,
        amount_in: params.amount_in,
        min_receive: params.min_receive,
        tx_fee: Amount::from_smallest_units(params.fee),
    };
    let mut bundle = build_swap_tx(&req)
        .map_err(|e| WalletError::TxBuilder(format!("swap build failed: {e}")))?;
    for idx in bundle.inputs_to_sign.clone() {
        sign_swap_user_input(&mut bundle.tx, idx, spend_sk, spend_pk)?;
    }

    let tx = bundle.tx;
    let tx_id = tx
        .id()
        .map_err(|e| WalletError::TxBuilder(format!("tx id compute failed: {e}")))?;
    let tx_bytes = bincode::serialize(&tx)?;

    Ok(SwapOutcome {
        tx_hex: hex::encode(&tx_bytes),
        tx_id_hex: tx_id.to_hex(),
        pool_outpoint: pool_outpoint.to_string(),
        user_outpoint: user_outpoint.to_string(),
        user_input_value,
        change,
        amount_out: bundle.amount_out,
        pool_fee_paid: bundle.pool_fee_paid,
        new_pool_datum: bundle.new_pool_datum,
        tx_size: tx_bytes.len(),
    })
}

/// Everything [`execute_create_pool`] needs besides keys and an RPC client.
#[derive(Debug, Clone)]
pub struct CreatePoolParams {
    /// Token A identifier (32-byte hex).
    pub token_a_hex: String,
    /// Token B identifier (32-byte hex).
    pub token_b_hex: String,
    /// Swap fee in basis points (0..=10000).
    pub fee_bps: u16,
    /// Initial reserve of token A (smallest units, datum-level).
    pub reserve_a: u64,
    /// Initial reserve of token B (smallest units, datum-level).
    pub reserve_b: u64,
    /// Native value locked into the pool UTXO (carried through swaps).
    pub pool_value: u64,
    /// Optional explicit funding UTXO; `None` → auto-select.
    pub input: Option<String>,
    /// Optional explicit value of `input`.
    pub input_value: Option<u64>,
    /// Network fee (smallest units).
    pub fee: u64,
}

/// The result of [`execute_create_pool`]: a fully **signed** transaction
/// (hex) plus pool metadata. Broadcasting is the caller's decision.
#[derive(Debug, Clone)]
pub struct CreatePoolOutcome {
    /// Hex-encoded bincode of the signed transaction.
    pub tx_hex: String,
    /// Local transaction id (hex).
    pub tx_id_hex: String,
    /// The new pool's outpoint, canonical `<txid>#0` — pass to `swap --pool`.
    pub pool_outpoint: String,
    /// The genesis pool datum (reserves, lp_total, fee).
    pub pool_datum: PoolDatum,
    /// LP shares credited in the datum (`lp_total = ⌊sqrt(a·b)⌋`).
    /// **Datum-level accounting only — no on-chain LP token (D-6+).**
    pub lp_total: u64,
    /// The funding outpoint that was spent (canonical `txid#idx`).
    pub user_outpoint: String,
    /// Value of the funding UTXO.
    pub user_input_value: u64,
    /// Change returned to the user's own p2pkh output.
    pub change: u64,
    /// Size of the serialized transaction in bytes.
    pub tx_size: usize,
}

/// Build and **sign** the transaction that bootstraps a brand-new AMM
/// pool UTXO (Faz 6 / D-5).
///
/// Shared by `qv-wallet create-pool` (CLI) and
/// `POST /api/defi/create-pool` (HTTP). Delegates the assembly to
/// [`qv_defi::build_create_pool_tx`] (genesis `lp_total = ⌊sqrt(a·b)⌋`
/// through `compute_add_liquidity`'s empty-pool path), then signs the
/// funding input (ADR-012 sighash). Does **not** broadcast.
///
/// **Honest scope (D-5):** LP shares live only in the datum's `lp_total`
/// field — no on-chain LP token, no add/remove-liquidity spend path yet.
/// `amm_pool_lock` would pass an add-liquidity-shaped transition (product
/// grows) but blocks remove-liquidity (product shrinks).
pub async fn execute_create_pool(
    rpc: &RpcClient,
    spend_sk: &PqcSecretKey,
    spend_pk: &PqcPublicKey,
    params: &CreatePoolParams,
) -> WalletResult<CreatePoolOutcome> {
    let token_a_id = parse_token_id("--token-a", &params.token_a_hex)?;
    let token_b_id = parse_token_id("--token-b", &params.token_b_hex)?;

    // Funding must cover the pool UTXO's native value plus the fee.
    let needed = params
        .pool_value
        .checked_add(params.fee)
        .ok_or_else(|| WalletError::InvalidArg("pool-value + fee overflows".into()))?;
    let user_pk_hash = pubkey_hash(spend_pk.as_bytes());
    let (user_outpoint, user_input_value) = resolve_funding(
        rpc,
        &user_pk_hash,
        params.input.as_deref(),
        params.input_value,
        needed,
    )
    .await?;

    let req = CreatePoolRequest {
        token_a_id,
        token_b_id,
        fee_bps: params.fee_bps,
        reserve_a: params.reserve_a,
        reserve_b: params.reserve_b,
        pool_value: Amount::from_smallest_units(params.pool_value),
        user_outpoint,
        user_input_value: Amount::from_smallest_units(user_input_value),
        user_locking_script: Script::new(p2pkh_pqc(&user_pk_hash)),
        tx_fee: Amount::from_smallest_units(params.fee),
    };
    let mut bundle = build_create_pool_tx(&req)
        .map_err(|e| WalletError::TxBuilder(format!("create-pool build failed: {e}")))?;
    for idx in bundle.inputs_to_sign.clone() {
        sign_swap_user_input(&mut bundle.tx, idx, spend_sk, spend_pk)?;
    }

    let tx = bundle.tx;
    let tx_id = tx
        .id()
        .map_err(|e| WalletError::TxBuilder(format!("tx id compute failed: {e}")))?;
    let tx_bytes = bincode::serialize(&tx)?;
    let change = user_input_value.saturating_sub(needed);

    Ok(CreatePoolOutcome {
        tx_hex: hex::encode(&tx_bytes),
        tx_id_hex: tx_id.to_hex(),
        pool_outpoint: format!("{}#0", tx_id.to_hex()),
        pool_datum: bundle.pool_datum,
        lp_total: bundle.lp_total,
        user_outpoint: user_outpoint.to_string(),
        user_input_value,
        change,
        tx_size: tx_bytes.len(),
    })
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
    use qv_core::{Datum, Slot, TxOutput};
    use qv_crypto::{DilithiumLevel, PqcKeyPair};
    use qv_defi::tx_helpers::{POOL_INPUT_INDEX, USER_INPUT_INDEX};
    use qv_defi::{build_swap_tx, SwapRequest};
    use qv_script::templates::{p2pkh_pqc, pubkey_hash};
    use qv_script::validate_script;

    fn pool_datum() -> PoolDatum {
        PoolDatum {
            token_a_id: Hash256::from_bytes([0xA1; 32]),
            token_b_id: Hash256::from_bytes([0xB2; 32]),
            reserve_a: 10_000,
            reserve_b: 10_000,
            lp_total: 5_000,
            fee_bps: 30,
        }
    }

    fn p2pkh_match(value: u64, tx_byte: u8, index: u32) -> P2pkhMatch {
        P2pkhMatch {
            tx_id: format!("{tx_byte:02x}").repeat(32),
            output_index: index,
            value,
        }
    }

    #[test]
    fn direction_mapping_is_exact() {
        assert_eq!(
            direction_from_arg(SwapDirectionArg::AToB),
            SwapDirection::AtoB
        );
        assert_eq!(
            direction_from_arg(SwapDirectionArg::BToA),
            SwapDirection::BtoA
        );
        assert!(direction_label(SwapDirection::AtoB).starts_with("A -> B"));
        assert!(direction_label(SwapDirection::BtoA).starts_with("B -> A"));
    }

    #[test]
    fn direction_from_str_accepts_canonical_spellings() {
        assert_eq!(direction_from_str("a-to-b").unwrap(), SwapDirection::AtoB);
        assert_eq!(direction_from_str("b-to-a").unwrap(), SwapDirection::BtoA);
        // Trim + case-insensitive (the UI sends lowercase, but be lenient).
        assert_eq!(direction_from_str(" A-TO-B ").unwrap(), SwapDirection::AtoB);
    }

    #[test]
    fn direction_from_str_rejects_garbage() {
        for bad in ["", "atob", "a_to_b", "sell-a", "AtoB"] {
            let err = direction_from_str(bad).unwrap_err();
            assert!(matches!(err, WalletError::InvalidArg(_)), "input {bad:?}");
        }
    }

    #[test]
    fn parse_token_id_roundtrip_and_errors() {
        let id = parse_token_id("--token-a", &"a1".repeat(32)).unwrap();
        assert_eq!(id, Hash256::from_bytes([0xA1; 32]));
        // Whitespace tolerated.
        let id2 = parse_token_id("--token-a", &format!(" {} ", "a1".repeat(32))).unwrap();
        assert_eq!(id2, id);

        // Bad hex.
        assert!(matches!(
            parse_token_id("--token-a", "zz"),
            Err(WalletError::InvalidArg(_))
        ));
        // Wrong length (31 bytes).
        assert!(matches!(
            parse_token_id("--token-b", &"a1".repeat(31)),
            Err(WalletError::InvalidArg(_))
        ));
    }

    /// `execute_create_pool`'s building block: the bundle produced from a
    /// `CreatePoolRequest` mirrors what `execute_create_pool` signs. Verify
    /// the signed create-pool tx passes the user's own p2pkh script —
    /// i.e. the shared `sign_swap_user_input` path also covers the
    /// single-input create-pool shape.
    #[test]
    fn signed_create_pool_tx_passes_user_script() {
        let kp = PqcKeyPair::generate(DilithiumLevel::Level3).unwrap();
        let pk_hash = pubkey_hash(kp.public.as_bytes());
        let user_script = Script::new(p2pkh_pqc(&pk_hash));

        let req = qv_defi::CreatePoolRequest {
            token_a_id: Hash256::from_bytes([0xA1; 32]),
            token_b_id: Hash256::from_bytes([0xB2; 32]),
            fee_bps: 30,
            reserve_a: 10_000,
            reserve_b: 10_000,
            pool_value: Amount::from(1_000),
            user_outpoint: OutPoint::new(TxId::from_bytes([7; 32]), 0),
            user_input_value: Amount::from(5_000),
            user_locking_script: user_script.clone(),
            tx_fee: Amount::from(10),
        };
        let mut bundle = build_create_pool_tx(&req).unwrap();
        assert_eq!(bundle.inputs_to_sign, vec![0]);
        sign_swap_user_input(&mut bundle.tx, 0, &kp.secret, &kp.public).unwrap();
        assert!(!bundle.tx.inputs[0].witness.is_empty());

        // Resolved prevouts: just the user funding UTXO.
        let resolved = vec![TxOutput::new(req.user_input_value, user_script.clone())];
        let witness_bytes = bundle.tx.inputs[0].witness.as_bytes().to_vec();
        let res = validate_script(
            &user_script,
            &witness_bytes,
            &bundle.tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap();
        assert!(res.success, "create-pool funding witness must verify");
    }

    #[test]
    fn parse_outpoint_accepts_both_separators() {
        let txid_hex = "ab".repeat(32);
        let hash = OutPoint::new(TxId::from_bytes([0xAB; 32]), 7);
        assert_eq!(parse_outpoint(&format!("{txid_hex}#7")).unwrap(), hash);
        assert_eq!(parse_outpoint(&format!("{txid_hex}:7")).unwrap(), hash);
    }

    #[test]
    fn parse_outpoint_rejects_garbage() {
        let no_separator = "ab".repeat(32);
        for bad in ["", "deadbeef", "xyz#1", no_separator.as_str()] {
            let err = parse_outpoint(bad).unwrap_err();
            assert!(matches!(err, WalletError::InvalidArg(_)), "input {bad:?}");
        }
    }

    #[test]
    fn pool_datum_hex_roundtrip() {
        let datum = pool_datum();
        let hex_str = hex::encode(datum.to_canonical_bytes());
        let decoded = decode_pool_datum(&hex_str).unwrap();
        assert_eq!(decoded, datum);
    }

    #[test]
    fn pool_datum_rejects_bad_hex_and_length() {
        assert!(matches!(
            decode_pool_datum("zz"),
            Err(WalletError::InvalidArg(_))
        ));
        // 89 bytes — one short of the canonical 90.
        assert!(matches!(
            decode_pool_datum(&"00".repeat(89)),
            Err(WalletError::InvalidArg(_))
        ));
    }

    #[test]
    fn pool_script_must_match_canonical_lock() {
        let datum = pool_datum();
        let canonical = amm_pool_lock(
            datum.token_a_id.as_bytes(),
            datum.token_b_id.as_bytes(),
            datum.fee_bps,
        );
        let ok = decode_pool_script(&hex::encode(&canonical), &datum).unwrap();
        assert_eq!(ok.as_bytes(), canonical.as_slice());

        // Same script bytes but a datum with a different fee → mismatch.
        let mut other = datum;
        other.fee_bps = 100;
        assert!(matches!(
            decode_pool_script(&hex::encode(&canonical), &other),
            Err(WalletError::InvalidArg(_))
        ));
    }

    #[test]
    fn select_funding_picks_smallest_sufficient() {
        let utxos = vec![
            p2pkh_match(5_000, 0x01, 0),
            p2pkh_match(1_200, 0x02, 1),
            p2pkh_match(900, 0x03, 0), // insufficient for needed=1000
        ];
        let pick = select_funding_utxo(&utxos, 1_000).unwrap();
        assert_eq!(pick.value, 1_200);
        assert_eq!(pick.output_index, 1);
    }

    #[test]
    fn select_funding_tie_breaks_deterministically() {
        let utxos = vec![
            p2pkh_match(1_000, 0x0B, 3),
            p2pkh_match(1_000, 0x0A, 9),
            p2pkh_match(1_000, 0x0A, 2),
        ];
        let pick = select_funding_utxo(&utxos, 500).unwrap();
        assert_eq!(pick.tx_id, "0a".repeat(32));
        assert_eq!(pick.output_index, 2);
    }

    #[test]
    fn select_funding_none_when_no_single_utxo_covers() {
        let utxos = vec![p2pkh_match(400, 0x01, 0), p2pkh_match(500, 0x02, 0)];
        assert!(select_funding_utxo(&utxos, 1_000).is_none());
        assert!(select_funding_utxo(&[], 1).is_none());
    }

    /// End-to-end: build the swap bundle exactly like `cmd_swap` does,
    /// sign the user input, then run the script VM on **both** inputs —
    /// the pool covenant (empty witness) and the user's p2pkh prevout
    /// (the witness this module produced).
    #[test]
    fn signed_swap_tx_passes_both_input_scripts() {
        let kp = PqcKeyPair::generate(DilithiumLevel::Level3).unwrap();
        let pk_hash = pubkey_hash(kp.public.as_bytes());
        let user_script = Script::new(p2pkh_pqc(&pk_hash));

        let datum = pool_datum();
        let req = SwapRequest {
            pool_outpoint: OutPoint::new(TxId::from_bytes([9; 32]), 0),
            pool_datum: datum.clone(),
            pool_value: Amount::from(1_000),
            user_outpoint: OutPoint::new(TxId::from_bytes([8; 32]), 1),
            user_input_value: Amount::from(5_000),
            user_locking_script: user_script.clone(),
            direction: SwapDirection::AtoB,
            amount_in: 1_000,
            min_receive: 900,
            tx_fee: Amount::from(10),
        };
        let mut bundle = build_swap_tx(&req).unwrap();
        assert_eq!(bundle.inputs_to_sign, vec![USER_INPUT_INDEX]);

        sign_swap_user_input(&mut bundle.tx, USER_INPUT_INDEX, &kp.secret, &kp.public)
            .unwrap();
        // Pool input stays witness-less; user input is now witnessed.
        assert!(bundle.tx.inputs[POOL_INPUT_INDEX].witness.is_empty());
        assert!(!bundle.tx.inputs[USER_INPUT_INDEX].witness.is_empty());

        // Resolved prevouts in input order: pool UTXO, user UTXO.
        let pool_script = Script::new(amm_pool_lock(
            datum.token_a_id.as_bytes(),
            datum.token_b_id.as_bytes(),
            datum.fee_bps,
        ));
        let resolved = vec![
            TxOutput::new(req.pool_value, pool_script.clone())
                .with_datum(Datum::new(datum.to_canonical_bytes())),
            TxOutput::new(req.user_input_value, user_script.clone()),
        ];

        // Input #0: AMM covenant validates with an empty witness.
        let pool_res =
            validate_script(&pool_script, &[], &bundle.tx, &resolved, Slot::from(1)).unwrap();
        assert!(pool_res.success, "pool covenant must accept the swap");

        // Input #1: p2pkh prevout validates with our witness.
        let witness_bytes = bundle.tx.inputs[USER_INPUT_INDEX].witness.as_bytes().to_vec();
        let user_res = validate_script(
            &user_script,
            &witness_bytes,
            &bundle.tx,
            &resolved,
            Slot::from(1),
        )
        .unwrap();
        assert!(user_res.success, "user p2pkh witness must verify");
    }

    #[test]
    fn sign_swap_user_input_rejects_out_of_range_index() {
        let kp = PqcKeyPair::generate(DilithiumLevel::Level2).unwrap();
        let mut tx = Transaction::new(
            vec![qv_core::TxInput::new(OutPoint::new(
                TxId::from_bytes([1; 32]),
                0,
            ))],
            vec![TxOutput::new(Amount::from(1), Script::new(vec![0x01]))],
        );
        let err = sign_swap_user_input(&mut tx, 5, &kp.secret, &kp.public).unwrap_err();
        assert!(matches!(err, WalletError::TxBuilder(_)));
    }
}
