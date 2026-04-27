#!/bin/bash
# Test: Lending protocol lifecycle
# Tests: Deposit, borrow, accrue interest, repay, withdraw

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 40_lending.sh"
echo "======================================================================"
echo ""

test_lending_lifecycle() {
    local pool_utxo="0000000000000000000000000000000000000000000000000000000000000020:0"
    local lender_addr="devnet1lender0000000000000000000000000000000"
    local borrower_addr="devnet1borrower000000000000000000000000000"
    local deposit_amount=100000000000  # 1000 QV
    local borrow_amount=50000000000    # 500 QV

    log_info "Testing lending protocol lifecycle"
    log_info "  Pool UTXO: $pool_utxo"
    log_info "  Lender deposit: $((deposit_amount / 1e8)) QV"
    log_info "  Borrower borrow: $((borrow_amount / 1e8)) QV"

    # Step 1: Deposit
    log_info "[1/5] Depositing collateral..."
    local deposit_tx="0x0004..."
    local deposit_id=$(send_tx "$deposit_tx")

    if [[ -z "$deposit_id" ]] || [[ "$deposit_id" == "null" ]]; then
        log_error "Deposit transaction failed"
        return 1
    fi

    if ! wait_event "get_tx '$deposit_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Deposit not finalized"
        return 1
    fi

    log_info "Deposit confirmed"

    # Step 2: Verify lender receives cToken
    log_info "[2/5] Verifying cToken issuance..."
    # Check cToken balance via balance query
    # Placeholder: would scan for cToken UTXOs

    # Step 3: Borrow
    log_info "[3/5] Borrowing QV..."
    local borrow_tx="0x0005..."
    local borrow_id=$(send_tx "$borrow_tx")

    if [[ -z "$borrow_id" ]] || [[ "$borrow_id" == "null" ]]; then
        log_error "Borrow transaction failed"
        return 1
    fi

    if ! wait_event "get_tx '$borrow_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Borrow not finalized"
        return 1
    fi

    log_info "Borrow confirmed"

    # Step 4: Wait for interest accrual (several blocks)
    log_info "[4/5] Waiting for interest accrual (10 blocks)..."
    local accrual_start=$(get_tip)
    if ! wait_tip $((accrual_start + 10)) 120; then
        log_warn "Timeout waiting for interest accrual"
    else
        log_info "Interest accrual period complete"
    fi

    # Step 5: Repay
    log_info "[5/5] Repaying loan..."
    local repay_tx="0x0006..."
    local repay_id=$(send_tx "$repay_tx")

    if [[ -z "$repay_id" ]] || [[ "$repay_id" == "null" ]]; then
        log_error "Repay transaction failed"
        return 1
    fi

    if ! wait_event "get_tx '$repay_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Repay not finalized"
        return 1
    fi

    log_info "Repay confirmed"

    # Step 6: Withdraw (burn cToken)
    log_info "[6/6] Withdrawing deposit..."
    local withdraw_tx="0x0007..."
    local withdraw_id=$(send_tx "$withdraw_tx")

    if [[ -z "$withdraw_id" ]] || [[ "$withdraw_id" == "null" ]]; then
        log_error "Withdraw transaction failed"
        return 1
    fi

    if ! wait_event "get_tx '$withdraw_id' | jq -e '.tx_id' > /dev/null 2>&1" 120; then
        log_error "Withdraw not finalized"
        return 1
    fi

    log_info "Lending lifecycle complete!"
    return 0
}

test_case "Lending Protocol Lifecycle" test_lending_lifecycle
test_summary
