//! Criterion benchmarks for `qv-crypto`.
//!
//! Run with:
//! ```bash
//! cargo bench -p qv-crypto
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//!
//! Groups:
//! - `hash/sha3_256/{64,1024,65536,1048576}`
//! - `hash/blake3/{64,1024,65536,1048576}`
//! - `pqc_sign/sign/{level2,3,5}`
//! - `pqc_sign/verify/{level2,3,5}`
//! - `hybrid_kem/encapsulate/{level1,3,5}`
//! - `hybrid_kem/decapsulate/{level1,3,5}`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use qv_crypto::{
    blake3, decapsulate_hybrid, encapsulate_hybrid, generate_hybrid_keypair, generate_pqc_keypair,
    sha3_256, sign_pqc, verify_pqc, DilithiumLevel, KyberLevel,
};

// ---------------------------------------------------------------------------
// Hash throughput
// ---------------------------------------------------------------------------

fn bench_hash(c: &mut Criterion) {
    let sizes = [64usize, 1024, 65_536, 1_048_576];

    let mut g = c.benchmark_group("hash/sha3_256");
    for &n in &sizes {
        let data = vec![0x5a_u8; n];
        g.throughput(Throughput::Bytes(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &data, |b, d| {
            b.iter(|| sha3_256(black_box(d)));
        });
    }
    g.finish();

    let mut g = c.benchmark_group("hash/blake3");
    for &n in &sizes {
        let data = vec![0x5a_u8; n];
        g.throughput(Throughput::Bytes(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &data, |b, d| {
            b.iter(|| blake3(black_box(d)));
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// PQC signatures
// ---------------------------------------------------------------------------

fn bench_pqc_sign(c: &mut Criterion) {
    let levels = [
        ("level2", DilithiumLevel::Level2),
        ("level3", DilithiumLevel::Level3),
        ("level5", DilithiumLevel::Level5),
    ];
    let msg = b"block header commitment (benchmarked msg)";

    let mut g = c.benchmark_group("pqc_sign/sign");
    for (name, level) in levels {
        let kp = generate_pqc_keypair(level).expect("keygen");
        g.bench_with_input(BenchmarkId::from_parameter(name), &kp, |b, kp| {
            b.iter(|| sign_pqc(&kp.secret, black_box(msg)).expect("sign"));
        });
    }
    g.finish();

    let mut g = c.benchmark_group("pqc_sign/verify");
    for (name, level) in levels {
        let kp = generate_pqc_keypair(level).expect("keygen");
        let sig = sign_pqc(&kp.secret, msg).expect("sign");
        g.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(kp, sig),
            |b, (kp, sig)| {
                b.iter(|| verify_pqc(&kp.public, black_box(msg), sig).expect("verify"));
            },
        );
    }
    g.finish();

    let mut g = c.benchmark_group("pqc_sign/keypair");
    for (name, level) in levels {
        g.bench_with_input(BenchmarkId::from_parameter(name), &level, |b, l| {
            b.iter(|| generate_pqc_keypair(*l).expect("keygen"));
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// Hybrid KEM
// ---------------------------------------------------------------------------

fn bench_hybrid_kem(c: &mut Criterion) {
    let levels = [
        ("level1", KyberLevel::Level1),
        ("level3", KyberLevel::Level3),
        ("level5", KyberLevel::Level5),
    ];

    let mut g = c.benchmark_group("hybrid_kem/keypair");
    for (name, level) in levels {
        g.bench_with_input(BenchmarkId::from_parameter(name), &level, |b, l| {
            b.iter(|| generate_hybrid_keypair(*l).expect("keygen"));
        });
    }
    g.finish();

    let mut g = c.benchmark_group("hybrid_kem/encapsulate");
    for (name, level) in levels {
        let kp = generate_hybrid_keypair(level).expect("keygen");
        g.bench_with_input(BenchmarkId::from_parameter(name), &kp, |b, kp| {
            b.iter(|| encapsulate_hybrid(black_box(&kp.public)).expect("encap"));
        });
    }
    g.finish();

    let mut g = c.benchmark_group("hybrid_kem/decapsulate");
    for (name, level) in levels {
        let kp = generate_hybrid_keypair(level).expect("keygen");
        let (ct, _) = encapsulate_hybrid(&kp.public).expect("encap");
        g.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(kp, ct),
            |b, (kp, ct)| {
                b.iter(|| decapsulate_hybrid(kp, black_box(ct)).expect("decap"));
            },
        );
    }
    g.finish();
}

criterion_group!(benches, bench_hash, bench_pqc_sign, bench_hybrid_kem);
criterion_main!(benches);
