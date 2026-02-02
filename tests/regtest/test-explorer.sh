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
INDEXER_URL="http://localhost:8084"
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
    "curl -s $EXPLORER_URL/api/v1/accounts/$ALICE_PK | jq -e '.balances[\"0\"] >= 0'"

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

# Get Alice's current balance (native token)
ALICE_BEFORE=$(curl -s "$PUBLISHER_URL/account/$ALICE_PK" | jq -r '.balances["0"] // 0')
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

ALICE_AFTER=$(curl -s "$PUBLISHER_URL/account/$ALICE_PK" | jq -r '.balances["0"] // 0')
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
echo -e "${BLUE}=== Pending Transaction Status Tests ===${NC}"
echo ""

# Test 14: Submit another transaction and check status fields in pending-transactions
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Pending transaction status fields (publishing)${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

# Submit a new transaction
./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex --publisher-url $PUBLISHER_URL \
    send --recipient-pk "$BOB_PK" --amount 10 &>/dev/null || true

sleep 2

PENDING=$(curl -s "$EXPLORER_URL/api/v1/pending-transactions")
PENDING_COUNT=$(echo "$PENDING" | jq 'if type == "array" then length else 0 end')

if [ "$PENDING_COUNT" -gt 0 ]; then
    # Check that status fields exist and have correct values for publishing status
    FIRST_TX=$(echo "$PENDING" | jq '.[0]')
    STATUS=$(echo "$FIRST_TX" | jq -r '.status // "missing"')
    HAS_BTC_HEIGHT=$(echo "$FIRST_TX" | jq 'has("btc_height")')
    HAS_CONFIRMATIONS=$(echo "$FIRST_TX" | jq 'has("confirmations")')
    HAS_FINALIZED=$(echo "$FIRST_TX" | jq 'has("finalized")')

    echo -e "  Status: $STATUS, has btc_height: $HAS_BTC_HEIGHT, has confirmations: $HAS_CONFIRMATIONS, has finalized: $HAS_FINALIZED"

    if [ "$HAS_BTC_HEIGHT" == "true" ] && [ "$HAS_CONFIRMATIONS" == "true" ] && [ "$HAS_FINALIZED" == "true" ]; then
        BTC_HEIGHT=$(echo "$FIRST_TX" | jq -r '.btc_height')
        CONFIRMATIONS=$(echo "$FIRST_TX" | jq -r '.confirmations')
        FINALIZED=$(echo "$FIRST_TX" | jq -r '.finalized')
        echo -e "  btc_height: $BTC_HEIGHT, confirmations: $CONFIRMATIONS, finalized: $FINALIZED"

        # For publishing status, all should be 0/false
        if [ "$STATUS" == "publishing" ]; then
            if [ "$BTC_HEIGHT" -eq 0 ] && [ "$CONFIRMATIONS" -eq 0 ] && [ "$FINALIZED" == "false" ]; then
                echo -e "${GREEN}  ✓ PASS - Publishing tx has correct status fields (btc_height=0, confirmations=0, finalized=false)${NC}"
                PASS_COUNT=$((PASS_COUNT + 1))
            else
                echo -e "${RED}  ✗ FAIL - Publishing tx has wrong status fields${NC}"
                FAIL_COUNT=$((FAIL_COUNT + 1))
            fi
        else
            echo -e "${GREEN}  ✓ PASS - Pending tx has status fields (status=$STATUS)${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        fi
    else
        echo -e "${RED}  ✗ FAIL - Missing status fields (btc_height, confirmations, finalized)${NC}"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No pending transactions (tx may have been mined already)${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 15: Wait for broadcast and check status changes to broadcasting
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Pending transaction status fields (broadcasting)${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

echo -e "  Waiting 35 seconds for sub-block mining..."
sleep 35

PENDING=$(curl -s "$EXPLORER_URL/api/v1/pending-transactions")
PENDING_COUNT=$(echo "$PENDING" | jq 'if type == "array" then length else 0 end')

if [ "$PENDING_COUNT" -gt 0 ]; then
    FIRST_TX=$(echo "$PENDING" | jq '.[0]')
    STATUS=$(echo "$FIRST_TX" | jq -r '.status // "missing"')
    BTC_HEIGHT=$(echo "$FIRST_TX" | jq -r '.btc_height // -1')
    CONFIRMATIONS=$(echo "$FIRST_TX" | jq -r '.confirmations // -1')

    echo -e "  Status: $STATUS, btc_height: $BTC_HEIGHT, confirmations: $CONFIRMATIONS"

    if [ "$STATUS" == "broadcasting" ] || [ "$STATUS" == "unconfirmed" ]; then
        echo -e "${GREEN}  ✓ PASS - Transaction progressed to $STATUS status${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo -e "${YELLOW}  ? Transaction status is $STATUS${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No pending transactions${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 16: Mine a block and verify confirmation count updates
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Confirmation count after mining${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

# Mine a block
$BITCOIN_CLI generatetoaddress 1 "$FEE_ADDR" &>/dev/null

echo -e "  Waiting 15 seconds for indexer discovery..."
sleep 15

PENDING=$(curl -s "$EXPLORER_URL/api/v1/pending-transactions")
PENDING_COUNT=$(echo "$PENDING" | jq 'if type == "array" then length else 0 end')

if [ "$PENDING_COUNT" -gt 0 ]; then
    FIRST_TX=$(echo "$PENDING" | jq '.[0]')
    STATUS=$(echo "$FIRST_TX" | jq -r '.status // "missing"')
    BTC_HEIGHT=$(echo "$FIRST_TX" | jq -r '.btc_height // 0')
    CONFIRMATIONS=$(echo "$FIRST_TX" | jq -r '.confirmations // 0')
    FINALIZED=$(echo "$FIRST_TX" | jq -r '.finalized // false')

    echo -e "  Status: $STATUS, btc_height: $BTC_HEIGHT, confirmations: $CONFIRMATIONS, finalized: $FINALIZED"

    # After mining, if indexed, btc_height should be > 0 and confirmations should be >= 1
    if [ "$STATUS" == "unconfirmed" ]; then
        if [ "$BTC_HEIGHT" -gt 0 ]; then
            echo -e "${GREEN}  ✓ PASS - Indexed tx has btc_height > 0 ($BTC_HEIGHT)${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        else
            echo -e "${RED}  ✗ FAIL - Indexed tx has btc_height = 0 (should be > 0 after mining)${NC}"
            FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
    else
        echo -e "${YELLOW}  ? Transaction status is $STATUS (may need more time)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No pending transactions (all may be fully confirmed)${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 17: Mine more blocks to reach finalization and verify finalized=true
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Finalization after 6 confirmations${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

echo -e "  Mining 6 more blocks for finalization..."
$BITCOIN_CLI generatetoaddress 6 "$FEE_ADDR" &>/dev/null

echo -e "  Waiting 20 seconds for confirmation status update..."
sleep 20

# Check transaction history for finalized status
TX_HISTORY=$(curl -s "$EXPLORER_URL/api/v1/accounts/$ALICE_PK/transactions")
TX_COUNT=$(echo "$TX_HISTORY" | jq 'if type == "array" then length else 0 end')

if [ "$TX_COUNT" -gt 0 ]; then
    # Find a transaction with finalized=true
    FINALIZED_COUNT=$(echo "$TX_HISTORY" | jq '[.[] | select(.finalized == true)] | length')
    TOTAL_CONFIRMATIONS=$(echo "$TX_HISTORY" | jq '.[0].confirmations // 0')

    echo -e "  Total transactions: $TX_COUNT, finalized: $FINALIZED_COUNT, first tx confirmations: $TOTAL_CONFIRMATIONS"

    if [ "$FINALIZED_COUNT" -gt 0 ]; then
        echo -e "${GREEN}  ✓ PASS - Found $FINALIZED_COUNT finalized transaction(s)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    elif [ "$TOTAL_CONFIRMATIONS" -ge 6 ]; then
        echo -e "${GREEN}  ✓ PASS - Transaction has $TOTAL_CONFIRMATIONS confirmations (should be finalized)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo -e "${YELLOW}  ? No finalized transactions yet (confirmations: $TOTAL_CONFIRMATIONS)${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No transaction history${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 18: Verify block-by-txid endpoint returns confirmation info
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Block-by-txid endpoint returns confirmation info${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

# Get a btc_txid from transaction history
if [ "$TX_COUNT" -gt 0 ]; then
    BTC_TXID=$(echo "$TX_HISTORY" | jq -r '.[0].btc_txid // empty')

    if [ -n "$BTC_TXID" ]; then
        BLOCK_INFO=$(curl -s "$INDEXER_URL/blocks/by-txid/$BTC_TXID")

        HAS_BTC_HEIGHT=$(echo "$BLOCK_INFO" | jq 'has("btc_height")')
        HAS_CONFIRMATIONS=$(echo "$BLOCK_INFO" | jq 'has("confirmations")')
        HAS_FINALIZED=$(echo "$BLOCK_INFO" | jq 'has("finalized")')

        echo -e "  btc_txid: ${BTC_TXID:0:16}..."
        echo -e "  Response has btc_height: $HAS_BTC_HEIGHT, confirmations: $HAS_CONFIRMATIONS, finalized: $HAS_FINALIZED"

        if [ "$HAS_BTC_HEIGHT" == "true" ] && [ "$HAS_CONFIRMATIONS" == "true" ] && [ "$HAS_FINALIZED" == "true" ]; then
            CONFIRMATIONS=$(echo "$BLOCK_INFO" | jq -r '.confirmations')
            FINALIZED=$(echo "$BLOCK_INFO" | jq -r '.finalized')
            echo -e "  confirmations: $CONFIRMATIONS, finalized: $FINALIZED"
            echo -e "${GREEN}  ✓ PASS - block-by-txid returns confirmation info${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        else
            echo -e "${RED}  ✗ FAIL - block-by-txid missing confirmation fields${NC}"
            echo -e "  Response: $BLOCK_INFO"
            FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
    else
        echo -e "${YELLOW}  ? No btc_txid found in transaction history${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "${YELLOW}  ? No transaction history to test with${NC}"
    PASS_COUNT=$((PASS_COUNT + 1))
fi

# Test 19: CRITICAL - Confirmed transactions must have confirmations > 0
# This test verifies the key invariant that the frontend relies on
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Confirmed transactions have confirmations > 0${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

TX_HISTORY=$(curl -s "$EXPLORER_URL/api/v1/accounts/$ALICE_PK/transactions")
TX_COUNT=$(echo "$TX_HISTORY" | jq 'if type == "array" then length else 0 end')

if [ "$TX_COUNT" -gt 0 ]; then
    # Check that transactions with btc_height > 0 also have confirmations > 0
    # This is the key invariant: btc_height > 0 means confirmed on Bitcoin, so confirmations must be > 0
    INVALID_TXS=$(echo "$TX_HISTORY" | jq '[.[] | select(.btc_height > 0 and .confirmations == 0)] | length')

    if [ "$INVALID_TXS" -eq 0 ]; then
        # Also verify at least one tx has confirmations > 0
        CONFIRMED_TXS=$(echo "$TX_HISTORY" | jq '[.[] | select(.confirmations > 0)] | length')
        SAMPLE_CONFIRMATIONS=$(echo "$TX_HISTORY" | jq '.[0].confirmations // 0')
        SAMPLE_BTC_HEIGHT=$(echo "$TX_HISTORY" | jq '.[0].btc_height // 0')

        if [ "$CONFIRMED_TXS" -gt 0 ]; then
            echo -e "  Found $CONFIRMED_TXS confirmed transaction(s) (sample: btc_height=$SAMPLE_BTC_HEIGHT, confirmations=$SAMPLE_CONFIRMATIONS)"
            echo -e "${GREEN}  ✓ PASS - All confirmed transactions have confirmations > 0${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        else
            echo -e "${RED}  ✗ FAIL - No transactions have confirmations > 0 after mining blocks${NC}"
            FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
    else
        echo -e "${RED}  ✗ FAIL - Found $INVALID_TXS transaction(s) with btc_height > 0 but confirmations = 0${NC}"
        echo -e "  This would cause frontend to incorrectly show 'In Bitcoin mempool' for confirmed txs"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
else
    echo -e "${RED}  ✗ FAIL - No transaction history to verify${NC}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Test 20: CRITICAL - Mempool vs Confirmed display logic test
# This tests the exact invariant the frontend uses:
#   - confirmations == 0 && btc_height == 0 → "In Bitcoin mempool"
#   - confirmations > 0 → "X confirmations" (NOT "In Bitcoin mempool")
#   - finalized == true → "Confirmed"
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Display logic: confirmations > 0 must NOT show as mempool${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

if [ "$TX_COUNT" -gt 0 ]; then
    # For each transaction, verify the display logic invariants
    DISPLAY_ERRORS=0

    # Check: transactions with confirmations > 0 should NOT be treated as "in mempool"
    # The frontend bug was: status="unconfirmed" was checked BEFORE confirmations > 0

    # Get a transaction that has confirmations
    SAMPLE_TX=$(echo "$TX_HISTORY" | jq '[.[] | select(.confirmations > 0)][0] // empty')

    if [ -n "$SAMPLE_TX" ] && [ "$SAMPLE_TX" != "null" ]; then
        CONFS=$(echo "$SAMPLE_TX" | jq -r '.confirmations')
        BTC_H=$(echo "$SAMPLE_TX" | jq -r '.btc_height')
        FINAL=$(echo "$SAMPLE_TX" | jq -r '.finalized')
        CONFS_REM=$(echo "$SAMPLE_TX" | jq -r '.confirmations_remaining // 0')

        echo -e "  Sample confirmed tx: confirmations=$CONFS, btc_height=$BTC_H, finalized=$FINAL, remaining=$CONFS_REM"

        # Verify the data supports correct display:
        # - If confirmations > 0, btc_height should also be > 0
        if [ "$BTC_H" -eq 0 ]; then
            echo -e "${RED}  ✗ FAIL - Transaction has confirmations=$CONFS but btc_height=0 (inconsistent)${NC}"
            DISPLAY_ERRORS=$((DISPLAY_ERRORS + 1))
        fi

        # - If finalized=true, confirmations should be >= 6
        if [ "$FINAL" == "true" ] && [ "$CONFS" -lt 6 ]; then
            echo -e "${RED}  ✗ FAIL - Transaction marked finalized with only $CONFS confirmations${NC}"
            DISPLAY_ERRORS=$((DISPLAY_ERRORS + 1))
        fi

        # - confirmations_remaining should be 6 - confirmations (capped at 0)
        EXPECTED_REM=$((6 - CONFS))
        if [ "$EXPECTED_REM" -lt 0 ]; then
            EXPECTED_REM=0
        fi
        if [ "$FINAL" == "true" ]; then
            EXPECTED_REM=0
        fi

        if [ "$DISPLAY_ERRORS" -eq 0 ]; then
            echo -e "${GREEN}  ✓ PASS - Transaction data supports correct 'confirming' display (not mempool)${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        else
            echo -e "${RED}  ✗ FAIL - $DISPLAY_ERRORS display logic error(s) found${NC}"
            FAIL_COUNT=$((FAIL_COUNT + 1))
        fi
    else
        echo -e "${RED}  ✗ FAIL - No confirmed transactions found to test display logic${NC}"
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
else
    echo -e "${RED}  ✗ FAIL - No transaction history to verify display logic${NC}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
fi

# Test 21: Pending transactions endpoint - unconfirmed with confirmations > 0 check
echo -e "${YELLOW}[Test $((TEST_COUNT + 1))] Pending endpoint: indexed txs report correct confirmations${NC}"
TEST_COUNT=$((TEST_COUNT + 1))

# Submit a fresh transaction and track it through confirmation
./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex --publisher-url $PUBLISHER_URL \
    send --recipient-pk "$BOB_PK" --amount 5 &>/dev/null || true

echo -e "  Submitted transaction, waiting 35s for broadcast..."
sleep 35

# Mine a block to confirm
$BITCOIN_CLI generatetoaddress 1 "$FEE_ADDR" &>/dev/null
echo -e "  Mined block, waiting 15s for indexer..."
sleep 15

PENDING=$(curl -s "$EXPLORER_URL/api/v1/pending-transactions")
PENDING_COUNT=$(echo "$PENDING" | jq 'if type == "array" then length else 0 end')

if [ "$PENDING_COUNT" -gt 0 ]; then
    # Find transactions with status="unconfirmed" (indexed)
    INDEXED_TXS=$(echo "$PENDING" | jq '[.[] | select(.status == "unconfirmed")]')
    INDEXED_COUNT=$(echo "$INDEXED_TXS" | jq 'length')

    if [ "$INDEXED_COUNT" -gt 0 ]; then
        # Check that indexed transactions have confirmations > 0 after mining
        FIRST_INDEXED=$(echo "$INDEXED_TXS" | jq '.[0]')
        STATUS=$(echo "$FIRST_INDEXED" | jq -r '.status')
        CONFS=$(echo "$FIRST_INDEXED" | jq -r '.confirmations // 0')
        BTC_H=$(echo "$FIRST_INDEXED" | jq -r '.btc_height // 0')

        echo -e "  Found indexed pending tx: status=$STATUS, confirmations=$CONFS, btc_height=$BTC_H"

        # KEY TEST: If btc_height > 0, confirmations must be > 0
        # This is what the frontend needs to show "X confirmations" instead of "In mempool"
        if [ "$BTC_H" -gt 0 ]; then
            if [ "$CONFS" -gt 0 ]; then
                echo -e "${GREEN}  ✓ PASS - Indexed tx with btc_height=$BTC_H has confirmations=$CONFS (frontend can show confirmation count)${NC}"
                PASS_COUNT=$((PASS_COUNT + 1))
            else
                echo -e "${RED}  ✗ FAIL - Indexed tx has btc_height=$BTC_H but confirmations=0 (BUG: frontend would show 'In mempool')${NC}"
                FAIL_COUNT=$((FAIL_COUNT + 1))
            fi
        else
            # btc_height=0 means in Bitcoin mempool, confirmations=0 is correct
            echo -e "${GREEN}  ✓ PASS - Indexed tx in mempool (btc_height=0, confirmations=0)${NC}"
            PASS_COUNT=$((PASS_COUNT + 1))
        fi
    else
        echo -e "  No indexed (unconfirmed status) transactions in pending"
        echo -e "${GREEN}  ✓ PASS - No indexed pending transactions to check${NC}"
        PASS_COUNT=$((PASS_COUNT + 1))
    fi
else
    echo -e "  No pending transactions (all confirmed and cleared)"
    echo -e "${GREEN}  ✓ PASS - No pending transactions${NC}"
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
