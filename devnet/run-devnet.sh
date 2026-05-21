#!/usr/bin/env bash
# QuantumVault — 4-node local devnet launcher.
#   ./run-devnet.sh start | stop | status
# Env: QV_NODE_BIN, QV_DEVNET_WORK, QV_WARMUP, QV_STAGGER
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORK="${QV_DEVNET_WORK:-$SCRIPT_DIR/work4}"
WARMUP="${QV_WARMUP:-12}"
STAGGER="${QV_STAGGER:-1}"

P2P=(17001 17002 17003 17004)
RPC=(8545 8546 8547 8548)
MET=(9601 9602 9603 9604)
VRF=(
  "1111111111111111111111111111111111111111111111111111111111111111"
  "2222222222222222222222222222222222222222222222222222222222222222"
  "3333333333333333333333333333333333333333333333333333333333333333"
  "4444444444444444444444444444444444444444444444444444444444444444")
NKEY=(
  "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"
  "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"
  "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3"
  "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4")
STAKE=2500000000000

cmd="${1:-start}"
stop_devnet() {
  if [ -f "$WORK/pids" ]; then
    for pid in $(cat "$WORK/pids"); do kill "$pid" 2>/dev/null || true; done
    rm -f "$WORK/pids"; echo "[stop] devnet stopped."
  else echo "[stop] no running devnet."; fi
}
status_devnet() {
  for i in 0 1 2 3; do
    b='{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
    r="$(curl -s --max-time 3 -X POST "http://127.0.0.1:${RPC[$i]}" -H 'Content-Type: application/json' -d "$b" 2>/dev/null||true)"
    echo "  node$i rpc=${RPC[$i]} ${r:-<unreachable>}"
  done
}
case "$cmd" in
  stop) stop_devnet; exit 0;;
  status) status_devnet; exit 0;;
  start) ;;
  *) echo "usage: $0 {start|stop|status}"; exit 1;;
esac

if [ -n "${QV_NODE_BIN:-}" ]; then
  BIN="$QV_NODE_BIN"
else
  echo "[build] cargo build -p qv-node ..."
  ( cd "$PROJECT_ROOT" && cargo build -p qv-node )
  # Ask cargo for the real target directory (honours any custom target-dir).
  TARGET_DIR="$(cd "$PROJECT_ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
      | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)"
  [ -n "$TARGET_DIR" ] || TARGET_DIR="$PROJECT_ROOT/target"
  BIN="$TARGET_DIR/debug/qv-node"
fi
[ -x "$BIN" ] || { echo "qv-node binary not found: $BIN"; echo "Hint: pass QV_NODE_BIN=<path> explicitly."; exit 1; }
echo "[bin] $BIN"
mkdir -p "$WORK"; rm -f "$WORK"/node*.log "$WORK"/node*.toml "$WORK/pids"

for i in 0 1 2 3; do
  cfg="$WORK/node$i.toml"; seeds=""
  for j in 0 1 2 3; do [ "$j" -ne "$i" ] && seeds="${seeds}\"/ip4/127.0.0.1/tcp/${P2P[$j]}\", "; done
  {
    echo "network = \"devnet\""
    echo "data_dir = \"$WORK/node$i-data\""
    echo "listen_addr = \"/ip4/127.0.0.1/tcp/${P2P[$i]}\""
    echo "rpc_addr = \"127.0.0.1:${RPC[$i]}\""
    echo "metrics_addr = \"127.0.0.1:${MET[$i]}\""
    echo "bootstrap_peers = []"
    echo "seed_nodes = [${seeds%, }]"
    echo "storage_backend = \"memory\""
    echo "node_key_seed_hex = \"${NKEY[$i]}\""
    echo "round_robin_leader = true"
    echo "startup_warmup_secs = $WARMUP"
    echo ""; echo "[gossip]"
    echo "max_peers = 64"; echo "max_inbound_peers = 32"; echo "target_outbound_peers = 16"
    echo "message_ttl = 16"; echo "heartbeat_interval_ms = 1000"
    echo ""; echo "[mempool]"
    echo "max_clear_pool_size = 10000"; echo "max_encrypted_pool_size = 1000"
    echo "min_fee_rate = 0"; echo "tx_ttl_slots = 200"
    echo ""; echo "[stake_pool]"
    echo "vrf_seed_hex = \"${VRF[$i]}\""
    echo "initial_stake = $STAKE"; echo "active_slot_coeff = 0.05"
    for j in 0 1 2 3; do
      echo ""; echo "[[genesis_pools]]"
      echo "vrf_seed_hex = \"${VRF[$j]}\""; echo "stake = $STAKE"
    done
  } > "$cfg"
done
echo "[config] 4 node configs written to $WORK"

PIDS=()
for i in 0 1 2 3; do
  ( cd "$PROJECT_ROOT" && exec "$BIN" --config "$WORK/node$i.toml" --network devnet --log-level info ) > "$WORK/node$i.log" 2>&1 &
  PIDS+=("$!")
  echo "  node$i pid=$! rpc=127.0.0.1:${RPC[$i]} p2p=${P2P[$i]} metrics=${MET[$i]}"
  sleep "$STAGGER"
done
echo "${PIDS[@]}" > "$WORK/pids"
echo "[up] 4-node devnet running (warmup ${WARMUP}s). logs: $WORK/nodeN.log"
echo "     monitor: python3 $SCRIPT_DIR/monitor.py --work $WORK"
echo "     stop:    $0 stop"
