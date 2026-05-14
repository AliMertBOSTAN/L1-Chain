//! Pool registration transaction builder.
//!
//! When an operator wants to register their pool on-chain, they must create
//! a special UTXO whose datum encodes the pool parameters (matching `StakePool`).

use crate::config::OperatorConfig;
use crate::keys::OperatorKeys;
use crate::{MinerError, MinerResult};
use qv_core::{Amount, Datum, OutPoint, Script, Transaction, TxId, TxInput, TxOutput};

/// Build a pool registration transaction.
///
/// This creates a UTXO containing the pool's registration parameters in its datum.
/// The pool parameters (VRF key, KES key, pledge, margin, fixed cost) are encoded
/// in the datum, which the ledger layer will interpret when processing the transaction.
///
/// # Parameters
/// - `config`: Operator configuration (pool_id, margin, fixed_cost, etc.).
/// - `vrf_key`: VRF public key bytes.
/// - `kes_key`: KES public key bytes.
/// - `operator_keys`: Operator's key pairs (for cold key signature).
///
/// # Returns
/// A transaction ready to be signed and broadcast.
pub fn build_pool_registration_tx(
    config: &OperatorConfig,
    vrf_key: &[u8],
    kes_key: &[u8],
    _operator_keys: &OperatorKeys,
) -> MinerResult<Transaction> {
    // Construct the registration datum.
    // In a real implementation, this would encode all pool parameters.
    let registration_datum = PoolRegistrationDatum {
        pool_id: config.pool_id.clone(),
        vrf_key: vrf_key.to_vec(),
        kes_key: kes_key.to_vec(),
        pledge: config.pledge,
        margin_bps: config.margin_bps,
        fixed_cost: config.fixed_cost,
        reward_account: config.reward_account.clone(),
    };

    // Serialize the datum to bytes.
    let datum_bytes = serde_json::to_vec(&registration_datum).map_err(|e| {
        MinerError::Serialization(format!("failed to serialize pool registration datum: {e}"))
    })?;

    // Create the registration output.
    // This is a special UTXO that locks the pool parameters.
    let registration_output = TxOutput {
        value: Amount::from_smallest_units(config.pledge),
        // `Script` and `Datum` expose `::new(Vec<u8>)` constructors
        // (no `From<Vec<u8>>` impl on the qv-core side).
        locking_script: Script::new(vec![]), // TODO: Script::standard_registration_lock()
        datum: Some(Datum::new(datum_bytes)),
        stealth_info: None,
    };

    // Build the transaction.
    // In a real implementation, this would:
    // 1. Select UTXOs from the operator's wallet to cover the pledge.
    // 2. Create a change output.
    // 3. Sign all inputs with the cold key.
    // For now, create a placeholder with a dummy input and the registration output.
    // `TxInput::new` initialises the witness to empty; `Transaction::new` sets
    // defaults for `validity_interval`, `lock_time`, and `fee`.
    let dummy_input = TxInput::new(OutPoint::new(TxId::from_bytes([0u8; 32]), 0));
    let tx = Transaction::new(vec![dummy_input], vec![registration_output]);

    Ok(tx)
}

/// Pool registration parameters encoded in a UTXO datum.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PoolRegistrationDatum {
    /// Pool identifier (unique within the network).
    pub pool_id: String,

    /// Operator's VRF public key.
    pub vrf_key: Vec<u8>,

    /// Operator's KES public key.
    pub kes_key: Vec<u8>,

    /// Operator pledge (amount staked by the operator).
    pub pledge: u64,

    /// Operator margin (basis points: 0-10000).
    pub margin_bps: u32,

    /// Fixed cost per epoch.
    pub fixed_cost: u64,

    /// Reward account address.
    pub reward_account: String,
}

/// Submit a pool registration transaction via node RPC.
pub async fn submit_via_rpc(_tx: &Transaction, _node_rpc_url: &str) -> MinerResult<String> {
    // Placeholder: in a real implementation, call the node's RPC method.
    // e.g., qv_submitTransaction or qv_broadcastTx
    // For now, return a dummy tx hash.
    Ok("txid_placeholder".to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::config::Network;

    fn sample_config() -> OperatorConfig {
        OperatorConfig {
            pool_id: "pool_test".to_string(),
            pool_name: "TestPool".to_string(),
            keystore_path: std::path::PathBuf::from("keys/operator.keystore"),
            pledge: 1_000_000_000,
            margin_bps: 300,
            fixed_cost: 10_000_000,
            reward_account: "qvaddr_test".to_string(),
            network: Network::Testnet,
            node_rpc_url: "http://localhost:8080".to_string(),
            node_gossip_addr: None,
            clear_mempool_capacity: None,
            encrypted_mempool_capacity: None,
            decryption_committee_share_path: None,
            genesis_time: None,
            kes_rotation_period_epochs: None,
        }
    }

    #[test]
    fn pool_registration_datum_serialization() {
        let datum = PoolRegistrationDatum {
            pool_id: "pool_test".to_string(),
            vrf_key: vec![1, 2, 3],
            kes_key: vec![4, 5, 6],
            pledge: 1_000_000_000,
            margin_bps: 300,
            fixed_cost: 10_000_000,
            reward_account: "qvaddr_test".to_string(),
        };

        let json = serde_json::to_string(&datum).unwrap();
        assert!(json.contains("pool_test"));
        assert!(json.contains("1000000000"));

        let deserialized: PoolRegistrationDatum = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pool_id, "pool_test");
        assert_eq!(deserialized.pledge, 1_000_000_000);
    }

    // C-04/C-06 closed via ADR-006 (ml-dsa swap). Test still ignored purely
    // because `OperatorKeys::generate()` runs the depth-11 KES leaf tree
    // (~2s); run via `cargo test -- --ignored` when KES path is the focus.
    #[test]
    #[ignore]
    fn build_pool_registration_tx_ok() {
        let config = sample_config();
        let keys = OperatorKeys::generate().unwrap();

        let tx = build_pool_registration_tx(
            &config,
            keys.vrf.public_bytes(),
            keys.kes.public_bytes(),
            &keys,
        );

        assert!(tx.is_ok());
        let tx = tx.unwrap();
        assert_eq!(tx.outputs.len(), 1);
        assert!(tx.outputs[0].datum.is_some());
    }

    #[test]
    #[ignore] // same slow-KES reason as `build_pool_registration_tx_ok` (post-ADR-006)
    fn registration_output_has_correct_value() {
        let config = sample_config();
        let keys = OperatorKeys::generate().unwrap();

        let tx = build_pool_registration_tx(
            &config,
            keys.vrf.public_bytes(),
            keys.kes.public_bytes(),
            &keys,
        )
        .unwrap();

        let output = &tx.outputs[0];
        assert_eq!(output.value, Amount::from_smallest_units(config.pledge));
    }
}
