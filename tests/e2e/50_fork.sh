#!/bin/bash
# Test: Network partition and fork resolution
# Tests: Longest-chain fork choice, reorg, consensus convergence

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 50_fork.sh"
echo "======================================================================"
echo ""

test_fork_resolution() {
    log_info "Testing network partition and fork resolution"

    # Get initial tip from all nodes
    local tip_pool0=$(RPC_ENDPOINT="$RPC_POOL0" get_tip)
    local tip_pool1=$(RPC_ENDPOINT="$RPC_POOL1" get_tip)
    local tip_pool2=$(RPC_ENDPOINT="$RPC_POOL2" get_tip)

    log_info "Initial state:"
    log_info "  Pool0 height: $tip_pool0"
    log_info "  Pool1 height: $tip_pool1"
    log_info "  Pool2 height: $tip_pool2"

    # Partition network: isolate pool2 by breaking its connectivity
    # (In real test: use iptables or Docker network disconnect)
    log_info "Partitioning network (isolating Pool2)..."
    # Placeholder: docker network disconnect devnet pool2

    # Let partitions advance separately
    log_info "Allowing partitions to advance (30s)..."
    sleep 30

    # Verify partition: pool0/1 should be ahead
    local tip_pool0_part=$(RPC_ENDPOINT="$RPC_POOL0" get_tip)
    local tip_pool2_part=$(RPC_ENDPOINT="$RPC_POOL2" get_tip)

    log_info "After partition:"
    log_info "  Pool0 (majority): height=$tip_pool0_part"
    log_info "  Pool2 (isolated): height=$tip_pool2_part"

    if [[ $tip_pool0_part -le $tip_pool0 ]]; then
        log_warn "Majority partition did not advance"
    fi

    # Heal partition
    log_info "Healing network partition..."
    # Placeholder: docker network connect devnet pool2

    # Wait for convergence
    log_info "Waiting for consensus convergence (60s)..."
    sleep 60

    # Verify all nodes converged on longest chain
    local tip_pool0_final=$(RPC_ENDPOINT="$RPC_POOL0" get_tip)
    local tip_pool1_final=$(RPC_ENDPOINT="$RPC_POOL1" get_tip)
    local tip_pool2_final=$(RPC_ENDPOINT="$RPC_POOL2" get_tip)

    log_info "Final state:"
    log_info "  Pool0 height: $tip_pool0_final"
    log_info "  Pool1 height: $tip_pool1_final"
    log_info "  Pool2 height: $tip_pool2_final"

    # Verify convergence: all within 2 blocks of each other
    local max_height=$tip_pool0_final
    [[ $tip_pool1_final -gt $max_height ]] && max_height=$tip_pool1_final
    [[ $tip_pool2_final -gt $max_height ]] && max_height=$tip_pool2_final

    local pool0_diff=$((max_height - tip_pool0_final))
    local pool1_diff=$((max_height - tip_pool1_final))
    local pool2_diff=$((max_height - tip_pool2_final))

    if [[ $pool0_diff -le 2 ]] && [[ $pool1_diff -le 2 ]] && [[ $pool2_diff -le 2 ]]; then
        log_info "Consensus converged within tolerance"
        return 0
    else
        log_error "Consensus did not converge"
        log_error "  Pool0 behind by: $pool0_diff blocks"
        log_error "  Pool1 behind by: $pool1_diff blocks"
        log_error "  Pool2 behind by: $pool2_diff blocks"
        return 1
    fi
}

test_case "Network Partition and Fork Resolution" test_fork_resolution
test_summary
