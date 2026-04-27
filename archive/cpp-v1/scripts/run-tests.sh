#!/bin/bash
#
# run-tests.sh — Test runner with options for QuantumVault
#
# Runs the test suite with various options for output, coverage, and debugging.
#
# Usage:
#   ./scripts/run-tests.sh                    # Run all tests
#   ./scripts/run-tests.sh --module crypto    # Run crypto tests only
#   ./scripts/run-tests.sh --verbose          # Verbose output
#   ./scripts/run-tests.sh --coverage         # Generate coverage report (requires gcov/lcov)
#   ./scripts/run-tests.sh --valgrind         # Run under Valgrind (memory debugging)
#   ./scripts/run-tests.sh --output short     # Minimal output
#   ./scripts/run-tests.sh --filter "PoW"     # Run tests matching "PoW"
#

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/build"
MODULE=""
VERBOSE=false
COVERAGE=false
VALGRIND=false
OUTPUT_MODE="default"
FILTER=""

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_header() {
    echo -e "${YELLOW}[TESTS]${NC} $1"
}

print_pass() {
    echo -e "${GREEN}✓${NC} $1"
}

print_fail() {
    echo -e "${RED}✗${NC} $1"
}

print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

# Parse command-line arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --module)
            MODULE="$2"
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --coverage)
            COVERAGE=true
            shift
            ;;
        --valgrind)
            VALGRIND=true
            shift
            ;;
        --output)
            OUTPUT_MODE="$2"
            shift 2
            ;;
        --filter)
            FILTER="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  --module MODULE       Run tests for specific module (crypto, consensus, core, etc.)"
            echo "  --verbose, -v         Verbose test output"
            echo "  --coverage            Generate coverage report (requires gcov/lcov)"
            echo "  --valgrind            Run tests under Valgrind (memory debugging)"
            echo "  --output MODE         Output mode: default, short, quiet"
            echo "  --filter PATTERN      Run tests matching pattern (regex)"
            exit 1
            ;;
    esac
done

# ============================================================================
# Verify build directory exists
# ============================================================================
if [[ ! -d "${BUILD_DIR}" ]]; then
    print_fail "Build directory not found at ${BUILD_DIR}"
    echo ""
    echo "  Build the project first:"
    echo "    ./scripts/build.sh --preset dev"
    exit 1
fi

# ============================================================================
# Build ctest command
# ============================================================================
CTEST_CMD=(ctest --test-dir "${BUILD_DIR}")

# Add output mode
case "${OUTPUT_MODE}" in
    default)
        CTEST_CMD+=(--output-on-failure)
        ;;
    short)
        CTEST_CMD+=(--output-on-failure -q)
        ;;
    quiet)
        CTEST_CMD+=(-q)
        ;;
esac

# Add verbosity
if [[ "${VERBOSE}" == true ]]; then
    CTEST_CMD+=(-VV)
fi

# Add module filter
if [[ -n "${MODULE}" ]]; then
    CTEST_CMD+=(-L "${MODULE}")
fi

# Add custom filter
if [[ -n "${FILTER}" ]]; then
    CTEST_CMD+=(-R "${FILTER}")
fi

# ============================================================================
# Warn about coverage/valgrind requirements
# ============================================================================
if [[ "${COVERAGE}" == true ]]; then
    if ! command -v lcov &> /dev/null; then
        print_fail "lcov not found (required for coverage)"
        echo "  Install: sudo apt-get install lcov  (or equivalent)"
        exit 1
    fi
fi

if [[ "${VALGRIND}" == true ]]; then
    if ! command -v valgrind &> /dev/null; then
        print_fail "Valgrind not found"
        echo "  Install: sudo apt-get install valgrind  (or equivalent)"
        exit 1
    fi
fi

# ============================================================================
# Run tests
# ============================================================================
print_header "Running test suite"
if [[ -n "${MODULE}" ]]; then
    echo "  Module: ${MODULE}"
fi
if [[ -n "${FILTER}" ]]; then
    echo "  Filter: ${FILTER}"
fi
echo ""

TEST_FAILED=0

if [[ "${VALGRIND}" == true ]]; then
    print_info "Running under Valgrind (memory debugging)..."
    echo ""

    # Run each test under Valgrind
    if ! "${CTEST_CMD[@]}" --overwrite MemoryCheckCommand valgrind --memcheck; then
        TEST_FAILED=1
    fi
else
    # Run normal tests
    if ! "${CTEST_CMD[@]}"; then
        TEST_FAILED=1
    fi
fi

echo ""

# ============================================================================
# Generate coverage if requested
# ============================================================================
if [[ "${COVERAGE}" == true ]]; then
    print_header "Generating coverage report"

    # Reset coverage counters
    lcov --zerocounters --directory "${BUILD_DIR}"

    # Run tests again to collect coverage
    print_info "Re-running tests with coverage collection..."
    if ! "${CTEST_CMD[@]}" > /dev/null 2>&1; then
        print_fail "Tests failed during coverage run"
        TEST_FAILED=1
    fi

    # Generate coverage report
    print_info "Generating HTML coverage report..."
    COVERAGE_DIR="${BUILD_DIR}/coverage"
    mkdir -p "${COVERAGE_DIR}"

    if lcov --capture --directory "${BUILD_DIR}" --output-file "${COVERAGE_DIR}/coverage.info"; then
        if genhtml "${COVERAGE_DIR}/coverage.info" --output-directory "${COVERAGE_DIR}/html"; then
            print_pass "Coverage report generated"
            echo "  Open: file://${COVERAGE_DIR}/html/index.html"
        else
            print_fail "Failed to generate HTML report"
        fi
    else
        print_fail "Failed to capture coverage"
    fi

    echo ""
fi

# ============================================================================
# Summary
# ============================================================================
if [[ ${TEST_FAILED} -eq 0 ]]; then
    print_pass "All tests passed"
    exit 0
else
    print_fail "Some tests failed"
    exit 1
fi
