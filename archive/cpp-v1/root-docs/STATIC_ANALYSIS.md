# Static Analysis and Sanitizers Guide for QuantumVault

This guide explains how to use the static analysis and sanitizer tools configured for the QuantumVault C++ blockchain project. These tools are critical for ensuring security and correctness in cryptographic code.

## Overview

The project includes four layers of quality assurance:

1. **Compile-time Static Analysis** (clang-tidy, cppcheck, iwyu)
2. **Runtime Sanitizers** (ASAN, UBSAN, TSAN, MSAN)
3. **Code Coverage** (lcov/genhtml or llvm-cov)
4. **Build-in Code Conventions** (.clang-tidy naming rules)

## Why This Matters for QuantumVault

- **Cryptographic Code**: Undefined behavior, integer overflow, and memory errors can silently break security
- **Consensus Layer**: Data races and concurrency bugs can fork the chain
- **PQC Integration**: Post-quantum cryptography libraries require strict safety practices
- **UTXO Model**: Reference counting and memory management are critical

## Quick Reference

### For Daily Development

```bash
# Configure with Clang Tidy enabled (catches most issues immediately)
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON
ninja -C build

# Run tests with UBSAN (catches undefined behavior in crypto math)
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build test
```

### Before Committing

```bash
# Run all analysis tools
ninja -C build analyze

# Check for memory issues
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build
ctest --test-dir build
```

### Before Release

```bash
# Full quality check
cmake --preset dev \
  -DQV_ENABLE_CLANG_TIDY=ON \
  -DQV_ENABLE_CPPCHECK=ON \
  -DQV_ENABLE_COVERAGE=ON
ninja -C build
ninja -C build analyze
ninja -C build coverage
```

## Sanitizers in Detail

### AddressSanitizer (ASAN)
**What it detects**: Heap buffer overflows, stack overflows, use-after-free, double free, memory leaks

**When to use**: Testing, CI/CD, before releases

**Example**:
```bash
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build
ctest --test-dir build --output-on-failure
```

**Crypto-specific risks**: Attacks like Heartbleed exploit memory safety bugs in cryptographic code.

### UndefinedBehaviorSanitizer (UBSAN)
**What it detects**: Integer overflow, shift errors, null pointer dereference, misaligned access, type mismatches

**When to use**: Always during development (critical for crypto math)

**Example**:
```bash
# UBSAN catches integer overflow in cryptographic operations
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build
# Run crypto-specific tests
ctest --test-dir build -R crypto
```

**Why crucial for QuantumVault**:
- Kyber/ML-KEM uses extensive arithmetic on large integers
- Dilithium/ML-DSA requires precise polynomial arithmetic
- Stealth address generation involves modular arithmetic
- Even single-bit errors break cryptographic guarantees

### ThreadSanitizer (TSAN)
**What it detects**: Data races, mutex ordering issues, deadlocks

**When to use**: Testing consensus layer, P2P networking code

**Example**:
```bash
# Test consensus with race detection
cmake --preset dev -DQV_ENABLE_TSAN=ON
ninja -C build
ctest --test-dir build -R consensus
```

**Consensus-specific risks**:
- Block validation races can lead to chain forks
- Stake-based committee validation needs synchronization
- Double-spend prevention relies on atomic UTXO updates

### MemorySanitizer (MSAN)
**What it detects**: Use of uninitialized memory

**When to use**: Linux/Android builds in CI

**Note**: MSAN requires all dependencies to be built with MSAN instrumentation (complex setup).

**Example** (advanced):
```bash
cmake --preset dev -DQV_ENABLE_MSAN=ON
ninja -C build
```

## Static Analysis Tools

### clang-tidy
Performs AST-based analysis to catch bugs, suggest optimizations, and enforce coding standards.

**Auto-fixes issues** (use with version control):
```bash
# One-time setup
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON
ninja -C build

# Or run manually without slowing builds
ninja -C build lint
```

**Checks enabled**:
- Memory safety (cert-mem*)
- Exception safety (cert-except*)
- Cryptographic best practices
- C++20 modernizations

**Example findings**:
```cpp
// ❌ Flagged by cert-err*: exception in destructor
~Transaction() {
  try {
    hash_cache_.clear();
  } catch (...) { }  // clang-tidy suggests not catching in destructors
}

// ✓ Better: RAII without exception handling
~Transaction() {
  hash_cache_.clear();  // No throw guarantee
}
```

### cppcheck
Checks for issues clang-tidy might miss, especially logic errors.

**Run with**:
```bash
ninja -C build cppcheck
cat build/cppcheck_report.txt
```

**Checks enabled** (all):
- Logic errors
- Resource leaks (memory, file handles)
- Arithmetic errors
- STL misuse
- Variable scope issues

### include-what-you-use (IWYU)
Optimizes includes: removes unnecessary ones, adds missing ones.

**Run with**:
```bash
ninja -C build iwyu
```

**Why it matters**:
- Faster builds (fewer dependencies)
- Clearer code dependencies
- Prevents accidental API exposure

## Code Coverage

Generate HTML coverage reports to identify untested code paths.

**Setup**:
```bash
cmake --preset dev -DQV_ENABLE_COVERAGE=ON
ninja -C build
ninja -C build coverage
open build/coverage_report/index.html
```

**Interpreting coverage**:
- **Red** (0%): Untested code — high risk
- **Yellow** (partial): Incomplete coverage — test branches
- **Green** (100%): Fully tested

**Coverage targets by module**:
- `src/crypto/*` — Aim for 95%+ (security-critical)
- `src/consensus/*` — Aim for 95%+ (must be deterministic)
- `src/core/*` — Aim for 90%+ (foundational)
- `src/net/*` — Aim for 80%+ (less critical than crypto)
- `src/rpc/*` — Aim for 70%+ (tested via integration tests)

## Naming Conventions (enforced by .clang-tidy)

Consistent naming prevents bugs and improves readability.

```cpp
// ✓ Correct
namespace qv::crypto {

class KeyDerivation {  // PascalCase
  bool is_valid_;      // Member: snake_case + _
  static const int MAX_ROUNDS = 1024;  // Global const: UPPER_CASE
};

void validate_signature(const std::string& sig);  // Function: snake_case

enum class ConsensusState {  // Enum: PascalCase
  IDLE,                      // Enum constant: UPPER_CASE
  VALIDATING,
  FINALIZED
};

}

// ❌ Incorrect (will be flagged)
class keyDerivation { };  // camelCase not allowed
void ValidateSignature() { };  // Should be snake_case
```

## Practical Workflows

### Debugging a Crypto Function

```cpp
// ❌ Code with potential integer overflow
uint64_t poly_mul(uint64_t a, uint64_t b) {
  return a * b;  // UBSAN: multiplication overflow
}

// ✓ Safe version
std::optional<uint64_t> poly_mul(uint64_t a, uint64_t b) {
  if (a > UINT64_MAX / b) {
    return std::nullopt;  // Overflow detected
  }
  return a * b;
}

// ✓ Or using checked arithmetic
uint64_t poly_mul_checked(uint64_t a, uint64_t b) {
  __uint128_t result = (__uint128_t)a * b;
  if (result > UINT64_MAX) {
    throw std::overflow_error("polynomial multiplication overflow");
  }
  return (uint64_t)result;
}
```

Run with UBSAN to catch overflows:
```bash
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build
ctest --test-dir build -R poly_mul -V
```

### Finding Memory Issues in P2P Code

```cpp
// ❌ Use-after-free
std::unique_ptr<PeerConnection> conn = create_connection();
auto addr = conn->address();
conn.reset();
process_address(addr);  // ASAN: use-after-free

// ✓ Safe version
std::unique_ptr<PeerConnection> conn = create_connection();
auto addr = conn->address();
auto addr_copy = addr;  // Copy before releasing
conn.reset();
process_address(addr_copy);
```

Run with ASAN:
```bash
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build
ctest --test-dir build -R network -V
```

### Checking for Data Races in Consensus

```cpp
// ❌ Data race: uncommitted_blocks_ read without lock
void validate_blocks() {
  for (auto& block : uncommitted_blocks_) {  // Race: no lock
    validate_block(block);
  }
}

void add_block(const Block& b) {
  uncommitted_blocks_.push_back(b);  // Race: no lock
}

// ✓ Safe version with mutex
class BlockValidator {
  std::mutex blocks_mutex_;
  std::vector<Block> uncommitted_blocks_;

public:
  void validate_blocks() {
    std::lock_guard<std::mutex> lock(blocks_mutex_);
    for (auto& block : uncommitted_blocks_) {
      validate_block(block);
    }
  }

  void add_block(const Block& b) {
    std::lock_guard<std::mutex> lock(blocks_mutex_);
    uncommitted_blocks_.push_back(b);
  }
};
```

Run with TSAN:
```bash
cmake --preset dev -DQV_ENABLE_TSAN=ON
ninja -C build
ctest --test-dir build -R consensus -V
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Quality Checks

on: [push, pull_request]

jobs:
  static-analysis:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Install dependencies
        run: |
          sudo apt-get install -y clang-tidy cppcheck lcov
      - name: Configure
        run: |
          cmake --preset dev \
            -DQV_ENABLE_CLANG_TIDY=ON \
            -DQV_ENABLE_CPPCHECK=ON \
            -DQV_ENABLE_COVERAGE=ON
      - name: Build
        run: ninja -C build
      - name: Analyze
        run: ninja -C build analyze
      - name: Coverage
        run: |
          ninja -C build coverage
          # Upload to codecov
          bash <(curl -s https://codecov.io/bash)

  sanitizers:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        sanitizer: [ASAN, UBSAN, TSAN]
    steps:
      - uses: actions/checkout@v3
      - name: Configure with ${{ matrix.sanitizer }}
        run: |
          cmake --preset dev -DQV_ENABLE_${{ matrix.sanitizer }}=ON
      - name: Build and Test
        run: |
          ninja -C build
          ctest --test-dir build --output-on-failure
```

## Best Practices

1. **Enable clang-tidy in development** — catch issues immediately
2. **Run UBSAN before committing** — prevents cryptographic disasters
3. **Use ASAN in CI** — detect memory leaks early
4. **Run TSAN on consensus code** — prevent chain forks
5. **Maintain >90% code coverage** for crypto and consensus modules
6. **Never disable critical checks** (cert-*, bugprone-use-after-move)
7. **Use std::expected<T, Error>** instead of exceptions for crypto functions
8. **Review all clang-tidy auto-fixes** before committing

## Troubleshooting

### Build fails with "Multiple sanitizers enabled"

This is a warning, not an error. Disable one sanitizer:
```bash
cmake --preset dev -DQV_ENABLE_ASAN=ON -DQV_ENABLE_TSAN=OFF
```

### UBSAN finds too many errors

UBSAN is strict — that's good! Fix errors in order of severity:
1. Integer overflow (cryptographic impact)
2. Null pointer (crash risk)
3. Signed shifts (rare but dangerous)
4. Alignment issues (platform-specific)

### Coverage report is empty

Ensure tests actually run:
```bash
ninja -C build test  # Not 'coverage' yet
ninja -C build coverage
```

### clang-tidy takes too long

Disable auto-fixing on builds:
```bash
cmake --preset dev  # No QV_ENABLE_CLANG_TIDY
ninja -C build lint  # Run manually
```

## Resources

- [AddressSanitizer Docs](https://clang.llvm.org/docs/AddressSanitizer/)
- [UBSAN Docs](https://clang.llvm.org/docs/UndefinedBehaviorSanitizer/)
- [clang-tidy Checks](https://clang.llvm.org/extra/clang-tidy/checks/)
- [CERT Secure Coding Standards](https://wiki.sei.cmu.edu/confluence/display/c/SEI+CERT+C+Coding+Standard)
- [CppCoreGuidelines](https://github.com/isocpp/CppCoreGuidelines)

## Support

For issues with sanitizers or static analysis:

1. Check the compiler output carefully
2. Enable verbose output: `ctest --test-dir build -V`
3. Check the configuration in `cmake/` directory
4. Verify your compiler version supports the sanitizer
5. For PQC-specific issues, check liboqs documentation
