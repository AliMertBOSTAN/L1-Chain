#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Limit message size to 4 MiB
    if data.len() > 4 * 1024 * 1024 {
        return;
    }

    // Try to decode envelope
    if let Ok(_envelope) = bincode::deserialize::<qv_net::Envelope>(data) {
        // If it decoded, version should be valid
        // (version checking happens inside Envelope::decode)
    }
    // Errors are expected; just ensure no panics
});
