#!/bin/bash
# Test: Transfer to stealth address and scanner verification
# Tests: Stealth address protocol, view key scanning, output detection

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 20_stealth_transfer.sh"
echo "======================================================================"
echo ""

test_stealth_transfer() {
    local sender_addr="devnet1alice00000000000000000000000000000000"
    local recipient_view_key="abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    local transfer_amount=50000000000  # 500 QV

    log_info "Testing stealth address transfer"
    log_info "  Sender: Alice ($sender_addr)"
    log_info "  Recipient view key: ${recipient_view_key:0:16}..."
    log_info "  Amount: $((transfer_amount / 1e8)) QV"

    # Create transaction with stealth output
    # Placeholder: qv-wallet create-stealth --to-view-key $recipient_view_key --amount 500QV
    log_info "Creating stealth transfer transaction..."
    local tx_hex="0x0002..."

    # Submit transaction
    log_info "Submitting transaction..."
    local tx_id=$(send_tx "$tx_hex")
    log_info "Transaction ID: $tx_id"

    if [[ -z "$tx_id" ]] || [[ "$tx_id" == "null" ]]; then
        log_error "Transaction submission failed"
        return 1
    fi

    # Wait for block inclusion
    log_info "Waiting for transaction finality..."
    if ! wait_event "get_tx '$tx_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Transaction not found in chain"
        return 1
    fi

    # Scan for stealth outputs using recipient's view key
    local from_height=0
    local to_height=$(get_tip)

    log_info "Scanning blocks [$from_height, $to_height] for stealth outputs..."
    local scan_result=$(scan_stealth "$recipient_view_key" "$from_height" "$to_height")

    # Verify stealth output was detected
    if echo "$scan_result" | jq -e '.[0].tx_id' > /dev/null 2>&1; then
        log_info "Stealth output detected:"
        echo "$scan_result" | jq '.[] | {height, tx_id: .tx_id[0:16], output_index, value}'
        return 0
    else
        log_error "Stealth output not detected in scan"
        echo "Scan result: $scan_result"
        return 1
    fi
}

test_case "Stealth Transfer with Scanner" test_stealth_transfer
test_summary
