#!/bin/bash
#
# build.sh — Unified build script for QuantumVault
#
# Configures and builds the project with CMake and Ninja.
#
# Usage:
#   ./scripts/build.sh                          # Build with default (dev) preset
#   ./scripts/build.sh --preset release         # Release build
#   ./scripts/build.sh --preset sanitize        # Debug with ASAN+UBSAN
#   ./scripts/build.sh --clean                  # Clean build directory first
#   ./scripts/build.sh --jobs 4                 # Use 4 parallel jobs
#   ./scripts/build.sh --test                   # Build and run tests
#   ./scripts/build.sh --preset release --test  # Release build + run tests
#

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${PROJECT_ROOT}/build"
PRESET="dev"
CLEAN_BUILD=false
JOBS=""
RUN_TESTS=false

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

print_header() {
    echo -e "${YELLOW}[BUILD]${NC} $1"
}

print_pass() {
    echo -e "${GREEN}✓${NC} $1"
}

print_step() {
    echo -e "${BLUE}→${NC} $1"
}

# Parse command-line arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --preset)
            PRESET="$2"
            shift 2
            ;;
        --clean)
            CLEAN_BUILD=true
            shift
            ;;
        --jobs)
            JOBS="-j$2"
            shift 2
            ;;
        --test)
            RUN_TESTS=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  --preset PRESET    Build preset: dev, release, sanitize (default: dev)"
            echo "  --clean            Clean build directory before building"
            echo "  --jobs N           Number of parallel jobs (default: auto)"
            echo "  --test             Run tests after building"
            exit 1
            ;;
    esac
done

# ============================================================================
# Validate preset
# ============================================================================
case "${PRESET}" in
    dev|release|sanitize)
        ;;
    *)
        echo "Error: Unknown preset '${PRESET}'"
        echo "Valid presets: dev, release, sanitize"
        exit 1
        ;;
esac

# ============================================================================
# Clean if requested
# ============================================================================
if [[ "${CLEAN_BUILD}" == true ]]; then
    print_step "Cleaning build directory..."
    if [[ -d "${BUILD_DIR}" ]]; then
        rm -rf "${BUILD_DIR}"
        print_pass "Build directory cleaned"
    fi
    echo ""
fi

# ============================================================================
# Create build directory
# ============================================================================
if [[ ! -d "${BUILD_DIR}" ]]; then
    mkdir -p "${BUILD_DIR}"
fi

# ============================================================================
# Configure
# ============================================================================
print_header "Configuring (preset: ${PRESET})"
print_step "Running: cmake --preset ${PRESET}"

if cmake --preset "${PRESET}" -B "${BUILD_DIR}" -S "${PROJECT_ROOT}"; then
    print_pass "Configuration successful"
else
    echo ""
    echo "Configuration failed. Check CMakeError.log or CMakeOutput.log."
    exit 1
fi

echo ""

# ============================================================================
# Build
# ============================================================================
print_header "Building"

# Determine default jobs if not specified
if [[ -z "${JOBS}" ]]; then
    if command -v nproc &> /dev/null; then
        num_jobs=$(nproc)
        JOBS="-j${num_jobs}"
    else
        JOBS="-j4"
    fi
fi

print_step "Running: ninja -C ${BUILD_DIR} ${JOBS}"

if ninja -C "${BUILD_DIR}" ${JOBS}; then
    print_pass "Build successful"
else
    echo ""
    echo "Build failed. Check output above for errors."
    exit 1
fi

echo ""

# ============================================================================
# Run tests if requested
# ============================================================================
if [[ "${RUN_TESTS}" == true ]]; then
    print_header "Running tests"
    print_step "Running: ctest --test-dir ${BUILD_DIR} --output-on-failure"
    echo ""

    if ctest --test-dir "${BUILD_DIR}" --output-on-failure; then
        print_pass "All tests passed"
    else
        echo ""
        echo "Some tests failed. Check output above."
        exit 1
    fi
fi

# ============================================================================
# Summary
# ============================================================================
echo ""
print_header "Build complete"
echo "  Preset:   ${PRESET}"
echo "  Build dir: ${BUILD_DIR}"
echo "  Output:    ${BUILD_DIR}/bin"

if [[ "${RUN_TESTS}" == true ]]; then
    echo "  Tests:    PASSED"
fi

echo ""
print_pass "Ready to use"
