//! End-to-end integration test: full lifecycle of a transfer transaction.
//!
//! This test proves that a transaction works through the entire pipeline:
//! 1. Generate keypairs for Alice and Bob
//! 2. Create a genesis block allocating funds to Alice
//! 3. Apply genesis to a fresh UTXO store
//! 4. Build a transfer TX: Alice → Bob + change
//! 5. Sign the TX with Alice's key
//! 6. Validate the TX through the full validation pipeline
//! 7. Build a block containing the TX
//! 8. Apply the block to the UTXO store
//! 9. Verify final state: Alice's original UTXO consumed, Bob and Alice change UTXOs created

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use qv_core::{
    Amount, Block, BlockHash, BlockHeader, Hash256, Height, MerkleRoot, OutPoint, Script, Slot,
    Timestamp, TxInput, TxOutput, UtxoCommitment, ValidityInterval, BLOCK_VERSION,
};
use qv_crypto::{generate_pqc_keypair, DilithiumLevel};
use qv_node::genesis::build_genesis_block;
use qv_node::validation::validate_transaction;
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use qv_storage::kv::MemoryKvStore;
use qv_storage::utxo_store::UtxoStore;
use qv_wallet::tx_builder::TxBuilder;

/// Helper to create a simple block header for testing.
fn make_block_header(
    prev_hash: BlockHash,
    height: Height,
    slot: Slot,
    merkle_root: MerkleRoot,
) -> BlockHeader {
    BlockHeader {
        version: BLOCK_VERSION,
        prev_hash,
        height,
        slot,
        timestamp: Timestamp::from_unix_secs(slot.as_u64()),
        merkle_root,
        utxo_commitment: UtxoCommitment::ZERO,
        vrf_proof: vec![],
        kes_sig: vec![],
        producer_key_hash: Hash256::ZERO,
    }
}

#[tokio::test]
async fn test_transfer_e2e() {
    // ========================================================================
    // Step 1: Generate keypairs for Alice and Bob
    // ========================================================================
    let alice_keypair = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
    let bob_keypair = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();

    let alice_sk = alice_keypair.secret.clone();
    let alice_pk = alice_keypair.public.clone();
    let bob_pk = bob_keypair.public.clone();

    // ========================================================================
    // Step 2: Create genesis block with Alice receiving 1_000_000_000 units
    // ========================================================================
    let genesis = build_genesis_block(&[(alice_pk.clone(), 1_000_000_000)]);

    // Verify genesis structure
    assert_eq!(genesis.transactions.len(), 1);
    let genesis_tx = &genesis.transactions[0];
    assert!(
        genesis_tx.inputs.is_empty(),
        "genesis tx must have no inputs"
    );
    assert_eq!(
        genesis_tx.outputs.len(),
        1,
        "genesis tx must have exactly 1 output (for Alice)"
    );
    assert_eq!(
        genesis_tx.outputs[0].value.as_u64(),
        1_000_000_000,
        "Alice's genesis output must have 1_000_000_000 units"
    );

    // ========================================================================
    // Step 3: Apply genesis block to a fresh UTXO store
    // ========================================================================
    let utxo_store = UtxoStore::new(MemoryKvStore::new());
    utxo_store.apply_block(&genesis).unwrap();

    // Verify Alice's UTXO exists and has the correct amount
    let genesis_tx_id = genesis_tx.id().unwrap();
    let alice_genesis_outpoint = OutPoint::new(genesis_tx_id.clone(), 0);
    let alice_utxo = utxo_store
        .get(&alice_genesis_outpoint)
        .unwrap()
        .expect("Alice's genesis UTXO must exist");
    assert_eq!(
        alice_utxo.value.as_u64(),
        1_000_000_000,
        "Alice's UTXO value must match genesis allocation"
    );

    // ========================================================================
    // Step 4: Build a transfer TX
    // ========================================================================
    // Transfer spec:
    //   Input:  Alice's 1_000_000_000 units
    //   Output 0: Bob receives 500_000_000 units
    //   Output 1: Alice change receives 499_999_000 units
    //   Fee: 1_000 units
    //   Total: 500_000_000 + 499_999_000 + 1_000 = 1_000_000_000 ✓

    // Create locking script for Bob
    let bob_pubkey_hash = pubkey_hash(bob_pk.as_bytes());
    let bob_locking_script = Script::new(p2pkh_pqc(&bob_pubkey_hash));

    // Create locking script for Alice change
    let alice_pubkey_hash = pubkey_hash(alice_pk.as_bytes());
    let alice_change_script = Script::new(p2pkh_pqc(&alice_pubkey_hash));

    // Build the transaction
    let mut builder = TxBuilder::new(ValidityInterval::UNBOUNDED);
    builder.add_input(TxInput::new(alice_genesis_outpoint.clone()));
    builder.add_output(TxOutput::new(Amount::from(500_000_000), bob_locking_script));
    builder.add_output(TxOutput::new(
        Amount::from(499_999_000),
        alice_change_script,
    ));

    // ========================================================================
    // Step 5: Sign the TX with Alice's keypair
    // ========================================================================
    builder.sign_with(&alice_sk, &alice_pk).unwrap();

    // Build the signed transaction
    let transfer_tx = builder.build_unsigned().unwrap();

    // Verify the transaction structure
    assert_eq!(transfer_tx.inputs.len(), 1, "transfer tx must have 1 input");
    assert_eq!(
        transfer_tx.outputs.len(),
        2,
        "transfer tx must have 2 outputs (Bob + Alice change)"
    );
    assert!(
        !transfer_tx.inputs[0].witness.is_empty(),
        "witness must be populated"
    );

    // ========================================================================
    // Step 6: Validate the TX through the full validation pipeline
    // ========================================================================
    let validated = validate_transaction(
        &transfer_tx,
        &utxo_store,
        Slot::from(1),
        1, // min_fee_rate = 1 unit
    )
    .unwrap();

    // Verify validation result
    assert_eq!(
        validated.fee, 1_000,
        "fee calculation: 1_000_000_000 - 500_000_000 - 499_999_000 = 1_000"
    );
    assert_eq!(
        validated.resolved_inputs.len(),
        1,
        "exactly 1 input resolved"
    );
    assert_eq!(
        validated.resolved_inputs[0].value.as_u64(),
        1_000_000_000,
        "resolved input matches Alice's genesis UTXO"
    );

    // ========================================================================
    // Step 7: Build a block containing the TX
    // ========================================================================
    let transfer_tx_id = transfer_tx.id().unwrap();
    let transfer_ids = vec![transfer_tx_id.clone()];
    let transfer_merkle_root = qv_core::merkle_root_of(&transfer_ids);

    let block_header = make_block_header(
        genesis.hash().unwrap(),
        Height::from(1),
        Slot::from(1),
        transfer_merkle_root,
    );

    let block = Block::new(block_header, vec![transfer_tx.clone()]);

    // Verify block structure
    block.validate_structure().unwrap();

    // ========================================================================
    // Step 8: Apply the block to the UTXO store
    // ========================================================================
    utxo_store.apply_block(&block).unwrap();

    // ========================================================================
    // Step 9: Verify final state
    // ========================================================================

    // 9a. Alice's original UTXO must be spent (removed)
    let alice_original_gone = utxo_store.get(&alice_genesis_outpoint).unwrap().is_none();
    assert!(
        alice_original_gone,
        "Alice's original genesis UTXO must be consumed"
    );

    // 9b. Bob's new UTXO must exist with correct amount
    let bob_outpoint = OutPoint::new(transfer_tx_id.clone(), 0);
    let bob_utxo = utxo_store
        .get(&bob_outpoint)
        .unwrap()
        .expect("Bob's UTXO must exist");
    assert_eq!(
        bob_utxo.value.as_u64(),
        500_000_000,
        "Bob received exactly 500_000_000 units"
    );

    // 9c. Alice's change UTXO must exist with correct amount
    let alice_change_outpoint = OutPoint::new(transfer_tx_id.clone(), 1);
    let alice_change_utxo = utxo_store
        .get(&alice_change_outpoint)
        .unwrap()
        .expect("Alice's change UTXO must exist");
    assert_eq!(
        alice_change_utxo.value.as_u64(),
        499_999_000,
        "Alice's change is 499_999_000 units (1_000_000_000 - 500_000_000 - 1_000 fee)"
    );

    // ========================================================================
    // Step 10: Sanity checks on final UTXO set
    // ========================================================================

    // Total value in system must equal genesis (conservation of value)
    let total_value: u64 = vec![bob_utxo.value, alice_change_utxo.value]
        .iter()
        .map(|a| a.as_u64())
        .sum();
    assert_eq!(
        total_value, 999_999_000,
        "total value after transfer is 999_999_000 (fee of 1_000 removed from system)"
    );

    // UTXO store should have exactly 2 entries
    let all_entries = utxo_store.entries().unwrap();
    assert_eq!(
        all_entries.len(),
        2,
        "UTXO store must have exactly 2 entries: Bob + Alice change"
    );

    println!(
        "✓ End-to-end transfer test passed: Alice → Bob transfer with change, full validation pipeline"
    );
}
