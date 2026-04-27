#!/bin/bash
# Smoke test: Bring up devnet, verify blocks are produced, tear down
# Tests: Docker Compose startup, block production, health checks

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
DEVNET_DIR="$PROJECT_ROOT/devnet"

# Source test helpers
source "$SCRIPT_DIR/lib.sh"

echo ""
echo "======================================================================"
echo "AŞAMA 13 E2E Test: 00_smoke.sh"
echo "======================================================================"
echo ""

# Check prerequisites
if ! command -v docker &> /dev/null; then
    log_error "docker not found in PATH"
    exit 1
fi

if ! command -v docker-compose &> /dev/null; then
    log_error "docker-compose not found in PATH"
    exit 1
fi

# Change to devnet directory
cd "$DEVNET_DIR"

log_info "Starting devnet (docker-compose up -d)..."
if ! docker-compose up -d 2>&1 | tee /tmp/docker-compose-up.log; then
    log_error "docker-compose up failed"
    docker-compose logs --tail=50
    docker-compose down --volumes
    exit 1
fi

log_info "Waiting for services to stabilize (30s)..."
sleep 30

log_info "Checking all services are healthy..."
if ! check_all_services; then
    log_error "Services failed health check"
    docker-compose logs --tail=100
    docker-compose down --volumes
    exit 1
fi

log_info "Verifying chain advancement..."
INITIAL_HEIGHT=$(get_tip)
log_info "Initial height: $INITIAL_HEIGHT"

if ! wait_tip 5 120; then
    log_error "Chain did not advance to height 5 within 120s"
    docker-compose logs --tail=100
    docker-compose down --volumes
    exit 1
fi

FINAL_HEIGHT=$(get_tip)
log_info "Final height: $FINAL_HEIGHT"

if [[ $FINAL_HEIGHT -lt 5 ]]; then
    log_error "Chain did not produce enough blocks"
    docker-compose down --volumes
    exit 1
fi

log_info "Checking all nodes are synced..."
for rpc in "$RPC_POOL0" "$RPC_POOL1" "$RPC_POOL2"; do
    RPC_ENDPOINT="$rpc" HEIGHT=$(get_tip)
    log_info "  Node at $rpc: height=$HEIGHT"
    if [[ $HEIGHT -lt $((FINAL_HEIGHT - 2)) ]]; then
        log_warn "Node at $rpc is behind (height=$HEIGHT vs tip=$FINAL_HEIGHT)"
    fi
done

log_info "Tearing down devnet..."
docker-compose down --volumes

echo ""
log_info "Smoke test completed successfully!"
echo ""
exit 0
