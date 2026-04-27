#!/bin/bash
# Test: Simple UTXO transfer from Alice to Bob
# Tests: Transaction creation, signing, mempool submission, finality

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 10_simple_transfer.sh"
echo "======================================================================"
echo ""

test_simple_transfer() {
    local alice_addr="devnet1alice00000000000000000000000000000000"
    local bob_addr="devnet1bob0000000000000000000000000000000000"
    local transfer_amount=10000000000  # 100 QV in satoshis

    log_info "Testing simple UTXO transfer"
    log_info "  From: Alice ($alice_addr)"
    log_info "  To:   Bob ($bob_addr)"
    log_info "  Amount: $((transfer_amount / 1e8)) QV"

    # Get Alice's initial balance
    local alice_initial=$(get_balance_for "$alice_addr")
    log_info "Alice initial balance: $((alice_initial / 1e8)) QV"

    if [[ $alice_initial -lt $transfer_amount ]]; then
        log_error "Alice has insufficient balance"
        return 1
    fi

    # Create and sign transaction (placeholder: would use qv-wallet)
    # For now, we simulate: tx_create_transfer --from alice --to bob --amount 100QV
    log_info "Creating transaction..."
    local tx_hex="0x0001..."  # Placeholder

    # Submit transaction
    log_info "Submitting transaction..."
    local tx_id=$(send_tx "$tx_hex")
    log_info "Transaction ID: $tx_id"

    if [[ -z "$tx_id" ]] || [[ "$tx_id" == "null" ]]; then
        log_error "Transaction submission failed"
        return 1
    fi

    # Wait for inclusion in a block (k-deep finality)
    log_info "Waiting for transaction finality..."
    if ! wait_event "get_tx '$tx_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Transaction not found in chain"
        return 1
    fi

    # Check Bob's new balance
    local bob_final=$(get_balance_for "$bob_addr")
    log_info "Bob final balance: $((bob_final / 1e8)) QV"

    # Verify amount reached Bob
    if [[ $bob_final -lt $transfer_amount ]]; then
        log_error "Transfer did not complete: Bob's balance is $bob_final"
        return 1
    fi

    log_info "Transfer completed successfully!"
    return 0
}

test_case "Simple UTXO Transfer" test_simple_transfer
test_summary
