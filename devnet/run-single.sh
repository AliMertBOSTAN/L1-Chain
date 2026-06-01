#!/usr/bin/env bash
# QuantumVault — tek-node devnet + cüzdan UI (bash).
#
# Kullanım:
#   ./run-single.sh start | stop | status | clean
#
# Çevre değişkenleri:
#   QV_SINGLE_WORK   varsayılan: devnet/work-single
#   QV_WALLET_PW     varsayılan: devnetpw

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORK="${QV_SINGLE_WORK:-$SCRIPT_DIR/work-single}"

RPC_PORT=8545
METRICS_PORT=9601
P2P_PORT=17001
WALLET_PORT=7777
WALLET_PW="${QV_WALLET_PW:-devnetpw}"

stop_single() {
  if [ -f "$WORK/pids" ]; then
    for pid in $(cat "$WORK/pids"); do
      kill "$pid" 2>/dev/null || true
    done
    rm -f "$WORK/pids"
    echo "[stop] tek-node devnet durduruldu."
  else
    echo "[stop] çalışan bir tek-node bulunamadı."
  fi
}

show_status() {
  body='{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}'
  if r=$(curl -s --max-time 3 -X POST "http://127.0.0.1:$RPC_PORT" \
        -H 'Content-Type: application/json' -d "$body" 2>/dev/null); then
    echo "  node     rpc=$RPC_PORT   ${r}"
  else
    echo "  node     rpc=$RPC_PORT   <unreachable>"
  fi
  if r=$(curl -s --max-time 3 "http://127.0.0.1:$WALLET_PORT/api/status" 2>/dev/null); then
    echo "  wallet   http=$WALLET_PORT  ${r}"
  else
    echo "  wallet   http=$WALLET_PORT  <unreachable>"
  fi
}

clean_single() {
  stop_single
  if [ -d "$WORK" ]; then
    rm -rf "$WORK"
    echo "[clean] $WORK silindi."
  else
    echo "[clean] zaten temiz."
  fi
}

cmd="${1:-start}"
case "$cmd" in
  stop)   stop_single; exit 0 ;;
  status) show_status; exit 0 ;;
  clean)  clean_single; exit 0 ;;
  start)  ;;
  *) echo "usage: $0 {start|stop|status|clean}"; exit 1 ;;
esac

stop_single >/dev/null 2>&1 || true

# 1) Build
echo "[1/5] cargo build -p qv-node -p qv-wallet ..."
( cd "$PROJECT_ROOT" && cargo build -p qv-node -p qv-wallet )
TARGET_DIR="$(cd "$PROJECT_ROOT" && cargo metadata --format-version 1 --no-deps | \
  python3 -c "import sys,json;print(json.load(sys.stdin)['target_directory'])" 2>/dev/null \
  || echo "$PROJECT_ROOT/target")"
NODE_BIN="$TARGET_DIR/debug/qv-node"
WALLET_BIN="$TARGET_DIR/debug/qv-wallet"
[ -x "$NODE_BIN" ]   || { echo "qv-node not found at $NODE_BIN"; exit 1; }
[ -x "$WALLET_BIN" ] || { echo "qv-wallet not found at $WALLET_BIN"; exit 1; }

# 2) Work dir
mkdir -p "$WORK"
NODE_DATA="$WORK/node-data"
NODE_CONFIG="$WORK/node.toml"
WALLET_KEYSTORE="$WORK/wallet.json"
rm -rf "$NODE_DATA" "$NODE_CONFIG" "$WALLET_KEYSTORE"

# 3) Node --init
echo "[2/5] qv-node --init ..."
"$NODE_BIN" --init --network devnet \
  --data-dir "$NODE_DATA" \
  --config   "$NODE_CONFIG" \
  --rpc-addr "127.0.0.1:$RPC_PORT" \
  --metrics-addr "127.0.0.1:$METRICS_PORT" \
  > "$WORK/init.log" 2>&1

# 4) Start node
echo "[3/5] qv-node baslatiliyor ..."
( cd "$PROJECT_ROOT" && \
  "$NODE_BIN" --config "$NODE_CONFIG" --data-dir "$NODE_DATA" --network devnet \
    --rpc-addr "127.0.0.1:$RPC_PORT" --metrics-addr "127.0.0.1:$METRICS_PORT" \
    --log-level info \
    > "$WORK/node.log" 2> "$WORK/node.err" & echo $! > "$WORK/pids" )
echo "  node    pid=$(cat "$WORK/pids") rpc=127.0.0.1:$RPC_PORT log=$WORK/node.log"

# 5) Wait for RPC
echo "  node RPC bekleniyor ..."
for i in $(seq 1 15); do
  sleep 1
  if curl -s --max-time 2 -X POST "http://127.0.0.1:$RPC_PORT" \
       -H 'Content-Type: application/json' \
       -d '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}' >/dev/null 2>&1; then
    echo "  node RPC hazir ($i sn)"
    break
  fi
done

# 6) Wallet devnet-import
echo "[4/5] cuzdan devnet-import ..."
"$WALLET_BIN" --keystore "$WALLET_KEYSTORE" --rpc "http://127.0.0.1:$RPC_PORT" \
  devnet-import --password "$WALLET_PW" \
  > "$WORK/wallet-init.log" 2> "$WORK/wallet-init.err" || true

# 7) Wallet UI
echo "[5/5] cuzdan UI 127.0.0.1:$WALLET_PORT ..."
( cd "$PROJECT_ROOT" && \
  "$WALLET_BIN" --keystore "$WALLET_KEYSTORE" --rpc "http://127.0.0.1:$RPC_PORT" \
    serve --bind "127.0.0.1:$WALLET_PORT" \
    > "$WORK/wallet.log" 2> "$WORK/wallet.err" & echo $! >> "$WORK/pids" )

WALLET_PID=$(tail -1 "$WORK/pids")
echo "  wallet  pid=$WALLET_PID ui=http://127.0.0.1:$WALLET_PORT log=$WORK/wallet.log"

sleep 2
echo ""
echo "[ok] tek-node devnet calisiyor."
echo "     wallet UI : http://127.0.0.1:$WALLET_PORT   (parola: $WALLET_PW)"
echo "     node RPC  : http://127.0.0.1:$RPC_PORT"
echo "     loglar    : $WORK"
echo "     durdur    : $0 stop"

# Try to open the browser (Linux: xdg-open, macOS: open, Windows: start)
if command -v xdg-open >/dev/null 2>&1; then xdg-open "http://127.0.0.1:$WALLET_PORT" >/dev/null 2>&1 &
elif command -v open >/dev/null 2>&1; then open "http://127.0.0.1:$WALLET_PORT" >/dev/null 2>&1 &
fi
