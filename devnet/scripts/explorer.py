#!/usr/bin/env python3
"""
QuantumVault Devnet Block Explorer

Minimal read-only Flask app with Jinja2 templates.
Queries qv-node JSON-RPC for blocks, transactions, and addresses.

Routes:
  GET  /health               - Health check
  GET  /                     - Dashboard (latest blocks, network stats)
  GET  /block/<height>       - Block details by height
  GET  /block/hash/<hash>    - Block details by hash
  GET  /tx/<tx_hash>         - Transaction details
  GET  /address/<addr>       - Address balance and UTXOs (stealth addr scanning)
  GET  /api/stats            - JSON: network statistics
  GET  /api/blocks           - JSON: recent blocks
"""

import os
import sys
import json
import logging
from typing import Dict, Any, Optional, Tuple, List
from datetime import datetime
from functools import lru_cache

try:
    from flask import Flask, render_template_string, jsonify, request
    import requests
except ImportError:
    print("Error: flask and requests required. Install with: pip install flask requests", file=sys.stderr)
    sys.exit(1)

# Configuration
RPC_ENDPOINT = os.getenv("RPC_ENDPOINT", "http://pool0:9944")
EXPLORER_PORT = int(os.getenv("EXPLORER_PORT", 5000))

app = Flask(__name__)
app.logger.setLevel(logging.INFO)


def call_rpc(method: str, params: List[Any]) -> Tuple[Optional[Any], Optional[str]]:
    """Call RPC endpoint, return (result, error_msg)."""
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1,
    }
    try:
        resp = requests.post(RPC_ENDPOINT, json=payload, timeout=10)
        resp.raise_for_status()
        data = resp.json()

        if data.get("error"):
            error_msg = data["error"].get("message", "Unknown RPC error")
            return None, f"RPC error: {error_msg}"

        return data.get("result"), None
    except requests.RequestException as e:
        return None, f"RPC request failed: {str(e)}"
    except Exception as e:
        return None, f"RPC parse failed: {str(e)}"


# HTML templates (embedded for simplicity)
LAYOUT_TEMPLATE = """
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>{% block title %}QuantumVault Explorer{% endblock %}</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f5f5f5; color: #333; }
        header { background: #1a1a1a; color: #fff; padding: 20px; text-align: center; }
        header h1 { font-size: 24px; margin-bottom: 8px; }
        header p { font-size: 14px; color: #ccc; }
        nav { background: #2a2a2a; padding: 12px; }
        nav a { color: #fff; margin: 0 12px; text-decoration: none; font-size: 14px; }
        nav a:hover { text-decoration: underline; }
        .container { max-width: 1200px; margin: 20px auto; padding: 0 20px; }
        .error { background: #fee; border-left: 4px solid #f00; padding: 12px; margin: 20px 0; border-radius: 4px; }
        .card { background: #fff; border: 1px solid #ddd; border-radius: 4px; padding: 20px; margin: 20px 0; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
        table { width: 100%; border-collapse: collapse; }
        th { background: #f0f0f0; padding: 12px; text-align: left; font-weight: 600; border-bottom: 2px solid #ddd; }
        td { padding: 10px 12px; border-bottom: 1px solid #eee; }
        tr:hover td { background: #fafafa; }
        .code { font-family: 'Courier New', monospace; font-size: 13px; }
        .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin: 20px 0; }
        .stat-card { background: #fff; border: 1px solid #ddd; padding: 16px; border-radius: 4px; text-align: center; }
        .stat-card .label { font-size: 12px; color: #666; text-transform: uppercase; font-weight: 600; }
        .stat-card .value { font-size: 28px; font-weight: bold; color: #1a1a1a; margin-top: 8px; }
        .truncate { max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
        a { color: #0066cc; text-decoration: none; }
        a:hover { text-decoration: underline; }
        footer { text-align: center; padding: 20px; color: #666; font-size: 12px; }
    </style>
</head>
<body>
    <header>
        <h1>QuantumVault Block Explorer</h1>
        <p>Devnet | Real-time blockchain monitoring</p>
    </header>

    <nav>
        <a href="/">Dashboard</a>
        <a href="/#blocks">Blocks</a>
        <a href="/#search">Search</a>
        <a href="/api/stats">API Stats</a>
    </nav>

    <div class="container">
        {% block content %}{% endblock %}
    </div>

    <footer>
        <p>QuantumVault L1 Blockchain Explorer | <a href="/health">Health</a></p>
    </footer>
</body>
</html>
"""

DASHBOARD_TEMPLATE = """
{% extends "layout.html" %}
{% block title %}Dashboard - QuantumVault Explorer{% endblock %}
{% block content %}

<div class="card">
    <h2>Network Statistics</h2>
    <div class="stat-grid">
        <div class="stat-card">
            <div class="label">Latest Block Height</div>
            <div class="value">{{ tip.height }}</div>
        </div>
        <div class="stat-card">
            <div class="label">Block Hash</div>
            <div class="value" style="font-size: 14px; word-break: break-all;">{{ tip.block_hash[:16] }}...</div>
        </div>
        <div class="stat-card">
            <div class="label">Latest Block Time</div>
            <div class="value">{{ tip.timestamp_str }}</div>
        </div>
    </div>
</div>

<div class="card" id="blocks">
    <h2>Recent Blocks</h2>
    {% if blocks %}
    <table>
        <thead>
            <tr>
                <th>Height</th>
                <th>Block Hash</th>
                <th>Timestamp</th>
                <th>Transactions</th>
            </tr>
        </thead>
        <tbody>
            {% for block in blocks %}
            <tr>
                <td><a href="/block/{{ block.height }}">{{ block.height }}</a></td>
                <td><span class="code truncate"><a href="/block/hash/{{ block.hash }}">{{ block.hash[:16] }}...</a></span></td>
                <td>{{ block.timestamp_str }}</td>
                <td>{{ block.tx_count }}</td>
            </tr>
            {% endfor %}
        </tbody>
    </table>
    {% else %}
    <p style="color: #999;">No blocks yet. Waiting for genesis...</p>
    {% endif %}
</div>

<div class="card" id="search">
    <h2>Search</h2>
    <form method="get" action="/" style="display: grid; grid-template-columns: 1fr auto; gap: 10px;">
        <input type="text" name="q" placeholder="Block height, block hash, or tx hash..." required style="padding: 8px; border: 1px solid #ddd; border-radius: 4px;">
        <button type="submit" style="padding: 8px 16px; background: #0066cc; color: white; border: none; border-radius: 4px; cursor: pointer;">Search</button>
    </form>
</div>

{% endblock %}
"""

BLOCK_TEMPLATE = """
{% extends "layout.html" %}
{% block title %}Block {{ block.height }} - QuantumVault Explorer{% endblock %}
{% block content %}

<div class="card">
    <h2>Block #{{ block.height }}</h2>
    <table>
        <tr>
            <th style="width: 150px;">Height</th>
            <td>{{ block.height }}</td>
        </tr>
        <tr>
            <th>Block Hash</th>
            <td class="code truncate">{{ block.hash }}</td>
        </tr>
        <tr>
            <th>Previous Hash</th>
            <td class="code truncate"><a href="/block/hash/{{ block.prev_hash }}">{{ block.prev_hash[:16] }}...</a></td>
        </tr>
        <tr>
            <th>Timestamp</th>
            <td>{{ block.timestamp_str }}</td>
        </tr>
        <tr>
            <th>Producer</th>
            <td class="code truncate">{{ block.producer[:16] }}...</td>
        </tr>
        <tr>
            <th>Merkle Root</th>
            <td class="code truncate">{{ block.merkle_root[:16] }}...</td>
        </tr>
        <tr>
            <th>Transactions</th>
            <td>{{ block.tx_count }}</td>
        </tr>
    </table>
</div>

<div class="card">
    <h2>Transactions</h2>
    {% if block.transactions %}
    <table>
        <thead>
            <tr>
                <th>TX Hash</th>
                <th>Inputs</th>
                <th>Outputs</th>
            </tr>
        </thead>
        <tbody>
            {% for tx in block.transactions %}
            <tr>
                <td class="code truncate"><a href="/tx/{{ tx.hash }}">{{ tx.hash[:16] }}...</a></td>
                <td>{{ tx.input_count }}</td>
                <td>{{ tx.output_count }}</td>
            </tr>
            {% endfor %}
        </tbody>
    </table>
    {% else %}
    <p style="color: #999;">No transactions in this block.</p>
    {% endif %}
</div>

{% endblock %}
"""

TX_TEMPLATE = """
{% extends "layout.html" %}
{% block title %}TX {{ tx.hash[:16] }} - QuantumVault Explorer{% endblock %}
{% block content %}

<div class="card">
    <h2>Transaction</h2>
    <table>
        <tr>
            <th style="width: 150px;">TX Hash</th>
            <td class="code">{{ tx.hash }}</td>
        </tr>
        <tr>
            <th>Inputs</th>
            <td>{{ tx.input_count }}</td>
        </tr>
        <tr>
            <th>Outputs</th>
            <td>{{ tx.output_count }}</td>
        </tr>
        <tr>
            <th>Value (satoshis)</th>
            <td>{{ tx.total_value }}</td>
        </tr>
    </table>
</div>

<div class="card">
    <h2>Outputs</h2>
    {% if tx.outputs %}
    <table>
        <thead>
            <tr>
                <th>Index</th>
                <th>Value (sat)</th>
                <th>Script Hash</th>
            </tr>
        </thead>
        <tbody>
            {% for output in tx.outputs %}
            <tr>
                <td>{{ output.index }}</td>
                <td>{{ output.value }}</td>
                <td class="code truncate">{{ output.script_hash[:16] }}...</td>
            </tr>
            {% endfor %}
        </tbody>
    </table>
    {% else %}
    <p style="color: #999;">No outputs.</p>
    {% endif %}
</div>

{% endblock %}
"""

ADDRESS_TEMPLATE = """
{% extends "layout.html" %}
{% block title %}Address {{ address }} - QuantumVault Explorer{% endblock %}
{% block content %}

<div class="card">
    <h2>Address</h2>
    <table>
        <tr>
            <th style="width: 150px;">Stealth Address</th>
            <td class="code">{{ address }}</td>
        </tr>
        <tr>
            <th>Balance (satoshis)</th>
            <td>{{ balance_sats }}</td>
        </tr>
        <tr>
            <th>Balance (QV)</th>
            <td>{{ (balance_sats / 1e8)|round(8) }}</td>
        </tr>
        <tr>
            <th>UTXOs</th>
            <td>{{ utxo_count }}</td>
        </tr>
    </table>
</div>

<div class="card">
    <h2>UTXOs</h2>
    {% if utxos %}
    <table>
        <thead>
            <tr>
                <th>TX Hash</th>
                <th>Output Index</th>
                <th>Value (sat)</th>
            </tr>
        </thead>
        <tbody>
            {% for utxo in utxos %}
            <tr>
                <td class="code truncate"><a href="/tx/{{ utxo.tx_id }}">{{ utxo.tx_id[:16] }}...</a></td>
                <td>{{ utxo.output_index }}</td>
                <td>{{ utxo.value }}</td>
            </tr>
            {% endfor %}
        </tbody>
    </table>
    {% else %}
    <p style="color: #999;">No UTXOs found for this address.</p>
    {% endif %}
</div>

{% endblock %}
"""

# Register template globals
app.jinja_env.globals.update(
    int=int,
    float=float,
    len=len,
    str=str,
)


@app.route("/health", methods=["GET"])
def health() -> Tuple[dict, int]:
    """Health check endpoint."""
    return jsonify({"status": "healthy", "timestamp": datetime.utcnow().isoformat()}), 200


@app.route("/", methods=["GET"])
def dashboard():
    """Dashboard with network stats and recent blocks."""
    # Get tip
    tip_result, err = call_rpc("qv_getTip", [])
    if err:
        return (
            f"<h1>Explorer Error</h1><p>{err}</p>",
            503,
        )

    if not tip_result:
        tip_result = {"block_hash": "0x00", "height": 0, "timestamp": 0}

    # Format tip
    tip = {
        "height": tip_result.get("height", 0),
        "block_hash": tip_result.get("block_hash", "0x00"),
        "timestamp_str": datetime.fromtimestamp(tip_result.get("timestamp", 0)).isoformat(),
    }

    # Fetch recent blocks (simplified: just use tip for now)
    blocks = []
    if tip["height"] > 0:
        blocks.append({
            "height": tip["height"],
            "hash": tip["block_hash"],
            "timestamp_str": tip["timestamp_str"],
            "tx_count": 0,
        })

    from jinja2 import Template

    layout = Template(LAYOUT_TEMPLATE)
    dashboard_tmpl = Template(DASHBOARD_TEMPLATE)
    return layout.render(
        content=dashboard_tmpl.render(
            tip=tip,
            blocks=blocks,
        )
    )


@app.route("/block/<int:height>", methods=["GET"])
def block_by_height(height: int):
    """Block details by height."""
    result, err = call_rpc("qv_getBlockByHeight", [height])

    if err:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Error:</strong> {err}</div>"
        ), 503

    if not result:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Block not found:</strong> Height {height}</div>"
        ), 404

    block = {
        "height": result.get("height", height),
        "hash": result.get("block_hash", "unknown"),
        "prev_hash": result.get("prev_hash", "unknown"),
        "timestamp_str": datetime.fromtimestamp(result.get("timestamp", 0)).isoformat(),
        "producer": result.get("producer_key_hash", "unknown"),
        "merkle_root": result.get("merkle_root", "unknown"),
        "tx_count": len(result.get("transactions", [])),
        "transactions": [
            {
                "hash": tx.get("tx_id", "unknown"),
                "input_count": len(tx.get("inputs", [])),
                "output_count": len(tx.get("outputs", [])),
            }
            for tx in result.get("transactions", [])
        ],
    }

    from jinja2 import Template
    layout = Template(LAYOUT_TEMPLATE)
    block_tmpl = Template(BLOCK_TEMPLATE)
    return layout.render(content=block_tmpl.render(block=block))


@app.route("/block/hash/<hash_str>", methods=["GET"])
def block_by_hash(hash_str: str):
    """Block details by hash."""
    result, err = call_rpc("qv_getBlockByHash", [hash_str])

    if err:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Error:</strong> {err}</div>"
        ), 503

    if not result:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Block not found:</strong> {hash_str}</div>"
        ), 404

    block = {
        "height": result.get("height", 0),
        "hash": result.get("block_hash", hash_str),
        "prev_hash": result.get("prev_hash", "unknown"),
        "timestamp_str": datetime.fromtimestamp(result.get("timestamp", 0)).isoformat(),
        "producer": result.get("producer_key_hash", "unknown"),
        "merkle_root": result.get("merkle_root", "unknown"),
        "tx_count": len(result.get("transactions", [])),
        "transactions": [
            {
                "hash": tx.get("tx_id", "unknown"),
                "input_count": len(tx.get("inputs", [])),
                "output_count": len(tx.get("outputs", [])),
            }
            for tx in result.get("transactions", [])
        ],
    }

    from jinja2 import Template
    layout = Template(LAYOUT_TEMPLATE)
    block_tmpl = Template(BLOCK_TEMPLATE)
    return layout.render(content=block_tmpl.render(block=block))


@app.route("/tx/<tx_hash>", methods=["GET"])
def tx(tx_hash: str):
    """Transaction details."""
    result, err = call_rpc("qv_getTx", [tx_hash])

    if err:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Error:</strong> {err}</div>"
        ), 503

    if not result:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Transaction not found:</strong> {tx_hash}</div>"
        ), 404

    tx_obj = {
        "hash": result.get("tx_id", tx_hash),
        "input_count": len(result.get("inputs", [])),
        "output_count": len(result.get("outputs", [])),
        "total_value": sum(out.get("value", 0) for out in result.get("outputs", [])),
        "outputs": [
            {
                "index": i,
                "value": out.get("value", 0),
                "script_hash": out.get("locking_script", "unknown")[:16],
            }
            for i, out in enumerate(result.get("outputs", []))
        ],
    }

    from jinja2 import Template
    layout = Template(LAYOUT_TEMPLATE)
    tx_tmpl = Template(TX_TEMPLATE)
    return layout.render(content=tx_tmpl.render(tx=tx_obj))


@app.route("/address/<addr>", methods=["GET"])
def address(addr: str):
    """Address details and UTXOs."""
    # Scan stealth outputs for address
    result, err = call_rpc("qv_getBalanceFor", [addr])

    if err:
        from jinja2 import Template
        layout = Template(LAYOUT_TEMPLATE)
        return layout.render(
            content=f"<div class='error'><strong>Error:</strong> {err}</div>"
        ), 503

    balance_sats = result if isinstance(result, int) else 0
    utxo_count = 0
    utxos = []

    addr_obj = {
        "address": addr,
        "balance_sats": balance_sats,
        "balance_qv": balance_sats / 1e8,
        "utxo_count": utxo_count,
        "utxos": utxos,
    }

    from jinja2 import Template
    layout = Template(LAYOUT_TEMPLATE)
    addr_tmpl = Template(ADDRESS_TEMPLATE)
    return layout.render(content=addr_tmpl.render(**addr_obj))


@app.route("/api/stats", methods=["GET"])
def api_stats() -> Tuple[dict, int]:
    """JSON API: network statistics."""
    tip_result, err = call_rpc("qv_getTip", [])

    if err:
        return jsonify({"error": err}), 503

    return jsonify({
        "network": "devnet",
        "height": tip_result.get("height", 0) if tip_result else 0,
        "block_hash": tip_result.get("block_hash", "0x00") if tip_result else "0x00",
        "timestamp": tip_result.get("timestamp", 0) if tip_result else 0,
    }), 200


@app.route("/api/blocks", methods=["GET"])
def api_blocks() -> Tuple[dict, int]:
    """JSON API: recent blocks."""
    tip_result, err = call_rpc("qv_getTip", [])

    if err:
        return jsonify({"error": err}), 503

    blocks = []
    if tip_result:
        blocks.append({
            "height": tip_result.get("height", 0),
            "hash": tip_result.get("block_hash", "0x00"),
            "timestamp": tip_result.get("timestamp", 0),
        })

    return jsonify({"blocks": blocks}), 200


if __name__ == "__main__":
    app.logger.info(f"Starting QuantumVault Explorer on 0.0.0.0:{EXPLORER_PORT}")
    app.logger.info(f"RPC endpoint: {RPC_ENDPOINT}")
    app.logger.info(f"Dashboard: http://localhost:{EXPLORER_PORT}")

    app.run(host="0.0.0.0", port=EXPLORER_PORT, debug=False, use_reloader=False)
