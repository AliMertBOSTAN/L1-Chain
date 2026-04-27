#!/bin/bash
#
# format-all.sh — Auto-format all C++ code with clang-format
#
# Formats all .cpp and .hpp files in src/, include/, tests/, and tools/
# using clang-format -i (in-place modification).
#

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_header() {
    echo -e "${YELLOW}[FORMAT]${NC} $1"
}

print_pass() {
    echo -e "${GREEN}✓${NC} $1"
}

# ============================================================================
# Find and format all C++ files
# ============================================================================
print_header "Formatting all C++ files in the project..."
echo "  Root: ${PROJECT_ROOT}"
echo ""

TOTAL_FILES=0
FORMATTED_FILES=0

# Function to format files in a directory
format_directory() {
    local dir="$1"
    local dir_name="$(basename "${dir}")"

    if [[ ! -d "${dir}" ]]; then
        return
    fi

    echo -n "  ${dir_name:10}... "

    local count=0
    local formatted=0

    find "${dir}" -type f \( -name "*.cpp" -o -name "*.hpp" \) 2>/dev/null | while read -r file; do
        ((count++))
        ((TOTAL_FILES++))

        if clang-format -i "${file}"; then
            ((formatted++))
            ((FORMATTED_FILES++))
        fi
    done

    echo "formatted ${count} file(s)"
}

# Format all relevant directories
format_directory "${PROJECT_ROOT}/src"
format_directory "${PROJECT_ROOT}/include"
format_directory "${PROJECT_ROOT}/tests"
format_directory "${PROJECT_ROOT}/tools"

echo ""
print_pass "Formatting complete"
echo "  Total files processed: ${TOTAL_FILES}"
echo ""
echo "All C++ code has been auto-formatted. Review changes before committing."
