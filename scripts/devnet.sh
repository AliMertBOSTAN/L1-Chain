#!/usr/bin/env bash
set -euo pipefail

# QuantumVault Local Devnet Launcher
# Starts 3 nodes with shared genesis for local testing.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARY="${PROJECT_ROOT}/target/release/qv-node"
DATA_ROOT="${PROJECT_ROOT}/data/devnet"

# Port assignments
NODE0_P2P=10333
NODE0_RPC=8545
NODE0_METRICS=9090

NODE1_P2P=10334
NODE1_RPC=8546
NODE1_METRICS=9091

NODE2_P2P=10335
NODE2_RPC=8547
NODE2_METRICS=9092

echo "=== QuantumVault Devnet Launcher ==="
echo ""

# Step 1: Build
echo "[1/5] Building release binary..."
cd "${PROJECT_ROOT}"
cargo build --release -p qv-node 2>&1 | tail -5

# Step 2: Clean data directories
echo "[2/5] Preparing data directories..."
rm -rf "${DATA_ROOT}"
mkdir -p "${DATA_ROOT}/node0" "${DATA_ROOT}/node1" "${DATA_ROOT}/node2"

# Step 3: Initialize node0 (generates genesis)
echo "[3/5] Initializing node0 (genesis generation)..."
"${BINARY}" --init \
    --network devnet \
    --data-dir "${DATA_ROOT}/node0" \
    --listen "/ip4/127.0.0.1/tcp/${NODE0_P2P}" \
    --rpc-addr "127.0.0.1:${NODE0_RPC}" \
    --metrics-addr "127.0.0.1:${NODE0_METRICS}" \
    -c "${DATA_ROOT}/node0/config.toml"

echo ""

# Step 4: Start nodes
echo "[4/5] Starting 3 devnet nodes..."

# Node 0 (genesis producer)
"${BINARY}" \
    --network devnet \
    --data-dir "${DATA_ROOT}/node0" \
    --listen "/ip4/127.0.0.1/tcp/${NODE0_P2P}" \
    --rpc-addr "127.0.0.1:${NODE0_RPC}" \
    --metrics-addr "127.0.0.1:${NODE0_METRICS}" \
    -c "${DATA_ROOT}/node0/config.toml" \
    &
PID0=$!

sleep 1

# Node 1
"${BINARY}" \
    --network devnet \
    --data-dir "${DATA_ROOT}/node1" \
    --listen "/ip4/127.0.0.1/tcp/${NODE1_P2P}" \
    --rpc-addr "127.0.0.1:${NODE1_RPC}" \
    --metrics-addr "127.0.0.1:${NODE1_METRICS}" \
    --bootstrap "/ip4/127.0.0.1/tcp/${NODE0_P2P}" \
    &
PID1=$!

# Node 2
"${BINARY}" \
    --network devnet \
    --data-dir "${DATA_ROOT}/node2" \
    --listen "/ip4/127.0.0.1/tcp/${NODE2_P2P}" \
    --rpc-addr "127.0.0.1:${NODE2_RPC}" \
    --metrics-addr "127.0.0.1:${NODE2_METRICS}" \
    --bootstrap "/ip4/127.0.0.1/tcp/${NODE0_P2P}" \
    &
PID2=$!

echo "  Node 0: PID=${PID0} (P2P: ${NODE0_P2P}, RPC: ${NODE0_RPC})"
echo "  Node 1: PID=${PID1} (P2P: ${NODE1_P2P}, RPC: ${NODE1_RPC})"
echo "  Node 2: PID=${PID2} (P2P: ${NODE2_P2P}, RPC: ${NODE2_RPC})"

# Step 5: Wait and show status
echo ""
echo "[5/5] Devnet running. Press Ctrl-C to stop all nodes."
echo ""
echo "  Genesis keys: ${DATA_ROOT}/node0/genesis-keys.json"
echo "  RPC endpoint: http://127.0.0.1:${NODE0_RPC}"
echo ""

# Cleanup on exit
cleanup() {
    echo ""
    echo "Shutting down devnet..."
    kill ${PID0} ${PID1} ${PID2} 2>/dev/null || true
    wait ${PID0} ${PID1} ${PID2} 2>/dev/null || true
    echo "Done."
}
trap cleanup EXIT INT TERM

# Wait for any node to exit
wait -n ${PID0} ${PID1} ${PID2} 2>/dev/null || true
