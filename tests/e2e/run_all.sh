#!/bin/bash
# E2E Test Suite Orchestrator
# Runs all test suites in order, collects results, generates summary

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
DEVNET_DIR="$PROJECT_ROOT/devnet"
LOG_DIR="/tmp/qv-e2e-logs"

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Ensure log directory exists
mkdir -p "$LOG_DIR"

echo ""
echo "======================================================================"
echo "QuantumVault AŞAMA 13 End-to-End Test Suite"
echo "======================================================================"
echo ""

# Test manifest
declare -a TESTS=(
    "00_smoke.sh"
    "10_simple_transfer.sh"
    "20_stealth_transfer.sh"
    "30_amm_swap.sh"
    "40_lending.sh"
    "50_fork.sh"
    "60_encrypted_mempool.sh"
)

declare -A TEST_RESULTS
declare -A TEST_DURATIONS

TOTAL_TESTS=${#TESTS[@]}
TESTS_PASSED=0
TESTS_FAILED=0

# Check Docker is available
if ! command -v docker &> /dev/null; then
    echo -e "${RED}ERROR: docker not found in PATH${NC}"
    exit 1
fi

echo -e "${BLUE}[SETUP]${NC} Bringing up devnet..."
cd "$DEVNET_DIR"

if ! docker-compose up -d 2>&1 | head -20; then
    echo -e "${RED}ERROR: Failed to bring up devnet${NC}"
    exit 1
fi

# Wait for services
sleep 30

echo ""
echo -e "${BLUE}[INFO]${NC} Running test suite..."
echo ""

for test_file in "${TESTS[@]}"; do
    test_name="${test_file%.sh}"
    test_path="$SCRIPT_DIR/$test_file"
    test_log="$LOG_DIR/$test_name.log"

    echo -e "${BLUE}[TEST]${NC} $test_name"
    echo "      Log: $test_log"

    local start_time=$SECONDS

    if bash "$test_path" > "$test_log" 2>&1; then
        TEST_RESULTS[$test_name]="PASS"
        echo -e "      ${GREEN}✓ PASS${NC}"
        ((TESTS_PASSED++))
    else
        TEST_RESULTS[$test_name]="FAIL"
        echo -e "      ${RED}✗ FAIL${NC}"
        echo "      --- Last 20 lines of log ---"
        tail -20 "$test_log" | sed 's/^/      /'
        ((TESTS_FAILED++))
    fi

    local end_time=$SECONDS
    local duration=$((end_time - start_time))
    TEST_DURATIONS[$test_name]=$duration
    echo ""
done

echo ""
echo -e "${BLUE}[TEARDOWN]${NC} Tearing down devnet..."
cd "$DEVNET_DIR"
docker-compose down --volumes --remove-orphans

echo ""
echo "======================================================================"
echo "Test Summary"
echo "======================================================================"
echo ""

for test_file in "${TESTS[@]}"; do
    test_name="${test_file%.sh}"
    result="${TEST_RESULTS[$test_name]}"
    duration="${TEST_DURATIONS[$test_name]:-0}"

    if [[ "$result" == "PASS" ]]; then
        echo -e "${GREEN}✓${NC} $test_name (${duration}s)"
    else
        echo -e "${RED}✗${NC} $test_name (${duration}s)"
    fi
done

echo ""
echo "----"
echo "Total:  $TOTAL_TESTS"
echo -e "Passed: ${GREEN}$TESTS_PASSED${NC}"
echo -e "Failed: ${RED}$TESTS_FAILED${NC}"
echo ""

if [[ $TESTS_FAILED -eq 0 ]]; then
    echo -e "${GREEN}All tests passed!${NC}"
    echo ""
    echo "Test logs: $LOG_DIR"
    exit 0
else
    echo -e "${RED}$TESTS_FAILED test(s) failed.${NC}"
    echo ""
    echo "Test logs: $LOG_DIR"
    echo "Run individual test for details: bash tests/e2e/<test>.sh"
    exit 1
fi
