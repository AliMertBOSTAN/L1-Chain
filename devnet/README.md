# QuantumVault Devnet

Local development network for QuantumVault L1 blockchain testing and integration development.

## Overview

The devnet consists of:
- **3 stake pool nodes** (pool0, pool1, pool2) with P2P networking and RPC endpoints
- **Block explorer** (Flask web UI) for visualizing blocks and transactions
- **Faucet service** (REST API) for distributing test QV to addresses
- **Docker Compose** orchestration with persistent data volumes

## Quick Start

### Prerequisites

- Docker (19.03+)
- Docker Compose (1.25+)
- Bash 4.0+

### Build & Launch

```bash
cd devnet

# Generate genesis configuration (pool keys, epoch nonce, faucet address)
bash scripts/genesis.sh

# Build the qv:dev image (compiles Rust binaries)
docker-compose build

# Start the devnet (3 nodes, explorer, faucet)
docker-compose up -d

# Check service health
docker-compose ps
curl http://localhost:9944/health  # Pool0 RPC
curl http://localhost:5000/health  # Explorer
curl http://localhost:5001/health  # Faucet

# View logs
docker-compose logs -f pool0     # Follow pool0 output
docker-compose logs explorer     # Explorer errors
docker-compose logs faucet       # Faucet logs

# Tear down
docker-compose down --volumes
```

## Network Topology

```
┌──────────────────────────────────────────────────────────────┐
│ Docker Bridge Network: devnet (172.25.0.0/16)               │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─────────────────┐       ┌─────────────────┐              │
│  │ qv-pool0        │       │ qv-pool1        │              │
│  │ ─────────────── │       │ ─────────────── │              │
│  │ P2P: 30303      │       │ P2P: 30304      │              │
│  │ RPC: 9944       │◄─────►│ RPC: 9945       │              │
│  │ Metrics: 9100   │       │ Metrics: 9101   │              │
│  └─────────────────┘       └─────────────────┘              │
│          ▲                           ▲                       │
│          │ gossip                    │ gossip                │
│          ├──────────────┬────────────┤                       │
│          │              │            │                       │
│          ▼              ▼            ▼                       │
│  ┌─────────────────┐                                         │
│  │ qv-pool2        │                                         │
│  │ ─────────────── │                                         │
│  │ P2P: 30305      │                                         │
│  │ RPC: 9946       │                                         │
│  │ Metrics: 9102   │                                         │
│  └─────────────────┘                                         │
│                                                               │
│  ┌──────────────┐         ┌──────────────┐                  │
│  │ Explorer     │         │ Faucet       │                  │
│  │ Port: 5000   │         │ Port: 5001   │                  │
│  │ (queries pool0)        │ (queries pool0)                  │
│  └──────────────┘         └──────────────┘                  │
│                                                               │
└──────────────────────────────────────────────────────────────┘
```

## Services

### Pool Nodes (qv-node)

JSON-RPC endpoints for blockchain queries and transaction submission.

```bash
# Pool0 (primary)
RPC_URL=http://localhost:9944
curl -s -X POST $RPC_URL \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"qv_getTip","params":[],"id":1}' | jq

# Pool1, Pool2 (redundancy)
RPC_URL=http://localhost:9945  # Pool1
RPC_URL=http://localhost:9946  # Pool2
```

Available RPC methods:
- `qv_getTip` - Get current block height and tip hash
- `qv_getBlockByHeight(height)` - Fetch block by height
- `qv_getBlockByHash(hash)` - Fetch block by hash
- `qv_getTx(tx_id)` - Get transaction by ID
- `qv_sendTransaction(tx_hex)` - Submit signed transaction
- `qv_getUtxo(outpoint)` - Query UTXO existence
- `qv_getBalanceFor(view_key)` - Scan balance for stealth address
- `qv_scanStealth(view_key, from_height, to_height)` - Find stealth outputs
- `qv_getMempoolStatus()` - Mempool size and fee stats

WebSocket subscriptions:
- `qv_subscribeNewBlocks` - New block events
- `qv_subscribeNewTx` - New transaction events

### Block Explorer (Flask)

Web-based read-only interface for browsing blocks, transactions, and addresses.

```
http://localhost:5000          # Dashboard (recent blocks, stats)
http://localhost:5000/block/42 # Block details by height
http://localhost:5000/tx/<hash> # Transaction details
http://localhost:5000/address/<addr> # Address balance and UTXOs
http://localhost:5000/api/stats # JSON API
```

**Features:**
- Real-time network stats (tip height, latest block time)
- Block details (producer, merkle root, transactions)
- Transaction introspection (inputs, outputs, values)
- Stealth address scanning (view key based)
- Responsive HTML UI with search

### Faucet Service (Flask + REST API)

Automated test QV distribution for development.

```bash
# Request 100 QV to stealth address (rate-limited: 1/min per IP)
curl -s "http://localhost:5001/drip?address=devnet1alice00000000000000000000000000000000"

# Response:
{
  "tx_id": "abc123...",
  "amount_qv": 100.0,
  "to_address": "devnet1alice00...",
  "status": "submitted"
}

# Faucet statistics
curl http://localhost:5001/status | jq
```

**Configuration:**
- Environment variables:
  - `FAUCET_ADDRESS` - Faucet's stealth address (funds UTXO)
  - `RPC_ENDPOINT` - Node RPC URL for transaction submission
  - `LOG_DIR` - Directory for persistent drip logs
  - `FLASK_PORT` - HTTP port (default: 5001)

**Rate limiting:**
- 1 drip per minute per IP address
- Persistent log at `$LOG_DIR/drips.jsonl`

## Genesis Configuration

Located at `genesis/genesis.toml` (auto-generated by `genesis.sh`):

```toml
[consensus]
slot_duration_ms = 1000        # 1-second slots (faster iteration)
epoch_slots = 600              # 10-minute epochs
k_finality = 50                # k-deep finality (50 blocks ~ 50s)

[pools]
# 3 stake pools with equal initial stake (33.3% each)
[[pools]]
pool_name = "Pool0"
pledge = 700000000000000       # ~7M QV (1/3 of 21M)

[initial_utxos]
# Faucet pre-funded with 1M QV
[[initial_utxos]]
value = 100000000000000
stealth_address = "devnet1faucet..."
```

### Regenerating Genesis

If you need to restart with fresh pool keys and epoch nonce:

```bash
rm genesis/genesis.final.toml genesis/accounts.toml genesis/bootstrap.peers
bash scripts/genesis.sh
docker-compose down --volumes
docker-compose up -d
```

## Environment Variables

Set in `.env` or `docker-compose.yml`:

```bash
# Node Configuration
QV_NETWORK=devnet
QV_BOOTSTRAP=/bootstrap/bootstrap.peers
QV_LOG_LEVEL=info

# RPC
RPC_BIND_ADDR=0.0.0.0:9944

# Metrics (Prometheus)
METRICS_BIND_ADDR=0.0.0.0:9100

# Faucet
FAUCET_ADDRESS=devnet1faucet0000000000000000000000000000000
LOG_DIR=/faucet/logs

# Explorer
EXPLORER_PORT=5000
```

## Data Persistence

Each node maintains a persistent RocksDB instance:

```bash
# Check node data
docker-compose exec pool0 ls -lh /data/pool0/

# Remove all persistent data (careful!)
docker-compose down --volumes
```

## Port Assignments

| Service | Gossip | RPC  | Metrics | Notes |
|---------|--------|------|---------|-------|
| pool0   | 30303  | 9944 | 9100    | Primary |
| pool1   | 30304  | 9945 | 9101    | Peer |
| pool2   | 30305  | 9946 | 9102    | Peer |
| Explorer| —      | —    | —       | Port 5000 (web) |
| Faucet  | —      | —    | —       | Port 5001 (REST) |

**Important:** All ports are exposed on `localhost` for development. In production, restrict access via firewall/security groups.

## Monitoring

### Logs

```bash
# All services
docker-compose logs -f

# Specific service
docker-compose logs -f pool0

# Last N lines
docker-compose logs --tail=50 pool1

# JSON output (for parsing)
docker-compose logs --no-log-prefix -f pool0 | jq -R 'fromjson?'
```

### Health Checks

```bash
# Docker health status
docker-compose ps

# Manual RPC ping
curl http://localhost:9944/health

# Mempool status
curl -s -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"qv_getMempoolStatus","params":[],"id":1}' | jq
```

### Metrics (Prometheus)

Pool nodes export Prometheus metrics on port 9100:

```bash
curl http://localhost:9100/metrics | grep qv_
# Example outputs:
# qv_chain_height{pool="pool0"} 42
# qv_mempool_size_bytes{pool="pool0"} 15234
# qv_consensus_slot{pool="pool0"} 123
```

## Troubleshooting

### Services won't start

**Symptom:** `docker-compose up` fails or services exit immediately.

**Solutions:**
```bash
# Check Docker daemon
docker ps

# View full logs
docker-compose logs --no-log-prefix

# Validate compose file
docker-compose config

# Rebuild image (clears cache)
docker-compose build --no-cache

# Check port conflicts
lsof -i :9944
```

### Nodes don't sync

**Symptom:** Pool1 and Pool2 have different heights.

**Solutions:**
```bash
# Check network connectivity between nodes
docker-compose exec pool0 ping pool1

# Verify bootstrap peers are reachable
cat devnet/bootstrap/bootstrap.peers

# Check logs for P2P errors
docker-compose logs pool1 | grep -i error

# Reset network
docker-compose down --volumes
docker-compose up -d
```

### Faucet drips fail

**Symptom:** `curl /drip?address=...` returns "RPC request failed".

**Solutions:**
```bash
# Check RPC endpoint
curl http://pool0:9944/health

# Verify faucet can reach pool0
docker-compose exec faucet curl http://pool0:9944/health

# Check logs
docker-compose logs faucet | tail -20

# Monitor drip log
docker-compose exec faucet tail -f /faucet/logs/drips.jsonl
```

### High memory/CPU usage

**Symptom:** Node process consuming excessive resources.

**Solutions:**
```bash
# Monitor resource usage
docker stats

# Reduce block production rate (edit docker-compose.yml)
# Increase SLOT_DURATION_MS from 1000 to 2000

# Clear stale block data
docker-compose down --volumes
```

## Integration Testing

Run end-to-end test suite:

```bash
# All tests (includes bringing up devnet)
bash tests/e2e/run_all.sh

# Individual test
bash tests/e2e/10_simple_transfer.sh

# Verbose output
bash -x tests/e2e/10_simple_transfer.sh
```

See [tests/e2e/README.md](../../tests/e2e/) for test documentation.

## Reference

- **RPC Specification:** See `crates/qv-node/src/rpc.rs`
- **Genesis Format:** See `devnet/genesis/genesis.toml` for all parameters
- **Docker Compose:** See `devnet/docker-compose.yml` for service definitions
- **Scripts:** See `devnet/scripts/` for automation tools
