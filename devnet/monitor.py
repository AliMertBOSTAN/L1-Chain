#!/usr/bin/env python3
"""QuantumVault 4-node devnet monitor — a live CLI explorer.

Polls every node's JSON-RPC and Prometheus metrics endpoints and renders a
terminal dashboard covering consensus, p2p/gossip, the transaction lifecycle
and wallet balances.

  python3 monitor.py                 live view, refresh every 2s
  python3 monitor.py --once          print a single snapshot and exit
  python3 monitor.py --iter 10       refresh 10 times then exit
  python3 monitor.py --work DIR      directory holding nodeN.log files
"""
import json, os, re, sys, time, urllib.request

NODES = [
    {"name": "node0", "rpc": 8545, "met": 9601, "p2p": 17001},
    {"name": "node1", "rpc": 8546, "met": 9602, "p2p": 17002},
    {"name": "node2", "rpc": 8547, "met": 9603, "p2p": 17003},
    {"name": "node3", "rpc": 8548, "met": 9604, "p2p": 17004},
]
SLOT_MS = 500          # config/devnet.toml slot_duration_ms
K_FINALITY = 5         # config/devnet.toml k_finality

# Persistent across refreshes: every block that carried transactions, so a
# transaction is never missed on a fast chain. height -> (slot, leader, count)
_SEEN_TX = {}
_LAST_SCANNED = 0

def rpc(port, method, params=None):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method,
                       "params": params or []}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}", body,
                                 {"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=3) as r:
            return json.loads(r.read()).get("result")
    except Exception:
        return None

def scrape(port):
    out = {}
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}", timeout=3) as r:
            for line in r.read().decode("utf-8", "replace").splitlines():
                line = line.strip()
                if not line or line.startswith("#") or " " not in line:
                    continue
                name, _, val = line.rpartition(" ")
                try:
                    out[name] = float(val)
                except ValueError:
                    pass
    except Exception:
        pass
    return out

def metric(m, needle):
    total = 0.0
    found = False
    for k, v in m.items():
        if needle in k:
            total += v
            found = True
    return total if found else None

def tail(path, n=6):
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as f:
            return f.readlines()[-n:]
    except Exception:
        return []

def short(h):
    if not h:
        return "-"
    h = str(h)
    return h[:10] + ".." if len(h) > 12 else h

def collect(work):
    snap = []
    for n in NODES:
        tip = rpc(n["rpc"], "qv_getTip")
        mem = rpc(n["rpc"], "qv_getMempoolStatus")
        met = scrape(n["met"])
        snap.append({
            "name": n["name"], "rpc": n["rpc"], "p2p": n["p2p"],
            "log": os.path.join(work, n["name"] + ".log"),
            "up": tip is not None,
            "height": (tip or {}).get("height"),
            "hash": (tip or {}).get("block_hash"),
            "mempool": (mem or {}).get("clear_pool_size"),
            "peers": metric(met, "peers_connected"),
            "blocks_validated": metric(met, "blocks_validated"),
            "gossip_in": metric(met, "gossip_messages_in"),
            "tx_received": metric(met, "tx_received"),
        })
    return snap

def render(work, tick):
    snap = collect(work)
    slot = int(time.time() * 1000 // SLOT_MS)
    leader = slot % 4
    lines = []
    lines.append("=" * 78)
    lines.append("  QuantumVault L1 — 4-node devnet monitor      "
                 f"refresh #{tick}   {time.strftime('%H:%M:%S')}")
    lines.append("=" * 78)

    # NODES
    lines.append("")
    lines.append("  NODES")
    lines.append("  %-7s %-7s %-6s %-9s %-8s %-9s %s" %
                 ("node", "height", "peers", "mempool", "blocks", "gossipIn", "tip"))
    for s in snap:
        if not s["up"]:
            lines.append("  %-7s  (unreachable on rpc %d)" % (s["name"], s["rpc"]))
            continue
        lines.append("  %-7s %-7s %-6s %-9s %-8s %-9s %s" % (
            s["name"],
            s["height"] if s["height"] is not None else "-",
            int(s["peers"]) if s["peers"] is not None else "-",
            s["mempool"] if s["mempool"] is not None else "-",
            int(s["blocks_validated"]) if s["blocks_validated"] is not None else "-",
            int(s["gossip_in"]) if s["gossip_in"] is not None else "-",
            short(s["hash"]),
        ))

    # CONSENSUS
    heights = [s["height"] for s in snap if s["up"] and s["height"] is not None]
    stake = rpc(NODES[0]["rpc"], "qv_getStakeDistribution")
    nonce = rpc(NODES[0]["rpc"], "qv_getEpochNonce")
    lines.append("")
    lines.append("  CONSENSUS  (Ouroboros-style, devnet round-robin leader)")
    lines.append(f"    wall-clock slot : {slot}    leader of this slot: node{leader}")
    if nonce:
        lines.append(f"    epoch           : {nonce.get('epoch')}   "
                     f"nonce {short(nonce.get('nonce_hex'))}")
    if stake:
        pools = stake.get("pools", [])
        lines.append(f"    stake pools     : {len(pools)}   "
                     f"total stake {stake.get('total_stake')}")
    if heights:
        tip = max(heights)
        fin = max(0, tip - K_FINALITY)
        lines.append(f"    chain tip       : height {tip}   "
                     f"k-final (k={K_FINALITY}) up to height {fin}")

    # P2P
    lines.append("")
    lines.append("  P2P / GOSSIP")
    for s in snap:
        if not s["up"]:
            continue
        peers = int(s["peers"]) if s["peers"] is not None else 0
        bar = "#" * peers + "." * max(0, 3 - peers)
        lines.append(f"    {s['name']}  peers [{bar}] {peers}/3   "
                     f"p2p tcp/{s['p2p']}   "
                     f"gossip-in {int(s['gossip_in']) if s['gossip_in'] is not None else 0}")

    # CONVERGENCE
    lines.append("")
    hs = set(heights)
    hashes = set(s["hash"] for s in snap if s["up"] and s["hash"])
    if heights and len(hashes) == 1 and len(hs) == 1:
        lines.append("  CONVERGENCE : OK — all nodes agree on the same tip")
    elif heights and max(hs) - min(hs) <= 1:
        lines.append("  CONVERGENCE : SYNCING — heights within 1 block "
                     f"({sorted(hs)})")
    elif heights:
        lines.append(f"  CONVERGENCE : DIVERGED — tip heights {sorted(hs)}")
    else:
        lines.append("  CONVERGENCE : no nodes reporting yet")

    # TX LIFECYCLE — scan new blocks for transactions and remember any
    # tx-bearing block across refreshes (so it is never missed).
    global _LAST_SCANNED
    if heights:
        tip = max(heights)
        scan_from = (_LAST_SCANNED + 1) if _LAST_SCANNED > 0 else max(1, tip - 30)
        for h in range(scan_from, tip + 1):
            blk = rpc(NODES[0]["rpc"], "qv_getBlockByHeight", [h])
            if not blk:
                continue
            bslot = blk.get("header", {}).get("slot")
            if isinstance(bslot, dict):
                bslot = list(bslot.values())[0]
            ntx = len(blk.get("transactions", []))
            if ntx >= 1:
                ldr = (bslot % 4) if isinstance(bslot, int) else "?"
                _SEEN_TX[h] = (bslot, ldr, ntx)
        _LAST_SCANNED = tip

    lines.append("")
    lines.append("  RECENT BLOCKS  (height : slot -> leader : tx-count)")
    if heights:
        tip = max(heights)
        for h in range(tip, max(-1, tip - 6), -1):
            blk = rpc(NODES[0]["rpc"], "qv_getBlockByHeight", [h])
            if not blk:
                continue
            bslot = blk.get("header", {}).get("slot")
            if isinstance(bslot, dict):
                bslot = list(bslot.values())[0]
            ntx = len(blk.get("transactions", []))
            ldr = (bslot % 4) if isinstance(bslot, int) else "?"
            mark = "  <-- TX HERE" if (h > 0 and ntx >= 1) else ""
            lines.append(f"    #{h:<6} slot {bslot} -> node{ldr}   {ntx} tx{mark}")

    lines.append("")
    lines.append("  TRANSACTIONS ON CHAIN  (tx-bearing blocks, kept since monitor start)")
    if _SEEN_TX:
        for h in sorted(_SEEN_TX)[-10:]:
            bslot, ldr, ntx = _SEEN_TX[h]
            lines.append(f"    block #{h}  slot {bslot} -> produced by node{ldr}   "
                         f"{ntx} transaction(s)")
    else:
        lines.append("    (none yet — run:  cargo run -p qv-node --example transfer_demo)")

    # WALLETS (if a transfer demo wrote wallets.json)
    for cand in ("wallets.json", os.path.join(work, "..", "wallets.json"),
                 os.path.join(os.path.dirname(work), "wallets.json")):
        if os.path.exists(cand):
            try:
                w = json.load(open(cand))
                lines.append("")
                lines.append("  WALLETS")
                for key in ("wallet_a", "wallet_b"):
                    ww = w.get(key, {})
                    op = ww.get("genesis_outpoint")
                    u = rpc(NODES[0]["rpc"], "qv_getUtxo", [op]) if op else None
                    bal = u.get("value") if u else "spent/moved"
                    lines.append(f"    {key}: genesis UTXO {short(op)}  balance {bal}")
            except Exception:
                pass
            break

    # RECENT LOG ACTIVITY
    lines.append("")
    lines.append("  RECENT NODE LOG ACTIVITY")
    keys = ("produced block", "block accepted", "transaction accepted",
            "connected", "disconnected", "network identity")
    events = []
    for s in snap:
        for ln in tail(s["log"], 30):
            clean = re.sub(r"\x1b\[[0-9;]*m", "", ln).strip()
            low = clean.lower()
            if any(k in low for k in keys):
                ts = clean.split("T")[1][:12] if "T" in clean else ""
                msg = clean.split(": ", 1)[-1]
                if len(msg) > 68:
                    msg = msg[:68]
                events.append((clean[:30], s["name"], ts, msg))
    events.sort()
    for _, name, ts, msg in events[-12:]:
        lines.append(f"    {ts}  [{name}]  {msg}")
    lines.append("=" * 78)
    return "\n".join(lines)

def main():
    args = sys.argv[1:]
    once = "--once" in args
    iters = None
    work = "./work4"
    if "--iter" in args:
        iters = int(args[args.index("--iter") + 1])
    if "--work" in args:
        work = args[args.index("--work") + 1]
    tick = 0
    while True:
        tick += 1
        out = render(work, tick)
        if once:
            print(out)
            return
        os.system("cls" if os.name == "nt" else "clear")
        print(out)
        if iters is not None and tick >= iters:
            return
        time.sleep(2)

if __name__ == "__main__":
    main()
