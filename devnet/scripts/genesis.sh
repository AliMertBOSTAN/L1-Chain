#!/bin/bash
# Genesis initialization script for QuantumVault devnet
# Generates pool keys, faucet UTXO, and populates genesis.toml

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEVNET_DIR="$(dirname "$SCRIPT_DIR")"
GENESIS_TEMPLATE="$DEVNET_DIR/genesis/genesis.toml"
GENESIS_OUT="$DEVNET_DIR/genesis/genesis.final.toml"
ACCOUNTS_OUT="$DEVNET_DIR/genesis/accounts.toml"
BOOTSTRAP_DIR="$DEVNET_DIR/bootstrap"
BOOTSTRAP_PEERS="$BOOTSTRAP_DIR/bootstrap.peers"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "======================================================================"
echo "QuantumVault Devnet Genesis Initialization"
echo "======================================================================"

# Ensure directories exist
mkdir -p "$BOOTSTRAP_DIR"

# Generate epoch nonce (32 bytes hex)
echo -e "${YELLOW}[1/5]${NC} Generating epoch nonce..."
EPOCH_NONCE=$(openssl rand -hex 32)
echo "      Epoch nonce: $EPOCH_NONCE"

# Initialize pool keys using qv-miner init (mock implementation)
# In real scenario, this calls qv-miner init --pool-name=Pool0 --output=/tmp/pool0.json, etc.
# For now, we generate placeholder key material.

echo -e "${YELLOW}[2/5]${NC} Initializing stake pool keys..."

declare -a POOL_NAMES=("Pool0" "Pool1" "Pool2")
declare -a VRF_KEYS=()
declare -a KES_KEYS=()
declare -a POOL_IDS=()

for i in {0..2}; do
  POOL_NAME="${POOL_NAMES[$i]}"

  # Generate dummy VRF key (32 bytes = 64 hex chars)
  VRF_KEY=$(openssl rand -hex 32)
  VRF_KEYS+=("$VRF_KEY")

  # Generate dummy KES key (Kyber public key ~1184 bytes, here simplified)
  KES_KEY=$(openssl rand -hex 1184)
  KES_KEYS+=("$KES_KEY")

  # Pool ID = SHA3-256(vrf_key) — mock with first 64 chars of VRF key
  POOL_ID=$(echo -n "$VRF_KEY" | sha256sum | cut -d' ' -f1)
  POOL_IDS+=("$POOL_ID")

  echo "      $POOL_NAME: pool_id=${POOL_ID:0:16}... vrf=${VRF_KEY:0:16}..."
done

echo -e "${YELLOW}[3/5]${NC} Generating faucet and test addresses..."

# Faucet stealth address (placeholder: will be derived from real key in production)
FAUCET_STEALTH_ADDR="devnet1faucet0000000000000000000000000000000"
FAUCET_SCRIPT_HASH=$(echo -n "p2pkh_pqc:faucet" | sha256sum | cut -d' ' -f1)

# Test address for e2e scenarios
TEST_STEALTH_ADDR="devnet1test00000000000000000000000000000000"
TEST_SCRIPT_HASH=$(echo -n "p2pkh_pqc:test" | sha256sum | cut -d' ' -f1)

echo "      Faucet: $FAUCET_STEALTH_ADDR (script_hash=${FAUCET_SCRIPT_HASH:0:16}...)"
echo "      Test:   $TEST_STEALTH_ADDR (script_hash=${TEST_SCRIPT_HASH:0:16}...)"

echo -e "${YELLOW}[4/5]${NC} Building genesis.toml from template..."

# Copy template and substitute values
cp "$GENESIS_TEMPLATE" "$GENESIS_OUT"

# Substitute epoch nonce
sed -i "s/epoch_nonce = \".*\"/epoch_nonce = \"$EPOCH_NONCE\"/" "$GENESIS_OUT"

# Substitute pool keys for each pool (indices 0, 1, 2)
for i in {0..2}; do
  # Line number calculation: pools start around line 80, each pool block is ~10 lines
  POOL_START_LINE=$((82 + i * 11))

  # Use a more robust substitution: replace within [[pools]] blocks
  python3 << PYTHON_END
import re

with open('$GENESIS_OUT', 'r') as f:
    content = f.read()

# Find and replace pool $i fields
pool_blocks = re.findall(r'\[\[pools\]\].*?(?=\[\[pools\]\]|\[|$)', content, re.DOTALL)

if $i < len(pool_blocks):
    old_block = pool_blocks[$i]
    new_block = old_block
    new_block = re.sub(r'pool_id = ""', f'pool_id = "{POOL_IDS[$i]}"', new_block)
    new_block = re.sub(r'vrf_public_key = ""', f'vrf_public_key = "{VRF_KEYS[$i]}"', new_block)
    new_block = re.sub(r'kes_public_key = ""', f'kes_public_key = "{KES_KEYS[$i]}"', new_block)

    content = content.replace(old_block, new_block, 1)

with open('$GENESIS_OUT', 'w') as f:
    f.write(content)
PYTHON_END
done

# Substitute faucet UTXO
sed -i "s|stealth_address = \"devnet1faucet.*\"|stealth_address = \"$FAUCET_STEALTH_ADDR\"|" "$GENESIS_OUT"
sed -i "s|locking_script_hex = \"00\"  # Placeholder; will be replaced|locking_script_hex = \"$FAUCET_SCRIPT_HASH\"|" "$GENESIS_OUT"

# Substitute test UTXO
sed -i "s|stealth_address = \"devnet1test.*\"|stealth_address = \"$TEST_STEALTH_ADDR\"|" "$GENESIS_OUT"

echo "      Genesis written to: $GENESIS_OUT"

echo -e "${YELLOW}[5/5]${NC} Writing accounts.toml for wallet initialization..."

cat > "$ACCOUNTS_OUT" << ACCOUNTS_EOF
# Pre-funded accounts for devnet testing
# Format: stealth_address = { view_key_hex, spend_key_hex }

[faucet]
stealth_address = "$FAUCET_STEALTH_ADDR"
view_key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
spend_key_hex = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
balance_sats = 100000000000000  # 1M QV

[test_account]
stealth_address = "$TEST_STEALTH_ADDR"
view_key_hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
spend_key_hex = "9876543210fedcba9876543210fedcba9876543210fedcba9876543210fedcba"
balance_sats = 50000000000000  # 500k QV

ACCOUNTS_EOF

echo "      Accounts written to: $ACCOUNTS_OUT"

echo -e "${YELLOW}Bootstrap Peers${NC}"

# Create bootstrap peers file (list of node addresses for P2P discovery)
# In a real devnet, these are the multiaddr strings from the genesis pools
cat > "$BOOTSTRAP_PEERS" << BOOTSTRAP_EOF
# Bootstrap peers for devnet P2P discovery
# Format: /ip4/host/tcp/port/p2p/peer_id

/ip4/pool0/tcp/30303/p2p/QmPool0DevnetPeerId0000000000000000000001
/ip4/pool1/tcp/30304/p2p/QmPool1DevnetPeerId0000000000000000000002
/ip4/pool2/tcp/30305/p2p/QmPool2DevnetPeerId0000000000000000000003

BOOTSTRAP_EOF

echo "      Bootstrap peers written to: $BOOTSTRAP_PEERS"

echo ""
echo -e "${GREEN}Genesis initialization complete!${NC}"
echo ""
echo "Summary:"
echo "  Pools:           3 (Pool0, Pool1, Pool2)"
echo "  Epoch nonce:     ${EPOCH_NONCE:0:16}..."
echo "  Faucet address:  $FAUCET_STEALTH_ADDR"
echo "  Test address:    $TEST_STEALTH_ADDR"
echo "  Epoch duration:  10 minutes (600 slots at 1s/slot)"
echo "  Total supply:    21,000,000 QV"
echo ""
echo "Files generated:"
echo "  - $GENESIS_OUT"
echo "  - $ACCOUNTS_OUT"
echo "  - $BOOTSTRAP_PEERS"
echo ""
echo "Next steps:"
echo "  1. Review genesis.final.toml and adjust parameters as needed"
echo "  2. Run: docker-compose build"
echo "  3. Run: docker-compose up"
echo ""
