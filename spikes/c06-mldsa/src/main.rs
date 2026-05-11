//! C-06 spike — verify `ml-dsa = "0.0.4"` (RustCrypto) for deterministic seeded keygen.
//!
//! What we are checking:
//!   1. `MlDsa65::key_gen_internal(&B32)` produces a deterministic keypair from a 32-byte
//!      seed (FIPS 204 §6.1 KeyGen_internal — exposed as a `pub` trait method).
//!   2. Public key wire length is 1952 bytes (FIPS 204 ML-DSA-65 spec).
//!   3. Sign + verify roundtrip works via `signature::{Signer, Verifier}` traits.
//!   4. Tampered signature is rejected.
//!   5. Different seeds produce different public keys.
//!
//! If this all prints OK, we have a green light to swap `qv-crypto::pqc_sign::from_seed`
//! to delegate into `ml-dsa::KeyGen::key_gen_internal` (closing envanter C-04 + C-06).

use ml_dsa::{B32, KeyGen, MlDsa65};
use ml_dsa::signature::{Keypair, Signer, Verifier};

fn header(label: &str) {
    println!("\n────  {label}  ────");
}

fn b32_from_bytes(bytes: &[u8; 32]) -> B32 {
    // B32 = `hybrid_array::Array<u8, U32>`. Array<T, N> derefs to `[T]`, so
    // `copy_from_slice` works. This idiom is exactly what `ml-dsa`'s own
    // tests (lib.rs:925) use.
    let mut xi = B32::default();
    xi.copy_from_slice(bytes);
    xi
}

fn main() {
    println!("c06-mldsa-spike — ml-dsa 0.0.4 verification");

    let seed_aa = b32_from_bytes(&[0xAAu8; 32]);
    let seed_aa2 = b32_from_bytes(&[0xAAu8; 32]);
    let seed_bb = b32_from_bytes(&[0xBBu8; 32]);

    // ─── 1. Determinism from seed ────────────────────────────────────────────
    header("1) determinism: key_gen_internal twice with same seed → identical pk");
    let kp_a1 = <MlDsa65 as KeyGen>::key_gen_internal(&seed_aa);
    let kp_a2 = <MlDsa65 as KeyGen>::key_gen_internal(&seed_aa2);

    let pk_a1 = kp_a1.verifying_key().encode();
    let pk_a2 = kp_a2.verifying_key().encode();
    assert_eq!(
        pk_a1.as_slice(),
        pk_a2.as_slice(),
        "same seed should yield identical public keys"
    );
    println!("  ✅ same seed → same pk ({} bytes)", pk_a1.as_slice().len());

    // ─── 2. Public key size matches FIPS 204 ML-DSA-65 spec ──────────────────
    header("2) wire size: pk should be 1952 bytes");
    assert_eq!(
        pk_a1.as_slice().len(),
        1952,
        "ML-DSA-65 public key must be 1952 bytes (FIPS 204)"
    );
    println!("  ✅ pk len = 1952");
    println!("  pk[0..32] hex = {}", hex::encode(&pk_a1.as_slice()[..32]));

    // ─── 3. Different seed → different pk ────────────────────────────────────
    header("3) divergence: different seed → different pk");
    let kp_b = <MlDsa65 as KeyGen>::key_gen_internal(&seed_bb);
    let pk_b = kp_b.verifying_key().encode();
    assert_ne!(
        pk_a1.as_slice(),
        pk_b.as_slice(),
        "different seeds must produce different keys"
    );
    println!("  ✅ pk_a != pk_b");

    // ─── 4. Sign + verify roundtrip ──────────────────────────────────────────
    header("4) sign + verify roundtrip");
    let msg = b"QuantumVault L1 - C-06 verification message";
    // `Signer::sign` panics if signing fails (the deterministic path through
    // `sign_deterministic(msg, &[])`). For valid keypairs this won't happen.
    let sig = kp_a1.signing_key().sign(msg);
    kp_a1
        .verifying_key()
        .verify(msg, &sig)
        .expect("valid signature should verify");
    println!("  ✅ sign/verify OK");

    // ─── 5. Tampered message rejected ────────────────────────────────────────
    header("5) tamper detection: tampered msg fails verify");
    let bad_msg = b"QuantumVault L1 - TAMPERED message";
    let bad_result = kp_a1.verifying_key().verify(bad_msg, &sig);
    assert!(
        bad_result.is_err(),
        "tampered message must NOT verify against original signature"
    );
    println!("  ✅ tampered msg correctly rejected");

    // ─── 6. Cross-keypair verify rejection ───────────────────────────────────
    header("6) cross-key verify: kp_b verifying kp_a's signature must fail");
    let cross = kp_b.verifying_key().verify(msg, &sig);
    assert!(
        cross.is_err(),
        "kp_b must NOT verify a signature produced by kp_a"
    );
    println!("  ✅ cross-key verify correctly rejected");

    // ─── Summary ─────────────────────────────────────────────────────────────
    header("SUMMARY");
    println!("  ✅ all 6 checks passed.");
    println!("  ml-dsa 0.0.4 is the right crate for closing C-04 / C-06.");
    println!();
    println!("  Recommended integration shape for qv-crypto::pqc_sign::from_seed:");
    println!("    use ml_dsa::{{B32, KeyGen, MlDsa65}};");
    println!("    let mut xi = B32::default();");
    println!("    xi.copy_from_slice(seed);                                 // [u8; 32]");
    println!("    let kp = <MlDsa65 as KeyGen>::key_gen_internal(&xi);      // L3 deterministic");
    println!("    let pk = kp.verifying_key().encode().as_slice().to_vec(); // 1952 B");
    println!("    let sk = kp.signing_key().encode().as_slice().to_vec();   // 4032 B");
    println!("    // wrap into PqcKeyPair {{ public, secret, level: Level3 }}");
}
