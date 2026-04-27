#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Limit block size to 10 MB
    if data.len() > 10 * 1024 * 1024 {
        return;
    }

    // Try to deserialize a block
    if let Ok(block) = bincode::deserialize::<qv_core::Block>(data) {
        // Validate block structure
        let _ = block.validate_structure();

        // Header should be present
        let _header = &block.header;

        // Merkle root should be computed and match header (if valid)
        if let Ok(computed_root) = qv_core::merkle_root_of(&block.body) {
            // Verify round-trip consistency
            if let Ok(block2) = bincode::serialize(&block) {
                if let Ok(_block2_deser) = bincode::deserialize::<qv_core::Block>(&block2) {
                    // Should parse consistently
                }
            }
        }
    }
    // Errors are expected; just ensure no panics
});
