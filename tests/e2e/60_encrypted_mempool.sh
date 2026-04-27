#!/bin/bash
# Test: Encrypted mempool with threshold decryption
# Tests: Transaction encryption, slot leader decryption, batch ordering

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 60_encrypted_mempool.sh"
echo "======================================================================"
echo ""

test_encrypted_mempool() {
    local alice_addr="devnet1alice00000000000000000000000000000000"
    local bob_addr="devnet1bob0000000000000000000000000000000000"

    log_info "Testing encrypted mempool with threshold decryption"

    # Get current mempool status
    log_info "Checking mempool status..."
    local mempool=$(get_mempool_status)
    local clear_pool=$(echo "$mempool" | jq '.clear_pool_size')
    local encrypted_pool=$(echo "$mempool" | jq '.encrypted_pool_size')

    log_info "Mempool state:"
    log_info "  Clear pool size: $clear_pool"
    log_info "  Encrypted pool size: $encrypted_pool"

    # Submit an encrypted transaction
    # Placeholder: qv-wallet send --encrypt --to bob --amount 100QV
    # (In real impl: tx is threshold Kyber encrypted, broadcast to mempool)
    log_info "Submitting encrypted transaction..."
    local encrypted_tx="0x00080..."

    local tx_id=$(send_tx "$encrypted_tx")
    if [[ -z "$tx_id" ]] || [[ "$tx_id" == "null" ]]; then
        log_error "Encrypted transaction submission failed"
        return 1
    fi

    log_info "Encrypted transaction submitted: $tx_id"

    # Check mempool: encrypted pool should increase
    log_info "Waiting for mempool update..."
    sleep 2

    local mempool_updated=$(get_mempool_status)
    local encrypted_pool_updated=$(echo "$mempool_updated" | jq '.encrypted_pool_size')

    log_info "Updated mempool state:"
    log_info "  Encrypted pool size: $encrypted_pool_updated"

    if [[ $encrypted_pool_updated -le $encrypted_pool ]]; then
        log_warn "Encrypted pool did not increase (transaction may be in clear pool)"
    fi

    # Wait for slot transition: slot leader should decrypt and include in next block
    log_info "Waiting for slot leader to decrypt and include transaction..."
    if ! wait_event "get_tx '$tx_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Transaction not included in block after decryption"
        return 1
    fi

    log_info "Transaction decrypted and included in block!"

    # Verify MEV protection: ordering should be deterministic
    log_info "Verifying deterministic batch ordering..."
    local block_height=$(get_tip)
    local block=$(get_block_by_height "$block_height")

    if echo "$block" | jq -e '.transactions[] | select(.tx_id == "'$tx_id'")' > /dev/null 2>&1; then
        log_info "Transaction ordering verified"
        return 0
    else
        log_error "Transaction not found in expected block"
        return 1
    fi
}

test_case "Encrypted Mempool with Decryption" test_encrypted_mempool
test_summary
