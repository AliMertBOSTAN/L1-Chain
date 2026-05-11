//! Black-box integration tests for `qv-crypto`.
//!
//! These exercise the public crate API across modules. Per-module unit
//! tests live inside `src/*.rs`; this file focuses on:
//! - cross-module consistency,
//! - property-based invariants (via `proptest`),
//! - realistic end-to-end scenarios.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use proptest::prelude::*;

use qv_crypto::{
    decapsulate_hybrid, encapsulate_hybrid, from_seed_pqc, generate_hybrid_keypair,
    generate_pqc_keypair, sha3_256, sign_pqc, verify_pqc, DilithiumLevel, HashAlgorithm, Hasher,
    KyberLevel, PqcKeyPair, SecureBytes,
};

// ---------------------------------------------------------------------------
// Hash properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn sha3_is_deterministic(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let a = sha3_256(&data);
        let b = sha3_256(&data);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn streaming_equivalent_to_oneshot(
        data in proptest::collection::vec(any::<u8>(), 0..2048),
        split in 0usize..2048,
    ) {
        let split = split.min(data.len());
        let one_shot = sha3_256(&data);

        let mut h = Hasher::new(HashAlgorithm::Sha3_256);
        h.update(&data[..split]);
        h.update(&data[split..]);
        let streamed = h.finalize();
        prop_assert_eq!(one_shot, streamed);
    }

    #[test]
    fn distinct_inputs_almost_always_distinct_hashes(
        a in proptest::collection::vec(any::<u8>(), 1..256),
        b in proptest::collection::vec(any::<u8>(), 1..256),
    ) {
        if a != b {
            prop_assert_ne!(sha3_256(&a), sha3_256(&b));
        }
    }
}

// ---------------------------------------------------------------------------
// PQC signatures
// ---------------------------------------------------------------------------

#[test]
fn signature_roundtrip_preserves_identity() {
    let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
    let msg = b"QuantumVault proto-block header commitment";
    let sig = sign_pqc(&kp.secret, msg).unwrap();
    assert!(verify_pqc(&kp.public, msg, &sig).unwrap());
    assert_eq!(sig.level(), DilithiumLevel::Level3);
}

#[test]
fn signature_is_bound_to_message() {
    let kp = generate_pqc_keypair(DilithiumLevel::Level3).unwrap();
    let sig = sign_pqc(&kp.secret, b"message-a").unwrap();
    assert!(!verify_pqc(&kp.public, b"message-b", &sig).unwrap());
}

proptest! {
    #[test]
    fn any_valid_message_roundtrips(msg in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let kp = generate_pqc_keypair(DilithiumLevel::Level2).unwrap();
        let sig = sign_pqc(&kp.secret, &msg).unwrap();
        prop_assert!(verify_pqc(&kp.public, &msg, &sig).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Seeded keygen workflow tests — `from_seed_pqc` (envanter ID C-04 + C-06,
// CLOSED 2026-05-07 via ADR-006: full ml-dsa swap).
// ---------------------------------------------------------------------------
//
// These tests exercise the production use-cases that closing C-04 unblocked
// (HD wallet derivation, KES leaf-key derivation per ADR-005, stealth
// one-time spend key recovery). With the ml-dsa swap landed they run by default.

#[test]
fn from_seed_models_hd_derivation_pattern() {
    let mnemonic_seed = [0xABu8; 32];

    let mut derive_account = |idx: u32| -> PqcKeyPair {
        let mut input = Vec::with_capacity(40);
        input.extend_from_slice(b"QuantumVault-Spend-v1");
        input.extend_from_slice(&mnemonic_seed);
        input.extend_from_slice(&idx.to_le_bytes());
        let child_seed = sha3_256(&input);
        from_seed_pqc(DilithiumLevel::Level3, &child_seed).unwrap()
    };

    let acct_0 = derive_account(0);
    let acct_0_again = derive_account(0);
    let acct_1 = derive_account(1);

    assert_eq!(acct_0.public.as_bytes(), acct_0_again.public.as_bytes());
    assert_ne!(acct_0.public.as_bytes(), acct_1.public.as_bytes());

    let sig = sign_pqc(&acct_0.secret, b"acct 0 tx").unwrap();
    assert!(verify_pqc(&acct_0.public, b"acct 0 tx", &sig).unwrap());
}

#[test]
fn from_seed_models_kes_leaf_derivation_pattern() {
    let kes_master_seed = [0xCDu8; 32];

    let mut leaf_for_period = |period: u32| -> PqcKeyPair {
        let mut input = Vec::with_capacity(48);
        input.extend_from_slice(b"QuantumVault-KES-leaf-v1");
        input.extend_from_slice(&kes_master_seed);
        input.extend_from_slice(&period.to_le_bytes());
        let leaf_seed = sha3_256(&input);
        from_seed_pqc(DilithiumLevel::Level3, &leaf_seed).unwrap()
    };

    let leaf_0 = leaf_for_period(0);
    let leaf_1 = leaf_for_period(1);
    let leaf_2 = leaf_for_period(2);

    assert_ne!(leaf_0.public.as_bytes(), leaf_1.public.as_bytes());
    assert_ne!(leaf_1.public.as_bytes(), leaf_2.public.as_bytes());
    assert_ne!(leaf_0.public.as_bytes(), leaf_2.public.as_bytes());

    for (period, leaf) in [(0u32, &leaf_0), (1, &leaf_1), (2, &leaf_2)] {
        let mut bound = Vec::new();
        bound.extend_from_slice(b"period:");
        bound.extend_from_slice(&period.to_le_bytes());
        bound.extend_from_slice(b"|msg");
        let sig = sign_pqc(&leaf.secret, &bound).unwrap();
        assert!(verify_pqc(&leaf.public, &bound, &sig).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Hybrid KEM
// ---------------------------------------------------------------------------

#[test]
fn hybrid_kem_sender_receiver_agree() {
    let recipient = generate_hybrid_keypair(KyberLevel::Level3).unwrap();
    let (ct, ss_sender) = encapsulate_hybrid(&recipient.public).unwrap();
    let ss_receiver = decapsulate_hybrid(&recipient, &ct).unwrap();
    assert_eq!(ss_sender, ss_receiver);
}

#[test]
fn hybrid_kem_two_independent_encapsulations_differ() {
    // Encapsulating twice to the same recipient must produce distinct
    // ciphertexts and distinct secrets (ephemeral X25519 key rotates).
    let recipient = generate_hybrid_keypair(KyberLevel::Level3).unwrap();
    let (ct1, ss1) = encapsulate_hybrid(&recipient.public).unwrap();
    let (ct2, ss2) = encapsulate_hybrid(&recipient.public).unwrap();
    assert_ne!(ct1.bytes, ct2.bytes);
    assert_ne!(ss1.as_bytes(), ss2.as_bytes());
}

// ---------------------------------------------------------------------------
// SecureBytes integration
// ---------------------------------------------------------------------------

#[test]
fn secure_bytes_does_not_print_content_in_debug() {
    let sb = SecureBytes::from_slice(b"ssh-rsa AAAA... dangerous secret");
    let rendered = format!("{sb:?}");
    assert!(!rendered.contains("dangerous"));
    assert!(!rendered.contains("ssh-rsa"));
}
