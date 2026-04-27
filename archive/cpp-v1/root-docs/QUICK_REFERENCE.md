# QuantumVault Static Analysis — Quick Reference Card

## Files Created

```
QuantumVault/
├── .clang-tidy                          # Naming conventions & checks
├── .iwyu_mapping                        # Include mappings
├── STATIC_ANALYSIS.md                   # Developer guide (read this first)
├── CONFIG_FILES_SUMMARY.md              # What was installed
├── QUICK_REFERENCE.md                   # This file
├── cmake/
│   ├── sanitizers.cmake                 # Options: QV_ENABLE_ASAN/UBSAN/TSAN/MSAN
│   ├── coverage.cmake                   # Option: QV_ENABLE_COVERAGE
│   ├── static_analysis.cmake            # Targets: lint, cppcheck, iwyu, analyze
│   └── README.md                        # CMake documentation
└── CMakeLists.txt                       # Updated with module includes
```

## Common Commands

### Development (Daily)
```bash
# With clang-tidy checking everything
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON
ninja -C build

# With UBSAN (catches crypto errors)
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build && ctest --test-dir build
```

### Code Review (Before Commit)
```bash
ninja -C build analyze          # Run all tools
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build && ctest --test-dir build
```

### Release Check (Before Tag)
```bash
cmake --preset dev \
  -DQV_ENABLE_CLANG_TIDY=ON \
  -DQV_ENABLE_CPPCHECK=ON \
  -DQV_ENABLE_COVERAGE=ON
ninja -C build && ninja -C build analyze && ninja -C build coverage
```

## Custom Targets

| Target | Tool | What It Does |
|--------|------|-------------|
| `lint` | clang-tidy | Auto-fix code style & bugs |
| `cppcheck` | cppcheck | Check for logic errors |
| `iwyu` | include-what-you-use | Optimize includes |
| `analyze` | all tools | Run lint + cppcheck |
| `coverage` | gcov/llvm-cov | Generate coverage report (build/coverage_report/index.html) |

## Sanitizer Options

| Option | Detects | Use When |
|--------|---------|----------|
| `QV_ENABLE_ASAN` | Memory errors (overflow, UAF, leaks) | Testing, CI |
| `QV_ENABLE_UBSAN` | Undefined behavior (overflow, shift) | Crypto development |
| `QV_ENABLE_TSAN` | Data races | Testing consensus |
| `QV_ENABLE_MSAN` | Uninitialized memory | Linux CI (complex setup) |

## Naming Conventions (Enforced)

```cpp
namespace qv::core { }                    // lower_case

class Transaction { }                     // PascalCase
struct Block { }                          // PascalCase

void validate_signature() { }             // lower_case function
void validate() { }                       // snake_case

uint64_t total_stake_;                   // member: lower_case + _
int x;                                   // local var: lower_case

enum class ConsensusState { }            // PascalCase
static const int MAX_ROUNDS = 1024;      // global const: UPPER_CASE
#define QV_ASSERT(x)                     // macro: UPPER_CASE
```

## Critical Checks (Will Fail Build)

These must pass:
- `cert-*` — CERT secure coding
- `bugprone-use-after-move` — Memory safety
- `bugprone-undefined-memory-manipulation` — Security
- `clang-analyzer-core.NullDereference` — Crash prevention
- `concurrency-mt-unsafe` — Thread safety

## Issue Patterns

### Integer Overflow (UBSAN)
```cpp
// ❌ Bad
uint16_t result = a * b;

// ✓ Good
uint32_t result = static_cast<uint32_t>(a) * static_cast<uint32_t>(b);
```

### Memory Leak (ASAN, cppcheck)
```cpp
// ❌ Bad
auto* block = new Block();
if (size < MIN) return nullptr;  // Leak!

// ✓ Good
auto block = std::make_unique<Block>();
if (size < MIN) return std::unexpected("too small");
```

### Data Race (TSAN)
```cpp
// ❌ Bad
std::vector<int> stakes_;
void add(int x) { stakes_.push_back(x); }
int total() { return std::accumulate(stakes_.begin(), stakes_.end(), 0); }

// ✓ Good
std::mutex stakes_mutex_;
std::vector<int> stakes_;
void add(int x) {
  std::lock_guard lock(stakes_mutex_);
  stakes_.push_back(x);
}
int total() const {
  std::lock_guard lock(stakes_mutex_);
  return std::accumulate(stakes_.begin(), stakes_.end(), 0);
}
```

### Use-After-Move (clang-tidy)
```cpp
// ❌ Bad
auto peer = get_connection();
send(std::move(peer));
use(peer);  // ERROR: moved

// ✓ Good
auto peer = get_connection();
auto id = peer->id();
send(std::move(peer));
use(id);  // Use copy instead
```

## Configuration Examples

### Full Setup (Recommended for Release)
```bash
cmake --preset dev \
  -DQV_ENABLE_CLANG_TIDY=ON \
  -DQV_ENABLE_CPPCHECK=ON \
  -DQV_ENABLE_COVERAGE=ON \
  -DQV_ENABLE_UBSAN=ON
ninja -C build
ctest --test-dir build
ninja -C build coverage
```

### Crypto Development (Catch Math Errors)
```bash
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build
ctest --test-dir build -R crypto -V
```

### Memory Safety Check
```bash
cmake --preset dev -DQV_ENABLE_ASAN=ON
ninja -C build
ctest --test-dir build
```

### Consensus Validation (Race Detection)
```bash
cmake --preset dev -DQV_ENABLE_TSAN=ON
ninja -C build
ctest --test-dir build -R consensus -V
```

## Troubleshooting Quick Fixes

| Problem | Solution |
|---------|----------|
| "clang-tidy not found" | `brew install llvm` (macOS) or `apt-get install clang-tools` (Linux) |
| "Multiple sanitizers enabled" | Normal warning; disable one if tests fail |
| "cppcheck not found" | `brew install cppcheck` or `apt-get install cppcheck` |
| "Coverage report empty" | Run `ninja -C build test` first, then `ninja -C build coverage` |
| UBSAN too verbose | Normal in crypto code; fix overflow issues |
| TSAN reports races | Add `std::mutex` and `std::lock_guard` |

## Files to Read

1. **First**: `STATIC_ANALYSIS.md` (comprehensive guide)
2. **Second**: `cmake/README.md` (CMake module details)
3. **For Examples**: `ANALYSIS_EXAMPLES.md` (real code fixes)
4. **For Setup**: `CONFIG_FILES_SUMMARY.md` (what was installed)

## Configuration Files Location

All in: `C:\Users\mbostan\Desktop\L1\L1 Blockchain\`

- Configuration: `.clang-tidy`, `.iwyu_mapping`
- CMake modules: `cmake/*.cmake`
- Documentation: `*.md` files in root

## Testing the Setup

```bash
# Verify clang-tidy
cmake --preset dev -DQV_ENABLE_CLANG_TIDY=ON
cmake --build build --target help | grep lint

# Verify custom targets exist
ninja -C build --help | grep -E "lint|cppcheck|analyze|coverage"
```

## Key Takeaways for QuantumVault

1. **Crypto code is unforgiving**: UBSAN catches silent failures
2. **ASAN prevents exploits**: Memory safety = security
3. **TSAN prevents forks**: Consensus races break blockchain
4. **clang-tidy enforces clarity**: Consistent naming prevents bugs
5. **High coverage = confidence**: Aim for 90%+ in critical modules

## When in Doubt

Run this before committing:
```bash
ninja -C build analyze
cmake --preset dev -DQV_ENABLE_UBSAN=ON
ninja -C build && ctest --test-dir build
```

Success means no errors from any tool.
