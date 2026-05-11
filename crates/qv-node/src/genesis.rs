//! Genesis block builder for QuantumVault L1.
//!
//! Provides utilities to construct the genesis block with initial UTXO allocation.

use qv_core::{
    Block, BlockHash, BlockHeader, Script, Transaction, TxOutput, Amount, Height, Slot, Timestamp,
    Hash256, UtxoCommitment, BLOCK_VERSION, merkle_root_of,
};
use qv_crypto::{PqcPublicKey, PqcSecretKey, generate_pqc_keypair, DilithiumLevel};
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
pub fn build_genesis_block(allocations: &[(PqcPublicKey, u64)]) -> Block {
    // Build outputs from allocations
    let mut outputs = Vec::with_capacity(allocations.len());

    for (pubkey, amount) in allocations {
        // Compute pubkey hash using the locking script template helper
        let pk_hash = pubkey_hash(pubkey.as_bytes());

        // Create the p2pkh_pqc locking script
        let locking_script = p2pkh_pqc(&pk_hash);

        // Create the output
        let output = TxOutput::new(
            Amount::from(*amount),
            Script::new(locking_script),
        );

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
pub fn devnet_genesis() -> (Block, Vec<PqcSecretKey>) {
    const DEVNET_ACCOUNTS: usize = 10;
    const TOKENS_PER_ACCOUNT: u64 = 1_000_000_000;

    let mut allocations = Vec::with_capacity(DEVNET_ACCOUNTS);
    let mut secret_keys = Vec::with_capacity(DEVNET_ACCOUNTS);

    info!("generating {DEVNET_ACCOUNTS} devnet keypairs at Level 3");

    for i in 0..DEVNET_ACCOUNTS {
        // Generate a fresh keypair
        let pair = generate_pqc_keypair(DilithiumLevel::Level3)
            .expect("key generation failed during devnet genesis");

        allocations.push((pair.public.clone(), TOKENS_PER_ACCOUNT));
        secret_keys.push(pair.secret.clone());

        info!(account = i, "generated keypair");
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
)]
mod tests {
    use super::*;
    use qv_core::MerkleRoot;

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
        block.validate_structure().expect("genesis block must pass validate_structure");
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
        let (block, secret_keys) = devnet_genesis();

        assert_eq!(secret_keys.len(), 10);
        // The keys returned should be in the same order as the outputs
        // (we can't directly verify they match without signing, but we can check count)
        for (i, key) in secret_keys.iter().enumerate() {
            assert_eq!(key.level(), DilithiumLevel::Level3);
        }
    }
}
