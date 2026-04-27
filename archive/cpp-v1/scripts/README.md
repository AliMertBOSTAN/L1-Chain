# QuantumVault CI/Build Scripts

This directory contains bash scripts for building, testing, and maintaining code quality in the QuantumVault C++ blockchain project.

## Quick Start

After cloning the repository, run the setup script to install git hooks:

```bash
./scripts/setup-hooks.sh
```

Then build and test with:

```bash
./scripts/build.sh --preset dev --test
```

## Scripts Overview

### Git Hook Setup

#### `setup-hooks.sh`
Installs git hooks into `.git/hooks/`. Run once after cloning.

```bash
./scripts/setup-hooks.sh
```

Copies hooks from `scripts/hooks/` and makes them executable. Prints status for each hook.

### Git Hooks

#### `hooks/pre-commit`
Runs before each commit. Enforces:

1. **Code formatting** (clang-format) — Rejects commits with incorrectly formatted code
2. **Static analysis** (clang-tidy) — Warns about code issues (non-blocking by default)
3. **Blocking markers** — Rejects commits containing `TODO_BEFORE_MERGE` or `FIXME_BEFORE_MERGE`
4. **RAII policy** — Rejects raw `new` or `delete` (use `std::unique_ptr`)
5. **Namespace safety** — Rejects `using namespace std;`

Bypass with `git commit --no-verify` in emergencies (not recommended).

#### `hooks/pre-push`
Runs before pushing to remote. Enforces:

- All tests must pass (`ctest --test-dir build --output-on-failure`)

Bypass with `git push --no-verify` in emergencies (not recommended).

### Code Formatting

#### `format-check.sh`
Check formatting of all C++ files in `src/` and `include/`.

```bash
# Check which files need formatting
./scripts/format-check.sh

# Auto-format all files
./scripts/format-check.sh --fix
```

Exit codes: 0 if clean, 1 if files need formatting.

#### `format-all.sh`
Auto-format all C++ files in `src/`, `include/`, `tests/`, and `tools/`.

```bash
./scripts/format-all.sh
```

Uses `clang-format -i` (in-place modification). Review changes before committing.

### Building

#### `build.sh`
Unified build script with preset selection.

```bash
# Build with dev preset (default)
./scripts/build.sh

# Release build
./scripts/build.sh --preset release

# Debug build with sanitizers (ASAN+UBSAN)
./scripts/build.sh --preset sanitize

# Clean rebuild
./scripts/build.sh --clean

# Parallel build with 8 jobs
./scripts/build.sh --jobs 8

# Build and run tests
./scripts/build.sh --preset dev --test
```

**Presets:**
- `dev` — Debug, all warnings enabled
- `release` — Optimized, no debug symbols
- `sanitize` — Debug with address and undefined behavior sanitizers

Output goes to `build/bin/`.

### Testing

#### `run-tests.sh`
Test runner with multiple options.

```bash
# Run all tests
./scripts/run-tests.sh

# Run tests for a specific module
./scripts/run-tests.sh --module crypto
./scripts/run-tests.sh --module consensus
./scripts/run-tests.sh --module core

# Verbose output
./scripts/run-tests.sh --verbose

# Run specific tests by name
./scripts/run-tests.sh --filter "PoW"

# Generate coverage report (requires lcov/gcov)
./scripts/run-tests.sh --coverage

# Run under Valgrind (memory debugging)
./scripts/run-tests.sh --valgrind

# Combine options
./scripts/run-tests.sh --module crypto --verbose
```

**Modules:**
- `crypto` — PQC signing, KEM, hashing
- `consensus` — PoW, PoS, block validation
- `core` — Transactions, blocks, UTXO
- `privacy` — Stealth addresses
- `storage` — Block store, mempool, UTXO store
- `vm` — DSL interpreter
- `net` — P2P networking (optional)
- `da` — Data availability (optional)

Coverage reports are generated in `build/coverage/html/`.

### Continuous Integration

#### `ci.sh`
Full CI pipeline — runs all checks sequentially.

```bash
# Full CI pipeline (including sanitizer tests)
./scripts/ci.sh

# Quick mode (skip sanitizer testing)
./scripts/ci.sh --quick

# Verbose output
./scripts/ci.sh --verbose
```

**Pipeline steps:**

1. Format check (clang-format)
2. Debug build
3. Test suite
4. Static analysis (clang-tidy)
5. Sanitizer build (ASAN+UBSAN)
6. Tests under sanitizers

Exits with status 1 if any step fails. Each step is logged with pass/fail status.

## Workflow Examples

### Daily Development

```bash
# Initial setup (once per clone)
./scripts/setup-hooks.sh

# Build and test locally
./scripts/build.sh --preset dev --test

# Make changes...

# Auto-format before committing
./scripts/format-all.sh

# Commit (hooks verify format, analysis, markers, RAII, namespaces)
git add .
git commit -m "feature: add quantum signature verification"

# Push (hooks verify all tests pass)
git push
```

### Before Merging a PR

```bash
# Run full CI to ensure everything passes
./scripts/ci.sh

# Or quick CI if time-constrained
./scripts/ci.sh --quick
```

### Release Build

```bash
# Optimized release build
./scripts/build.sh --preset release

# Run full test suite
./scripts/run-tests.sh

# Verify with sanitizers
./scripts/build.sh --preset sanitize --test
```

### Debugging

```bash
# Verbose test output
./scripts/run-tests.sh --verbose

# Run specific test
./scripts/run-tests.sh --filter "test_hybrid_kem"

# Memory debugging with Valgrind
./scripts/run-tests.sh --valgrind

# Clang-tidy detailed analysis
clang-tidy -p build src/crypto/pqc_sign.cpp
```

## Requirements

### For Basic Build and Tests
- Bash
- CMake 3.20+
- Ninja
- C++20 compiler (g++ 10+, clang 10+, MSVC 2019+)
- Dependencies: liboqs, libp2p-cpp, protobuf, spdlog, GTest, leveldb/rocksdb

### For Code Formatting
- `clang-format` (part of LLVM toolchain)

### For Static Analysis
- `clang-tidy` (optional, part of LLVM toolchain)

### For Coverage Reports
- `lcov` and `gcov` (for coverage data)
- `genhtml` (part of lcov, generates HTML reports)

### For Memory Debugging
- `valgrind` (optional, for memory profiling)

### Installation (Ubuntu/Debian)

```bash
# Build essentials
sudo apt-get install build-essential cmake ninja-build

# LLVM tools (clang-format, clang-tidy)
sudo apt-get install clang-tools

# Coverage tools
sudo apt-get install lcov

# Memory debugging (optional)
sudo apt-get install valgrind

# QuantumVault dependencies
# (See project documentation for specific versions)
```

## Exit Codes

All scripts use standard exit codes:

- **0** — Success
- **1** — Failure (check output for details)

Pre-commit and pre-push hooks:
- **0** — All checks passed, operation proceeds
- **1** — Check failed, operation blocked

## Troubleshooting

### Build Fails
```bash
# Clean and reconfigure
./scripts/build.sh --clean --preset dev
```

### Tests Fail
```bash
# Run with verbose output
./scripts/run-tests.sh --verbose

# Check for platform-specific issues
ctest --test-dir build -VV
```

### Format Checker Complains
```bash
# Auto-fix formatting
./scripts/format-all.sh

# Or fix specific files
clang-format -i src/crypto/pqc_sign.cpp
```

### Pre-commit Hook Too Strict
```bash
# Bypass for emergency (not recommended)
git commit --no-verify

# But please fix issues before pushing
```

### Pre-push Hook Blocks Push
```bash
# Fix failing tests first
./scripts/run-tests.sh --verbose

# Then push again (no --no-verify needed)
git push
```

## Performance Tips

### Faster Builds
```bash
# Use more parallel jobs
./scripts/build.sh --jobs 16
```

### Faster Testing
```bash
# Run specific test module
./scripts/run-tests.sh --module crypto

# Skip sanitizer tests
./scripts/ci.sh --quick
```

## Integration with CI/CD Systems

These scripts are designed for local development and can easily integrate into CI/CD pipelines:

### GitHub Actions Example
```yaml
- name: Build and Test
  run: |
    ./scripts/build.sh --preset dev --test
    ./scripts/run-tests.sh
```

### GitLab CI Example
```yaml
test:
  script:
    - ./scripts/format-check.sh
    - ./scripts/build.sh --preset dev --test
    - ./scripts/ci.sh --quick
```

## Notes

- All scripts use `set -euo pipefail` for error handling
- Scripts auto-detect number of CPU cores for parallel builds
- Colored output for easy readability
- Comprehensive help text (pass `--help` to see usage details)
- Compatible with Bash 4.0+ (standard on modern systems)

## Contributing

When adding new checks or scripts:

1. Follow the naming convention: lowercase with hyphens
2. Add a clear shebang: `#!/bin/bash`
3. Use `set -euo pipefail` for safety
4. Include colored output for user-friendly messages
5. Document with comments and usage examples
6. Update this README with new functionality

## License

These scripts are part of the QuantumVault project and follow the same license terms.
