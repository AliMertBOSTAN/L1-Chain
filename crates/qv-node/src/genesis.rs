//! Genesis block builder for QuantumVault L1.
//!
//! Provides utilities to construct the genesis block with initial UTXO allocation.

use qv_core::{
    merkle_root_of, Amount, Block, BlockHash, BlockHeader, Hash256, Height, Script, Slot,
    Timestamp, Transaction, TxOutput, UtxoCommitment, BLOCK_VERSION,
};
use qv_crypto::{from_seed_pqc, sha3_256, DilithiumLevel, PqcPublicKey, PqcSecretKey};
use qv_script::templates::{p2pkh_pqc, pubkey_hash};
use tracing::info;

/// Build a genesis block with specified initial UTXO allocations.
///
/// The genesis transaction has no inputs. Each allocation is converted to an
/// output locked with a `p2pkh_pqc` script for the given public key.
///
/// # Arguments
///
/// * `allocations` - List of (public key, amount in smallest units) pairs
///
/// # Returns
///
/// A genesis `Block` with the specified allocations and a template header.
///
/// # Panics
///
/// Panics if the genesis transaction id cannot be computed (impossible in
/// practice; the id derives from a fully-constructed transaction with no
/// failure mode in the current implementation).
#[allow(clippy::expect_used)] // SAFETY: genesis tx id is infallible by construction
pub fn build_genesis_block(allocations: &[(PqcPublicKey, u64)]) -> Block {
    // Build outputs from allocations
    let mut outputs = Vec::with_capacity(allocations.len());

    for (pubkey, amount) in allocations {
        // Compute pubkey hash using the locking script template helper
        let pk_hash = pubkey_hash(pubkey.as_bytes());

        // Create the p2pkh_pqc locking script
        let locking_script = p2pkh_pqc(&pk_hash);

        // Create the output
        let output = TxOutput::new(Amount::from(*amount), Script::new(locking_script));

        outputs.push(output);
    }

    // Genesis transaction: no inputs, outputs from allocations
    let genesis_tx = Transaction::genesis(outputs);

    // Compute merkle root from the genesis transaction ID
    let tx_id = genesis_tx
        .id()
        .expect("genesis transaction must produce a valid id");
    let merkle = merkle_root_of(&[tx_id]);

    // Genesis header — prev_hash and utxo_commitment are zero,
    // but merkle_root is computed from the actual transaction.
    let header = BlockHeader {
        version: BLOCK_VERSION,
        prev_hash: BlockHash::ZERO,
        height: Height::GENESIS,
        slot: Slot::GENESIS,
        timestamp: Timestamp::from_unix_secs(0),
        merkle_root: merkle,
        utxo_commitment: UtxoCommitment::ZERO,
        vrf_proof: Vec::new(),
        kes_sig: Vec::new(),
        producer_key_hash: Hash256::ZERO,
    };

    Block::new(header, vec![genesis_tx])
}

/// Generate a devnet genesis block with 10 allocated keypairs.
///
/// This convenience function generates 10 fresh Dilithium keypairs at Level 3,
/// each allocated 1_000_000_000 tokens (smallest units), and returns both the
/// genesis block and the secret keys for wallet/signing use.
///
/// # Returns
///
/// A tuple of:
/// - The genesis `Block` with 10 outputs
/// - A `Vec` of the corresponding `PqcSecretKey`s in the same order
///
/// # Panics
///
/// Panics if key generation fails (should be extremely rare with proper entropy).
#[allow(clippy::expect_used)] // SAFETY: deterministic devnet keygen — failure is unreachable
pub fn devnet_genesis() -> (Block, Vec<PqcSecretKey>) {
    const DEVNET_ACCOUNTS: usize = 10;
    const TOKENS_PER_ACCOUNT: u64 = 1_000_000_000;

    let mut allocations = Vec::with_capacity(DEVNET_ACCOUNTS);
    let mut secret_keys = Vec::with_capacity(DEVNET_ACCOUNTS);

    info!("deriving {DEVNET_ACCOUNTS} devnet keypairs at Level 3 from deterministic seeds");

    // **Deterministic seeded keygen** (FIPS 204 §6.1 KeyGen_internal via ADR-006).
    //
    // Each account's seed is `SHA3-256("qv-devnet-account-" || index_byte)`. This
    // means `qv-node init` and `qv-node run` and `examples/send_tx.rs` all
    // converge on the same genesis block, so wallets persist across restarts
    // and the smoke test loop is reproducible.
    //
    // For production networks this is replaced by the mainnet genesis ceremony
    // (envanter N-05); these fixed devnet keys are intentionally **public** —
    // they exist only in `archive/cpp-v1/`-grade testnet scenarios.
    for i in 0..DEVNET_ACCOUNTS {
        let mut preimage = Vec::with_capacity(32);
        preimage.extend_from_slice(b"qv-devnet-account-");
        preimage.push(i as u8);
        let seed = sha3_256(&preimage);

        let pair = from_seed_pqc(DilithiumLevel::Level3, &seed)
            .expect("from_seed_pqc must succeed for deterministic devnet keypair");

        allocations.push((pair.public.clone(), TOKENS_PER_ACCOUNT));
        secret_keys.push(pair.secret.clone());

        info!(account = i, "derived keypair from seed");
    }

    let genesis_block = build_genesis_block(&allocations);

    info!(
        block_hash = ?genesis_block.header,
        tx_count = genesis_block.transactions.len(),
        "devnet genesis block created"
    );

    (genesis_block, secret_keys)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use qv_core::MerkleRoot;
    // Tests still use `generate_pqc_keypair` for the small `build_genesis_block`
    // unit tests where determinism doesn't matter; the production `devnet_genesis`
    // path uses `from_seed_pqc` for reproducible smoke tests (see ADR-006).
    use qv_crypto::generate_pqc_keypair;

    #[test]
    fn genesis_block_has_no_inputs() {
        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let block = build_genesis_block(&[(kp.public, 1000)]);

        assert_eq!(block.transactions.len(), 1);
        let genesis_tx = &block.transactions[0];
        assert!(genesis_tx.inputs.is_empty());
        assert_eq!(genesis_tx.outputs.len(), 1);
    }

    #[test]
    fn genesis_block_creates_p2pkh_outputs() {
        let kp1 = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let kp2 = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();

        let block = build_genesis_block(&[(kp1.public, 1000), (kp2.public, 2000)]);

        let genesis_tx = &block.transactions[0];
        assert_eq!(genesis_tx.outputs.len(), 2);

        // Check that outputs have the right amounts
        assert_eq!(genesis_tx.outputs[0].value.as_u64(), 1000);
        assert_eq!(genesis_tx.outputs[1].value.as_u64(), 2000);

        // Check that outputs have non-empty locking scripts
        assert!(!genesis_tx.outputs[0].locking_script.is_empty());
        assert!(!genesis_tx.outputs[1].locking_script.is_empty());
    }

    #[test]
    fn genesis_block_header_is_correct() {
        let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
        let block = build_genesis_block(&[(kp.public, 1000)]);

        assert_eq!(block.header.version, BLOCK_VERSION);
        assert_eq!(block.header.height, Height::GENESIS);
        assert_eq!(block.header.slot, Slot::GENESIS);
        assert_eq!(block.header.prev_hash, BlockHash::ZERO);
        // Merkle root must NOT be zero — it's computed from the genesis tx
        assert_ne!(block.header.merkle_root, MerkleRoot::ZERO);
        assert_eq!(block.header.utxo_commitment, UtxoCommitment::ZERO);
        assert!(block.header.vrf_proof.is_empty());
        assert!(block.header.kes_sig.is_empty());
        assert_eq!(block.header.producer_key_hash, Hash256::ZERO);

        // Validate that the block passes structural validation
        block
            .validate_structure()
            .expect("genesis block must pass validate_structure");
    }

    #[test]
    fn devnet_genesis_creates_10_accounts() {
        let (block, secret_keys) = devnet_genesis();

        assert_eq!(secret_keys.len(), 10);
        assert_eq!(block.transactions.len(), 1);

        let genesis_tx = &block.transactions[0];
        assert_eq!(genesis_tx.outputs.len(), 10);

        // Each output should have 1_000_000_000 tokens
        for output in &genesis_tx.outputs {
            assert_eq!(output.value.as_u64(), 1_000_000_000);
        }
    }

    #[test]
    fn devnet_genesis_keys_match_allocations() {
        let (_block, secret_keys) = devnet_genesis();

        assert_eq!(secret_keys.len(), 10);
        // The keys returned should be in the same order as the outputs
        // (we can't directly verify they match without signing, but we can check count)
        for key in secret_keys.iter() {
            assert_eq!(key.level(), DilithiumLevel::Level3);
        }
    }

    /// Determinism invariant — critical for devnet smoke test reproducibility.
    ///
    /// `qv-node init` writes `genesis-keys.json` based on `devnet_genesis()`, then
    /// `qv-node run` re-derives the genesis block via the same call inside
    /// `Node::new`. Without determinism, the two would produce different chains
    /// and the wallet's secret keys would not match any on-chain UTXO. With the
    /// ADR-006 `from_seed_pqc` swap, this test pins the deterministic contract.
    #[test]
    fn devnet_genesis_is_deterministic() {
        let (block_a, sks_a) = devnet_genesis();
        let (block_b, sks_b) = devnet_genesis();

        // Same block hash, same merkle root, same tx id sequence.
        assert_eq!(block_a.header.merkle_root, block_b.header.merkle_root);
        assert_eq!(block_a.transactions.len(), block_b.transactions.len());
        let tx_a = &block_a.transactions[0];
        let tx_b = &block_b.transactions[0];
        assert_eq!(
            tx_a.id().expect("tx id"),
            tx_b.id().expect("tx id"),
            "two devnet_genesis() calls must produce identical txid"
        );

        // Same secret key bytes (sensitive — but determinism requires it).
        assert_eq!(sks_a.len(), sks_b.len());
        for (i, (a, b)) in sks_a.iter().zip(sks_b.iter()).enumerate() {
            assert_eq!(
                a.expose_secret(),
                b.expose_secret(),
                "account {i} secret key must be deterministic"
            );
        }
    }
}
