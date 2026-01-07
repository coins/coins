#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Coins Regtest Integration Setup${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.data"
NETWORK_DIR="${DATA_DIR}/regtest"  # Network-specific directory
BITCOIN_DATADIR="${NETWORK_DIR}/bitcoin"  # Bitcoin regtest data
SUBCHAIN_DIR="${NETWORK_DIR}/subchains"
KEYS_DIR="${NETWORK_DIR}/keys"

# RPC Configuration
RPC_USER="user"
RPC_PASS="password"
RPC_PORT="18443"

# Subchain configuration
SUBCHAIN_COUNT=1000  # Number of pre-signed transactions
SUBCHAIN_FILE="${SUBCHAIN_DIR}/subchain_regtest.bin"

cd "$PROJECT_ROOT"

echo -e "${YELLOW}[1/9] Cleaning up old processes and data...${NC}"
# Kill any running processes (try multiple times to be sure)
killall -9 bitcoind 2>/dev/null || true
pkill -9 bitcoind 2>/dev/null || true
pkill -9 coins-publisher 2>/dev/null || true
sleep 3

# Verify no bitcoind is running
if pgrep bitcoind >/dev/null; then
    echo -e "${RED}✗ Could not stop existing bitcoind process${NC}"
    echo -e "${YELLOW}Please manually stop bitcoind and try again${NC}"
    exit 1
fi

# Clean up old regtest-specific data (preserve other networks)
rm -rf "${NETWORK_DIR}" 2>/dev/null || true
rm -rf indexer.db state.db explorer-tx-index.db 2>/dev/null || true  # Legacy paths
rm -f publisher_bls_sk.hex 2>/dev/null || true

# Create network-specific directory structure (all regtest data isolated)
mkdir -p "${SUBCHAIN_DIR}"
mkdir -p "${KEYS_DIR}"

echo -e "${GREEN}✓ Cleanup complete${NC}"
echo ""

echo -e "${YELLOW}[2/9] Starting Bitcoin Core (regtest)...${NC}"
# Clean regtest data
rm -rf "${BITCOIN_DATADIR}" 2>/dev/null || true
mkdir -p "${BITCOIN_DATADIR}"

# Create bitcoin.conf
cat > "${BITCOIN_DATADIR}/bitcoin.conf" <<EOF
# Bitcoin regtest configuration
regtest=1
server=1
daemon=1

# RPC settings
rpcuser=${RPC_USER}
rpcpassword=${RPC_PASS}
rpcport=${RPC_PORT}

# Transaction settings
fallbackfee=0.00001
txindex=1
acceptnonstdtxn=1

# Network settings (minimal for regtest)
listen=0
discover=0
dnsseed=0
upnp=0
natpmp=0
maxconnections=0

# Resource limits
dbcache=50
par=1
EOF

# Increase file descriptor limit for bitcoind
ulimit -n 2048 2>/dev/null || true

# Start bitcoind with config file
bitcoind -datadir="${BITCOIN_DATADIR}" -conf="${BITCOIN_DATADIR}/bitcoin.conf" 2>&1 | grep -v "file descriptors" || true

# Check if bitcoind actually started despite the warning
sleep 2

# Wait for bitcoind to start
echo -n "Waiting for bitcoind to start"
for i in {1..30}; do
    if bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
    echo -e "${RED}✗ Failed to start bitcoind${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Bitcoin Core started${NC}"
echo ""

echo -e "${YELLOW}[3/9] Creating publisher wallet...${NC}"
# Create watch-only wallet for publisher
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    createwallet "coins-publisher" true false "" false true &>/dev/null

echo -e "${GREEN}✓ Wallet created${NC}"
echo ""

echo -e "${YELLOW}[4/9] Building project...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

echo -e "${YELLOW}[5/9] Generating subchain with funding...${NC}"

# Create subchain config
cat > /tmp/subchain_regtest.toml <<EOF
count = ${SUBCHAIN_COUNT}
network = "regtest"
output = "${SUBCHAIN_FILE}"
EOF

# Step 1: Run subchain-setup once to get the address (will error when asking for outpoint)
echo -e "  ${BLUE}→ Generating subchain key and address...${NC}"
SUBCHAIN_ADDR=$(
    (
        echo ""  # Will trigger error when it asks for outpoint
    ) | ./target/release/subchain-setup --config /tmp/subchain_regtest.toml 2>&1 | \
    grep "Generated one-time address:" | \
    awk '{print $4}'
)

if [ -z "$SUBCHAIN_ADDR" ]; then
    echo -e "${RED}✗ Failed to get subchain address${NC}"
    exit 1
fi

echo -e "  ${GREEN}✓ Subchain address: ${SUBCHAIN_ADDR}${NC}"

# Step 2: Mine blocks to that address (need 101+ for mature coinbase)
echo -e "  ${BLUE}→ Mining 101 blocks to subchain address...${NC}"
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    generatetoaddress 101 "${SUBCHAIN_ADDR}" &>/dev/null

echo -e "  ${GREEN}✓ Blocks mined${NC}"

# Step 3: Get a mature UTXO
echo -e "  ${BLUE}→ Finding mature UTXO...${NC}"
UTXO_INFO=$(bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    scantxoutset start "[\"addr(${SUBCHAIN_ADDR})\"]" | \
    jq -r '.unspents[0] | "\(.txid):\(.vout) \((.amount*100000000)|floor)"')

UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')

if [ -z "$UTXO_OUTPOINT" ] || [ "$UTXO_OUTPOINT" = "null:null" ]; then
    echo -e "${RED}✗ Failed to find UTXO${NC}"
    exit 1
fi

echo -e "  ${GREEN}✓ Found UTXO: ${UTXO_OUTPOINT} (${UTXO_VALUE} sats)${NC}"

# Step 4: Generate subchain with the UTXO
echo -e "  ${BLUE}→ Generating subchain file (${SUBCHAIN_COUNT} transactions)...${NC}"
printf "%s\n%s\n" "${UTXO_OUTPOINT}" "${UTXO_VALUE}" | \
    ./target/release/subchain-setup --config /tmp/subchain_regtest.toml &>/dev/null

if [ ! -f "${SUBCHAIN_FILE}" ]; then
    echo -e "${RED}✗ Failed to create subchain file${NC}"
    exit 1
fi

SUBCHAIN_SIZE=$(du -h "${SUBCHAIN_FILE}" | awk '{print $1}')
echo -e "${GREEN}✓ Subchain created (${SUBCHAIN_SIZE})${NC}"
echo ""

echo -e "${YELLOW}[6/9] Determining publisher address and mining blocks...${NC}"
# Create directories
mkdir -p "${KEYS_DIR}"

# Start publisher briefly to determine publisher address
echo -e "  ${BLUE}→ Starting publisher temporarily to get publisher address...${NC}"
mkdir -p .data/regtest/logs
./target/release/coins-publisher --config config/publisher.toml > .data/regtest/logs/publisher_temp.log 2>&1 &
TEMP_PUB_PID=$!

# Wait for publisher to initialize and log its address
sleep 5

# Extract publisher address from log
FEE_ADDR=$(grep "Publisher initialized" .data/regtest/logs/publisher_temp.log | grep -oE 'bcrt1[a-z0-9]+' | head -1)

# Stop temporary publisher
kill $TEMP_PUB_PID 2>/dev/null || true
sleep 2

if [ -z "$FEE_ADDR" ]; then
    echo -e "${RED}✗ Failed to get publisher address${NC}"
    cat .data/regtest/logs/publisher_temp.log
    exit 1
fi

echo -e "  ${GREEN}✓ Publisher address: ${FEE_ADDR}${NC}"
echo -e "  ${BLUE}→ Mining 150 blocks to publisher address...${NC}"

# Generate blocks to publisher address
# Need 100+ blocks for coinbase maturity, plus extra for testing
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    generatetoaddress 150 "${FEE_ADDR}" &>/dev/null

BLOCK_COUNT=$(bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockcount)
echo -e "${GREEN}✓ Mined 150 blocks (total: ${BLOCK_COUNT})${NC}"
echo ""

echo -e "${YELLOW}[7/9] Setting up test accounts...${NC}"
cargo run --release --example setup_test_accounts &>/dev/null
echo -e "${GREEN}✓ Test accounts created${NC}"
echo ""

echo -e "${YELLOW}[8/9] Starting publisher...${NC}"
./target/release/coins-publisher --config config/publisher.toml > .data/regtest/logs/publisher.log 2>&1 &
PUBLISHER_PID=$!
echo "$PUBLISHER_PID" > .data/regtest/publisher.pid

# Wait for publisher to start
echo -n "Waiting for publisher to start"
for i in {1..30}; do
    if curl -s http://localhost:8080/health &>/dev/null; then
        echo ""
        break
    fi
    if ! kill -0 $PUBLISHER_PID 2>/dev/null; then
        echo ""
        echo -e "${RED}✗ Publisher crashed. Check .data/regtest/logs/publisher.log${NC}"
        tail -20 .data/regtest/logs/publisher.log
        exit 1
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8080/health &>/dev/null; then
    echo ""
    echo -e "${RED}✗ Publisher failed to start${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Publisher running (PID: ${PUBLISHER_PID})${NC}"
echo ""

echo -e "${YELLOW}[9/9] Running integration test...${NC}"
cargo run --release --example submit_txs 2>&1 | tail -5

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Setup Complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "Subchain: ${SUBCHAIN_FILE}"
echo -e "Address:  ${SUBCHAIN_ADDR}"
echo -e "Blocks:   ${BLOCK_COUNT}"
echo -e "Logs:     .data/regtest/logs/publisher.log"
echo ""
echo -e "${BLUE}Run tests with:${NC}"
echo -e "  cargo run --release --example submit_txs"
echo ""
echo -e "${BLUE}Query accounts:${NC}"
echo -e "  curl http://localhost:8080/account/<pubkey_hex>"
echo ""
echo -e "${BLUE}Stop services:${NC}"
echo -e "  ./scripts/stop-regtest.sh"
echo ""
