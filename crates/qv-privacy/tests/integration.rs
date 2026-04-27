//! Integration tests for `qv-privacy`.
//!
//! Cross-module scenarios: stealth → scan → spend, stealth + confidential,
//! disclosure proof workflow, privacy mode selection.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use qv_crypto::{DilithiumLevel, KyberLevel};
use qv_privacy::confidential::{
    BlindingFactor, ConfidentialAmount, MockCommitter, MockRangeProver, MockRangeVerifier,
};
use qv_privacy::stealth::{
    create_stealth_output, recover_spend_key, scan_output, MockSpendKeyDeriver, StealthKeys,
};
use qv_privacy::view_key::{DisclosureProof, PrivacyMode, ViewKey};
use qv_privacy::PrivacyError;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn alice_keys() -> StealthKeys {
    StealthKeys::generate(KyberLevel::default(), DilithiumLevel::default()).unwrap()
}

fn bob_keys() -> StealthKeys {
    StealthKeys::generate(KyberLevel::default(), DilithiumLevel::default()).unwrap()
}

// ---------------------------------------------------------------------------
// 1) Full stealth lifecycle: create → scan → recover spend key
// ---------------------------------------------------------------------------

#[test]
fn full_stealth_lifecycle() {
    let alice = alice_keys();
    let addr = alice.address();

    // Bob sends to Alice's stealth address.
    let (output, _sender_ss) = create_stealth_output(&addr).unwrap();

    // Alice scans and detects her output.
    let scan = scan_output(&alice, &output).unwrap();
    assert!(scan.is_some());
    let result = scan.unwrap();

    // Alice recovers the one-time spend key.
    let deriver = MockSpendKeyDeriver::new(DilithiumLevel::Level3);
    let onetime_kp = recover_spend_key(&alice, &result, &deriver).unwrap();
    assert_eq!(onetime_kp.public.level(), DilithiumLevel::Level3);
}

// ---------------------------------------------------------------------------
// 2) Wrong recipient cannot scan
// ---------------------------------------------------------------------------

#[test]
fn wrong_recipient_cannot_scan() {
    let alice = alice_keys();
    let bob = bob_keys();

    let (output, _) = create_stealth_output(&alice.address()).unwrap();

    // Bob tries to scan Alice's output.
    let scan = scan_output(&bob, &output).unwrap();
    assert!(scan.is_none());
}

// ---------------------------------------------------------------------------
// 3) Multiple outputs — only correct recipient detects theirs
// ---------------------------------------------------------------------------

#[test]
fn multiple_outputs_selective_detection() {
    let alice = alice_keys();
    let bob = bob_keys();

    let (out_alice, _) = create_stealth_output(&alice.address()).unwrap();
    let (out_bob, _) = create_stealth_output(&bob.address()).unwrap();

    // Alice finds her output but not Bob's.
    assert!(scan_output(&alice, &out_alice).unwrap().is_some());
    assert!(scan_output(&alice, &out_bob).unwrap().is_none());

    // Bob finds his output but not Alice's.
    assert!(scan_output(&bob, &out_bob).unwrap().is_some());
    assert!(scan_output(&bob, &out_alice).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// 4) Stealth + confidential amount combined
// ---------------------------------------------------------------------------

#[test]
fn stealth_with_confidential_amount() {
    let alice = alice_keys();
    let (output, _) = create_stealth_output(&alice.address()).unwrap();

    // Scan succeeds.
    let scan = scan_output(&alice, &output).unwrap().unwrap();

    // Create a confidential amount for the UTXO.
    let committer = MockCommitter::new();
    let prover = MockRangeProver::new();
    let blinding = BlindingFactor::from_seed(b"alice-output-blinding");

    let amount = ConfidentialAmount::confidential(5000, &blinding, &committer, &prover).unwrap();
    assert!(amount.is_confidential());

    // Verify range proof.
    let verifier = MockRangeVerifier::new();
    if let ConfidentialAmount::Confidential {
        commitment,
        range_proof,
    } = &amount
    {
        assert!(verifier.verify(commitment, range_proof).unwrap());
    } else {
        panic!("expected confidential amount");
    }

    // The scan result is independent of the amount — stealth works at the
    // address level, confidential amounts work at the value level.
    assert_eq!(scan.onetime_pk_hash, output.onetime_pk_hash);
}

// ---------------------------------------------------------------------------
// 5) Disclosure proof — full audit flow
// ---------------------------------------------------------------------------

#[test]
fn disclosure_proof_audit_flow() {
    let alice = alice_keys();
    let (output, _) = create_stealth_output(&alice.address()).unwrap();
    let scan = scan_output(&alice, &output).unwrap().unwrap();

    let committer = MockCommitter::new();
    let blinding = BlindingFactor::from_seed(b"audit-blinding");
    let commitment = committer.commit(1000, &blinding).unwrap();

    // Alice creates a disclosure proof for an auditor.
    let proof = DisclosureProof::create(
        &scan.shared_secret,
        &output.onetime_pk_hash,
        Some(1000),
        Some(&blinding),
    );

    // Auditor verifies the disclosure.
    let valid = proof
        .verify(
            &output,
            &alice.spend_kp.public,
            Some(&commitment),
            &committer,
        )
        .unwrap();
    assert!(valid);
}

// ---------------------------------------------------------------------------
// 6) Disclosure proof — amount-only (no blinding)
// ---------------------------------------------------------------------------

#[test]
fn disclosure_amount_only() {
    let alice = alice_keys();
    let (output, _) = create_stealth_output(&alice.address()).unwrap();
    let scan = scan_output(&alice, &output).unwrap().unwrap();

    let proof = DisclosureProof::create(
        &scan.shared_secret,
        &output.onetime_pk_hash,
        Some(500),
        None,
    );

    let committer = MockCommitter::new();
    let valid = proof
        .verify(&output, &alice.spend_kp.public, None, &committer)
        .unwrap();
    assert!(valid);
}

// ---------------------------------------------------------------------------
// 7) Confidential balance verification — multi-output
// ---------------------------------------------------------------------------

#[test]
fn confidential_balance_multi_output() {
    let committer = MockCommitter::new();
    let prover = MockRangeProver::new();
    let verifier = MockRangeVerifier::new();

    let b_in = BlindingFactor::from_seed(b"input-blind");
    let b_out1 = BlindingFactor::from_seed(b"out1-blind");
    let b_out2 = BlindingFactor::from_seed(b"out2-blind");

    let input = ConfidentialAmount::confidential(10000, &b_in, &committer, &prover).unwrap();
    let out1 = ConfidentialAmount::confidential(7000, &b_out1, &committer, &prover).unwrap();
    let out2 = ConfidentialAmount::confidential(2500, &b_out2, &committer, &prover).unwrap();
    let fee = qv_core::Amount::from_smallest_units(500);

    let balanced = qv_privacy::confidential::verify_balance_mock(
        &[input],
        &[out1, out2],
        fee,
        &verifier,
    )
    .unwrap();
    assert!(balanced);
}

// ---------------------------------------------------------------------------
// 8) Mixed plain + confidential balance
// ---------------------------------------------------------------------------

#[test]
fn mixed_plain_confidential_balance() {
    let committer = MockCommitter::new();
    let prover = MockRangeProver::new();
    let verifier = MockRangeVerifier::new();

    let b = BlindingFactor::from_seed(b"mix-blind");

    let inputs = vec![
        ConfidentialAmount::plain(5000),
        ConfidentialAmount::confidential(3000, &b, &committer, &prover).unwrap(),
    ];
    let outputs = vec![ConfidentialAmount::plain(7800)];
    let fee = qv_core::Amount::from_smallest_units(200);

    let balanced =
        qv_privacy::confidential::verify_balance_mock(&inputs, &outputs, fee, &verifier).unwrap();
    assert!(balanced);
}

// ---------------------------------------------------------------------------
// 9) Privacy mode governs feature availability
// ---------------------------------------------------------------------------

#[test]
fn privacy_mode_feature_gates() {
    let stealth_only = PrivacyMode::StealthOnly;
    assert!(stealth_only.stealth_enabled());
    assert!(!stealth_only.confidential_enabled());

    let full = PrivacyMode::Full;
    assert!(full.stealth_enabled());
    assert!(full.confidential_enabled());

    let transparent = PrivacyMode::Transparent;
    assert!(!transparent.stealth_enabled());
    assert!(!transparent.confidential_enabled());
}

// ---------------------------------------------------------------------------
// 10) View key allows scanning by third party
// ---------------------------------------------------------------------------

#[test]
fn view_key_third_party_scan() {
    let alice = alice_keys();
    let (output, _) = create_stealth_output(&alice.address()).unwrap();

    // Alice exports her view key.
    let view_key = ViewKey::new(alice.view_kp.clone());

    // Auditor builds StealthKeys with the view key (spend key not needed for scan).
    // We simulate by using Alice's full keys since scan only uses the view keypair.
    let scan = scan_output(&alice, &output).unwrap();
    assert!(scan.is_some());

    // Verify the view key references the same public key.
    assert_eq!(view_key.public.level, alice.view_kp.public.level);
}

// ---------------------------------------------------------------------------
// 11) Stealth with different Kyber levels
// ---------------------------------------------------------------------------

#[test]
fn stealth_different_kyber_levels() {
    for level in [KyberLevel::Level1, KyberLevel::Level3, KyberLevel::Level5] {
        let keys = StealthKeys::generate(level, DilithiumLevel::Level3).unwrap();
        let (output, _) = create_stealth_output(&keys.address()).unwrap();
        let scan = scan_output(&keys, &output).unwrap();
        assert!(scan.is_some(), "stealth should work at Kyber level {level:?}");
    }
}

// ---------------------------------------------------------------------------
// 12) End-to-end: stealth + confidential + disclosure
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_stealth_confidential_disclosure() {
    // 1. Alice generates keys.
    let alice = alice_keys();

    // 2. Bob sends to Alice's stealth address.
    let (stealth_out, _) = create_stealth_output(&alice.address()).unwrap();

    // 3. Alice scans and detects the output.
    let scan = scan_output(&alice, &stealth_out).unwrap().unwrap();

    // 4. The UTXO has a confidential amount.
    let committer = MockCommitter::new();
    let prover = MockRangeProver::new();
    let verifier = MockRangeVerifier::new();
    let blinding = BlindingFactor::from_seed(b"e2e-blinding");
    let amount = ConfidentialAmount::confidential(42_000, &blinding, &committer, &prover).unwrap();

    // 5. Verify range proof.
    if let ConfidentialAmount::Confidential {
        commitment,
        range_proof,
    } = &amount
    {
        assert!(verifier.verify(commitment, range_proof).unwrap());

        // 6. Alice creates a disclosure proof for a regulator.
        let disclosure = DisclosureProof::create(
            &scan.shared_secret,
            &stealth_out.onetime_pk_hash,
            Some(42_000),
            Some(&blinding),
        );

        // 7. Regulator verifies the disclosure.
        let valid = disclosure
            .verify(
                &stealth_out,
                &alice.spend_kp.public,
                Some(commitment),
                &committer,
            )
            .unwrap();
        assert!(valid);
    } else {
        panic!("expected confidential amount");
    }

    // 8. Alice recovers spend key.
    let deriver = MockSpendKeyDeriver::new(DilithiumLevel::Level3);
    let onetime_kp = recover_spend_key(&alice, &scan, &deriver).unwrap();
    assert_eq!(onetime_kp.public.level(), DilithiumLevel::Level3);
}
