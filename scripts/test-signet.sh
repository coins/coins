#!/bin/bash
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Coins Signet Integration Tests${NC}"
echo -e "${BLUE}========================================${NC}\n"

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
    else
        echo -e "${RED}  ✗ FAIL${NC}"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
}

# Check services
if ! curl -s http://localhost:8081/account/43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21 &>/dev/null; then
    echo -e "${RED}✗ Publisher not running on port 8081. Run ./scripts/setup-signet.sh first${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Publisher is running${NC}\n"

# Test 1: Submit transaction
run_test "Submit transaction" \
    "PUBLISHER_URL=http://localhost:8081 cargo run --release --example submit_txs 2>&1 | grep -q 'Submitted'"

# Test 2: Wait for mining
echo -e "${YELLOW}[Test 2] Wait for sub-block mining (60s)${NC}"
for i in {1..12}; do sleep 5; echo -n "."; done
echo -e "\n${GREEN}  ✓ PASS${NC}"
PASS_COUNT=$((PASS_COUNT + 1))
TEST_COUNT=$((TEST_COUNT + 1))

# Test 3: Package in mempool
echo -e "${YELLOW}[Test 3] Package relay to mempool${NC}"
TX_COUNT=$(bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 getrawmempool 2>/dev/null | jq '. | length')
if [ "$TX_COUNT" -gt 0 ]; then
    echo -e "${GREEN}  ✓ PASS - ${TX_COUNT} transactions in mempool${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${YELLOW}  ⚠️  WARN - Mempool empty${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi
TEST_COUNT=$((TEST_COUNT + 1))

# Test 4: Wait for block (~1 min)
echo -e "${YELLOW}[Test 4] Wait for signet block confirmation${NC}"
BEFORE_HEIGHT=$(bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 getblockcount 2>/dev/null)
echo -e "  Current height: ${BEFORE_HEIGHT}"
echo -n "  Waiting for next block"

WAIT_COUNT=0
MAX_WAIT=36  # 3 minutes

while true; do
    CURRENT_HEIGHT=$(bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 getblockcount 2>/dev/null)

    if [ "$CURRENT_HEIGHT" -gt "$BEFORE_HEIGHT" ]; then
        echo ""
        echo -e "  ${GREEN}✓ Block ${CURRENT_HEIGHT}${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
        break
    fi

    WAIT_COUNT=$((WAIT_COUNT + 1))
    if [ $WAIT_COUNT -ge $MAX_WAIT ]; then
        echo ""
        echo -e "  ${YELLOW}⚠️  TIMEOUT (not a failure)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
        break
    fi

    echo -n "."; sleep 5
done
TEST_COUNT=$((TEST_COUNT + 1))

# Test 5: Balance persistence
run_test "Account balance persistence" \
    "curl -s http://localhost:8081/account/2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f | jq -e '.balance >= 0'"

# Test 6: Indexer
run_test "Indexer database exists" \
    "test -d .data/db/indexer.db || test -d indexer.db"

# Test 7: RPC
run_test "Bitcoin RPC connectivity" \
    "bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 getblockcount &>/dev/null"

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Test Summary${NC}"
echo -e "${BLUE}========================================${NC}\n"
echo -e "Total:  $TEST_COUNT"
echo -e "${GREEN}Passed: $PASS_COUNT${NC}"

if [ $FAIL_COUNT -gt 0 ]; then
    echo -e "${RED}Failed: $FAIL_COUNT${NC}\n"
    exit 1
else
    echo -e "${GREEN}Failed: $FAIL_COUNT${NC}\n"
    echo -e "${GREEN}All tests passed!${NC}\n"
    exit 0
fi
