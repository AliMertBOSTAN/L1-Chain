#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Limit script size to 16KB
    if data.len() > 16 * 1024 {
        return;
    }

    // Try to decode script from bytes
    if let Ok(instructions) = qv_script::decode_script(data) {
        // Create a minimal context for execution
        let tx = qv_core::Transaction {
            inputs: vec![],
            outputs: vec![],
            validity_interval: qv_core::ValidityInterval::unbounded(),
            witness: vec![],
        };

        let context = qv_script::Context {
            tx: &tx,
            resolved_inputs: &vec![],
            current_slot: qv_core::Slot::from(0u64),
            tx_hash: qv_core::Hash256::ZERO,
        };

        // Execute with default gas limit
        let result = qv_script::execute(&instructions, &context);

        // Result should be valid (success or failure, but not crash)
        match result {
            Ok(exec_result) => {
                // Gas should be bounded
                assert!(exec_result.gas_used <= qv_script::DEFAULT_GAS_LIMIT);
            }
            Err(_) => {
                // Errors are expected; just ensure no panics
            }
        }
    }
});
