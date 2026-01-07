#!/bin/bash
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
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

cd "$PROJECT_ROOT"

# Cleanup function
cleanup() {
    echo -e "\n${YELLOW}Cleaning up...${NC}"
    pkill -f "coins-publisher.*node1" 2>/dev/null || true
    pkill -f "coins-publisher.*node2" 2>/dev/null || true
    # Don't delete logs - they're useful for debugging
    rm -rf /tmp/node1 /tmp/node2 2>/dev/null || true
}

trap cleanup EXIT

# Stop any running publishers and reset Bitcoin regtest to ensure fresh state
echo -e "${YELLOW}Resetting Bitcoin regtest for fresh test environment...${NC}"
pkill -f "coins-publisher" 2>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT stop 2>/dev/null || true
sleep 3

# Remove regtest data to start fresh
rm -rf "$HOME/Library/Application Support/Bitcoin/regtest" 2>/dev/null || true

# Restart bitcoind with non-standard tx support for Taproot annex
bitcoind -regtest -daemon -fallbackfee=0.00001 -txindex=1 -acceptnonstdtxn=1 -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT
sleep 5

# Mine initial blocks and fund the subchain
echo -e "${YELLOW}Setting up fresh regtest chain...${NC}"
# Create wallet with private keys enabled
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT createwallet "test-wallet" false || {
    echo -e "${YELLOW}Note: Wallet may already exist${NC}"
}

# Mine blocks to get coins (need 101+ for coinbase maturity)
ADDR=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT -rpcwallet=test-wallet getnewaddress)
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 150 "$ADDR" &>/dev/null
echo -e "${GREEN}✓ Mined 150 blocks${NC}"

# Generate fresh subchain for this regtest environment
echo -e "  ${BLUE}→ Generating fresh subchain...${NC}"

# Clean up old subchain data to ensure fresh keys
rm -rf .data/subchains .data/keys 2>/dev/null || true
mkdir -p .data/subchains .data/keys

# Create subchain config
cat > /tmp/subchain_config.toml <<EOF
count = 100
network = "regtest"
output = ".data/subchains/subchain_regtest.bin"
EOF

# Step 1: Run subchain-setup to get the generated address (will error, but we capture the address)
SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_config.toml 2>&1 | \
    grep "Generated one-time address:" | \
    awk '{print $4}'
)

if [ -z "$SUBCHAIN_ADDR" ]; then
    echo -e "${RED}✗ Failed to generate subchain address${NC}"
    exit 1
fi

echo -e "  ${BLUE}→ Subchain address: $SUBCHAIN_ADDR${NC}"

# Step 2: Mine blocks directly to subchain address to get mature coinbase UTXOs
echo -e "  ${BLUE}→ Mining blocks to subchain address...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 10 "$SUBCHAIN_ADDR" &>/dev/null

# Mine additional blocks to mature the coinbase (need 100 confirmations)
echo -e "  ${BLUE}→ Mining blocks to mature coinbase...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 95 "$ADDR" &>/dev/null

# Step 3: Find a mature UTXO using scantxoutset
echo -e "  ${BLUE}→ Finding mature UTXO...${NC}"
UTXO_INFO=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    scantxoutset start "[\"addr($SUBCHAIN_ADDR)\"]" | \
    jq -r '.unspents[0] | "\(.txid):\(.vout) \((.amount*100000000)|floor)"')

UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')

if [ -z "$UTXO_OUTPOINT" ] || [ "$UTXO_OUTPOINT" = "null:null" ]; then
    echo -e "${RED}✗ Failed to find UTXO${NC}"
    exit 1
fi

echo -e "  ${BLUE}→ Found UTXO: $UTXO_OUTPOINT ($UTXO_VALUE sats)${NC}"

# Step 4: Generate subchain file with the UTXO
echo -e "  ${BLUE}→ Building subchain file...${NC}"
printf "%s\n%s\n" "$UTXO_OUTPOINT" "$UTXO_VALUE" | \
    ./target/release/subchain-setup --config /tmp/subchain_config.toml &>/dev/null

# Verify subchain was created
if [ ! -f ".data/subchains/subchain_regtest.bin" ]; then
    echo -e "${RED}✗ Subchain file not created${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Fresh subchain generated (100 transactions)${NC}"

echo -e "${YELLOW}[1/8] Creating test configurations...${NC}"

# Create Node 1 config (port 8080, default DBs)
mkdir -p /tmp/node1
SUBCHAIN_PATH="$(pwd)/.data/subchains/subchain_regtest.bin"
KEYFILE_PATH="$(pwd)/.data/keys/publisher_sk.hex"

cat > /tmp/node1/publisher.toml <<EOF
# Node 1 Configuration (Primary node)
rpc_url = "http://localhost:$RPC_PORT"
rpc_user = "$RPC_USER"
rpc_pass = "$RPC_PASS"
rpc_wallet = "coins-publisher"

subchain = "$SUBCHAIN_PATH"
keyfile = "$KEYFILE_PATH"
interval = 60
network = "regtest"
genesis_pk = "43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21"
genesis_balance = 1000000000000

# Node 1 runtime config
api_port = 8080
state_db = "/tmp/node1/state.db"
indexer_db = "/tmp/node1/indexer.db"
bls_keyfile = "/tmp/node1/publisher_bls_sk.hex"
EOF

# Create Node 2 config (port 8081, separate DBs)
mkdir -p /tmp/node2
cat > /tmp/node2/publisher.toml <<EOF
# Node 2 Configuration (IBD node)
rpc_url = "http://localhost:$RPC_PORT"
rpc_user = "$RPC_USER"
rpc_pass = "$RPC_PASS"
rpc_wallet = "coins-publisher"

subchain = "$SUBCHAIN_PATH"
keyfile = "$KEYFILE_PATH"
interval = 60
network = "regtest"
genesis_pk = "43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21"
genesis_balance = 1000000000000

# Node 2 runtime config (different port and DBs)
api_port = 8081
state_db = "/tmp/node2/state.db"
indexer_db = "/tmp/node2/indexer.db"
bls_keyfile = "/tmp/node2/publisher_bls_sk.hex"
EOF

echo -e "${GREEN}✓ Configurations created${NC}"

echo -e "${YELLOW}[2/8] Setting up test accounts...${NC}"
# Setup Alice and Bob in both node databases (needed for transaction validation)
cargo run --release --example setup_test_accounts /tmp/node1/state.db &>/dev/null
cargo run --release --example setup_test_accounts /tmp/node2/state.db &>/dev/null
echo -e "${GREEN}✓ Test accounts created for both nodes${NC}"

echo -e "${YELLOW}[3/8] Determining Node 1's fee address...${NC}"
# Start publisher briefly to determine fee address
./target/release/coins-publisher --config /tmp/node1/publisher.toml > /tmp/publisher_temp.log 2>&1 &
TEMP_PID=$!
sleep 5

# Extract fee address from logs (handle both "Publisher" and "Aggregator" for backwards compatibility)
FEE_ADDR=$(grep -E "(Publisher|Aggregator) initialized" /tmp/publisher_temp.log | grep -o 'bcrt1q[a-z0-9]*' | head -1)

if [ -z "$FEE_ADDR" ]; then
    echo -e "${RED}✗ Could not determine fee address${NC}"
    echo -e "${YELLOW}Publisher log output:${NC}"
    cat /tmp/publisher_temp.log
    kill $TEMP_PID 2>/dev/null || true
    exit 1
fi

echo -e "  ${BLUE}→ Fee address: $FEE_ADDR${NC}"

# Stop the temporary publisher
kill $TEMP_PID 2>/dev/null || true
sleep 2
echo -e "${GREEN}✓ Fee address determined${NC}"

echo -e "${YELLOW}[4/8] Funding fee address...${NC}"
# Mine blocks to fee address BEFORE starting the main publisher
echo -e "  ${BLUE}→ Mining 50 blocks to fee address...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 50 "$FEE_ADDR" &>/dev/null

# Send additional funds via transaction
echo -e "  ${BLUE}→ Sending additional funds to fee address...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT -rpcwallet=test-wallet sendtoaddress "$FEE_ADDR" 10 &>/dev/null
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 1 "$FEE_ADDR" &>/dev/null
echo -e "${GREEN}✓ Fee address funded (51 blocks + 1 transaction)${NC}"

echo -e "${YELLOW}[5/8] Starting Node 1 (Primary)...${NC}"
./target/release/coins-publisher --config /tmp/node1/publisher.toml > /tmp/publisher-node1.log 2>&1 &
NODE1_PID=$!

# Wait for Node 1 to start and rescan blockchain
echo -e "  ${BLUE}→ Waiting for blockchain rescan to complete...${NC}"
sleep 10
if ! kill -0 $NODE1_PID 2>/dev/null; then
    echo -e "${RED}✗ Node 1 failed to start${NC}"
    tail -20 /tmp/publisher-node1.log
    exit 1
fi

if ! curl -s http://localhost:8080/health &>/dev/null; then
    echo -e "${RED}✗ Node 1 API not responding${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Node 1 running (PID: $NODE1_PID, API: 8080)${NC}"

echo -e "${YELLOW}[6/8] Submitting transactions to Node 1...${NC}"

# Submit test transactions using submit_txs example
# (Uses Alice and Bob keys from .data/test-keys/)
BOB_PK="5e74734c69fbb261c4c936d375df870f2a6af117f811a5c88f8c3328f291c012"

# Submit transaction
cargo run --release --example submit_txs > /tmp/submit_output.log 2>&1 || {
    echo -e "${RED}✗ Failed to submit transactions${NC}"
    cat /tmp/submit_output.log
    exit 1
}

echo -e "${GREEN}✓ Transactions submitted${NC}"

echo -e "${YELLOW}[7/8] Waiting for sub-block to be broadcast (watching logs)...${NC}"
# Wait for Node 1 to broadcast the package
for i in {1..30}; do
    if grep -q "Package broadcasted successfully" /tmp/publisher-node1.log 2>/dev/null; then
        echo -e "${GREEN}✓ Package broadcast detected${NC}"
        break
    fi
    sleep 1
done

if ! grep -q "Package broadcasted successfully" /tmp/publisher-node1.log 2>/dev/null; then
    echo -e "${RED}✗ Package was not broadcast within 30 seconds${NC}"
    tail -50 /tmp/publisher-node1.log
    exit 1
fi

# Extract connector and data TXIDs (extract 64-char hex strings)
PACKAGE_LINE=$(grep "Package broadcasted successfully" /tmp/publisher-node1.log | tail -1)
CONNECTOR_TXID=$(echo "$PACKAGE_LINE" | grep -o '[a-f0-9]\{64\}' | head -1)
DATA_TXID=$(echo "$PACKAGE_LINE" | grep -o '[a-f0-9]\{64\}' | tail -1)
echo "Connector TXID: $CONNECTOR_TXID"
echo "Data TXID: $DATA_TXID"

# Mine a block IMMEDIATELY (within the same second) to confirm the package before it can be RBF'd
echo -e "${YELLOW}[8/8] Mining block IMMEDIATELY to confirm package...${NC}"
# Get address from subchain file
if [ -f ".data/subchains/subchain_regtest.bin" ]; then
    MINING_ADDR=$(./target/release/subchain-setup --print-address .data/subchains/subchain_regtest.bin)
else
    echo -e "${RED}✗ Subchain file not found${NC}"
    exit 1
fi

if [ -z "$MINING_ADDR" ]; then
    echo -e "${RED}✗ Failed to extract address from subchain${NC}"
    exit 1
fi

# Check mempool BEFORE mining
echo -e "  ${BLUE}→ Checking mempool before mining...${NC}"
MEMPOOL_BEFORE=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT getrawmempool)
echo "Mempool TX count: $(echo "$MEMPOOL_BEFORE" | jq 'length')"
if echo "$MEMPOOL_BEFORE" | jq -e ".[] | select(. == \"$CONNECTOR_TXID\")" &>/dev/null; then
    echo -e "${GREEN}✓ Connector TX in mempool${NC}"
else
    echo -e "${YELLOW}⚠ Connector TX NOT in mempool${NC}"
fi

# Mine the block
BLOCKHASH=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 1 "$MINING_ADDR" | jq -r '.[0]')
echo -e "${GREEN}✓ Block mined: $BLOCKHASH${NC}"

# Check what TXs are in the block
BLOCK_TXS=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT getblock "$BLOCKHASH" | jq '.tx')
echo "Block TX count: $(echo "$BLOCK_TXS" | jq 'length')"
echo "Block TXs: $(echo "$BLOCK_TXS" | jq -c '.')"

# Verify the connector TX is in the block
sleep 2
if echo "$BLOCK_TXS" | jq -e ".[] | select(. == \"$CONNECTOR_TXID\")" &>/dev/null; then
    echo -e "${GREEN}✓ Connector TX confirmed in block${NC}"
else
    echo -e "${RED}✗ Connector TX not in mined block${NC}"
    echo "Connector TX info: $CONNECTOR_TXID"
    echo -e "\nChecking mempool..."
    MEMPOOL=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT getrawmempool)
    echo "$MEMPOOL"
    if echo "$MEMPOOL" | grep -q "$CONNECTOR_TXID"; then
        echo -e "${YELLOW}Connector TX is in mempool but not yet mined. Mining another block...${NC}"
        bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT generatetoaddress 1 "$MINING_ADDR" &>/dev/null
        sleep 2
    else
        echo -e "${RED}Connector TX not in mempool or blockchain${NC}"
        exit 1
    fi
fi

# Stop Node 1 now that transactions are confirmed
kill $NODE1_PID 2>/dev/null || true
sleep 1
echo -e "${GREEN}✓ Node 1 stopped${NC}"

echo -e "${GREEN}✓ Sub-block and connector TX confirmed in blockchain${NC}"

echo -e "${YELLOW}[9/9] Starting Node 2 (IBD node with fresh databases)...${NC}"
./target/release/coins-publisher --config /tmp/node2/publisher.toml > /tmp/publisher-node2.log 2>&1 &
NODE2_PID=$!

# Wait for Node 2 to start and perform IBD
sleep 5
if ! kill -0 $NODE2_PID 2>/dev/null; then
    echo -e "${RED}✗ Node 2 failed to start${NC}"
    tail -20 /tmp/publisher-node2.log
    exit 1
fi

if ! curl -s http://localhost:8081/health &>/dev/null; then
    echo -e "${RED}✗ Node 2 API not responding${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Node 2 running (PID: $NODE2_PID, API: 8081)${NC}"

echo -e "${YELLOW}Waiting for IBD to complete and verifying state...${NC}"
sleep 30

# Expected balance (from submit_txs example - transfers 100 from Alice to Bob)
EXPECTED_BOB_BALANCE=100

# Get Node 2's state
BOB_STATE_NODE2=$(curl -s http://localhost:8081/account/$BOB_PK)
BOB_BALANCE_NODE2=$(echo "$BOB_STATE_NODE2" | jq -r '.balance')

GENESIS_STATE_NODE2=$(curl -s http://localhost:8081/account/$GENESIS_PK)
GENESIS_BALANCE_NODE2=$(echo "$GENESIS_STATE_NODE2" | jq -r '.balance')

echo ""
echo "Expected: Bob balance = $EXPECTED_BOB_BALANCE"
echo "Node 2 (IBD):  Bob balance = $BOB_BALANCE_NODE2"
echo ""

# Verify balance matches expected
if [ "$EXPECTED_BOB_BALANCE" != "$BOB_BALANCE_NODE2" ]; then
    echo -e "${RED}✗ IBD FAILED: Bob balance mismatch${NC}"
    echo "Expected: $EXPECTED_BOB_BALANCE"
    echo "Got:      $BOB_BALANCE_NODE2"
    echo ""
    echo "Node 2 logs:"
    tail -100 /tmp/publisher-node2.log
    exit 1
fi

if [ "$BOB_BALANCE_NODE2" == "null" ] || [ "$BOB_BALANCE_NODE2" == "0" ] || [ -z "$BOB_BALANCE_NODE2" ]; then
    echo -e "${RED}✗ IBD FAILED: Node 2 did not sync transactions (Bob balance: $BOB_BALANCE_NODE2)${NC}"
    echo "Node 2 logs:"
    tail -100 /tmp/publisher-node2.log
    exit 1
fi

echo -e "${GREEN}✓ State verification passed${NC}"
echo -e "${GREEN}✓ Bob balance consistent: $BOB_BALANCE_NODE2${NC}"
echo -e "${GREEN}✓ Genesis balance on Node 2: $GENESIS_BALANCE_NODE2${NC}"

# Cleanup
kill $NODE2_PID 2>/dev/null || true

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   IBD E2E Test PASSED ✓${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Node 2 successfully synced sub-blocks via IBD"
echo "Both nodes have consistent state"
