#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Create a test stealth key pair
    let view_key = qv_core::Hash256::from_bytes([1u8; 32]);
    let spend_key = qv_core::Hash256::from_bytes([2u8; 32]);

    // Construct a synthetic stealth output with the fuzz data as ephemeral key + ciphertext
    if data.len() >= 33 {
        let ephemeral_pk = &data[0..33];
        let ciphertext = &data[33..];

        let stealth_info = qv_core::StealthInfo {
            ephemeral_pubkey: ephemeral_pk.to_vec(),
            view_tag: if data.len() > 33 {
                (ciphertext[0] & 0x1F) as u8  // 5-bit tag
            } else {
                0
            },
        };

        // Try to scan the output
        let scan_result = qv_privacy::scan_output(
            &stealth_info,
            &view_key,
        );

        // Result should be deterministic (no panics)
        match scan_result {
            Ok(_matched) => {
                // If matched, recovery should succeed
                // (actual recovery depends on cryptographic implementation)
            }
            Err(_) => {
                // Most outputs will not match; errors are expected
            }
        }
    }
});
