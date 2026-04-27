#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try to deserialize a block
    if let Ok(block) = bincode::deserialize::<qv_core::Block>(data) {
        // Create in-memory UTXO set
        let mut utxo_set = qv_core::InMemoryUtxoSet::new();

        // Record initial commitment root
        let initial_root = utxo_set.commitment_root();

        // Try to apply block
        if utxo_set.apply_block(&block).is_ok() {
            // Get commitment after apply
            let applied_root = utxo_set.commitment_root();
            assert_ne!(initial_root, applied_root, "Commitment should change after apply");

            // Revert the block
            if utxo_set.revert_block(&block).is_ok() {
                // After revert, commitment should match initial
                let reverted_root = utxo_set.commitment_root();
                assert_eq!(initial_root, reverted_root,
                    "Commitment should match initial after revert");
            }
        }
    }
    // Errors are expected; just ensure no panics and consistency
});
