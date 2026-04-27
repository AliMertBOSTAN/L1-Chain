#!/bin/bash
#
# ci.sh — Full CI pipeline for QuantumVault
#
# Runs the complete continuous integration workflow:
# 1. Format check (clang-format)
# 2. Build (debug)
# 3. Run tests
# 4. Static analysis (clang-tidy)
# 5. Sanitizer build (ASAN+UBSAN)
# 6. Tests under sanitizers
#
# Exit 1 if any step fails.
#
# Usage:
#   ./scripts/ci.sh                    # Run full CI pipeline
#   ./scripts/ci.sh --quick            # Skip sanitizer tests
#   ./scripts/ci.sh --verbose          # Verbose output
#

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/build"

QUICK_MODE=false
VERBOSE=""

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Tracking
STEPS_PASSED=0
STEPS_TOTAL=0
FAILED_STEPS=()

print_header() {
    echo ""
    echo -e "${YELLOW}════════════════════════════════════════════════════════════════${NC}"
    echo -e "${YELLOW}[CI]${NC} $1"
    echo -e "${YELLOW}════════════════════════════════════════════════════════════════${NC}"
}

print_step() {
    echo -e "${BLUE}→${NC} $1"
}

print_pass() {
    echo -e "${GREEN}✓${NC} $1"
    ((STEPS_PASSED++))
}

print_fail() {
    echo -e "${RED}✗${NC} $1"
    FAILED_STEPS+=("$1")
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)
            QUICK_MODE=true
            shift
            ;;
        --verbose)
            VERBOSE="-v"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ============================================================================
# STEP 1: Format Check
# ============================================================================
print_header "Step 1/6: Format Check (clang-format)"
((STEPS_TOTAL++))

if "${PROJECT_ROOT}/scripts/format-check.sh"; then
    print_pass "Code formatting OK"
else
    print_fail "Code formatting check failed"
    echo ""
    echo "  Fix with: ./scripts/format-check.sh --fix"
fi

# ============================================================================
# STEP 2: Build (Debug)
# ============================================================================
print_header "Step 2/6: Build (Debug Preset)"
((STEPS_TOTAL++))

if "${PROJECT_ROOT}/scripts/build.sh" --preset dev --clean; then
    print_pass "Debug build successful"
else
    print_fail "Debug build failed"
fi

# ============================================================================
# STEP 3: Run Tests
# ============================================================================
print_header "Step 3/6: Test Suite"
((STEPS_TOTAL++))

if "${PROJECT_ROOT}/scripts/run-tests.sh" ${VERBOSE}; then
    print_pass "Test suite passed"
else
    print_fail "Test suite failed"
fi

# ============================================================================
# STEP 4: Static Analysis (clang-tidy)
# ============================================================================
print_header "Step 4/6: Static Analysis (clang-tidy)"
((STEPS_TOTAL++))

if command -v clang-tidy &> /dev/null; then
    print_step "Running clang-tidy on source files..."

    TIDY_FAILED=0
    TIDY_WARNINGS=0

    # Run clang-tidy on all source files in build
    for file in "${PROJECT_ROOT}"/src/**/*.cpp; do
        if [[ -f "${file}" ]]; then
            # Run clang-tidy (suppress some noisy checks)
            if tidy_output=$(clang-tidy -p "${BUILD_DIR}" "${file}" 2>&1 || true); then
                warning_count=$(echo "${tidy_output}" | grep -c "warning:" || true)
                TIDY_WARNINGS=$((TIDY_WARNINGS + warning_count))
            fi
        fi
    done

    if [[ ${TIDY_WARNINGS} -eq 0 ]]; then
        print_pass "clang-tidy: No issues found"
    else
        print_pass "clang-tidy: Found ${TIDY_WARNINGS} warning(s) (non-blocking)"
        echo "  Review with: clang-tidy -p build <file>"
    fi
else
    print_fail "clang-tidy not installed (skipping)"
    echo "  Install: sudo apt-get install clang-tools  (or equivalent)"
fi

# ============================================================================
# STEP 5: Sanitizer Build (ASAN+UBSAN)
# ============================================================================
print_header "Step 5/6: Sanitizer Build (ASAN+UBSAN)"
((STEPS_TOTAL++))

if [[ "${QUICK_MODE}" == true ]]; then
    echo "  (Skipped: --quick mode)"
else
    print_step "Building with sanitizers enabled..."

    if "${PROJECT_ROOT}/scripts/build.sh" --preset sanitize --clean; then
        print_pass "Sanitizer build successful"
    else
        print_fail "Sanitizer build failed"
    fi
fi

# ============================================================================
# STEP 6: Tests Under Sanitizers
# ============================================================================
print_header "Step 6/6: Tests Under Sanitizers"
((STEPS_TOTAL++))

if [[ "${QUICK_MODE}" == true ]]; then
    echo "  (Skipped: --quick mode)"
else
    # The sanitizer build outputs tests to the same binary location
    print_step "Running tests with address and undefined behavior sanitizers..."

    if ASAN_OPTIONS=halt_on_error=1 UBSAN_OPTIONS=halt_on_error=1 \
        "${PROJECT_ROOT}/scripts/run-tests.sh"; then
        print_pass "Sanitizer tests passed"
    else
        print_fail "Sanitizer tests failed"
    fi
fi

# ============================================================================
# CI SUMMARY
# ============================================================================
print_header "CI Summary"

echo ""
echo "  Steps passed: ${STEPS_PASSED}/${STEPS_TOTAL}"
echo ""

if [[ ${#FAILED_STEPS[@]} -eq 0 ]]; then
    echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}✓ CI PIPELINE PASSED${NC}"
    echo -e "${GREEN}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
    exit 0
else
    echo -e "${RED}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${RED}✗ CI PIPELINE FAILED${NC}"
    echo -e "${RED}═══════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo "Failed steps:"
    for step in "${FAILED_STEPS[@]}"; do
        echo "  - ${step}"
    done
    echo ""
    exit 1
fi
