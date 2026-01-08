#!/bin/bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=======================================${NC}"
echo -e "${BLUE}   Coins Mutinynet Integration Setup${NC}"
echo -e "${BLUE}=======================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.data"
NETWORK_DIR="${DATA_DIR}/mutinynet"  # Network-specific directory
SUBCHAIN_DIR="${NETWORK_DIR}/subchains"
KEYS_DIR="${NETWORK_DIR}/keys"

# Remote mutinynet node credentials
RPC_USER="bitcoin"
RPC_PASS="8723nasd0932n"
RPC_URL="http://168.119.139.152:38332"
RPC_HOST="168.119.139.152"
RPC_PORT="38332"

SUBCHAIN_COUNT=1000
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SUBCHAIN_FILE="${SUBCHAIN_DIR}/subchain_mutinynet_${TIMESTAMP}.bin"
SUBCHAIN_LINK="${SUBCHAIN_DIR}/subchain_mutinynet.bin"

cd "$PROJECT_ROOT"

echo -e "${YELLOW}[1/9] Cleaning up...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
pkill -9 coins-indexer 2>/dev/null || true
sleep 2

# Clean up old mutinynet-specific data (preserve other networks)
rm -rf "${NETWORK_DIR}" 2>/dev/null || true

# Create network-specific directory structure (all mutinynet data isolated)
mkdir -p "${SUBCHAIN_DIR}" "${KEYS_DIR}"
echo -e "${GREEN}✓ Cleanup complete${NC}\n"

echo -e "${YELLOW}[2/9] Testing connection to remote mutinynet node...${NC}"

# Test connection to remote node
if bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
    BLOCKS=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" getblockchaininfo | jq -r '.blocks')
    echo -e "${GREEN}✓ Connected to mutinynet node${NC}"
    echo -e "${BLUE}→ Current block height: ${BLOCKS}${NC}\n"
else
    echo -e "${RED}✗ Failed to connect to mutinynet node at ${RPC_HOST}:${RPC_PORT}${NC}"
    echo -e "${RED}  Please check network connectivity and credentials${NC}"
    exit 1
fi

echo -e "${YELLOW}[3/9] Building...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher --bin coins-indexer &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}\n"

echo -e "${YELLOW}[4/9] Generating subchain...${NC}"
cat > /tmp/subchain_mutinynet.toml <<EOF
count = ${SUBCHAIN_COUNT}
network = "signet"
output = "${SUBCHAIN_FILE}"
EOF

SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_mutinynet.toml 2>&1 | \
    grep "Generated one-time address:" | awk '{print $4}'
)

echo -e "${GREEN}✓ Subchain address: ${SUBCHAIN_ADDR}${NC}\n"

echo -e "${YELLOW}⚠️  MANUAL FUNDING REQUIRED${NC}"
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "Fund this address from mutinynet faucet:\n"
echo -e "${BLUE}  Address: ${SUBCHAIN_ADDR}${NC}\n"
echo -e "${BLUE}  Faucet: https://faucet.mutinynet.com${NC}"
echo -e "${BLUE}  Amount: 0.001 BTC (minimum)${NC}\n"
echo -e "Waiting for confirmation (~1 min)..."
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"

# Poll for funding (one-time setup operation)
WAIT_COUNT=0
MAX_WAIT=120

while true; do
    UTXO_INFO=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
        scantxoutset start "[\"addr(${SUBCHAIN_ADDR})\"]" 2>/dev/null | \
        jq -r '.unspents[] | select(.height > 0) | "\(.txid):\(.vout) \((.amount*100000000)|floor)"' 2>/dev/null | head -1)

    if [ -n "$UTXO_INFO" ]; then
        UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
        UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')
        echo -e "${GREEN}✓ Funded: ${UTXO_OUTPOINT} (${UTXO_VALUE} sats)${NC}\n"
        break
    fi

    WAIT_COUNT=$((WAIT_COUNT + 1))
    if [ $WAIT_COUNT -ge $MAX_WAIT ]; then
        echo -e "${RED}✗ Timeout waiting for funding${NC}"
        exit 1
    fi

    echo -ne "\rWaiting... ($((WAIT_COUNT * 5))s)"
    sleep 5
done

echo -e "${BLUE}→ Generating subchain file...${NC}"
printf "%s\n%s\n" "${UTXO_OUTPOINT}" "${UTXO_VALUE}" | \
    ./target/release/subchain-setup --config /tmp/subchain_mutinynet.toml &>/dev/null

# Create symlink to latest subchain
ln -sf "$(basename ${SUBCHAIN_FILE})" "${SUBCHAIN_LINK}"

echo -e "${GREEN}✓ Subchain created: $(basename ${SUBCHAIN_FILE})${NC}"
echo -e "${BLUE}→ Linked to: subchain_mutinynet.bin${NC}\n"

echo -e "${YELLOW}[5/9] Setting up test accounts...${NC}"
cargo run --release --example setup_test_accounts "${NETWORK_DIR}/state.db" &>/dev/null
echo -e "${GREEN}✓ Test accounts created${NC}\n"

echo -e "${YELLOW}[6/9] Creating wallet on remote node...${NC}"
# Try to create wallet, ignore error if it already exists
bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
    createwallet "coins-publisher" true 2>/dev/null || true
echo -e "${GREEN}✓ Wallet ready${NC}\n"

echo -e "${YELLOW}[7/9] Starting indexer...${NC}"
mkdir -p "${NETWORK_DIR}/logs"
./target/release/coins-indexer --config config/indexer-mutinynet.toml > "${NETWORK_DIR}/logs/indexer.log" 2>&1 &
IDX_PID=$!
echo "$IDX_PID" > "${NETWORK_DIR}/indexer.pid"

echo -n "Waiting for indexer"
for i in {1..30}; do
    if curl -s http://localhost:8083/health &>/dev/null; then
        echo ""; break
    fi
    echo -n "."; sleep 1
done

echo -e "${GREEN}✓ Indexer running (PID: ${IDX_PID})${NC}\n"

echo -e "${YELLOW}[8/9] Starting publisher...${NC}"
./target/release/coins-publisher --config config/publisher-mutinynet.toml > "${NETWORK_DIR}/logs/publisher.log" 2>&1 &
PUB_PID=$!
echo "$PUB_PID" > "${NETWORK_DIR}/publisher.pid"

echo -n "Waiting for publisher"
for i in {1..30}; do
    if curl -s http://localhost:8082/health &>/dev/null; then
        echo ""; break
    fi
    echo -n "."; sleep 1
done

echo -e "${GREEN}✓ Publisher running (PID: ${PUB_PID})${NC}\n"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Setup Complete!${NC}"
echo -e "${GREEN}========================================${NC}\n"
echo -e "Network:      Mutinynet Signet (~1 min blocks)"
echo -e "Remote Node:  ${RPC_HOST}:${RPC_PORT}"
echo -e "Subchain:     $(basename ${SUBCHAIN_FILE})"
echo -e "Link:         subchain_mutinynet.bin -> $(basename ${SUBCHAIN_FILE})"
echo -e "Logs:         ${NETWORK_DIR}/logs/publisher.log\n"
echo -e "${YELLOW}NEXT STEP:${NC}"
echo -e "${YELLOW}Check ${NETWORK_DIR}/logs/publisher.log for the publisher address${NC}"
echo -e "${YELLOW}Fund it from https://faucet.mutinynet.com (~0.01 BTC)${NC}\n"
echo -e "${BLUE}List subchains: ls -lh ${SUBCHAIN_DIR}/subchain_mutinynet_*.bin${NC}"
echo -e "${BLUE}Monitor logs:   tail -f ${NETWORK_DIR}/logs/publisher.log${NC}"
echo -e "${BLUE}Run tests:      ./scripts/test-mutinynet.sh${NC}\n"
