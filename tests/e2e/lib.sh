#!/bin/bash
# Shared helpers for QuantumVault e2e tests

set -euo pipefail

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration (set by callers or defaults)
: "${RPC_POOL0:=http://localhost:9944}"
: "${RPC_POOL1:=http://localhost:9945}"
: "${RPC_POOL2:=http://localhost:9946}"
: "${FAUCET_URL:=http://localhost:5001}"
: "${EXPLORER_URL:=http://localhost:5000}"

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

##############################################################################
# RPC Utilities
##############################################################################

rpc() {
    # Call JSON-RPC method on pool0 (or specified endpoint)
    local method="$1"
    shift
    local params=()
    while [[ $# -gt 0 ]]; do
        params+=("$1")
        shift
    done

    local endpoint="${RPC_ENDPOINT:-$RPC_POOL0}"
    local payload="{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[$( printf '"%s",' "${params[@]}" | sed 's/,$// )],\"id\":1}"

    curl -s -X POST "$endpoint" \
        -H "Content-Type: application/json" \
        -d "$payload" | jq -r '.result // .error'
}

rpc_raw() {
    # Raw RPC call (returns full JSON response including errors)
    local method="$1"
    shift
    local params=()
    while [[ $# -gt 0 ]]; do
        params+=("$1")
        shift
    done

    local endpoint="${RPC_ENDPOINT:-$RPC_POOL0}"
    local payload="{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":[$( printf '"%s",' "${params[@]}" | sed 's/,$// )],\"id\":1}"

    curl -s -X POST "$endpoint" \
        -H "Content-Type: application/json" \
        -d "$payload"
}

get_tip() {
    # Get current block height
    rpc "qv_getTip" | jq -r '.height // 0'
}

get_block_by_height() {
    # Get block by height
    local height="$1"
    rpc "qv_getBlockByHeight" "$height"
}

get_tx() {
    # Get transaction by ID
    local tx_id="$1"
    rpc "qv_getTx" "$tx_id"
}

send_tx() {
    # Send serialized transaction to mempool
    local tx_hex="$1"
    local result=$(rpc_raw "qv_sendTransaction" "$tx_hex")
    echo "$result" | jq -r '.result // .error // "unknown"'
}

get_utxo() {
    # Get UTXO by outpoint
    local outpoint="$1"
    rpc "qv_getUtxo" "$outpoint"
}

get_balance_for() {
    # Get balance for stealth address (view key)
    local view_key="$1"
    rpc "qv_getBalanceFor" "$view_key"
}

scan_stealth() {
    # Scan stealth outputs in block range
    local view_key="$1"
    local from_height="${2:-0}"
    local to_height="${3:-999999}"
    rpc "qv_scanStealth" "$view_key" "$from_height" "$to_height"
}

get_mempool_status() {
    # Get mempool statistics
    rpc "qv_getMempoolStatus"
}

##############################################################################
# Wait Utilities
##############################################################################

wait_tip() {
    # Wait for chain to advance to at least height N
    local target_height="$1"
    local max_wait="${2:-60}"  # seconds
    local start=$SECONDS

    echo -n "Waiting for height $target_height..."
    while [[ $((SECONDS - start)) -lt $max_wait ]]; do
        local height=$(get_tip)
        if [[ $height -ge $target_height ]]; then
            echo -e " ${GREEN}✓${NC} (height=$height)"
            return 0
        fi
        echo -n "."
        sleep 1
    done

    echo -e " ${RED}timeout${NC}"
    return 1
}

wait_event() {
    # Wait for an event to occur (polling a condition)
    local condition="$1"
    local max_wait="${2:-60}"
    local start=$SECONDS

    echo -n "Waiting for event: $condition..."
    while [[ $((SECONDS - start)) -lt $max_wait ]]; do
        if eval "$condition"; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        fi
        echo -n "."
        sleep 1
    done

    echo -e " ${RED}timeout${NC}"
    return 1
}

##############################################################################
# Assertion Utilities
##############################################################################

assert_eq() {
    # Assert two values are equal
    local expected="$1"
    local actual="$2"
    local msg="${3:-assertion}"

    if [[ "$expected" == "$actual" ]]; then
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}: $msg"
        echo "  Expected: $expected"
        echo "  Actual:   $actual"
        return 1
    fi
}

assert_ne() {
    # Assert two values are NOT equal
    local unexpected="$1"
    local actual="$2"
    local msg="${3:-assertion}"

    if [[ "$unexpected" != "$actual" ]]; then
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}: $msg"
        echo "  Should not equal: $unexpected"
        return 1
    fi
}

assert_grep() {
    # Assert string contains pattern
    local pattern="$1"
    local text="$2"
    local msg="${3:-assertion}"

    if echo "$text" | grep -q "$pattern"; then
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}: $msg"
        echo "  Pattern: $pattern"
        echo "  Text: $text"
        return 1
    fi
}

assert_success() {
    # Assert command succeeds
    local cmd="$1"
    local msg="${2:-command}"

    if eval "$cmd" > /dev/null 2>&1; then
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}: $msg"
        echo "  Command: $cmd"
        return 1
    fi
}

##############################################################################
# Test Harness
##############################################################################

test_case() {
    # Register and run a test case
    local test_name="$1"
    local test_func="$2"

    echo ""
    echo -e "${BLUE}[TEST]${NC} $test_name"

    if $test_func; then
        echo -e "${GREEN}✓ PASS${NC}: $test_name"
        ((TESTS_PASSED++))
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}: $test_name"
        ((TESTS_FAILED++))
        return 1
    fi
}

test_summary() {
    # Print test summary
    local total=$((TESTS_PASSED + TESTS_FAILED + TESTS_SKIPPED))
    echo ""
    echo "======================================================================"
    echo "Test Summary"
    echo "======================================================================"
    echo "Total:    $total"
    echo -e "Passed:   ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Failed:   ${RED}$TESTS_FAILED${NC}"
    echo -e "Skipped:  ${YELLOW}$TESTS_SKIPPED${NC}"
    echo ""

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}All tests passed!${NC}"
        return 0
    else
        echo -e "${RED}Some tests failed.${NC}"
        return 1
    fi
}

##############################################################################
# Service Checks
##############################################################################

check_service() {
    # Check if service is up
    local url="$1"
    local max_retries=30
    local i=0

    echo -n "Checking service $url..."
    while [[ $i -lt $max_retries ]]; do
        if curl -s "$url/health" > /dev/null 2>&1; then
            echo -e " ${GREEN}✓${NC}"
            return 0
        fi
        echo -n "."
        sleep 1
        ((i++))
    done

    echo -e " ${RED}timeout${NC}"
    return 1
}

check_all_services() {
    # Check all devnet services
    echo "Checking services..."
    check_service "$RPC_POOL0" || return 1
    check_service "$RPC_POOL1" || return 1
    check_service "$RPC_POOL2" || return 1
    check_service "$FAUCET_URL" || return 1
    check_service "$EXPLORER_URL" || return 1
    echo "All services healthy"
}

##############################################################################
# Logging
##############################################################################

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

##############################################################################
# Export for subshells
##############################################################################

export RPC_POOL0 RPC_POOL1 RPC_POOL2 FAUCET_URL EXPLORER_URL
export TESTS_PASSED TESTS_FAILED TESTS_SKIPPED
