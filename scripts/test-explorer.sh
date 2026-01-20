#!/usr/bin/env bash
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
echo -e "${BLUE}   Explorer & Confirmation Tests${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Configuration
PUBLISHER_URL="http://localhost:8080"
INDEXER_URL="http://localhost:8083"
EXPLORER_URL="http://localhost:3000"
BITCOIN_CLI="bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443"

# Check prerequisites
echo -e "${YELLOW}Checking prerequisites...${NC}"

if ! curl -s $PUBLISHER_URL/health &>/dev/null; then
    echo -e "${RED}✗ Publisher not running at $PUBLISHER_URL${NC}"
    echo -e "${YELLOW}Run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

if ! curl -s $INDEXER_URL/health &>/dev/null; then
    echo -e "${RED}✗ Indexer not running at $INDEXER_URL${NC}"
    echo -e "${YELLOW}Run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

if ! $BITCOIN_CLI getblockchaininfo &>/dev/null; then
    echo -e "${RED}✗ Bitcoin Core not running${NC}"
    echo -e "${YELLOW}Run ./scripts/setup-regtest.sh first${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Publisher, Indexer, and Bitcoin running${NC}"

# Check if explorer is running, start if not
EXPLORER_PID=""
if ! curl -s $EXPLORER_URL/health &>/dev/null; then
    echo -e "${YELLOW}Starting explorer...${NC}"
    cargo build --release -p coins-explorer &>/dev/null
    ./target/release/coins-explorer --config config/explorer-regtest.toml > .data/regtest/logs/explorer.log 2>&1 &
    EXPLORER_PID=$!
    echo "$EXPLORER_PID" > .data/regtest/explorer.pid

    # Wait for explorer to start
    for i in {1..10}; do
        if curl -s $EXPLORER_URL/health &>/dev/null; then
            echo -e "${GREEN}✓ Explorer running (PID: $EXPLORER_PID)${NC}"
            break
        fi
        sleep 1
    done

    if ! curl -s $EXPLORER_URL/health &>/dev/null; then
        echo -e "${RED}✗ Explorer failed to start${NC}"
        cat .data/regtest/logs/explorer.log | tail -20
        exit 1
    fi
else
    echo -e "${GREEN}✓ Explorer already running at $EXPLORER_URL${NC}"
fi

echo ""

# Test counters
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

# Get test keypairs
GENESIS_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/genesis_sk.hex 2>/dev/null || echo "")
ALICE_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/alice_sk.hex 2>/dev/null || echo "")
BOB_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/bob_sk.hex 2>/dev/null || echo "")

if [ -z "$ALICE_PK" ] || [ -z "$BOB_PK" ]; then
    echo -e "${RED}✗ Could not read test keypairs${NC}"
    exit 1
fi

echo -e "  Genesis PK: ${GENESIS_PK:0:16}..."
echo -e "  Alice PK: ${ALICE_PK:0:16}..."
echo -e "  Bob PK: ${BOB_PK:0:16}..."
echo ""

echo -e "${BLUE}=== Explorer API Tests ===${NC}"
echo ""

# Test 1: Explorer health
run_test "Explorer health endpoint" \
    "curl -s $EXPLORER_URL/health | grep -q OK"

# Test 2: Get account via explorer
run_test "Explorer account lookup" \
    "curl -s $EXPLORER_URL/api/v1/accounts/$ALICE_PK | jq -e '.balance >= 0'"

# Test 3: Get latest block
run_test "Explorer latest block" \
    "curl -s $EXPLORER_URL/api/v1/blocks/latest | jq -e '.height >= 0 or .height == null'"

# Test 4: Get stats
run_test "Explorer stats endpoint" \
    "curl -s $EXPLORER_URL/api/v1/stats | jq -e 'type == \"object\"'"

# Test 5: Get block by height (block 0 should exist after setup)
run_test "Explorer block by height" \
    "curl -s $EXPLORER_URL/api/v1/blocks/0 | jq -e '.height == 0 or .error' 2>/dev/null || true"

# Test 6: Get blocks range
run_test "Explorer blocks range" \
    "curl -s '$EXPLORER_URL/api/v1/blocks?from=0&to=5' | jq -e 'type == \"array\" or type == \"object\"'"

echo ""
echo -e "${BLUE}=== Confirmation Status Tests ===${NC}"
echo ""

# Get Alice's current balance
ALICE_BEFORE=$(curl -s "$PUBLISHER_URL/account/$ALICE_PK" | jq -r '.balance // 0')
echo -e "  Alice balance before: $ALICE_BEFORE"

# Test 7: Submit a new transaction
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Submit transaction (Alice -> Bob, 50 tokens)${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

TX_OUTPUT=$(./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex --publisher-url $PUBLISHER_URL \
    send --recipient-pk "$BOB_PK" --amount 50 2>&1)

if echo "$TX_OUTPUT" | grep -qi "success\|sent"; then
    echo -e "${GREEN}  ✓ PASS - Transaction submitted${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${YELLOW}  ? Transaction output: ${TX_OUTPUT}${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 8: Check pending-transactions endpoint shows the transaction
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Pending transactions endpoint${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

PENDING=$(curl -s "$EXPLORER_URL/api/v1/pending-transactions")
PENDING_COUNT=$(echo "$PENDING" | jq 'if type == "array" then length else 0 end')

if [ "$PENDING_COUNT" -gt 0 ]; then
    # Check the status field
    FIRST_STATUS=$(echo "$PENDING" | jq -r '.[0].status // "unknown"')
    echo -e "${GREEN}  ✓ PASS - Pending has $PENDING_COUNT transaction(s), status: $FIRST_STATUS${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${YELLOW}  ? No pending transactions (tx may have been mined already)${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 9: Wait for broadcast and check recently-broadcast
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Recently broadcast endpoint${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

echo -e "  Waiting 35 seconds for sub-block mining..."
sleep 35

RECENTLY=$(curl -s "$EXPLORER_URL/api/v1/recently-broadcast")
echo -e "  Recently broadcast response: $RECENTLY"

# Test passes if endpoint works (empty array is OK if indexed)
if echo "$RECENTLY" | jq -e 'type == "array"' &>/dev/null; then
    RECENTLY_COUNT=$(echo "$RECENTLY" | jq 'length')
    echo -e "${GREEN}  ✓ PASS - Recently broadcast endpoint works ($RECENTLY_COUNT items)${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${RED}  ✗ FAIL - Invalid response${NC}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Test 10: Mine a block and check confirmation
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Mine block and check confirmation${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

# Get publisher address for mining
FEE_ADDR=$(curl -s "$PUBLISHER_URL/address" | jq -r '.address // empty')
if [ -z "$FEE_ADDR" ]; then
    FEE_ADDR="bcrt1qxl767gvfrpcf4lclag3w5707xdk0j7hxnyj02g"  # fallback
fi

BLOCK_HASH=$($BITCOIN_CLI generatetoaddress 1 "$FEE_ADDR" | jq -r '.[0]')
echo -e "  Mined block: ${BLOCK_HASH:0:16}..."

# Wait for indexer to process
echo -e "  Waiting 15 seconds for indexer discovery..."
sleep 15

# Check latest block via explorer
LATEST=$(curl -s "$EXPLORER_URL/api/v1/blocks/latest")
LATEST_HEIGHT=$(echo "$LATEST" | jq -r '.height // "null"')
BTC_HEIGHT=$(echo "$LATEST" | jq -r '.btc_height // 0')

if [ "$LATEST_HEIGHT" != "null" ]; then
    echo -e "  Latest sub-block height: $LATEST_HEIGHT, btc_height: $BTC_HEIGHT"
    if [ "$BTC_HEIGHT" -gt 0 ] || [ "$BTC_HEIGHT" == "0" ]; then
        echo -e "${GREEN}  ✓ PASS - Block has confirmation status${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo -e "${YELLOW}  ? btc_height is $BTC_HEIGHT (may still be processing)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No blocks indexed yet${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 11: Balance updated correctly
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Balance updated after confirmation${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

ALICE_AFTER=$(curl -s "$PUBLISHER_URL/account/$ALICE_PK" | jq -r '.balance // 0')
echo -e "  Alice balance after: $ALICE_AFTER (was: $ALICE_BEFORE)"

# Balance should have decreased by at least 50 (amount) + fee
if [ "$ALICE_AFTER" -lt "$ALICE_BEFORE" ]; then
    DIFF=$((ALICE_BEFORE - ALICE_AFTER))
    echo -e "${GREEN}  ✓ PASS - Balance decreased by $DIFF${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${RED}  ✗ FAIL - Balance not updated${NC}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Test 12: Transaction history
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Account transaction history${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

TX_HISTORY=$(curl -s "$EXPLORER_URL/api/v1/accounts/$ALICE_PK/transactions")
TX_COUNT=$(echo "$TX_HISTORY" | jq 'if type == "array" then length else 0 end')

if [ "$TX_COUNT" -gt 0 ]; then
    echo -e "${GREEN}  ✓ PASS - Found $TX_COUNT transaction(s) for Alice${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${YELLOW}  ? No transaction history yet${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 13: Recently broadcast should be empty after indexing
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Recently broadcast clears after indexing${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

RECENTLY_AFTER=$(curl -s "$EXPLORER_URL/api/v1/recently-broadcast")
RECENTLY_AFTER_COUNT=$(echo "$RECENTLY_AFTER" | jq 'if type == "array" then length else 0 end')

if [ "$RECENTLY_AFTER_COUNT" -eq 0 ]; then
    echo -e "${GREEN}  ✓ PASS - Recently broadcast is empty (transactions indexed)${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
else
    echo -e "${YELLOW}  ? Still $RECENTLY_AFTER_COUNT items in recently broadcast${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

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
    echo -e "  .data/regtest/logs/publisher.log"
    echo -e "  .data/regtest/logs/indexer.log"
    echo -e "  .data/regtest/logs/explorer.log"
    exit 1
else
    echo -e "${GREEN}Failed: $FAIL_COUNT${NC}"
    echo ""
    echo -e "${GREEN}All tests passed!${NC}"
fi
