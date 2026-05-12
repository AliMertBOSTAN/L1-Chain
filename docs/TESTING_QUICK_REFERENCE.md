# QuantumVault Testing Quick Reference

**Last Updated**: 2026-04-10 (v1 C++ era; **partially superseded by Rust pivot 2026-04-15**)

> **⚠️ Pivot notu:** Bu doküman v1 C++ döneminden kalmıştır. Komutlar (`cmake`,
> `.cpp` dosyaları, `da/` klasörü) artık geçerli değildir; aktif Rust workspace'i
> `cargo test --workspace` ve `just test` ile koşturulur. Doc içeriği konsept
> rehberi olarak değerlendirilebilir; somut komutlar için `docs/TESTING_STRATEGY.md`
> (Rust güncel) ve `justfile` referans alınmalıdır.

This document provides quick access to testing commands and key information. For comprehensive details, see:
- **docs/TESTING_STRATEGY.md** — Complete testing strategy and architecture (Rust güncel)
- **docs/ADR/001-testing-framework.md** — v1 framework rationale; pivot sonrası `cargo-nextest + proptest + criterion`
- **crates/qv-crypto/src/** — Rust crate tests (yerine `tests/crypto/TEST_PLAN.md`)

---

## Quick Start

### Configure and Build with Testing

```bash
# Configure with debug symbols and no optimization (for testing)
cmake --preset dev

# Compile
ninja -C build

# Run all tests
ctest --test-dir build --verbose
```

### With Code Coverage

```bash
# Configure with coverage enabled
cmake --preset dev -DENABLE_COVERAGE=ON

# Run tests with coverage analysis
ninja -C build run_tests_with_coverage

# View HTML coverage report
open build/coverage/index.html  # macOS
xdg-open build/coverage/index.html  # Linux
start build/coverage/index.html  # Windows
```

### With Sanitizers (Memory/Undefined Behavior Detection)

```bash
cmake --preset dev -DENABLE_SANITIZERS=ON
ninja -C build run_unit_tests
```

---

## Common Test Commands

### Run All Tests
```bash
ctest --test-dir build --verbose
```

### Run Specific Test Categories
```bash
# Crypto tests only
ctest --test-dir build -R crypto --verbose

# Core module tests
ctest --test-dir build -R core --verbose

# Consensus tests
ctest --test-dir build -R consensus --verbose

# Integration tests only
ctest --test-dir build -L integration --verbose

# Unit tests only
ctest --test-dir build -L unit --verbose
```

### Run Tests Matching a Pattern
```bash
# All Dilithium tests
ctest --test-dir build -R dilithium --verbose

# All known-answer tests (KATs)
ctest --test-dir build -R kat --verbose

# All roundtrip tests
ctest --test-dir build -R roundtrip --verbose
```

### Run Tests with Output on Failure
```bash
ctest --test-dir build --output-on-failure
```

### Run Tests in Parallel
```bash
ctest --test-dir build -j 4  # Use 4 cores
```

### Run a Single Test
```bash
# First find the test name
ctest --test-dir build --verbose | grep -i "dilithium"

# Then run it
ctest --test-dir build -R "test_crypto.Dilithium_Sign" --verbose
```

---

## Benchmarks

### Run All Benchmarks
```bash
ninja -C build run_benchmarks
```

### Run Specific Benchmark Suite
```bash
./build/bin/bench_crypto --benchmark_format=json > results.json
./build/bin/bench_core
./build/bin/bench_consensus
./build/bin/bench_storage
```

### View Benchmark Options
```bash
./build/bin/bench_crypto --help
```

### Example Benchmark Output
```
Benchmark                      Time             CPU       Iterations
Dilithium_Sign            12.5 ms       12.4 ms           56        82 MB/s
Dilithium_Verify          25.3 ms       25.1 ms           28        41 MB/s
SHA3_256_1MB            123.4 ms       122.8 ms            6      8100 MB/s
```

### Export Benchmarks for Analysis
```bash
# JSON format (for trend analysis)
./build/bin/bench_crypto --benchmark_format=json > results_new.json

# CSV format
./build/bin/bench_crypto --benchmark_format=csv > results.csv
```

---

## Coverage Analysis

### Generate Coverage Report
```bash
# Configure with coverage support
cmake --preset dev -DENABLE_COVERAGE=ON

# Run tests
ctest --test-dir build --output-on-failure

# Generate report
ninja -C build test_coverage
```

### View Coverage
```bash
# HTML report
open build/coverage/index.html

# Command-line summary
lcov --summary build/coverage_filtered.info
```

### Check Coverage by Module
```bash
# Crypto module coverage
genhtml build/coverage_filtered.info \
  --output-directory /tmp/crypto_coverage \
  --prefix $(pwd) \
  --include '*crypto*'

open /tmp/crypto_coverage/index.html
```

---

## Test File Organization

```
tests/
├── crypto/              # Cryptographic operations
│   ├── test_*.cpp       # Unit tests
│   ├── bench_*.cpp      # Benchmarks
│   ├── TEST_PLAN.md     # Detailed test plan
│   └── kat/             # Known-Answer Test vectors
├── core/                # UTXO, transactions, blocks
│   ├── test_*.cpp
│   └── bench_*.cpp
├── consensus/           # Ouroboros Praos PoS (VRF + KES)
│   ├── test_*.cpp
│   └── bench_*.cpp
├── privacy/             # Stealth addresses
│   └── test_*.cpp
├── vm/                  # DSL bytecode interpreter
│   └── test_*.cpp
├── storage/             # Persistence layer
│   ├── test_*.cpp
│   └── bench_*.cpp
├── da/                  # Data availability
│   └── test_*.cpp
├── integration/         # End-to-end workflows
│   └── test_*.cpp
└── CMakeLists.txt       # Test configuration
```

---

## Test Coverage Targets

| Module | Target | Notes |
|--------|--------|-------|
| **crypto/** | 95% | Security-critical; all PQC primitives |
| **core/** | 90% | UTXO, transactions, blocks |
| **consensus/** | 85% | Ouroboros Praos PoS (VRF leader, KES sign, fork choice) |
| **privacy/** | 85% | Stealth address logic |
| **vm/** | 90% | Smart contract VM |
| **storage/** | 80% | Database operations |
| **da/** | 85% | Erasure coding, proofs |
| **net/** | 75% | P2P network layer |
| **rpc/** | 70% | JSON-RPC API |

---

## Performance Targets

### Cryptography
- **SHA3-256**: >100 MB/s
- **BLAKE3**: >200 MB/s
- **Dilithium Sign**: >10,000 ops/sec
- **Dilithium Verify**: >5,000 ops/sec
- **Kyber Encapsulate**: >5,000 ops/sec

### Blockchain
- **Block Validation**: <100ms per block
- **Tx Verification**: <1ms per tx
- **VRF Evaluate + Verify**: <2ms
- **KES Sign + Verify**: <50ms
- **Mempool Insert**: <1ms per tx
- **UTXO Operations**: <1ms per output

---

## Test Implementation Examples

### Writing a Unit Test

```cpp
#include <gtest/gtest.h>
#include "qv/crypto/dilithium.h"

// Simple test
TEST(DilithiumModule, SignVerifyRoundtrip) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(1));
  std::string message = "Test message";
  
  auto signature = qv::crypto::dilithium_sign(message, sk);
  bool valid = qv::crypto::dilithium_verify(signature, message, pk);
  
  EXPECT_TRUE(valid);
}

// Test with fixture (setup/teardown)
class DilithiumTests : public ::testing::Test {
protected:
  void SetUp() override {
    auto [s, p] = qv::crypto::dilithium_keygen(get_seed(42));
    sk = s;
    pk = p;
  }
  
  qv::crypto::SecretKey sk;
  qv::crypto::PublicKey pk;
};

TEST_F(DilithiumTests, SignDeterminism) {
  auto msg = "Determinism test";
  auto sig1 = qv::crypto::dilithium_sign(msg, sk);
  auto sig2 = qv::crypto::dilithium_sign(msg, sk);
  EXPECT_EQ(sig1, sig2);
}
```

### Writing a Benchmark

```cpp
#include <benchmark/benchmark.h>
#include "qv/crypto/dilithium.h"

static void Dilithium_Sign(benchmark::State& state) {
  auto [sk, pk] = qv::crypto::dilithium_keygen(get_seed(1));
  std::string message = "Hello, QuantumVault";
  
  for (auto _ : state) {
    auto sig = qv::crypto::dilithium_sign(message, sk);
    benchmark::DoNotOptimize(sig);
  }
  
  state.SetBytesProcessed(message.size() * state.iterations());
}
BENCHMARK(Dilithium_Sign)->Unit(benchmark::kMillisecond);
```

---

## Continuous Integration

### GitHub Actions

The project uses GitHub Actions for CI. Key workflows:

```yaml
# .github/workflows/test.yml
- name: Run Tests
  run: ctest --test-dir build --verbose

- name: Code Coverage
  run: |
    cmake --preset dev -DENABLE_COVERAGE=ON
    ninja -C build test_coverage
    curl -s https://codecov.io/bash | bash
```

### Pre-commit Checklist

Before committing changes:

1. **Run unit tests**: `ctest --test-dir build -L unit`
2. **Run integration tests**: `ctest --test-dir build -L integration`
3. **Check coverage**: Coverage should not decrease
4. **Run benchmarks**: Verify no performance regression

---

## Troubleshooting

### Test Build Fails

```bash
# Clean build
rm -rf build
cmake --preset dev
ninja -C build
```

### Specific Test Fails

```bash
# Run with verbose output
ctest --test-dir build -R "test_name" --verbose

# Run directly to see stderr/stdout
./build/bin/test_crypto --gtest_filter="TestClass.TestMethod"
```

### Coverage Report Missing

```bash
# Install lcov if missing
sudo apt-get install lcov  # Ubuntu
brew install lcov           # macOS

# Regenerate
cmake --preset dev -DENABLE_COVERAGE=ON
ninja -C build test_coverage
```

### Benchmark Results Unreliable

```bash
# Disable CPU frequency scaling (Linux)
sudo cpupower frequency-set -g performance

# Run benchmarks with explicit parameters
./build/bin/bench_crypto --benchmark_min_time=1 --benchmark_repetitions=3
```

---

## Adding New Tests

1. **Create test file** in `tests/<module>/test_<feature>.cpp`
2. **Write test cases** using `TEST()` or `TEST_F()` macros
3. **CMakeLists.txt auto-discovers** tests matching `**/test_*.cpp`
4. **Run tests**: `ctest --test-dir build`

Example:

```cpp
// tests/crypto/test_newfeature.cpp
#include <gtest/gtest.h>
#include "qv/crypto/newfeature.h"

TEST(NewFeature, BasicFunctionality) {
  EXPECT_EQ(new_function(42), 84);
}
```

No CMakeLists.txt changes needed—tests are auto-discovered!

---

## Further Reading

- **docs/TESTING_STRATEGY.md** — Comprehensive testing architecture
- **docs/ADR/001-testing-framework.md** — Framework decisions and trade-offs
- **tests/crypto/TEST_PLAN.md** — Detailed crypto module tests
- **[Google Test Documentation](https://google.github.io/googletest/)**
- **[Google Benchmark Documentation](https://github.com/google/benchmark/wiki)**

---

**Last Updated**: 2026-04-10  
**Next Review**: 2026-05-10
