# QuantumVault CMake Configuration Modules

This directory contains CMake modules for static analysis, code coverage, and runtime sanitization support for the QuantumVault blockchain project.

## Modules

### `sanitizers.cmake`
Provides AddressSanitizer, UndefinedBehaviorSanitizer, ThreadSanitizer, and MemorySanitizer support.

**Available Options:**
- `QV_ENABLE_ASAN` — AddressSanitizer (detects memory errors, heap overflow, use-after-free)
- `QV_ENABLE_UBSAN` — UndefinedBehaviorSanitizer (detects undefined behavior)
- `QV_ENABLE_TSAN` — ThreadSanitizer (detects data races)
- `QV_ENABLE_MSAN` — MemorySanitizer (detects uninitialized memory access)

**Usage:**
```bash
cmake --preset dev -DQV_ENABLE_ASAN=ON
cmake --preset dev -DQV_ENABLE_UBSAN=ON
cmake --preset dev -DQV_ENABLE_TSAN=ON
cmake --preset dev -DQV_ENABLE_MSAN=ON
```

**Warnings:**
- ASAN and TSAN are mutually exclusive (conflict in runtime)
- MSAN and ASAN are mutually exclusive
- MSAN and TSAN are mutually exclusive

Enabling conflicting sanitizers together will print a warning but continues.

### `coverage.cmake`
Generates code coverage reports using gcov/lcov (GCC) or llvm-cov (Clang).

**Available Option:**
- `QV_ENABLE_COVERAGE` — Enable code coverage reporting

**Usage:**
```bash
cmake --preset dev -DQV_ENABLE_COVERAGE=ON
ninja -C build
ninja -C build coverage
```

**Output:**
Coverage HTML report is generated in `build/coverage_report/index.html`

**Features:**
- Automatically excludes test files and third-party code from coverage metrics
- Supports both GCC (lcov/genhtml) and Clang (llvm-cov)
- Custom `coverage` target that runs all tests and generates HTML report

### `static_analysis.cmake`
Integrates clang-tidy, cppcheck, and include-what-you-use for comprehensive static analysis.

**Available Options:**
- `QV_ENABLE_CLANG_TIDY` — Enable clang-tidy analysis on all builds
- `QV_ENABLE_CPPCHECK` — Enable cppcheck analysis on all builds
- `QV_ENABLE_IWYU` — Enable include-what-you-use analysis on all builds

**Usage:**
```bash
# Enable analysis on every build
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON

# Or run manually via custom targets
cmake --preset dev
ninja -C build lint         # Run clang-tidy
ninja -C build cppcheck    # Run cppcheck
ninja -C build iwyu        # Run include-what-you-use
ninja -C build analyze     # Run all analysis tools
```

**Features:**
- `.clang-tidy` configuration file defines all checks and naming conventions
- clang-tidy with `--fix` and `--fix-errors` flags for automatic corrections
- cppcheck with C++20 support and comprehensive checks
- Custom targets for manual analysis without slowing down builds
- Combined `analyze` target runs all available tools

## Quick Start

### Development with Sanitizers (Recommended for Crypto Code)
```bash
# UBSAN catches undefined behavior (critical for arithmetic-heavy crypto)
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build
ctest --test-dir build

# Or with ASAN for memory safety
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build
ctest --test-dir build
```

### Code Quality Analysis
```bash
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON
ninja -C build      # Compile and run clang-tidy
```

### Coverage Report
```bash
cmake --preset dev -DQV_ENABLE_COVERAGE=ON
ninja -C build
ninja -C build coverage  # Generate HTML report
open build/coverage_report/index.html
```

### Complete Code Quality Check
```bash
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON -DQV_ENABLE_CPPCHECK=ON -DQV_ENABLE_COVERAGE=ON
ninja -C build
ninja -C build analyze
ninja -C build coverage
```

## Naming Conventions (from .clang-tidy)

The `.clang-tidy` file enforces QuantumVault naming conventions:

- **Namespaces**: `lower_case` (e.g., `qv::core`)
- **Classes/Structs**: `PascalCase` (e.g., `Transaction`, `Block`)
- **Enums**: `PascalCase` (e.g., `ConsensusState`)
- **Enum Constants**: `UPPER_CASE` (e.g., `CONSENSUS_ACTIVE`)
- **Functions**: `lower_case` (e.g., `validate_signature()`)
- **Variables**: `lower_case` (e.g., `utxo_set`)
- **Member Variables**: `lower_case` with `_` suffix for private/protected (e.g., `hash_value_`)
- **Global Constants**: `UPPER_CASE` (e.g., `MAX_BLOCK_SIZE`)
- **Macros**: `UPPER_CASE` (e.g., `QV_ASSERT`)

## Disabled Checks (in .clang-tidy)

The following checks are disabled for QuantumVault:

- `modernize-use-trailing-return-type` — Not required
- `readability-magic-numbers` — Crypto constants are necessary
- `cppcoreguidelines-avoid-magic-numbers` — Same reason
- `readability-identifier-length` — Short names are acceptable in crypto math (e.g., `a`, `b`, `h`)
- `cppcoreguidelines-pro-bounds-pointer-arithmetic` — Required for low-level crypto operations
- `cppcoreguidelines-pro-type-reinterpret-cast` — Required for serialization/deserialization
- `cppcoreguidelines-avoid-c-arrays` — Some use cases require fixed-size arrays

## Enabled as Warnings-as-Errors

Critical checks that will fail the build:

- All `cert-*` checks (CERT secure coding standards)
- `bugprone-use-after-move` — Memory safety critical
- `bugprone-undefined-memory-manipulation` — Security critical
- `bugprone-infinite-loop` — Logic safety
- `clang-analyzer-core.DivideZero` — Prevents crashes
- `clang-analyzer-core.NullDereference` — Memory safety
- `clang-analyzer-deadcode.DeadStores` — Code quality
- `concurrency-mt-unsafe` — Thread safety

## Integration with CI/CD

Add these to your CI pipeline:

```yaml
# Run static analysis
- ninja -C build lint
- ninja -C build cppcheck

# Run with sanitizers
- cmake --preset dev -DQV_ENABLE_UBSAN=ON && ninja -C build && ctest --test-dir build

# Generate coverage
- cmake --preset dev -DQV_ENABLE_COVERAGE=ON && ninja -C build coverage
```

## Troubleshooting

### clang-tidy not found
```bash
# macOS
brew install llvm

# Ubuntu/Debian
sudo apt-get install clang-tools

# Fedora
sudo dnf install clang-tools-extra
```

### cppcheck not found
```bash
# macOS
brew install cppcheck

# Ubuntu/Debian
sudo apt-get install cppcheck

# Fedora
sudo dnf install cppcheck
```

### Coverage tools not found (GCC)
```bash
# Ubuntu/Debian
sudo apt-get install lcov

# Fedora
sudo dnf install lcov

# macOS (use Clang instead - it's more reliable)
```

### include-what-you-use not found
```bash
# Build from source
git clone https://github.com/include-what-you-use/include-what-you-use.git
cd include-what-you-use
cmake .
make
sudo make install
```

### Sanitizer compilation fails
- Ensure your compiler supports the sanitizer (GCC 4.8+ or Clang 3.1+)
- Some sanitizers may not work on all platforms (MSAN is Linux/Android only)
- TSAN may have high performance overhead; suitable for testing only

### Performance issues with UBSAN
UBSAN is comprehensive but has overhead. For release builds, keep it disabled and use only in CI and development.

## References

- [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer/)
- [UndefinedBehaviorSanitizer](https://clang.llvm.org/docs/UndefinedBehaviorSanitizer/)
- [ThreadSanitizer](https://clang.llvm.org/docs/ThreadSanitizer/)
- [MemorySanitizer](https://clang.llvm.org/docs/MemorySanitizer/)
- [clang-tidy](https://clang.llvm.org/extra/clang-tidy/)
- [cppcheck](http://cppcheck.sourceforge.net/)
- [include-what-you-use](https://include-what-you-use.org/)
