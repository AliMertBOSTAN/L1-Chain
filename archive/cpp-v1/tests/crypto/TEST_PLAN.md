# Crypto Module Test Plan

**Version**: 1.0
**Last Updated**: 2026-04-10
**Status**: APPROVED
**Coverage Target**: 95%

---

## Overview

This document defines all tests for the `src/crypto/` module. The crypto module is security-critical and implements:
- **Dilithium** (ML-DSA): Post-quantum signature scheme
- **Kyber** (ML-KEM): Post-quantum key encapsulation mechanism
- **Hybrid KEM**: X25519 + Kyber combined for conservative quantum resistance
- **Hash Functions**: SHA3-256, SHA3-512, BLAKE3, Argon2id
- **Random Number Generation**: Secure PRNG for key generation

All cryptographic functions must:
1. Match official specification test vectors (KATs)
2. Pass determinism checks (same seed → same output)
3. Exhibit proper randomness (different runs produce different results where appropriate)
4. Handle edge cases (empty input, boundary sizes, corrupted input)
5. Maintain high performance (>10k ops/sec for signing, >100 MB/s for hashing)

---

## Test Organization

```
tests/crypto/
├── test_dilithium.cpp          # Dilithium signature tests
├── test_kyber.cpp               # Kyber KEM tests
├── test_hybrid_kem.cpp          # Hybrid KEM (X25519 + Kyber)
├── test_hash.cpp                # Hash function tests (SHA3, BLAKE3, Argon2id)
├── test_rng.cpp                 # Random number generation
├── kat/                          # Known-Answer Test vectors
│   ├── dilithium_ml_dsa_*.json   # ML-DSA official KATs
│   ├── kyber_ml_kem_*.json       # ML-KEM official KATs
│   ├── sha3_nist_*.json          # NIST SHA3 KATs
│   └── argon2id_*.json           # Argon2id reference vectors
├── bench_dilithium.cpp          # Dilithium performance benchmarks
├── bench_kyber.cpp              # Kyber performance benchmarks
├── bench_hash.cpp               # Hash function benchmarks
└── TEST_PLAN.md                 # This file
```

---

## 1. Dilithium Signature Tests (`test_dilithium.cpp`)

### Module Interface
```cpp
namespace qv::crypto {

struct SecretKey { /* 2544 bytes */ };
struct PublicKey { /* 1312 bytes */ };

// Keygen with seed for determinism in testing
std::pair<SecretKey, PublicKey> 
dilithium_keygen(const std::array<uint8_t, 32>& seed);

// Signature with deterministic seed option
std::vector<uint8_t> 
dilithium_sign(std::string_view message, const SecretKey& sk);

// Verification
bool dilithium_verify(
  std::string_view signature, 
  std::string_view message, 
  const PublicKey& pk
);

}
```

### Test Cases

#### 1.1 Key Generation Tests

| Test ID | Test Name | Input | Expected Output | Coverage |
|---------|-----------|-------|-----------------|----------|
| T1.1.1 | `keygen_determinism` | seed=42 | Same (sk, pk) every call | Determinism |
| T1.1.2 | `keygen_different_seeds` | seed1=1, seed2=2 | Different (sk, pk) pairs | Randomness |
| T1.1.3 | `keygen_zero_seed` | seed=0 | Valid (sk, pk) | Edge case |
| T1.1.4 | `keygen_max_seed` | seed=2^256-1 | Valid (sk, pk) | Edge case |
| T1.1.5 | `keygen_pubkey_derivable` | sk, seed | pubkey = derive(sk) | Consistency |

**Test Implementation**:
```cpp
TEST(DilithiumKeyGen, DeterminismWithSeed) {
  std::array<uint8_t, 32> seed;
  seed.fill(42);
  
  auto [sk1, pk1] = qv::crypto::dilithium_keygen(seed);
  auto [sk2, pk2] = qv::crypto::dilithium_keygen(seed);
  
  EXPECT_EQ(sk1, sk2);
  EXPECT_EQ(pk1, pk2);
}

TEST(DilithiumKeyGen, DifferentSeedsProduceDifferentKeys) {
  std::array<uint8_t, 32> seed1, seed2;
  seed1.fill(1);
  seed2.fill(2);
  
  auto [sk1, pk1] = qv::crypto::dilithium_keygen(seed1);
  auto [sk2, pk2] = qv::crypto::dilithium_keygen(seed2);
  
  EXPECT_NE(sk1, sk2);
  EXPECT_NE(pk1, pk2);
}
```

---

#### 1.2 Signing Tests

| Test ID | Test Name | Input | Expected Output | Coverage |
|---------|-----------|-------|-----------------|----------|
| T1.2.1 | `sign_valid` | msg, sk | Signature (2420 bytes) | Happy path |
| T1.2.2 | `sign_empty_message` | "", sk | Valid signature | Edge case |
| T1.2.3 | `sign_large_message` | 1MB data, sk | Valid signature | Boundary |
| T1.2.4 | `sign_determinism` | Same msg, sk, seed | Same signature | Determinism |
| T1.2.5 | `sign_different_messages` | msg1, msg2, sk | Different signatures | Uniqueness |
| T1.2.6 | `sign_different_keys` | msg, sk1, sk2 | Different signatures | Key isolation |

**Test Implementation**:
```cpp
TEST(DilithiumSign, ValidMessage) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(1));
  std::string message = "Hello, QuantumVault";
  
  auto signature = qv::crypto::dilithium_sign(message, sk);
  
  EXPECT_EQ(signature.size(), 2420); // ML-DSA signature size
  EXPECT_FALSE(signature.empty());
}

TEST(DilithiumSign, EmptyMessage) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(2));
  
  auto signature = qv::crypto::dilithium_sign("", sk);
  
  EXPECT_EQ(signature.size(), 2420);
}

TEST(DilithiumSign, LargeMessage1MB) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(3));
  std::string large_msg(1024 * 1024, 'X');
  
  auto signature = qv::crypto::dilithium_sign(large_msg, sk);
  
  EXPECT_EQ(signature.size(), 2420);
}
```

---

#### 1.3 Verification Tests

| Test ID | Test Name | Input | Expected Output | Coverage |
|---------|-----------|-------|-----------------|----------|
| T1.3.1 | `verify_valid_signature` | sig, msg, pk | true | Happy path |
| T1.3.2 | `verify_wrong_message` | sig, different_msg, pk | false | Tampering detection |
| T1.3.3 | `verify_wrong_key` | sig, msg, different_pk | false | Key isolation |
| T1.3.4 | `verify_corrupted_signature` | corrupted_sig, msg, pk | false | Integrity check |
| T1.3.5 | `verify_truncated_signature` | sig[0:1000], msg, pk | false | Boundary check |
| T1.3.6 | `verify_zero_signature` | zeros(2420), msg, pk | false | Invalid input |

**Test Implementation**:
```cpp
TEST(DilithiumVerify, ValidSignature) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(4));
  std::string message = "Test message";
  
  auto signature = qv::crypto::dilithium_sign(message, sk);
  bool valid = qv::crypto::dilithium_verify(signature, message, pk);
  
  EXPECT_TRUE(valid);
}

TEST(DilithiumVerify, WrongMessage) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(5));
  
  auto signature = qv::crypto::dilithium_sign("Message A", sk);
  bool valid = qv::crypto::dilithium_verify(signature, "Message B", pk);
  
  EXPECT_FALSE(valid);
}

TEST(DilithiumVerify, CorruptedSignature) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(6));
  std::string message = "Test message";
  
  auto signature = qv::crypto::dilithium_sign(message, sk);
  signature[0] ^= 0xFF; // Flip bits
  bool valid = qv::crypto::dilithium_verify(signature, message, pk);
  
  EXPECT_FALSE(valid);
}
```

---

#### 1.4 Known-Answer Tests (KATs)

KAT vectors from **ML-DSA (NIST FIPS 204)** specification, Appendix A.

| Test ID | Test Name | Source | Count | Coverage |
|---------|-----------|--------|-------|----------|
| T1.4.1 | `kat_keygen` | ML-DSA Section A.1 | 3 vectors | Specification compliance |
| T1.4.2 | `kat_sign` | ML-DSA Section A.2 | 3 vectors | Specification compliance |
| T1.4.3 | `kat_verify` | ML-DSA Section A.3 | 3 vectors | Specification compliance |

**Test Implementation**:
```cpp
TEST(DilithiumKAT, ML_DSA_KeyGen_Vector1) {
  const auto& vector = qv::test::kat_ml_dsa_keygen_vectors[0];
  
  std::array<uint8_t, 32> seed;
  std::copy(vector.seed.begin(), vector.seed.end(), seed.begin());
  
  auto [sk, pk] = qv::crypto::dilithium_keygen(seed);
  
  EXPECT_EQ(pk, vector.expected_pk);
  EXPECT_EQ(sk, vector.expected_sk);
}

TEST(DilithiumKAT, ML_DSA_Sign_Vector1) {
  const auto& vector = qv::test::kat_ml_dsa_sign_vectors[0];
  
  auto signature = qv::crypto::dilithium_sign(
    vector.message, 
    vector.secret_key
  );
  
  EXPECT_EQ(signature, vector.expected_signature);
}

TEST(DilithiumKAT, ML_DSA_Verify_Vector1) {
  const auto& vector = qv::test::kat_ml_dsa_verify_vectors[0];
  
  bool valid = qv::crypto::dilithium_verify(
    vector.signature,
    vector.message,
    vector.public_key
  );
  
  EXPECT_TRUE(valid);
}
```

---

#### 1.5 Round-Trip Tests

| Test ID | Test Name | Operation | Expected | Coverage |
|---------|-----------|-----------|----------|----------|
| T1.5.1 | `roundtrip_sign_verify` | sign → verify | true | Integration |
| T1.5.2 | `roundtrip_multiple` | 100x sign/verify | all true | Stress |

---

### Performance Benchmarks (`bench_dilithium.cpp`)

| Benchmark | Target | Unit | Notes |
|-----------|--------|------|-------|
| `dilithium_keygen` | <100 ms | ms | One-time per account |
| `dilithium_sign` | <0.1 ms | ms | Per transaction |
| `dilithium_verify` | <0.2 ms | ms | Per input validation |
| `dilithium_sign_throughput` | >10k ops/sec | ops/sec | Batch validation |
| `dilithium_verify_throughput` | >5k ops/sec | ops/sec | Block validation |

---

## 2. Kyber KEM Tests (`test_kyber.cpp`)

### Module Interface
```cpp
namespace qv::crypto {

struct KyberPublicKey { /* 1184 bytes */ };
struct KyberPrivateKey { /* 2400 bytes */ };
struct KyberCiphertext { /* 1088 bytes */ };

// Keygen with seed
std::pair<KyberPrivateKey, KyberPublicKey>
kyber_keygen(const std::array<uint8_t, 32>& seed);

// Encapsulate (generate shared secret for public key)
std::pair<KyberCiphertext, std::array<uint8_t, 32>>
kyber_encapsulate(const KyberPublicKey& pk);

// Decapsulate (recover shared secret from ciphertext)
std::optional<std::array<uint8_t, 32>>
kyber_decapsulate(const KyberCiphertext& ct, const KyberPrivateKey& sk);

}
```

### Test Cases

#### 2.1 Key Generation Tests

| Test ID | Test Name | Input | Expected Output | Coverage |
|---------|-----------|-------|-----------------|----------|
| T2.1.1 | `keygen_determinism` | seed=42 | Same (sk, pk) | Determinism |
| T2.1.2 | `keygen_different_seeds` | Different seeds | Different (sk, pk) | Randomness |
| T2.1.3 | `keygen_sizes` | seed | pk=1184B, sk=2400B | Sizes correct |

---

#### 2.2 Encapsulation Tests

| Test ID | Test Name | Input | Expected | Coverage |
|---------|-----------|-------|----------|----------|
| T2.2.1 | `encap_produces_pair` | pk | (ciphertext, shared_secret) | Happy path |
| T2.2.2 | `encap_determinism` | Same pk, seed | Same (ct, ss) | Determinism with seed |
| T2.2.3 | `encap_randomness` | Same pk | Different (ct, ss) | Proper randomness |
| T2.2.4 | `encap_size` | pk | ct=1088B, ss=32B | Size check |

---

#### 2.3 Decapsulation Tests

| Test ID | Test Name | Input | Expected | Coverage |
|---------|-----------|-------|----------|----------|
| T2.3.1 | `decap_valid` | ct, sk | Correct shared_secret | Happy path |
| T2.3.2 | `decap_wrong_key` | ct, wrong_sk | Different ss | Key isolation |
| T2.3.3 | `decap_corrupted` | corrupted_ct, sk | Different or error | Integrity |
| T2.3.4 | `decap_false_positive` | random_ct, sk | Fails gracefully | Security |

---

#### 2.4 Roundtrip Tests

| Test ID | Test Name | Operation | Expected | Coverage |
|---------|-----------|-----------|----------|----------|
| T2.4.1 | `roundtrip_encap_decap` | encap → decap | Shared secrets match | Integration |
| T2.4.2 | `roundtrip_100x` | 100x encap/decap | All match | Stress |

---

#### 2.5 Known-Answer Tests (KATs)

KAT vectors from **ML-KEM (NIST FIPS 203)** specification.

| Test ID | Test Name | Source | Count | Coverage |
|---------|-----------|--------|-------|----------|
| T2.5.1 | `kat_keygen` | ML-KEM Section A.1 | 3 vectors | Specification |
| T2.5.2 | `kat_encap` | ML-KEM Section A.2 | 3 vectors | Specification |
| T2.5.3 | `kat_decap` | ML-KEM Section A.3 | 3 vectors | Specification |

---

### Performance Benchmarks (`bench_kyber.cpp`)

| Benchmark | Target | Unit |
|-----------|--------|------|
| `kyber_keygen` | <10 ms | ms |
| `kyber_encapsulate` | <0.2 ms | ms |
| `kyber_decapsulate` | <0.2 ms | ms |
| `kyber_encap_throughput` | >5k ops/sec | ops/sec |
| `kyber_decap_throughput` | >5k ops/sec | ops/sec |

---

## 3. Hybrid KEM Tests (`test_hybrid_kem.cpp`)

### Module Interface
```cpp
namespace qv::crypto {

struct HybridCiphertext {
  std::array<uint8_t, 32> x25519_ephemeral;  // X25519 public key
  KyberCiphertext kyber_ciphertext;           // Kyber encapsulation
};

// Hybrid encapsulation (X25519 + Kyber)
std::pair<HybridCiphertext, std::array<uint8_t, 32>>
hybrid_encapsulate(
  const std::array<uint8_t, 32>& x25519_pk,
  const KyberPublicKey& kyber_pk
);

// Hybrid decapsulation
std::optional<std::array<uint8_t, 32>>
hybrid_decapsulate(
  const HybridCiphertext& ct,
  const std::array<uint8_t, 32>& x25519_sk,
  const KyberPrivateKey& kyber_sk
);

}
```

### Test Cases

#### 3.1 Encapsulation Tests

| Test ID | Test Name | Input | Expected | Coverage |
|---------|-----------|-------|----------|----------|
| T3.1.1 | `hybrid_encap_produces_pair` | x25519_pk, kyber_pk | (ct, ss) | Happy path |
| T3.1.2 | `hybrid_encap_contains_both` | keys | ct has both X25519 and Kyber | Structure |
| T3.1.3 | `hybrid_encap_size` | keys | ct size = 32 + 1088 | Size check |

#### 3.2 Decapsulation Tests

| Test ID | Test Name | Input | Expected | Coverage |
|---------|-----------|-------|----------|----------|
| T3.2.1 | `hybrid_decap_valid` | ct, sk | Correct shared_secret | Happy path |
| T3.2.2 | `hybrid_decap_wrong_x25519` | ct, wrong_x25519_sk, kyber_sk | Different ss | Key isolation |
| T3.2.3 | `hybrid_decap_wrong_kyber` | ct, x25519_sk, wrong_kyber_sk | Different ss | Key isolation |
| T3.2.4 | `hybrid_decap_fallback_x25519` | ct, x25519_sk, invalid_kyber_sk | Degraded but valid | Fallback |

#### 3.3 Roundtrip Tests

| Test ID | Test Name | Operation | Expected | Coverage |
|---------|-----------|-----------|----------|----------|
| T3.3.1 | `hybrid_roundtrip` | encap → decap | Shared secrets match | Integration |

#### 3.4 Security Properties

| Test ID | Test Name | Property | Expected | Coverage |
|---------|-----------|----------|----------|----------|
| T3.4.1 | `hybrid_post_quantum_resilient` | One component fails | Other component sufficient | Hybrid security |
| T3.4.2 | `hybrid_classical_fallback` | Kyber unavailable | X25519 alone succeeds | Graceful degradation |

---

## 4. Hash Function Tests (`test_hash.cpp`)

### Module Interface
```cpp
namespace qv::crypto {

// SHA3-256
std::array<uint8_t, 32> sha3_256(std::string_view data);
std::array<uint8_t, 32> sha3_256_streaming(
  std::function<bool(std::vector<uint8_t>&)> reader
);

// SHA3-512
std::array<uint8_t, 64> sha3_512(std::string_view data);

// BLAKE3
std::array<uint8_t, 32> blake3(std::string_view data);

// Argon2id (PoW hash)
std::array<uint8_t, 32> argon2id(
  std::string_view password,
  const std::array<uint8_t, 16>& salt,
  uint32_t time_cost,
  uint32_t memory_cost,
  uint32_t parallelism
);

}
```

### Test Cases

#### 4.1 SHA3-256 Tests

| Test ID | Test Name | Input | Source | Coverage |
|---------|-----------|-------|--------|----------|
| T4.1.1 | `sha3_256_nist_vector_1` | "" | NIST CAVP | KAT |
| T4.1.2 | `sha3_256_nist_vector_2` | 0xCC (100 bytes) | NIST CAVP | KAT |
| T4.1.3 | `sha3_256_nist_vector_3` | Various | NIST CAVP | KAT |
| T4.1.4 | `sha3_256_streaming` | 1MB data | Generated | Streaming |
| T4.1.5 | `sha3_256_determinism` | Same input | Generated | Determinism |
| T4.1.6 | `sha3_256_sensitivity` | Bit flip | Generated | Avalanche |

**Test Implementation**:
```cpp
TEST(SHA3, SHA3_256_NIST_Vector_1) {
  // Empty input
  const auto result = qv::crypto::sha3_256("");
  const auto expected = qv::test::get_nist_kat_sha3_256(0);
  EXPECT_EQ(result, expected);
}

TEST(SHA3, SHA3_256_Streaming) {
  auto data = generate_random_data(1024 * 1024);
  
  auto full_hash = qv::crypto::sha3_256(data);
  
  std::vector<uint8_t> buffer = data;
  size_t pos = 0;
  auto stream_hash = qv::crypto::sha3_256_streaming(
    [&](auto& chunk) {
      if (pos >= buffer.size()) return false;
      size_t size = std::min(size_t(4096), buffer.size() - pos);
      chunk.assign(buffer.begin() + pos, buffer.begin() + pos + size);
      pos += size;
      return true;
    }
  );
  
  EXPECT_EQ(full_hash, stream_hash);
}

TEST(SHA3, SHA3_256_AvalancheEffect) {
  const auto msg1 = "The quick brown fox";
  const auto msg2 = "The quick brown dox";
  
  const auto hash1 = qv::crypto::sha3_256(msg1);
  const auto hash2 = qv::crypto::sha3_256(msg2);
  
  // Hashes should differ significantly (avalanche effect)
  int different_bits = 0;
  for (size_t i = 0; i < hash1.size(); i++) {
    different_bits += __builtin_popcount(hash1[i] ^ hash2[i]);
  }
  
  EXPECT_GT(different_bits, 64); // Expect many bits different
}
```

#### 4.2 SHA3-512 Tests

| Test ID | Test Name | Source | Count | Coverage |
|---------|-----------|--------|-------|----------|
| T4.2.1 | `sha3_512_nist_vectors` | NIST CAVP | 3 | KAT |
| T4.2.2 | `sha3_512_output_size` | Various | 5 | Size check |

#### 4.3 BLAKE3 Tests

| Test ID | Test Name | Input | Source | Coverage |
|---------|-----------|-------|--------|----------|
| T4.3.1 | `blake3_reference_vectors` | Various | BLAKE3 spec | KAT |
| T4.3.2 | `blake3_throughput` | 1MB | Generated | Performance |

#### 4.4 Argon2id Tests (PoW)

| Test ID | Test Name | Input | Expected | Coverage |
|---------|-----------|-------|----------|----------|
| T4.4.1 | `argon2id_reference_vector` | (pwd, salt) | Known output | KAT |
| T4.4.2 | `argon2id_determinism` | Same input | Same output | Determinism |
| T4.4.3 | `argon2id_sensitivity` | Bit flip in pwd | Different output | Security |
| T4.4.4 | `argon2id_performance` | Target 100ms | <100ms | Tuning |

**Test Implementation**:
```cpp
TEST(Argon2id, DeterminismAndTiming) {
  const auto pwd = "correct horse battery staple";
  std::array<uint8_t, 16> salt;
  salt.fill(0x42);
  
  auto start = std::chrono::high_resolution_clock::now();
  const auto hash1 = qv::crypto::argon2id(
    pwd, salt, 2, 65536, 1
  );
  auto elapsed = std::chrono::high_resolution_clock::now() - start;
  
  const auto hash2 = qv::crypto::argon2id(
    pwd, salt, 2, 65536, 1
  );
  
  EXPECT_EQ(hash1, hash2);
  EXPECT_LT(elapsed.count(), 100'000'000); // <100ms
}
```

---

### Performance Benchmarks (`bench_hash.cpp`)

| Benchmark | Target | Unit |
|-----------|--------|------|
| `sha3_256_1KB` | >100 MB/s | MB/s |
| `sha3_256_1MB` | >100 MB/s | MB/s |
| `sha3_512_1KB` | >100 MB/s | MB/s |
| `blake3_1KB` | >200 MB/s | MB/s |
| `blake3_1MB` | >200 MB/s | MB/s |
| `argon2id_pow_hash` | <100 ms | ms |

---

## 5. Random Number Generation Tests (`test_rng.cpp`)

### Module Interface
```cpp
namespace qv::crypto {

// Seed PRNG
void prng_seed(const std::array<uint8_t, 32>& seed);

// Generate random bytes
std::vector<uint8_t> prng_next(size_t count);

// Thread-safe variant
class ThreadSafeRNG {
  void seed(const std::array<uint8_t, 32>& seed);
  std::vector<uint8_t> next(size_t count);
};

}
```

### Test Cases

| Test ID | Test Name | Expected | Coverage |
|---------|-----------|----------|----------|
| T5.1.1 | `rng_determinism_with_seed` | Same seed → same sequence | Determinism |
| T5.1.2 | `rng_different_seeds` | Different seeds → different sequences | Independence |
| T5.1.3 | `rng_output_sizes` | Exact sizes requested | Boundary |
| T5.1.4 | `rng_distribution` | Uniform distribution | Randomness quality |
| T5.1.5 | `rng_chi_square_test` | χ² test passes | Statistical test |
| T5.1.6 | `rng_thread_safety` | No race conditions | Concurrency |

**Test Implementation**:
```cpp
TEST(RNG, DeterminismWithSeed) {
  std::array<uint8_t, 32> seed;
  seed.fill(0xAB);
  
  qv::crypto::prng_seed(seed);
  auto seq1 = qv::crypto::prng_next(1000);
  
  qv::crypto::prng_seed(seed);
  auto seq2 = qv::crypto::prng_next(1000);
  
  EXPECT_EQ(seq1, seq2);
}

TEST(RNG, UniformDistribution) {
  std::array<uint8_t, 32> seed;
  seed.fill(0x42);
  qv::crypto::prng_seed(seed);
  
  auto bytes = qv::crypto::prng_next(10000);
  
  // Count frequency of each byte value
  std::array<int, 256> frequency = {};
  for (uint8_t byte : bytes) {
    frequency[byte]++;
  }
  
  // Chi-square test
  double chi_sq = 0;
  for (int count : frequency) {
    double expected = 10000.0 / 256.0; // ~39
    chi_sq += (count - expected) * (count - expected) / expected;
  }
  
  // With 255 DoF, critical value at 0.05 is ~293
  EXPECT_LT(chi_sq, 350);
}
```

---

## 6. Coverage Analysis

### Expected Coverage by Function

| Function | Coverage | Notes |
|----------|----------|-------|
| `dilithium_keygen` | 100% | KAT + parametric tests |
| `dilithium_sign` | 100% | KAT + edge cases |
| `dilithium_verify` | 100% | KAT + tampering tests |
| `kyber_keygen` | 100% | KAT + parametric tests |
| `kyber_encapsulate` | 100% | KAT + roundtrip tests |
| `kyber_decapsulate` | 100% | KAT + failure modes |
| `hybrid_encapsulate` | 98% | All paths except rare fallbacks |
| `hybrid_decapsulate` | 98% | All paths except rare fallbacks |
| `sha3_256` | 100% | NIST KATs |
| `sha3_512` | 100% | NIST KATs |
| `blake3` | 100% | Spec KATs |
| `argon2id` | 95% | PoW tuning path |
| `prng_seed` | 100% | Initialization |
| `prng_next` | 100% | Output generation |

**Target**: 95% overall coverage in crypto/

---

## 7. Test Execution Plan

### Phase 1: Unit Tests (Weeks 1-2)
1. Implement `test_dilithium.cpp` (KATs + edge cases)
2. Implement `test_kyber.cpp` (KATs + roundtrip)
3. Implement `test_hash.cpp` (NIST KATs)
4. Implement `test_rng.cpp` (Statistical tests)

### Phase 2: Benchmarks (Week 3)
1. Implement `bench_dilithium.cpp`
2. Implement `bench_kyber.cpp`
3. Implement `bench_hash.cpp`
4. Establish baseline performance metrics

### Phase 3: Integration (Week 4)
1. Integrate hybrid KEM tests
2. Run full test suite with coverage analysis
3. Document coverage gaps
4. Optimize hot paths if needed

### Phase 4: Regression Tests (Ongoing)
1. Maintain KAT database
2. Track performance regressions
3. Add new tests for discovered bugs

---

## 8. Continuous Integration

### CMake Integration

```cmake
# tests/crypto/CMakeLists.txt
find_package(GTest REQUIRED)
find_package(benchmark REQUIRED)

# Unit tests
add_executable(test_crypto
  test_dilithium.cpp
  test_kyber.cpp
  test_hybrid_kem.cpp
  test_hash.cpp
  test_rng.cpp
)
target_link_libraries(test_crypto
  qv_crypto
  GTest::gtest_main
)
gtest_discover_tests(test_crypto)

# Benchmarks
add_executable(bench_crypto
  bench_dilithium.cpp
  bench_kyber.cpp
  bench_hash.cpp
)
target_link_libraries(bench_crypto
  qv_crypto
  benchmark::benchmark
)
```

### CI Pipeline

```yaml
# .github/workflows/test-crypto.yml
name: Crypto Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Build
        run: |
          cmake --preset dev -DENABLE_COVERAGE=ON
          ninja -C build test_crypto
      
      - name: Run Tests
        run: ctest --test-dir build --verbose
      
      - name: Coverage
        run: |
          ninja -C build test_coverage
          curl -s https://codecov.io/bash | bash
      
      - name: Run Benchmarks
        run: ./build/bin/bench_crypto --benchmark_format=json > crypto_bench.json
      
      - name: Upload Benchmarks
        uses: actions/upload-artifact@v3
        with:
          name: crypto_benchmarks
          path: crypto_bench.json
```

---

## 9. Success Criteria

- [ ] All unit tests pass (100%)
- [ ] All KATs match official test vectors
- [ ] Code coverage >= 95%
- [ ] Benchmarks meet performance targets
- [ ] No security advisories for dependencies
- [ ] Determinism verified (same seed → same output)
- [ ] Edge cases handled (empty input, max sizes, corrupted data)
- [ ] CI pipeline green on all commits

---

## 10. References

- [ML-DSA Specification (NIST FIPS 204)](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)
- [ML-KEM Specification (NIST FIPS 203)](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.203.pdf)
- [liboqs Documentation](https://liboqs.org/)
- [NIST CAVP Test Vectors](https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/)
- [BLAKE3 Specification](https://github.com/BLAKE3-team/BLAKE3)
- [Argon2 Specification](https://github.com/P-H-C/phc-winner-argon2)
- [Google Test Documentation](https://google.github.io/googletest/)
- [Google Benchmark Documentation](https://github.com/google/benchmark)

---

**Approved by**: Architecture Team
**Last Review**: 2026-04-10
**Next Review**: 2026-05-10

