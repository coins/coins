#!/usr/bin/env bash
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

[regtest]
rpcport=${RPC_PORT}
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

echo -e "${YELLOW}[4/10] Creating indexer wallet...${NC}"
# Create watch-only wallet for indexer
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    createwallet "coins-indexer" true false "" false true &>/dev/null

echo -e "${GREEN}✓ Wallet created${NC}"
echo ""

echo -e "${YELLOW}[5/10] Building project...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher --bin coins-indexer --bin coins-client \
    --example get_pk &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

echo -e "${YELLOW}[6/10] Generating subchain with funding...${NC}"

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

# Get current block height for genesis_height optimization
GENESIS_HEIGHT=$(bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockcount)

printf "%s\n%s\n%s\n" "${UTXO_OUTPOINT}" "${UTXO_VALUE}" "${GENESIS_HEIGHT}" | \
    ./target/release/subchain-setup --config /tmp/subchain_regtest.toml &>/dev/null

if [ ! -f "${SUBCHAIN_FILE}" ]; then
    echo -e "${RED}✗ Failed to create subchain file${NC}"
    exit 1
fi

SUBCHAIN_SIZE=$(du -h "${SUBCHAIN_FILE}" | awk '{print $1}')
echo -e "${GREEN}✓ Subchain created (${SUBCHAIN_SIZE})${NC}"
echo ""

echo -e "${YELLOW}[7/10] Determining publisher address and mining blocks...${NC}"
# Create directories
mkdir -p "${KEYS_DIR}"

# Start publisher briefly to determine publisher address
echo -e "  ${BLUE}→ Starting publisher temporarily to get publisher address...${NC}"
mkdir -p .data/regtest/logs
./target/release/coins-publisher --config config/publisher-regtest.toml > .data/regtest/logs/publisher_temp.log 2>&1 &
TEMP_PUB_PID=$!

# Wait for publisher to initialize and query its address from API
FEE_ADDR=""
for i in {1..10}; do
    FEE_ADDR=$(curl -s http://localhost:8082/address 2>/dev/null | jq -r '.address' 2>/dev/null)
    if [ -n "$FEE_ADDR" ] && [ "$FEE_ADDR" != "null" ] && [ "$FEE_ADDR" != "" ]; then
        break
    fi
    sleep 1
done

# Stop temporary publisher
kill $TEMP_PUB_PID 2>/dev/null || true
sleep 2

if [ -z "$FEE_ADDR" ] || [ "$FEE_ADDR" == "null" ] || [ "$FEE_ADDR" == "" ]; then
    echo -e "${RED}✗ Failed to get publisher address from API${NC}"
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

echo -e "${YELLOW}[8/11] Creating test keypairs...${NC}"
mkdir -p .data/regtest/test-keys

# Create fixed genesis secret key (sk=2, matches config/indexer-regtest.toml genesis_pk)
echo "0200000000000000000000000000000000000000000000000000000000000000" > .data/regtest/test-keys/genesis_sk.hex
echo -e "  ${GREEN}✓ Genesis key created${NC}"

# Create Alice and Bob keypairs using coins-client
./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex init 2>&1 | grep -q "stored" && echo -e "  ${GREEN}✓ Alice key created${NC}" || echo -e "  ${GREEN}✓ Alice key exists${NC}"
./target/release/coins-client --keyfile .data/regtest/test-keys/bob_sk.hex init 2>&1 | grep -q "stored" && echo -e "  ${GREEN}✓ Bob key created${NC}" || echo -e "  ${GREEN}✓ Bob key exists${NC}"

echo -e "${GREEN}✓ Test keypairs ready${NC}"
echo ""

echo -e "${YELLOW}[9/11] Starting indexer...${NC}"
mkdir -p .data/regtest/logs
RUST_LOG=coins_indexer=debug ./target/release/coins-indexer --config config/indexer-regtest.toml > .data/regtest/logs/indexer.log 2>&1 &
INDEXER_PID=$!
echo "$INDEXER_PID" > .data/regtest/indexer.pid

# Wait for indexer to start
echo -n "Waiting for indexer to start"
for i in {1..30}; do
    if curl -s http://localhost:8083/health &>/dev/null; then
        echo ""
        break
    fi
    if ! kill -0 $INDEXER_PID 2>/dev/null; then
        echo ""
        echo -e "${RED}✗ Indexer crashed. Check .data/regtest/logs/indexer.log${NC}"
        tail -20 .data/regtest/logs/indexer.log
        exit 1
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8083/health &>/dev/null; then
    echo ""
    echo -e "${RED}✗ Indexer failed to start${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Indexer running (PID: ${INDEXER_PID})${NC}"
echo ""

echo -e "${YELLOW}[10/11] Starting publisher...${NC}"
./target/release/coins-publisher --config config/publisher-regtest.toml > .data/regtest/logs/publisher.log 2>&1 &
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

echo -e "${YELLOW}[11/11] Funding test accounts from genesis...${NC}"

# Get Alice and Bob public keys
ALICE_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/alice_sk.hex)
BOB_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/bob_sk.hex)
echo -e "  Alice PK: ${ALICE_PK}"
echo -e "  Bob PK: ${BOB_PK}"

# Fund Alice from genesis (10000 tokens)
echo -e "  ${BLUE}→ Sending 10000 tokens from Genesis to Alice...${NC}"
./target/release/coins-client --keyfile .data/regtest/test-keys/genesis_sk.hex --publisher-url http://localhost:8080 \
    send --recipient-pk "$ALICE_PK" --amount 10000 2>&1 | grep -E "success|fail" || echo -e "    Transaction submitted"

# Wait for publisher to mine the transaction
echo -e "  ${BLUE}→ Waiting 35 seconds for publisher to mine sub-block...${NC}"
sleep 35

# Mine a Bitcoin block to confirm the transactions
echo -e "  ${BLUE}→ Mining Bitcoin block to confirm transactions...${NC}"
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    generatetoaddress 1 "${FEE_ADDR}" &>/dev/null

# Wait for indexer to discover and process the block
echo -e "  ${BLUE}→ Waiting 15 seconds for indexer discovery...${NC}"
sleep 15

# Fund Bob from Alice (1000 tokens)
echo -e "  ${BLUE}→ Sending 1000 tokens from Alice to Bob...${NC}"
./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex --publisher-url http://localhost:8080 \
    send --recipient-pk "$BOB_PK" --amount 1000 2>&1 | grep -E "success|fail" || echo -e "    Transaction submitted"

# Wait for another mining cycle
echo -e "  ${BLUE}→ Waiting 35 seconds for publisher to mine sub-block...${NC}"
sleep 35

# Mine another Bitcoin block
bitcoin-cli -regtest -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
    generatetoaddress 1 "${FEE_ADDR}" &>/dev/null

# Wait for indexer
echo -e "  ${BLUE}→ Waiting 15 seconds for indexer discovery...${NC}"
sleep 15

# Verify accounts were created
echo -e "  ${BLUE}→ Verifying accounts...${NC}"
ALICE_BAL=$(curl -s "http://localhost:8080/account/${ALICE_PK}" | jq -r '.balance // "not found"')
BOB_BAL=$(curl -s "http://localhost:8080/account/${BOB_PK}" | jq -r '.balance // "not found"')

echo -e "  Alice balance: ${ALICE_BAL}"
echo -e "  Bob balance: ${BOB_BAL}"

if [ "$ALICE_BAL" != "not found" ] && [ "$BOB_BAL" != "not found" ]; then
    echo -e "${GREEN}✓ Test accounts funded and verified${NC}"
else
    echo -e "${YELLOW}⚠ Test accounts may not be ready yet (check logs)${NC}"
fi

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
echo -e "  ./scripts/test-regtest.sh"
echo ""
echo -e "${BLUE}Query accounts:${NC}"
echo -e "  curl http://localhost:8080/account/<pubkey_hex>"
echo ""
echo -e "${BLUE}Stop services:${NC}"
echo -e "  ./scripts/stop-regtest.sh"
echo ""
