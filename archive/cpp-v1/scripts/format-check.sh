#!/bin/bash
#
# format-check.sh — Check code formatting with clang-format
#
# Finds all .cpp and .hpp files in src/ and include/, checks formatting,
# and reports which files need formatting.
#
# Usage:
#   ./scripts/format-check.sh          # Check formatting, exit 1 if any differ
#   ./scripts/format-check.sh --fix    # Auto-format all files
#

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIX_MODE=false

if [[ "${1:-}" == "--fix" ]]; then
    FIX_MODE=true
fi

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_pass() {
    echo -e "${GREEN}✓${NC} $1"
}

print_fail() {
    echo -e "${RED}✗${NC} $1"
}

print_info() {
    echo -e "${YELLOW}ℹ${NC} $1"
}

# ============================================================================
# Find all C++ source and header files
# ============================================================================
echo "Searching for C++ files in ${PROJECT_ROOT}..."

TOTAL_FILES=0
BAD_FORMAT_FILES=()
FIXED_FILES=0

find "${PROJECT_ROOT}/src" "${PROJECT_ROOT}/include" -type f \( -name "*.cpp" -o -name "*.hpp" \) 2>/dev/null | while read -r file; do
    ((TOTAL_FILES++))

    if [[ "${FIX_MODE}" == true ]]; then
        # Auto-format the file in-place
        if clang-format -i "${file}"; then
            ((FIXED_FILES++))
        fi
    else
        # Check if file needs formatting
        original=$(cat "${file}")
        formatted=$(clang-format "${file}")

        if [[ "${original}" != "${formatted}" ]]; then
            BAD_FORMAT_FILES+=("${file}")
        fi
    fi
done

echo ""

# ============================================================================
# Report results
# ============================================================================
if [[ "${FIX_MODE}" == true ]]; then
    print_pass "Auto-formatted complete"
    echo "  Total files processed: ${TOTAL_FILES}"
    echo "  Files modified: ${FIXED_FILES}"
    exit 0
else
    if [[ ${#BAD_FORMAT_FILES[@]} -eq 0 ]]; then
        print_pass "All C++ files are properly formatted"
        echo "  Files checked: ${TOTAL_FILES}"
        exit 0
    else
        print_fail "Found ${#BAD_FORMAT_FILES[@]} file(s) that need formatting"
        echo "  Files with incorrect formatting:"
        for file in "${BAD_FORMAT_FILES[@]}"; do
            echo "    - ${file}"
        done
        echo ""
        echo "  To auto-format all files:"
        echo "    ./scripts/format-check.sh --fix"
        echo ""
        exit 1
    fi
fi
