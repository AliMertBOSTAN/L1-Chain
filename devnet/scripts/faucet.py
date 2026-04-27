#!/usr/bin/env python3
"""
QuantumVault Devnet Faucet Service

Minimal Flask app serving QV to stealth addresses for testing.
Rate-limited per IP: 1 request per minute.
Maintains persistent log of drips in LOG_DIR.

API:
  GET  /health                 - Health check
  POST /drip?address=<addr>    - Request 100 QV to stealth address
  GET  /status                 - Faucet statistics
"""

import os
import sys
import json
import time
import logging
import hashlib
from datetime import datetime, timedelta
from pathlib import Path
from collections import defaultdict
from typing import Dict, Optional, Tuple

try:
    from flask import Flask, request, jsonify
    import requests
except ImportError:
    print("Error: flask and requests required. Install with: pip install flask requests", file=sys.stderr)
    sys.exit(1)

# Configuration from environment
RPC_ENDPOINT = os.getenv("RPC_ENDPOINT", "http://pool0:9944")
FAUCET_ADDRESS = os.getenv("FAUCET_ADDRESS", "devnet1faucet0000000000000000000000000000000")
LOG_DIR = Path(os.getenv("LOG_DIR", "/tmp/faucet-logs"))
DRIP_AMOUNT_SATS = 10000000000  # 100 QV in satoshis (1 QV = 10^8 sats)
RATE_LIMIT_WINDOW = 60  # seconds
RATE_LIMIT_PER_IP = 1  # requests per window

# Create Flask app
app = Flask(__name__)
app.logger.setLevel(logging.INFO)

# Ensure log directory exists
LOG_DIR.mkdir(parents=True, exist_ok=True)

# In-memory tracking of IPs and their drip timestamps
# Structure: { "ip": [timestamp1, timestamp2, ...] }
ip_drips: Dict[str, list] = defaultdict(list)

# Persistent drip log file
DRIP_LOG_FILE = LOG_DIR / "drips.jsonl"


def log_drip(address: str, amount_sats: int, status: str, tx_id: Optional[str] = None) -> None:
    """Append drip record to persistent log (JSONL format)."""
    record = {
        "timestamp": datetime.utcnow().isoformat(),
        "address": address,
        "amount_sats": amount_sats,
        "status": status,
        "tx_id": tx_id,
    }
    try:
        with open(DRIP_LOG_FILE, "a") as f:
            f.write(json.dumps(record) + "\n")
    except IOError as e:
        app.logger.error(f"Failed to write drip log: {e}")


def get_client_ip() -> str:
    """Extract client IP from request, respecting X-Forwarded-For."""
    if request.headers.get("X-Forwarded-For"):
        return request.headers.get("X-Forwarded-For").split(",")[0].strip()
    return request.remote_addr or "unknown"


def is_rate_limited(ip: str) -> bool:
    """Check if IP has exceeded rate limit."""
    now = time.time()
    cutoff = now - RATE_LIMIT_WINDOW

    # Prune old timestamps
    ip_drips[ip] = [ts for ts in ip_drips[ip] if ts > cutoff]

    # Check limit
    return len(ip_drips[ip]) >= RATE_LIMIT_PER_IP


def record_drip(ip: str) -> None:
    """Record a drip for IP."""
    ip_drips[ip].append(time.time())


def call_rpc(method: str, params: list) -> Tuple[Optional[dict], Optional[str]]:
    """Call RPC endpoint, return (result, error_msg)."""
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": int(time.time() * 1000) % (2**31),
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


def create_transfer_tx(to_address: str, amount_sats: int) -> Optional[dict]:
    """
    Create a signed transaction from faucet to recipient.
    Returns tx dict or None on failure.

    Note: This is a placeholder. Real implementation requires:
    1. Querying UTXO set for faucet's UTXOs
    2. Selecting inputs to cover amount + fees
    3. Building transaction
    4. Signing with faucet's private key
    5. Serializing to hex
    """
    # Placeholder: would call qv-wallet RPC to build and sign
    # For now, return a stub that would fail on chain submission
    return {
        "version": 1,
        "inputs": [
            {
                "previous_output": "0000000000000000000000000000000000000000000000000000000000000000:0",
                "script_sig": "",
                "sequence": 0xffffffff,
            }
        ],
        "outputs": [
            {
                "value": amount_sats,
                "locking_script": "",  # Would be p2pkh_pqc(to_address_hash)
                "datum": None,
                "stealth_info": {"address": to_address},
            }
        ],
        "lock_time": 0,
    }


@app.route("/health", methods=["GET"])
def health() -> Tuple[dict, int]:
    """Health check endpoint."""
    return jsonify({"status": "healthy", "timestamp": datetime.utcnow().isoformat()}), 200


@app.route("/drip", methods=["POST"])
def drip() -> Tuple[dict, int]:
    """
    Request a drip (100 QV) to a stealth address.

    Query parameter: address=<stealth_addr>
    Returns: { tx_id, amount_sats, status }
    """
    client_ip = get_client_ip()

    # Rate limiting
    if is_rate_limited(client_ip):
        log_drip("", 0, "rate_limited", None)
        return (
            jsonify(
                {
                    "error": "Rate limited: 1 drip per minute per IP",
                    "retry_after": RATE_LIMIT_WINDOW,
                }
            ),
            429,
        )

    # Get target address
    to_address = request.args.get("address", "").strip()
    if not to_address:
        return jsonify({"error": "Missing query parameter: address"}), 400

    # Validate address format (basic check: starts with devnet1, length ~42)
    if not (to_address.startswith("devnet1") and len(to_address) > 20):
        log_drip(to_address, 0, "invalid_address", None)
        return jsonify({"error": "Invalid stealth address format"}), 400

    app.logger.info(f"Drip request from {client_ip} to {to_address[:20]}...")

    # Build transaction
    tx = create_transfer_tx(to_address, DRIP_AMOUNT_SATS)
    if not tx:
        log_drip(to_address, 0, "tx_creation_failed", None)
        return jsonify({"error": "Failed to create transaction"}), 500

    # Serialize and submit (placeholder: real implementation serializes to hex)
    tx_hex = json.dumps(tx)  # Placeholder serialization

    # Submit to mempool via RPC
    result, error = call_rpc("qv_sendTransaction", [tx_hex])
    if error:
        app.logger.warning(f"Transaction submission failed: {error}")
        log_drip(to_address, 0, "submission_failed", None)
        return jsonify({"error": error}), 502

    # Extract tx_id from result (RPC returns TxId or error)
    tx_id = result if isinstance(result, str) else str(result)
    record_drip(client_ip)
    log_drip(to_address, DRIP_AMOUNT_SATS, "submitted", tx_id)

    app.logger.info(f"Drip submitted: {tx_id[:16]}... to {to_address[:20]}...")

    return (
        jsonify(
            {
                "tx_id": tx_id,
                "amount_sats": DRIP_AMOUNT_SATS,
                "amount_qv": DRIP_AMOUNT_SATS / 1e8,
                "to_address": to_address,
                "status": "submitted",
            }
        ),
        200,
    )


@app.route("/status", methods=["GET"])
def status() -> Tuple[dict, int]:
    """Return faucet statistics."""
    # Count drips from log
    total_drips = 0
    total_sats = 0

    try:
        if DRIP_LOG_FILE.exists():
            with open(DRIP_LOG_FILE, "r") as f:
                for line in f:
                    try:
                        record = json.loads(line)
                        if record.get("status") == "submitted":
                            total_drips += 1
                            total_sats += record.get("amount_sats", 0)
                    except json.JSONDecodeError:
                        pass
    except IOError:
        pass

    return (
        jsonify(
            {
                "faucet_address": FAUCET_ADDRESS,
                "drip_amount_sats": DRIP_AMOUNT_SATS,
                "drip_amount_qv": DRIP_AMOUNT_SATS / 1e8,
                "total_drips": total_drips,
                "total_distributed_sats": total_sats,
                "total_distributed_qv": total_sats / 1e8,
                "rate_limit": f"{RATE_LIMIT_PER_IP} per {RATE_LIMIT_WINDOW}s",
                "rpc_endpoint": RPC_ENDPOINT,
            }
        ),
        200,
    )


@app.errorhandler(404)
def not_found(error: Exception) -> Tuple[dict, int]:
    """Handle 404 errors."""
    return (
        jsonify(
            {
                "error": "Not found",
                "available_endpoints": {
                    "GET /health": "Health check",
                    "POST /drip?address=<addr>": "Request 100 QV drip",
                    "GET /status": "Faucet statistics",
                },
            }
        ),
        404,
    )


if __name__ == "__main__":
    host = os.getenv("FLASK_HOST", "0.0.0.0")
    port = int(os.getenv("FLASK_PORT", 5001))

    app.logger.info(f"Starting QuantumVault Faucet on {host}:{port}")
    app.logger.info(f"RPC endpoint: {RPC_ENDPOINT}")
    app.logger.info(f"Faucet address: {FAUCET_ADDRESS}")
    app.logger.info(f"Drip amount: {DRIP_AMOUNT_SATS / 1e8} QV")
    app.logger.info(f"Rate limit: {RATE_LIMIT_PER_IP} per {RATE_LIMIT_WINDOW}s per IP")
    app.logger.info(f"Log directory: {LOG_DIR}")

    app.run(host=host, port=port, debug=False, use_reloader=False)
