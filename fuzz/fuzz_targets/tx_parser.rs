#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Parse transaction from arbitrary bytes
    if let Ok(tx) = bincode::deserialize::<qv_core::Transaction>(data) {
        // Validate structure
        let _ = tx.validate_structure();

        // Roundtrip: encode → decode → should produce identical bytes
        if let Ok(encoded) = bincode::serialize(&tx) {
            // Verify roundtrip consistency
            if let Ok(tx2) = bincode::deserialize::<qv_core::Transaction>(&encoded) {
                // Both transactions should have same ID
                assert_eq!(tx.id(), tx2.id());
                // Encoded bytes should be stable
                if let Ok(encoded2) = bincode::serialize(&tx2) {
                    assert_eq!(encoded, encoded2);
                }
            }
        }
    }
    // Errors are expected; just ensure no panics
});
