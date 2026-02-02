#!/usr/bin/env bash
# IBD (Initial Block Download) E2E Test
# Tests that a second node can sync historical sub-blocks from Bitcoin

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   IBD E2E Test${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Configuration
RPC_USER="user"
RPC_PASS="password"
RPC_PORT="18443"
PROJECT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NETWORK_DIR="${PROJECT_ROOT}/.data/regtest"
BITCOIN_DATADIR="${NETWORK_DIR}/bitcoin"
SUBCHAIN_DIR="${NETWORK_DIR}/subchains"
KEYS_DIR="${NETWORK_DIR}/keys"
SUBCHAIN_FILE="${SUBCHAIN_DIR}/subchain_regtest.bin"

# Genesis configuration (must match config/indexer-regtest.toml)
GENESIS_PK="d3cf876dc108c2d3a81c8716a91678d9851518685b04859b021a132ee7440603"
GENESIS_SK="0200000000000000000000000000000000000000000000000000000000000000"

cd "$PROJECT_ROOT"

# Cleanup function
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    pkill -f "coins-publisher.*node1" 2>/dev/null || true
    pkill -f "coins-publisher.*node2" 2>/dev/null || true
    pkill -f "coins-indexer.*node1" 2>/dev/null || true
    pkill -f "coins-indexer.*node2" 2>/dev/null || true
    # Don't delete logs - they're useful for debugging
}

trap cleanup EXIT

# Stop any running services and reset Bitcoin regtest
echo -e "${YELLOW}[1/11] Resetting environment...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
pkill -9 coins-indexer 2>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT stop 2>/dev/null || true
sleep 3

# Clean up old data
rm -rf "${NETWORK_DIR}" 2>/dev/null || true
rm -rf /tmp/node1 /tmp/node2 /tmp/test-keys 2>/dev/null || true
mkdir -p "${SUBCHAIN_DIR}" "${KEYS_DIR}" "${NETWORK_DIR}/logs"
mkdir -p /tmp/node1 /tmp/node2 /tmp/test-keys

# Start fresh bitcoind
mkdir -p "${BITCOIN_DATADIR}"
cat > "${BITCOIN_DATADIR}/bitcoin.conf" <<EOF
regtest=1
server=1
daemon=1
rpcuser=${RPC_USER}
rpcpassword=${RPC_PASS}
fallbackfee=0.00001
txindex=1
acceptnonstdtxn=1
listen=0
discover=0
dnsseed=0
[regtest]
rpcport=${RPC_PORT}
EOF

bitcoind -datadir="${BITCOIN_DATADIR}" -conf="${BITCOIN_DATADIR}/bitcoin.conf" 2>&1 | grep -v "file descriptors" || true
sleep 2

echo -n "Waiting for bitcoind"
for i in {1..30}; do
    if bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT getblockchaininfo &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done
echo -e "${GREEN}✓ Bitcoin Core started${NC}\n"

echo -e "${YELLOW}[2/11] Creating wallets...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    createwallet "coins-publisher" true false "" false true &>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    createwallet "coins-indexer" true false "" false true &>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    createwallet "test-wallet" false &>/dev/null || true
echo -e "${GREEN}✓ Wallets created${NC}\n"

echo -e "${YELLOW}[3/11] Building project...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher --bin coins-indexer --bin coins-client \
    --example get_pk &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}\n"

echo -e "${YELLOW}[4/11] Generating subchain...${NC}"
cat > /tmp/subchain_config.toml <<EOF
count = 100
network = "regtest"
output = "${SUBCHAIN_FILE}"
EOF

# Get subchain address
SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_config.toml 2>&1 | \
    grep "Generated one-time address:" | awk '{print $4}'
)

if [ -z "$SUBCHAIN_ADDR" ]; then
    echo -e "${RED}✗ Failed to generate subchain address${NC}"
    exit 1
fi
echo -e "  Subchain address: $SUBCHAIN_ADDR"

# Mine blocks to subchain address
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    generatetoaddress 101 "$SUBCHAIN_ADDR" &>/dev/null

# Get UTXO
UTXO_INFO=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    scantxoutset start "[\"addr($SUBCHAIN_ADDR)\"]" | \
    jq -r '.unspents[0] | "\(.txid):\(.vout) \((.amount*100000000)|floor)"')
UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')

# Generate subchain file
printf "%s\n%s\n" "$UTXO_OUTPOINT" "$UTXO_VALUE" | \
    ./target/release/subchain-setup --config /tmp/subchain_config.toml &>/dev/null

if [ ! -f "$SUBCHAIN_FILE" ]; then
    echo -e "${RED}✗ Subchain file not created${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Subchain generated (100 transactions)${NC}\n"

echo -e "${YELLOW}[5/11] Creating test keypairs...${NC}"
echo "$GENESIS_SK" > /tmp/test-keys/genesis_sk.hex
./target/release/coins-client --keyfile /tmp/test-keys/alice_sk.hex init &>/dev/null
ALICE_PK=$(./target/release/examples/get_pk /tmp/test-keys/alice_sk.hex)
echo -e "  Genesis PK: ${GENESIS_PK}"
echo -e "  Alice PK: ${ALICE_PK}"
echo -e "${GREEN}✓ Keypairs ready${NC}\n"

echo -e "${YELLOW}[6/11] Creating Node 1 configuration...${NC}"
# Create Node 1 indexer config
cat > /tmp/node1/indexer.toml <<EOF
subchain = "${SUBCHAIN_FILE}"
wallet_name = "coins-indexer"

[database]
state_db = "/tmp/node1/state.db"
indexer_db = "/tmp/node1/indexer.db"

[bitcoin]
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
network = "regtest"

[server]
api_port = 8083
host = "0.0.0.0"

[genesis]
genesis_pk = "${GENESIS_PK}"
genesis_balance = 1000000000000
EOF

# Create Node 1 publisher config
cat > /tmp/node1/publisher.toml <<EOF
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
rpc_wallet = "coins-publisher"
subchain = "${SUBCHAIN_FILE}"
keyfile = "${KEYS_DIR}/publisher_sk.hex"
interval = 30
network = "regtest"
indexer_url = "http://localhost:8083"
api_port = 8080
publish_format = "op_return"
fee_rate_sat_per_vb = 4
EOF

echo -e "${GREEN}✓ Node 1 configuration created${NC}\n"

echo -e "${YELLOW}[7/11] Funding publisher and starting Node 1...${NC}"
# Start publisher briefly to get its address
./target/release/coins-publisher --config /tmp/node1/publisher.toml > /tmp/node1/publisher_temp.log 2>&1 &
TEMP_PID=$!
sleep 5
FEE_ADDR=$(grep "Publisher initialized" /tmp/node1/publisher_temp.log | grep -oE 'bcrt1[a-z0-9]+' | head -1)
kill $TEMP_PID 2>/dev/null || true
sleep 2

if [ -z "$FEE_ADDR" ]; then
    echo -e "${RED}✗ Could not determine publisher address${NC}"
    cat /tmp/node1/publisher_temp.log
    exit 1
fi
echo -e "  Publisher address: $FEE_ADDR"

# Mine blocks to fund publisher
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    generatetoaddress 150 "$FEE_ADDR" &>/dev/null
echo -e "  ${GREEN}✓ Publisher funded (150 blocks)${NC}"

# Start Node 1 indexer
./target/release/coins-indexer --config /tmp/node1/indexer.toml > /tmp/node1/indexer.log 2>&1 &
NODE1_INDEXER_PID=$!
echo -n "  Starting indexer"
for i in {1..30}; do
    if curl -s http://localhost:8083/health &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8083/health &>/dev/null; then
    echo -e "\n${RED}✗ Node 1 indexer failed to start${NC}"
    tail -20 /tmp/node1/indexer.log
    exit 1
fi
echo -e "  ${GREEN}✓ Indexer running (PID: $NODE1_INDEXER_PID, port 8083)${NC}"

# Start Node 1 publisher
./target/release/coins-publisher --config /tmp/node1/publisher.toml > /tmp/node1/publisher.log 2>&1 &
NODE1_PUBLISHER_PID=$!
echo -n "  Starting publisher"
for i in {1..30}; do
    if curl -s http://localhost:8080/health &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8080/health &>/dev/null; then
    echo -e "\n${RED}✗ Node 1 publisher failed to start${NC}"
    tail -20 /tmp/node1/publisher.log
    exit 1
fi
echo -e "  ${GREEN}✓ Publisher running (PID: $NODE1_PUBLISHER_PID, port 8080)${NC}"
echo -e "${GREEN}✓ Node 1 running${NC}\n"

echo -e "${YELLOW}[8/11] Submitting transaction (Genesis -> Alice)...${NC}"
./target/release/coins-client --keyfile /tmp/test-keys/genesis_sk.hex --publisher-url http://localhost:8080 \
    send --recipient-pk "$ALICE_PK" --amount 100 2>&1 | grep -E "success|fail|error" || echo "  Transaction submitted"
echo -e "${GREEN}✓ Transaction submitted${NC}\n"

echo -e "${YELLOW}[9/11] Waiting for sub-block broadcast and mining...${NC}"
echo -n "  Waiting for broadcast"
for i in {1..60}; do
    if grep -q "Package broadcasted successfully" /tmp/node1/publisher.log 2>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! grep -q "Package broadcasted successfully" /tmp/node1/publisher.log 2>/dev/null; then
    echo -e "\n${RED}✗ Package was not broadcast within 60 seconds${NC}"
    tail -50 /tmp/node1/publisher.log
    exit 1
fi
echo -e "  ${GREEN}✓ Package broadcast detected${NC}"

# Extract TXIDs
PACKAGE_LINE=$(grep "Package broadcasted successfully" /tmp/node1/publisher.log | tail -1)
CONNECTOR_TXID=$(echo "$PACKAGE_LINE" | grep -o '[a-f0-9]\{64\}' | head -1)
DATA_TXID=$(echo "$PACKAGE_LINE" | grep -o '[a-f0-9]\{64\}' | tail -1)
echo -e "  Connector TXID: $CONNECTOR_TXID"
echo -e "  Data TXID: $DATA_TXID"

# Mine block to confirm
BLOCKHASH=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    generatetoaddress 1 "$FEE_ADDR" | jq -r '.[0]')
echo -e "  ${GREEN}✓ Block mined: ${BLOCKHASH:0:16}...${NC}"

# Wait for indexer to discover and process the block
echo -n "  Waiting for indexer to process"
ALICE_BAL_NODE1=""
for i in {1..60}; do
    ALICE_BAL_NODE1=$(curl -s "http://localhost:8083/accounts/${ALICE_PK}" | jq -r '.balances["0"] // "null"')
    if [ "$ALICE_BAL_NODE1" != "null" ] && [ "$ALICE_BAL_NODE1" != "0" ] && [ -n "$ALICE_BAL_NODE1" ]; then
        echo ""
        break
    fi
    echo -n "."
    sleep 2
done

echo -e "  Alice balance on Node 1: ${ALICE_BAL_NODE1}"

if [ "$ALICE_BAL_NODE1" == "null" ] || [ "$ALICE_BAL_NODE1" == "0" ] || [ -z "$ALICE_BAL_NODE1" ]; then
    echo -e "${RED}✗ Alice balance not found on Node 1${NC}"
    echo "Indexer logs:"
    tail -30 /tmp/node1/indexer.log
    exit 1
fi
echo -e "${GREEN}✓ Transaction confirmed on Node 1${NC}\n"

echo -e "${YELLOW}[10/11] Stopping Node 1 and starting Node 2 (fresh IBD)...${NC}"
kill $NODE1_PUBLISHER_PID 2>/dev/null || true
kill $NODE1_INDEXER_PID 2>/dev/null || true
sleep 2
echo -e "  ${GREEN}✓ Node 1 stopped${NC}"

# Create Node 2 configuration (different ports and databases)
cat > /tmp/node2/indexer.toml <<EOF
subchain = "${SUBCHAIN_FILE}"
wallet_name = "coins-indexer"

[database]
state_db = "/tmp/node2/state.db"
indexer_db = "/tmp/node2/indexer.db"

[bitcoin]
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
network = "regtest"

[server]
api_port = 8084
host = "0.0.0.0"

[genesis]
genesis_pk = "${GENESIS_PK}"
genesis_balance = 1000000000000
EOF

cat > /tmp/node2/publisher.toml <<EOF
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
rpc_wallet = "coins-publisher"
subchain = "${SUBCHAIN_FILE}"
keyfile = "${KEYS_DIR}/publisher_sk.hex"
interval = 30
network = "regtest"
indexer_url = "http://localhost:8084"
api_port = 8081
publish_format = "op_return"
fee_rate_sat_per_vb = 4
EOF

# Start Node 2 indexer
./target/release/coins-indexer --config /tmp/node2/indexer.toml > /tmp/node2/indexer.log 2>&1 &
NODE2_INDEXER_PID=$!
echo -n "  Starting Node 2 indexer"
for i in {1..30}; do
    if curl -s http://localhost:8084/health &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8084/health &>/dev/null; then
    echo -e "\n${RED}✗ Node 2 indexer failed to start${NC}"
    tail -20 /tmp/node2/indexer.log
    exit 1
fi
echo -e "  ${GREEN}✓ Node 2 indexer running (PID: $NODE2_INDEXER_PID, port 8084)${NC}"

# Start Node 2 publisher
./target/release/coins-publisher --config /tmp/node2/publisher.toml > /tmp/node2/publisher.log 2>&1 &
NODE2_PUBLISHER_PID=$!
echo -n "  Starting Node 2 publisher"
for i in {1..30}; do
    if curl -s http://localhost:8081/health &>/dev/null; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if ! curl -s http://localhost:8081/health &>/dev/null; then
    echo -e "\n${RED}✗ Node 2 publisher failed to start${NC}"
    tail -20 /tmp/node2/publisher.log
    exit 1
fi
echo -e "  ${GREEN}✓ Node 2 publisher running (PID: $NODE2_PUBLISHER_PID, port 8081)${NC}"
echo -e "${GREEN}✓ Node 2 running (fresh databases)${NC}\n"

echo -e "${YELLOW}[11/11] Verifying IBD sync...${NC}"
# Wait for Node 2 to perform IBD and discover Alice's account
echo -n "  Waiting for IBD to complete"
ALICE_BAL_NODE2=""
for i in {1..90}; do
    ALICE_BAL_NODE2=$(curl -s "http://localhost:8084/accounts/${ALICE_PK}" | jq -r '.balances["0"] // "null"')
    if [ "$ALICE_BAL_NODE2" != "null" ] && [ "$ALICE_BAL_NODE2" != "0" ] && [ -n "$ALICE_BAL_NODE2" ]; then
        echo ""
        break
    fi
    echo -n "."
    sleep 2
done

echo -e "  Alice balance on Node 2: ${ALICE_BAL_NODE2}"

# Verify balances match
echo ""
echo -e "  Expected (from Node 1): ${ALICE_BAL_NODE1}"
echo -e "  Got (from Node 2 IBD): ${ALICE_BAL_NODE2}"

if [ "$ALICE_BAL_NODE2" == "null" ] || [ "$ALICE_BAL_NODE2" == "0" ] || [ -z "$ALICE_BAL_NODE2" ]; then
    echo -e "${RED}✗ IBD FAILED: Node 2 did not sync Alice's balance${NC}"
    echo ""
    echo "Node 2 indexer logs:"
    tail -50 /tmp/node2/indexer.log
    exit 1
fi

if [ "$ALICE_BAL_NODE1" != "$ALICE_BAL_NODE2" ]; then
    echo -e "${RED}✗ IBD FAILED: Balance mismatch${NC}"
    echo "Node 1: $ALICE_BAL_NODE1"
    echo "Node 2: $ALICE_BAL_NODE2"
    exit 1
fi

echo -e "${GREEN}✓ Balances match!${NC}"

# Cleanup
kill $NODE2_PUBLISHER_PID 2>/dev/null || true
kill $NODE2_INDEXER_PID 2>/dev/null || true

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   IBD E2E Test PASSED${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Node 2 successfully synced sub-blocks via IBD"
echo "Both nodes have consistent state (Alice balance: $ALICE_BAL_NODE2)"
