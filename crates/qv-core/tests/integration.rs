//! Cross-module integration tests for `qv-core`.
//!
//! Unit tests inside each module cover that module in isolation. The tests
//! here exercise the *seams* between modules — an end-to-end path that a
//! real node would follow:
//!
//! 1. Build a [`Transaction`] with [`TxInput`] / [`TxOutput`].
//! 2. Derive its [`TxId`] via canonical bincode + SHA3-256.
//! 3. Assemble a [`Block`] whose `merkle_root` matches the body.
//! 4. Apply the block to an [`InMemoryUtxoSet`]: spend the inputs,
//!    insert the new outputs.
//! 5. Check that the resulting [`UtxoCommitment`] is deterministic
//!    across runs and independent of insertion order.
//! 6. Confirm [`ProtocolParams`] survives a TOML / JSON round trip.
//!
//! We also use `proptest` to assert arithmetic invariants on [`Amount`]
//! (associativity of `checked_add`, overflow detection) — these are the
//! kind of properties bugs usually slip past unit tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::needless_pass_by_value
)]

use proptest::prelude::*;

use qv_core::{
    merkle_root_of, Amount, Block, BlockError, BlockHeader, InMemoryUtxoSet, MerkleRoot, NetworkId,
    OutPoint, ProtocolParams, Script, Slot, Transaction, TxId, TxInput, TxOutput, UtxoCommitment,
    UtxoError, UtxoSet, ValidityInterval,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a transaction with a single funding-style input and `n` outputs.
fn tx_with_n_outputs(input_marker: u8, outputs: &[(u64, u8)]) -> Transaction {
    let txins = vec![TxInput::new(OutPoint::new(
        TxId::from_bytes([input_marker; 32]),
        0,
    ))];
    let txouts = outputs
        .iter()
        .map(|(value, script_byte)| {
            TxOutput::new(Amount::from(*value), Script::new(vec![*script_byte]))
        })
        .collect();
    Transaction::new(txins, txouts)
}

/// Wrap a set of transactions into a structurally valid block.
fn block_with(txs: Vec<Transaction>) -> Block {
    let ids: Vec<TxId> = txs.iter().map(|t| t.id().unwrap()).collect();
    let header = BlockHeader {
        merkle_root: merkle_root_of(&ids),
        ..BlockHeader::genesis_template()
    };
    Block::new(header, txs)
}

// ---------------------------------------------------------------------------
// End-to-end: transaction -> block -> utxo set
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_block_applies_to_utxo_set() {
    // Two transactions, each producing two fresh outputs.
    let tx_a = tx_with_n_outputs(0xA1, &[(100, 0x11), (200, 0x12)]);
    let tx_b = tx_with_n_outputs(0xB2, &[(300, 0x21), (400, 0x22)]);

    let block = block_with(vec![tx_a.clone(), tx_b.clone()]);
    block
        .validate_structure()
        .expect("structurally valid block");

    // Apply: insert every output of every tx into a fresh UTXO set.
    let mut utxo = InMemoryUtxoSet::new();
    for tx in &block.transactions {
        let txid = tx.id().unwrap();
        for (idx, out) in tx.outputs.iter().enumerate() {
            utxo.insert(OutPoint::new(txid, idx as u32), out.clone())
                .expect("fresh outpoint should insert cleanly");
        }
    }
    assert_eq!(utxo.len(), 4);

    // Round trip: commitment is stable and idempotent reads don't mutate it.
    let root_before = utxo.commitment_root();
    let root_again = utxo.commitment_root();
    assert_eq!(root_before, root_again);

    // Spend one of tx_a's outputs; commitment must change.
    let spent = OutPoint::new(tx_a.id().unwrap(), 0);
    utxo.remove(&spent).expect("exists");
    assert_eq!(utxo.len(), 3);
    assert_ne!(
        root_before,
        utxo.commitment_root(),
        "removing an entry must change the commitment"
    );
}

#[test]
fn commitment_is_independent_of_insertion_order() {
    let tx_a = tx_with_n_outputs(0xA1, &[(100, 0x11), (200, 0x12)]);
    let tx_b = tx_with_n_outputs(0xB2, &[(300, 0x21), (400, 0x22)]);

    let mut forward = InMemoryUtxoSet::new();
    for tx in [&tx_a, &tx_b] {
        let id = tx.id().unwrap();
        for (i, o) in tx.outputs.iter().enumerate() {
            forward
                .insert(OutPoint::new(id, i as u32), o.clone())
                .unwrap();
        }
    }

    let mut reverse = InMemoryUtxoSet::new();
    for tx in [&tx_b, &tx_a] {
        let id = tx.id().unwrap();
        // Also insert outputs in reverse index order.
        for (i, o) in tx.outputs.iter().enumerate().rev() {
            reverse
                .insert(OutPoint::new(id, i as u32), o.clone())
                .unwrap();
        }
    }

    assert_eq!(
        forward.commitment_root(),
        reverse.commitment_root(),
        "BTreeMap-backed commitment must be insertion-order agnostic"
    );
}

// ---------------------------------------------------------------------------
// Block / header / merkle consistency
// ---------------------------------------------------------------------------

#[test]
fn block_header_hash_depends_on_body_via_merkle_root() {
    // Two blocks with identical non-merkle header fields but different
    // transactions must produce different header hashes.
    let tx_a = tx_with_n_outputs(0x01, &[(10, 0x11)]);
    let tx_b = tx_with_n_outputs(0x02, &[(20, 0x22)]);

    let block_a = block_with(vec![tx_a]);
    let block_b = block_with(vec![tx_b]);

    let hash_a = block_a.hash().unwrap();
    let hash_b = block_b.hash().unwrap();
    assert_ne!(hash_a, hash_b);
}

#[test]
fn block_rejects_hand_crafted_duplicate_tx_even_with_valid_merkle() {
    // Attacker constructs a block with two identical txs and a matching
    // Merkle root — `validate_structure` must still reject because of
    // the CVE-2012-2459 mitigation.
    let tx = tx_with_n_outputs(0x01, &[(10, 0x11)]);
    let duplicated = vec![tx.clone(), tx.clone()];
    let ids: Vec<TxId> = duplicated.iter().map(|t| t.id().unwrap()).collect();

    let header = BlockHeader {
        merkle_root: merkle_root_of(&ids),
        ..BlockHeader::genesis_template()
    };
    let block = Block::new(header, duplicated);

    assert!(matches!(
        block.validate_structure(),
        Err(BlockError::DuplicateTx)
    ));
}

#[test]
fn empty_block_has_zero_merkle_root() {
    // Empty body must commit to MerkleRoot::ZERO — this is the convention
    // light clients rely on to detect the "no transactions" case without
    // fetching the body.
    let ids: Vec<TxId> = Vec::new();
    assert_eq!(merkle_root_of(&ids), MerkleRoot::ZERO);
}

// ---------------------------------------------------------------------------
// Transaction determinism
// ---------------------------------------------------------------------------

#[test]
fn txid_is_stable_across_canonical_bytes_roundtrip() {
    let tx = Transaction::new(
        vec![TxInput::new(OutPoint::new(TxId::from_bytes([7u8; 32]), 3))],
        vec![TxOutput::new(
            Amount::from(42),
            Script::new(vec![0xDE, 0xAD]),
        )],
    )
    .with_validity(ValidityInterval::at_or_after(Slot::from(10)))
    .with_fee(Amount::from(1));

    let id1 = tx.id().unwrap();

    // Encode, decode, re-hash. The round-tripped tx must produce the same id.
    let bytes = tx.canonical_bytes().unwrap();
    let decoded = Transaction::decode(&bytes).unwrap();
    assert_eq!(tx, decoded);
    assert_eq!(id1, decoded.id().unwrap());
}

#[test]
fn txid_is_sensitive_to_value_changes() {
    let tx = tx_with_n_outputs(0x01, &[(100, 0x11)]);
    let id_a = tx.id().unwrap();

    let mut tx2 = tx.clone();
    tx2.outputs[0].value = Amount::from(101);
    let id_b = tx2.id().unwrap();
    assert_ne!(id_a, id_b);
}

// ---------------------------------------------------------------------------
// UTXO commitment equals explicit helper on sorted entries
// ---------------------------------------------------------------------------

#[test]
fn inmemory_commitment_matches_helper_on_sorted_iter() {
    let mut set = InMemoryUtxoSet::new();
    set.insert(
        OutPoint::new(TxId::from_bytes([3u8; 32]), 0),
        TxOutput::new(Amount::from(30), Script::new(vec![0x33])),
    )
    .unwrap();
    set.insert(
        OutPoint::new(TxId::from_bytes([1u8; 32]), 0),
        TxOutput::new(Amount::from(10), Script::new(vec![0x11])),
    )
    .unwrap();
    set.insert(
        OutPoint::new(TxId::from_bytes([2u8; 32]), 0),
        TxOutput::new(Amount::from(20), Script::new(vec![0x22])),
    )
    .unwrap();

    let from_trait = set.commitment_root();
    let from_helper: UtxoCommitment = qv_core::commitment_root_of_sorted_entries(set.iter());
    assert_eq!(from_trait, from_helper);
}

#[test]
fn spending_twice_returns_notfound() {
    let mut set = InMemoryUtxoSet::new();
    let op = OutPoint::new(TxId::from_bytes([1u8; 32]), 0);
    set.insert(op, TxOutput::new(Amount::from(10), Script::default()))
        .unwrap();
    set.remove(&op).unwrap();
    assert_eq!(set.remove(&op), Err(UtxoError::NotFound));
}

// ---------------------------------------------------------------------------
// Protocol params round trip
// ---------------------------------------------------------------------------

#[test]
fn protocol_params_toml_roundtrip() {
    let p = ProtocolParams::mainnet();
    let toml = p.to_toml().unwrap();
    let back = ProtocolParams::from_toml(&toml).unwrap();
    assert_eq!(p, back);
    assert_eq!(back.network, NetworkId::Mainnet);
}

#[test]
fn protocol_params_json_roundtrip() {
    let p = ProtocolParams::testnet();
    let json = p.to_json().unwrap();
    let back = ProtocolParams::from_json(&json).unwrap();
    assert_eq!(p, back);
    assert_eq!(back.network, NetworkId::Testnet);
}

#[test]
fn ephemeral_params_validate() {
    ProtocolParams::ephemeral().validate().unwrap();
}

// ---------------------------------------------------------------------------
// Property-based: Amount arithmetic
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    /// `checked_add` is associative on values that don't overflow.
    #[test]
    fn amount_checked_add_is_associative(a in 0u64..1_000_000, b in 0u64..1_000_000, c in 0u64..1_000_000) {
        let x = Amount::from(a);
        let y = Amount::from(b);
        let z = Amount::from(c);

        let lhs = x.checked_add(y).and_then(|v| v.checked_add(z));
        let rhs = y.checked_add(z).and_then(|v| x.checked_add(v));
        prop_assert_eq!(lhs, rhs);
    }

    /// `checked_sum` over a vector equals folding `checked_add`.
    #[test]
    fn amount_checked_sum_matches_fold(values in proptest::collection::vec(0u64..1_000_000, 0..32)) {
        let amounts: Vec<Amount> = values.iter().copied().map(Amount::from).collect();
        let via_sum = Amount::checked_sum(amounts.iter().copied());
        let via_fold = amounts.iter().copied().try_fold(Amount::ZERO, |acc, v| acc.checked_add(v));
        prop_assert_eq!(via_sum, via_fold);
    }

    /// Overflow is never silently wrapped.
    #[test]
    fn amount_overflow_returns_none(offset in 1u64..1_000_000) {
        let near_max = Amount::from(u64::MAX - offset + 1);
        let spill = Amount::from(offset);
        // u64::MAX - offset + 1 + offset = u64::MAX + 1 → overflow
        prop_assert_eq!(near_max.checked_add(spill), None);
    }
}

// ---------------------------------------------------------------------------
// Property-based: Merkle is deterministic on any shape of input
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    #[test]
    fn merkle_is_deterministic_regardless_of_length(leaves in proptest::collection::vec(any::<[u8; 32]>(), 0..64)) {
        let ids: Vec<TxId> = leaves.into_iter().map(TxId::from_bytes).collect();
        let r1 = merkle_root_of(&ids);
        let r2 = merkle_root_of(&ids);
        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn merkle_is_sensitive_to_permutation(
        mut leaves in proptest::collection::vec(any::<[u8; 32]>(), 2..16)
    ) {
        // Guarantee at least two distinct leaves so the permutation changes something.
        if leaves[0] == leaves[1] {
            leaves[1][0] ^= 1;
        }
        let original: Vec<TxId> = leaves.iter().copied().map(TxId::from_bytes).collect();
        let mut swapped = original.clone();
        swapped.swap(0, 1);
        prop_assert_ne!(merkle_root_of(&original), merkle_root_of(&swapped));
    }
}
