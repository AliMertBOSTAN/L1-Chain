//! Devnet-only stealth faucet.
//!
//! On `--network devnet`, the node's genesis pre-funds the well-known
//! [`DEVNET_TEST_MNEMONIC`] (see [`crate::hd`]). Any new wallet — created
//! at runtime via "Create wallet" in the UI — starts with zero balance
//! and needs a way to acquire test funds. The CLI workaround is the
//! `devnet-import` subcommand, which *replaces* the user's keystore with
//! the devnet mnemonic; that's fine for throwaway wallets but destroys
//! whatever the user had before.
//!
//! This module implements a less-destructive alternative: a sponsored
//! send from the **devnet** spend key (account 0) to the **user's**
//! current stealth address. It is intentionally separate from the
//! everyday spend path:
//!
//! * No user keystore is read or modified.
//! * The devnet spend key is rebuilt from [`DEVNET_TEST_MNEMONIC`] each
//!   call — no on-disk state for the faucet itself.
//! * Inputs come from `qv_scanP2pkh(pkhash(devnet_pk))` (genesis
//!   allocations), and the change output is a `p2pkh_pqc` back to the
//!   same hash so subsequent faucet calls can find it.
//! * The payout output is a real stealth output to the user — the user's
//!   wallet detects it via its own scan, exactly like a normal incoming
//!   payment.
//!
//! **Never enable on mainnet.** Anyone can derive the devnet spend key.
//! The HTTP route [`crate::server::handle_devnet_faucet`] sits behind
//! `127.0.0.1` only and is intended for local devnet testing.

use crate::address::decode_address;
use crate::hd::{DefaultSeedDeriver, DEVNET_TEST_MNEMONIC};
use crate::rpc_client::RpcClient;
use crate::tx_builder::TxBuilder;
use crate::Mnemonic;
use crate::{WalletError, WalletResult};
use qv_core::{Amount, OutPoint, Script as CoreScript, TxId, TxInput, TxOutput, ValidityInterval};

/// Outcome of a successful faucet drip.
#[derive(Debug)]
pub struct FaucetReceipt {
    /// Hex-encoded transaction id that was broadcast.
    pub tx_id_hex: String,
    /// Bincode-then-hex of the signed transaction (for the UI to log).
    pub tx_hex: String,
    /// Whatever the node returned from `qv_sendTransaction`.
    pub rpc_result: serde_json::Value,
    /// Amount paid to the recipient.
    pub amount: u64,
    /// Fee paid (taken from the faucet's own balance).
    pub fee: u64,
}

/// Run a single faucet payout. Drips `amount` units from the devnet
/// mnemonic's account 0 to `recipient_address` and broadcasts the
/// transaction via `rpc`.
///
/// Returns `Err(InvalidArg("faucet exhausted: ..."))` when the devnet
/// account's plain-p2pkh balance can't cover `amount + fee`.
///
/// This function is `async` because it talks to the node over JSON-RPC.
pub async fn drip(
    rpc: &RpcClient,
    recipient_address: &str,
    amount: u64,
    fee: u64,
) -> WalletResult<FaucetReceipt> {
    if amount == 0 {
        return Err(WalletError::InvalidArg(
            "faucet amount must be positive".into(),
        ));
    }
    let outflow = amount.checked_add(fee).ok_or_else(|| {
        WalletError::InvalidArg("amount + fee overflows u64".into())
    })?;

    // 1. Rebuild the devnet spend key from the well-known mnemonic.
    //    No view key is needed — faucet inputs are plain p2pkh_pqc, not
    //    stealth (genesis allocations always use the plain template).
    let mnemonic = Mnemonic::from_phrase(DEVNET_TEST_MNEMONIC)
        .map_err(|e| WalletError::Mnemonic(format!("devnet mnemonic invalid: {e}")))?;
    let seed = mnemonic
        .to_seed("")
        .map_err(|e| WalletError::Mnemonic(format!("devnet seed: {e}")))?;
    let deriver = DefaultSeedDeriver::default_levels();
    let spend_kp = deriver
        .derive_spend_key(&seed, 0)
        .map_err(|e| WalletError::HdDerivation(format!("devnet account 0: {e}")))?;
    let pk_hash = qv_script::pubkey_hash(spend_kp.public.as_bytes());

    // 2. Scan the node for plain p2pkh UTXOs locked to the devnet hash.
    //    These are the genesis allocations (and any change from prior
    //    faucet runs).
    let plain = rpc
        .scan_p2pkh(&pk_hash)
        .await
        .map_err(|e| WalletError::Rpc(format!("scan_p2pkh (faucet): {e}")))?;
    let total: u64 = plain.iter().map(|u| u.value).sum();
    if total < outflow {
        return Err(WalletError::InvalidArg(format!(
            "faucet exhausted: need {outflow}, have {total} (account 0 plain UTXOs)"
        )));
    }

    // 3. Greedy largest-first coin selection.
    let mut sorted: Vec<_> = plain.iter().collect();
    sorted.sort_by(|a, b| b.value.cmp(&a.value));
    let mut picked = Vec::new();
    let mut acc: u64 = 0;
    for u in &sorted {
        if acc >= outflow {
            break;
        }
        acc = acc.saturating_add(u.value);
        picked.push(*u);
    }
    if acc < outflow {
        return Err(WalletError::InvalidArg(format!(
            "faucet selection insufficient: need {outflow}, picked {acc}"
        )));
    }
    let change = acc.saturating_sub(outflow);

    // 4. Decode recipient stealth address.
    let recipient = decode_address(recipient_address)?;

    // 5. Build the transaction.
    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    for u in &picked {
        let tx_id_bytes = hex::decode(&u.tx_id)
            .map_err(|e| WalletError::Rpc(format!("faucet: bad tx_id hex: {e}")))?;
        let tx_id_arr: [u8; 32] = tx_id_bytes
            .as_slice()
            .try_into()
            .map_err(|_| WalletError::Rpc("faucet: tx_id is not 32 bytes".into()))?;
        builder.add_input(TxInput::new(OutPoint::new(
            TxId::from_bytes(tx_id_arr),
            u.output_index,
        )));
    }

    // 5a. Stealth output to the user.
    builder.add_stealth_output(Amount::from(amount), &recipient)?;

    // 5b. Plain p2pkh change back to the devnet hash (so a future faucet
    //     call can find it).
    if change > 0 {
        let script_bytes = qv_script::p2pkh_pqc(&pk_hash);
        let change_out = TxOutput::new(Amount::from(change), CoreScript::new(script_bytes));
        builder.add_output(change_out);
    }

    // 6. Sign every input with the devnet spend keypair.
    for idx in 0..picked.len() {
        builder.sign_plain_input(idx, &spend_kp.secret, &spend_kp.public)?;
    }

    // 7. Serialise & broadcast.
    let tx = builder.build_unsigned()?;
    let tx_id = tx
        .id()
        .map_err(|e| WalletError::TxBuilder(format!("tx id compute: {e}")))?;
    let tx_bytes = bincode::serialize(&tx).map_err(WalletError::Bincode)?;
    let tx_hex = hex::encode(&tx_bytes);

    let rpc_result = rpc
        .send_transaction(&tx_hex)
        .await
        .map_err(|e| WalletError::Rpc(format!("faucet broadcast: {e}")))?;

    Ok(FaucetReceipt {
        tx_id_hex: tx_id.to_hex(),
        tx_hex,
        rpc_result,
        amount,
        fee,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn devnet_mnemonic_derives_consistently() {
        // Two derivations from the same constant must produce identical
        // spend public keys. Catches accidental drift in
        // `derive_spend_key`'s domain-separation string.
        let mnem = Mnemonic::from_phrase(DEVNET_TEST_MNEMONIC).unwrap();
        let seed = mnem.to_seed("").unwrap();
        let d = DefaultSeedDeriver::default_levels();
        let a = d.derive_spend_key(&seed, 0).unwrap();
        let b = d.derive_spend_key(&seed, 0).unwrap();
        assert_eq!(a.public.as_bytes(), b.public.as_bytes());
        // pk_hash also deterministic.
        let ha = qv_script::pubkey_hash(a.public.as_bytes());
        let hb = qv_script::pubkey_hash(b.public.as_bytes());
        assert_eq!(ha, hb);
    }

    #[tokio::test]
    async fn drip_rejects_zero_amount() {
        // No network call — drip() bails on the amount check first.
        let rpc = RpcClient::new("http://127.0.0.1:0");
        let err = drip(&rpc, "qvst1deadbeef", 0, 100).await.unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }

    #[tokio::test]
    async fn drip_rejects_overflow() {
        let rpc = RpcClient::new("http://127.0.0.1:0");
        let err = drip(&rpc, "qvst1deadbeef", u64::MAX, 1).await.unwrap_err();
        assert!(matches!(err, WalletError::InvalidArg(_)));
    }
}
