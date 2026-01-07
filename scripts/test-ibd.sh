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
    pkill -f "coins-aggregator.*node1" 2>/dev/null || true
    pkill -f "coins-aggregator.*node2" 2>/dev/null || true
    # Don't delete logs - they're useful for debugging
    rm -rf /tmp/node1 /tmp/node2 2>/dev/null || true
}

trap cleanup EXIT

# Ensure bitcoind is running
if ! bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT getblockchaininfo &>/dev/null; then
    echo -e "${RED}✗ Bitcoin Core not running. Please run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

echo -e "${YELLOW}[1/8] Creating test configurations...${NC}"

# Create Node 1 config (port 8080, default DBs)
mkdir -p /tmp/node1
cat > /tmp/node1/aggregator.toml <<EOF
# Node 1 Configuration (Primary node)
rpc_url = "http://localhost:$RPC_PORT"
rpc_user = "$RPC_USER"
rpc_pass = "$RPC_PASS"
rpc_wallet = "coins-aggregator"

subchain = ".data/subchains/subchain_regtest.bin"
keyfile = ".data/keys/aggregator_sk.hex"
interval = 5
network = "regtest"
genesis_pk = "43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21"
genesis_balance = 1000000000000

# Node 1 runtime config
api_port = 8080
state_db = "/tmp/node1/state.db"
indexer_db = "/tmp/node1/indexer.db"
bls_keyfile = "/tmp/node1/aggregator_bls_sk.hex"
EOF

# Create Node 2 config (port 8081, separate DBs)
mkdir -p /tmp/node2
cat > /tmp/node2/aggregator.toml <<EOF
# Node 2 Configuration (IBD node)
rpc_url = "http://localhost:$RPC_PORT"
rpc_user = "$RPC_USER"
rpc_pass = "$RPC_PASS"
rpc_wallet = "coins-aggregator"

subchain = ".data/subchains/subchain_regtest.bin"
keyfile = ".data/keys/aggregator_sk.hex"
interval = 5
network = "regtest"
genesis_pk = "43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21"
genesis_balance = 1000000000000

# Node 2 runtime config (different port and DBs)
api_port = 8081
state_db = "/tmp/node2/state.db"
indexer_db = "/tmp/node2/indexer.db"
bls_keyfile = "/tmp/node2/aggregator_bls_sk.hex"
EOF

echo -e "${GREEN}✓ Configurations created${NC}"

echo -e "${YELLOW}[2/8] Setting up test accounts for Node 1...${NC}"
# Setup Alice and Bob in Node 1's state database
cargo run --release --example setup_test_accounts /tmp/node1/state.db &>/dev/null
echo -e "${GREEN}✓ Test accounts created${NC}"

echo -e "${YELLOW}[3/8] Starting Node 1 (Primary)...${NC}"
./target/release/coins-aggregator --config /tmp/node1/aggregator.toml > /tmp/aggregator-node1.log 2>&1 &
NODE1_PID=$!

# Wait for Node 1 to start
sleep 3
if ! kill -0 $NODE1_PID 2>/dev/null; then
    echo -e "${RED}✗ Node 1 failed to start${NC}"
    tail -20 /tmp/aggregator-node1.log
    exit 1
fi

if ! curl -s http://localhost:8080/health &>/dev/null; then
    echo -e "${RED}✗ Node 1 API not responding${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Node 1 running (PID: $NODE1_PID, API: 8080)${NC}"

echo -e "${YELLOW}[4/9] Submitting transactions to Node 1...${NC}"

# Submit test transactions using submit_txs example
# (Uses Alice and Bob keys from test-data/keys/)
BOB_PK="5e74734c69fbb261c4c936d375df870f2a6af117f811a5c88f8c3328f291c012"

# Submit transaction
cargo run --release --example submit_txs > /tmp/submit_output.log 2>&1 || {
    echo -e "${RED}✗ Failed to submit transactions${NC}"
    cat /tmp/submit_output.log
    exit 1
}

echo -e "${GREEN}✓ Transactions submitted${NC}"

echo -e "${YELLOW}[5/9] Waiting for sub-block mining (60 seconds)...${NC}"
sleep 60

# Verify transactions were mined on Node 1
BOB_STATE_NODE1=$(curl -s http://localhost:8080/account/$BOB_PK)
BOB_BALANCE_NODE1=$(echo "$BOB_STATE_NODE1" | jq -r '.balance')

if [ "$BOB_BALANCE_NODE1" == "null" ] || [ "$BOB_BALANCE_NODE1" == "0" ]; then
    echo -e "${RED}✗ Transactions not mined on Node 1 (Bob balance: $BOB_BALANCE_NODE1)${NC}"
    tail -50 /tmp/aggregator-node1.log
    exit 1
fi

echo -e "${GREEN}✓ Sub-block mined on Node 1 (Bob balance: $BOB_BALANCE_NODE1)${NC}"

echo -e "${YELLOW}[6/9] Stopping Node 1...${NC}"
kill $NODE1_PID 2>/dev/null || true
sleep 2
echo -e "${GREEN}✓ Node 1 stopped${NC}"

echo -e "${YELLOW}[7/9] Starting Node 2 (IBD node with fresh databases)...${NC}"
./target/release/coins-aggregator --config /tmp/node2/aggregator.toml > /tmp/aggregator-node2.log 2>&1 &
NODE2_PID=$!

# Wait for Node 2 to start and perform IBD
sleep 5
if ! kill -0 $NODE2_PID 2>/dev/null; then
    echo -e "${RED}✗ Node 2 failed to start${NC}"
    tail -20 /tmp/aggregator-node2.log
    exit 1
fi

if ! curl -s http://localhost:8081/health &>/dev/null; then
    echo -e "${RED}✗ Node 2 API not responding${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Node 2 running (PID: $NODE2_PID, API: 8081)${NC}"

echo -e "${YELLOW}[8/9] Waiting for IBD to complete (30 seconds)...${NC}"
sleep 30

echo -e "${YELLOW}[9/9] Verifying state consistency...${NC}"

# Compare states between Node 1 (from disk) and Node 2 (from API)
BOB_STATE_NODE2=$(curl -s http://localhost:8081/account/$BOB_PK)
BOB_BALANCE_NODE2=$(echo "$BOB_STATE_NODE2" | jq -r '.balance')

GENESIS_STATE_NODE2=$(curl -s http://localhost:8081/account/$GENESIS_PK)
GENESIS_BALANCE_NODE2=$(echo "$GENESIS_STATE_NODE2" | jq -r '.balance')

echo ""
echo "Node 1 (disk): Bob balance = $BOB_BALANCE_NODE1"
echo "Node 2 (IBD):  Bob balance = $BOB_BALANCE_NODE2"
echo ""

# Verify balances match
if [ "$BOB_BALANCE_NODE1" != "$BOB_BALANCE_NODE2" ]; then
    echo -e "${RED}✗ IBD FAILED: Bob balance mismatch${NC}"
    echo "Expected: $BOB_BALANCE_NODE1"
    echo "Got:      $BOB_BALANCE_NODE2"
    echo ""
    echo "Node 2 logs:"
    tail -50 /tmp/aggregator-node2.log
    exit 1
fi

if [ "$BOB_BALANCE_NODE2" == "null" ] || [ "$BOB_BALANCE_NODE2" == "0" ]; then
    echo -e "${RED}✗ IBD FAILED: Node 2 did not sync transactions${NC}"
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
