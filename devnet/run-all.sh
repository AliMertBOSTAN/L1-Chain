#!/usr/bin/env bash
# QuantumVault — TAM PAKET: 4 node + cüzdan UI + node-monitor (bash).
#
# `run-devnet.sh start` 4 node'u açar; bu script üstüne cüzdan UI ve
# node-monitor Node.js panelini ekler. Tek komutla local geliştirme yığınını
# ayağa kaldırır.
#
# Kullanım:
#   ./run-all.sh start | stop | status | clean

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORK="${QV_DEVNET_WORK:-$SCRIPT_DIR/work4}"
WALLET_PORT=7777
MONITOR_PORT="${QV_MONITOR_PORT:-7070}"
WALLET_PW="${QV_WALLET_PW:-devnetpw}"

stop_extras() {
  if [ -f "$WORK/extras-pids" ]; then
    for pid in $(cat "$WORK/extras-pids"); do kill "$pid" 2>/dev/null || true; done
    rm -f "$WORK/extras-pids"
    echo "[stop] cuzdan + monitor durduruldu."
  else
    echo "[stop] ekstra process bulunamadi."
  fi
}

stop_all() {
  stop_extras
  "$SCRIPT_DIR/run-devnet.sh" stop
}

show_status() {
  "$SCRIPT_DIR/run-devnet.sh" status
  if r=$(curl -s --max-time 3 "http://127.0.0.1:$WALLET_PORT/api/status" 2>/dev/null); then
    echo "  wallet   http=$WALLET_PORT  ${r}"
  else
    echo "  wallet   http=$WALLET_PORT  <unreachable>"
  fi
  if curl -s --max-time 3 "http://127.0.0.1:$MONITOR_PORT/" -o /dev/null 2>/dev/null; then
    echo "  monitor  http=$MONITOR_PORT  up"
  else
    echo "  monitor  http=$MONITOR_PORT  <unreachable>"
  fi
}

clean_all() {
  stop_all
  if [ -d "$WORK" ]; then
    rm -rf "$WORK"
    echo "[clean] $WORK silindi."
  else
    echo "[clean] zaten temiz."
  fi
}

cmd="${1:-start}"
case "$cmd" in
  stop)   stop_all; exit 0 ;;
  status) show_status; exit 0 ;;
  clean)  clean_all; exit 0 ;;
  start)  ;;
  *) echo "usage: $0 {start|stop|status|clean}"; exit 1 ;;
esac

stop_extras >/dev/null 2>&1 || true

# 1) 4 node
echo "[1/3] 4-node devnet baslatiliyor (run-devnet.sh start) ..."
"$SCRIPT_DIR/run-devnet.sh" start

# 2) wait for node0
echo "  node0 RPC bekleniyor ..."
for i in $(seq 1 24); do
  sleep 1
  if curl -s --max-time 2 -X POST "http://127.0.0.1:8545" \
       -H 'Content-Type: application/json' \
       -d '{"jsonrpc":"2.0","id":1,"method":"qv_getTip","params":[]}' >/dev/null 2>&1; then
    echo "  node0 RPC hazir ($i sn)"
    break
  fi
done

# 3) Build wallet
echo "[2/3] qv-wallet derleniyor ..."
( cd "$PROJECT_ROOT" && cargo build -p qv-wallet )
TARGET_DIR="$(cd "$PROJECT_ROOT" && cargo metadata --format-version 1 --no-deps | \
  python3 -c "import sys,json;print(json.load(sys.stdin)['target_directory'])" 2>/dev/null \
  || echo "$PROJECT_ROOT/target")"
WALLET_BIN="$TARGET_DIR/debug/qv-wallet"
[ -x "$WALLET_BIN" ] || { echo "qv-wallet binary not found at $WALLET_BIN"; exit 1; }

WALLET_KEYSTORE="$WORK/wallet.json"
if [ -f "$WALLET_KEYSTORE" ]; then
  echo "  cuzdan keystore zaten var: $WALLET_KEYSTORE"
else
  echo "  cuzdan devnet-import (parola: $WALLET_PW) ..."
  "$WALLET_BIN" --keystore "$WALLET_KEYSTORE" --rpc "http://127.0.0.1:8545" \
    devnet-import --password "$WALLET_PW" \
    > "$WORK/wallet-init.log" 2> "$WORK/wallet-init.err" || true
fi

EXTRAS=""
( cd "$PROJECT_ROOT" && \
  "$WALLET_BIN" --keystore "$WALLET_KEYSTORE" --rpc "http://127.0.0.1:8545" \
    serve --bind "127.0.0.1:$WALLET_PORT" \
    > "$WORK/wallet.log" 2> "$WORK/wallet.err" & echo $! > "$WORK/extras-pids" )
WALLET_PID=$(cat "$WORK/extras-pids")
echo "  wallet  pid=$WALLET_PID ui=http://127.0.0.1:$WALLET_PORT"

# 4) node-monitor (varsa)
echo "[3/3] node-monitor baslatiliyor ..."
MONITOR_DIR="$PROJECT_ROOT/node-monitor"
MONITOR_JS="$MONITOR_DIR/index.js"
if [ -f "$MONITOR_JS" ] && command -v node >/dev/null 2>&1; then
  ( cd "$MONITOR_DIR" && \
    node "$MONITOR_JS" --work "$WORK" --port "$MONITOR_PORT" \
      > "$WORK/monitor.log" 2> "$WORK/monitor.err" & echo $! >> "$WORK/extras-pids" )
  MONITOR_PID=$(tail -1 "$WORK/extras-pids")
  echo "  monitor pid=$MONITOR_PID ui=http://127.0.0.1:$MONITOR_PORT"
else
  echo "  monitor atlandi (node-monitor/index.js veya node bulunamadi)."
fi

sleep 2
echo ""
echo "[ok] TAM PAKET calisiyor."
echo "     wallet UI : http://127.0.0.1:$WALLET_PORT   (parola: $WALLET_PW)"
echo "     monitor   : http://127.0.0.1:$MONITOR_PORT"
echo "     4 node RPC: 127.0.0.1:8545..8548"
echo "     loglar    : $WORK"
echo "     durdur    : $0 stop"

if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "http://127.0.0.1:$WALLET_PORT" >/dev/null 2>&1 &
  xdg-open "http://127.0.0.1:$MONITOR_PORT" >/dev/null 2>&1 &
elif command -v open >/dev/null 2>&1; then
  open "http://127.0.0.1:$WALLET_PORT" >/dev/null 2>&1 &
  open "http://127.0.0.1:$MONITOR_PORT" >/dev/null 2>&1 &
fi
