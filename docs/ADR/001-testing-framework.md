# ADR-001: Testing Framework Selection

**Status**: APPROVED
**Date**: 2026-04-10
**Authors**: Mert (QuantumVault Team)
**Supersedes**: None

## Context

QuantumVault requires a comprehensive testing infrastructure to validate:
- Cryptographic correctness (PQC and hybrid primitives)
- Consensus mechanism (PoW + PoS)
- UTXO state machine
- Smart contract VM execution
- Privacy guarantees (stealth addresses)
- Data availability proofs

We must select testing frameworks that:
1. Integrate well with C++20 / CMake / Nix build system
2. Provide Known-Answer Test (KAT) support for cryptographic validation
3. Support high-performance benchmarking
4. Enable code coverage analysis
5. Integrate with CI/CD pipeline

## Decision

We adopt the following testing toolchain:

| Component | Tool | Version | Rationale |
|-----------|------|---------|-----------|
| **Unit Testing** | Google Test (gtest) | 1.11+ | Industry standard, excellent CMake integration, mature |
| **Benchmarking** | Google Benchmark | 1.7+ | Native CMake support, statistical analysis, realistic timings |
| **Coverage** | gcov + LCOV | N/A | GCC/Clang native support, generates HTML reports |
| **Static Analysis** | clang-tidy + cppcheck | Latest | CI integration, catches common errors |
| **Fuzzing** | libFuzzer | LLVM native | Integrated with clang, detected security bugs in production |
| **Property Testing** | RapidCheck | Future | C++11 property-based testing, fuzzing complement |

## Rationale

### Why Google Test?

**Alternatives Considered**:
- **Catch2**: Excellent syntax, but less mature CMake integration, slower CI integration
- **doctest**: Lightweight but lacks benchmarking ecosystem
- **Boost.Test**: Heavy, unnecessary for our use case

**Why gtest wins**:
- `gtest` has first-class CMake support (`find_package(GTest)`)
- Official Google Benchmark integration via macros
- Native code coverage support (gcov)
- `TEST()` macro is concise: `TEST(ModuleName, Behavior) { ... }`
- Large ecosystem of tools and IDE integrations
- Per-test isolation (separate processes or threads available)
- Fixture support for complex setup/teardown

### Why Google Benchmark?

**Alternatives Considered**:
- **celero**: Good but less mature than gbench
- **hayai**: Simpler but less statistical rigor
- **nonius**: Excellent statistical analysis but harder CMake integration

**Why gbench wins**:
- Separate from gtest, allows unit tests and perf benchmarks to coexist
- Statistical analysis (mean, std dev, variance detection)
- Prevents compiler optimizations from invalidating benchmarks
- Automatic scaling: if operation is too fast, repeats automatically
- CSV export for historical trend analysis
- Native `BENCHMARK()` macro: similar feel to `TEST()`

### Why libFuzzer (Future)?

- Integrated into clang/LLVM (no separate compilation step)
- Incremental corpus building
- Proven track record (found CVEs in OpenSSL, Chromium, libpng)
- Will be used for serialization and VM bytecode fuzzing

### Why RapidCheck (Future)?

- Property-based testing: `∀ inputs: property_holds`
- Complements unit tests (which are example-based)
- Catches edge cases that manual tests miss
- Shrinking: failures are reduced to minimal examples
- Will be integrated after core test suite is stable

## Implementation

### Test Structure (CMakeLists.txt)

```cmake
# Find testing dependencies
find_package(GTest REQUIRED)
find_package(benchmark REQUIRED)

# Unit tests
add_executable(qv_test_crypto tests/crypto/test_dilithium.cpp ...)
target_link_libraries(qv_test_crypto GTest::gtest_main qv_crypto)
gtest_discover_tests(qv_test_crypto)

# Benchmarks
add_executable(qv_bench_crypto tests/crypto/bench_dilithium.cpp ...)
target_link_libraries(qv_bench_crypto benchmark::benchmark qv_crypto)
```

### Test File Naming Convention

- **Unit Tests**: `test_<module>.cpp` (e.g., `test_dilithium.cpp`)
- **Benchmarks**: `bench_<module>.cpp` (e.g., `bench_dilithium.cpp`)
- **Integration Tests**: `integration_<workflow>.cpp` (e.g., `integration_tx_lifecycle.cpp`)
- **Known-Answer Tests**: Embedded in `test_<module>.cpp` with `TEST_CATEGORY("kat_*")`

### Test Annotation Convention

```cpp
// Unit test with category
TEST(CryptoModule, Dilithium_Sign_Determinism) {
  // ...
}

// Known-Answer Test
TEST(CryptoModule, Dilithium_KAT_ML_DSA_Spec) {
  // Vectors from official ML-DSA specification
  const auto expected = ...;
  const auto actual = sign(message, key);
  EXPECT_EQ(actual, expected);
}

// Benchmark
BENCHMARK(Dilithium_Sign) {
  // Automatically timed
  sign(message, key);
}

// Property test (future)
BOOST_CHECK(qc::property(
  "Sign and verify always roundtrip",
  gen::messages(), gen::keys(),
  [](const auto& msg, const auto& key) {
    return verify(sign(msg, key), msg, pubkey(key));
  }
));
```

## Coverage Targets

- **Security-critical** (crypto/): 95%
- **Core** (core/, consensus/): 90%
- **Standard** (privacy/, vm/, da/): 85%
- **Network/RPC**: 75%

Coverage enforced in CI:
```bash
cmake --preset dev -DENABLE_COVERAGE=ON
ninja -C build coverage
# Fail if coverage < targets
```

## CI Integration

GitHub Actions workflow (`.github/workflows/test.yml`):

```yaml
- name: Run Tests
  run: ctest --test-dir build --verbose

- name: Run Benchmarks
  run: ./build/bin/crypto_benchmark --benchmark_format=json > results.json

- name: Code Coverage
  run: |
    ninja -C build coverage
    curl -s https://codecov.io/bash | bash

- name: Static Analysis
  run: |
    clang-tidy src/**/*.cpp -- -std=c++20 -I./include
    cppcheck --std=c++20 src/
```

## Performance Expectations

Benchmarks will track:
- **Regression detection**: Alert if >5% slowdown
- **Scaling**: Verify linear/expected complexity
- **Throughput**: Hash rate, sign rate, verify rate

Example output:
```
Benchmark                      Time             CPU       Iterations
Dilithium_Sign            12.5 ms       12.4 ms           56
Dilithium_Verify          25.3 ms       25.1 ms           28
Kyber_Encapsulate          8.7 ms        8.6 ms           82
SHA3_256_1MB            123.4 ms       122.8 ms            6
```

## Advantages

1. **Maturity**: gtest/gbench have proven track records in billions of lines of production code
2. **Integration**: Perfect CMake + Nix support
3. **Ecosystem**: Rich plugin ecosystem (IDE integrations, CI platforms)
4. **Extensibility**: Easy to add fuzzing, property testing, formal verification later
5. **Maintainability**: Large community, extensive documentation
6. **Performance**: Minimal test harness overhead; accurate benchmarks

## Disadvantages

1. **Template Heavy**: gtest generates verbose template code (but necessary for flexibility)
2. **Separate Binaries**: Unit tests and benchmarks are separate executables (manageable via ctest)
3. **No Built-in Fuzz**: Fuzzing requires separate libFuzzer integration (planned for Phase 2)

## Future Considerations

1. **Phase 2**: Add libFuzzer for serialization, VM bytecode, network messages
2. **Phase 3**: Integrate RapidCheck for property-based tests on core invariants
3. **Phase 4**: Consider symbolic execution (KLEE) for cryptographic path verification
4. **Phase 5**: Formal verification of consensus algorithm (TLA+)

## References

- [Google Test Documentation](https://google.github.io/googletest/)
- [Google Benchmark Documentation](https://github.com/google/benchmark/wiki)
- [libFuzzer Documentation](https://llvm.org/docs/LibFuzzer/)
- [Code Coverage with gcov/LCOV](https://gcc.gnu.org/onlinedocs/gcc/Instrument-Functions.html)

## Sign-Off

- **Architecture Review**: APPROVED
- **Security Review**: APPROVED
- **Integration Review**: APPROVED

---

**Implementation Timeline**:
- **Week 1-2**: Set up gtest + gbench in CMake
- **Week 3-4**: Write unit tests for crypto/ module
- **Week 5-6**: Write unit tests for core/ and consensus/
- **Week 7-8**: Integration tests + CI setup
- **Phase 2**: libFuzzer integration

---

**Appendix A: Example Test Implementation**

```cpp
#include <gtest/gtest.h>
#include "qv/crypto/dilithium.h"

namespace qv::test {

class DilithiumTest : public ::testing::Test {
protected:
  qv::crypto::SecretKey sk;
  qv::crypto::PublicKey pk;
  std::string message = "Hello, QuantumVault";

  void SetUp() override {
    auto [generated_sk, generated_pk] = 
      qv::crypto::dilithium_keygen(qv::crypto::get_seed(42));
    sk = generated_sk;
    pk = generated_pk;
  }
};

// Unit test
TEST_F(DilithiumTest, SignVerifyRoundtrip) {
  auto signature = qv::crypto::dilithium_sign(message, sk);
  EXPECT_TRUE(qv::crypto::dilithium_verify(signature, message, pk));
}

// Boundary test
TEST_F(DilithiumTest, SignEmptyMessage) {
  auto signature = qv::crypto::dilithium_sign("", sk);
  EXPECT_TRUE(qv::crypto::dilithium_verify(signature, "", pk));
}

// Known-Answer Test
TEST(DilithiumKAT, Official_ML_DSA_Vector) {
  // From NIST ML-DSA specification Appendix A
  const auto kat = qv::test::get_kat("dilithium_vector_1");
  const auto result = qv::crypto::dilithium_sign(kat.message, kat.secret_key);
  EXPECT_EQ(result, kat.expected_signature);
}

} // namespace qv::test
```

---

**Appendix B: Example Benchmark**

```cpp
#include <benchmark/benchmark.h>
#include "qv/crypto/dilithium.h"

namespace qv::test {

// Setup
static qv::crypto::SecretKey sk;
static std::string message = "Hello, QuantumVault";

static void SetUpBenchmark() {
  auto [secret, _] = qv::crypto::dilithium_keygen(
    qv::crypto::get_seed(42));
  sk = secret;
  message.resize(1024, 'X'); // 1KB message
}

// Benchmark
static void Dilithium_Sign(benchmark::State& state) {
  for (auto _ : state) {
    benchmark::DoNotOptimize(
      qv::crypto::dilithium_sign(message, sk));
  }
  state.SetBytesProcessed(message.size() * state.iterations());
}
BENCHMARK(Dilithium_Sign)->Unit(benchmark::kMillisecond);

// Throughput benchmark
static void Dilithium_Sign_Throughput(benchmark::State& state) {
  int ops = 0;
  for (auto _ : state) {
    qv::crypto::dilithium_sign(message, sk);
    ops++;
  }
  state.counters["ops/sec"] = benchmark::Counter(
    ops, benchmark::Counter::kIsRate);
}
BENCHMARK(Dilithium_Sign_Throughput);

} // namespace qv::test
```

---

