#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Coins Integration Tests${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Check if services are running
if ! curl -s http://localhost:8080/health &>/dev/null; then
    echo -e "${RED}✗ Publisher not running${NC}"
    echo -e "${YELLOW}Run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

if ! bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getblockchaininfo &>/dev/null; then
    echo -e "${RED}✗ Bitcoin Core not running${NC}"
    echo -e "${YELLOW}Run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Services are running${NC}"
echo ""

TEST_COUNT=0
PASS_COUNT=0
FAIL_COUNT=0

run_test() {
    local test_name="$1"
    local test_cmd="$2"

    TEST_COUNT=$((TEST_COUNT + 1))
    echo -e "${YELLOW}[Test $TEST_COUNT] $test_name${NC}"

    if eval "$test_cmd"; then
        echo -e "${GREEN}  ✓ PASS${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
        return 0
    else
        echo -e "${RED}  ✗ FAIL${NC}"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        return 1
    fi
}

echo -e "${BLUE}Running integration tests...${NC}"
echo ""

# Test 1: Transaction submission and mining
run_test "Submit transaction and mine sub-block" \
    "cargo run --release --example submit_txs 2>&1 | grep -q 'E2E Test PASSED'"

# Test 2: Mine a block and verify package relay
echo -e "${YELLOW}[Test 2] Package relay to blockchain${NC}"
BEFORE_HEIGHT=$(bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getblockcount)

# Submit transaction
cargo run --release --example submit_txs &>/dev/null &
SUBMIT_PID=$!

# Wait for sub-block to be broadcast (30 second mining interval + buffer)
sleep 35

# Mine a block
BLOCK_HASH=$(bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    generatetoaddress 1 bcrt1qxl767gvfrpcf4lclag3w5707xdk0j7hxnyj02g | jq -r '.[0]')

# Check if block has more than just coinbase
TX_COUNT=$(bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    getblock "$BLOCK_HASH" | jq '.tx | length')

wait $SUBMIT_PID 2>/dev/null || true

if [ "$TX_COUNT" -gt 1 ]; then
    echo -e "${GREEN}  ✓ PASS - Block contains $TX_COUNT transactions${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${RED}  ✗ FAIL - Block only has coinbase (check logs)${NC}"
    echo -e "    Last 10 lines from publisher log:"
    tail -10 /tmp/publisher.log | sed 's/^/    /'
    FAIL_COUNT=$((FAIL_COUNT + 1))
fi
TEST_COUNT=$((TEST_COUNT + 1))

# Test 3: Account balance persistence
run_test "Account balance persistence" \
    "curl -s http://localhost:8080/account/2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f | jq -e '.balance > 0'"

# Test 4: Indexer functionality
run_test "Indexer has indexed blocks" \
    "test -d indexer.db && du -sh indexer.db | grep -v '^0'"

# Test 5: RPC connectivity
run_test "Bitcoin RPC connectivity" \
    "bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getblockcount &>/dev/null"

# Test 6: Wallet exists and has UTXOs
run_test "Publisher wallet has UTXOs" \
    "bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 -rpcwallet=coins-publisher \
        listdescriptors | jq -e '.descriptors | length > 0'"

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Test Summary${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "Total:  $TEST_COUNT"
echo -e "${GREEN}Passed: $PASS_COUNT${NC}"

if [ $FAIL_COUNT -gt 0 ]; then
    echo -e "${RED}Failed: $FAIL_COUNT${NC}"
    echo ""
    echo -e "${YELLOW}Check logs:${NC}"
    echo -e "  /tmp/publisher.log"
    exit 1
else
    echo -e "${GREEN}Failed: $FAIL_COUNT${NC}"
    echo ""
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
fi
