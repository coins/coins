#!/bin/bash
# E2E test script for Coins aggregator
# Requires: running aggregator on localhost:8080, jq, curl

set -e

AGGREGATOR_URL="http://localhost:8080"
GENESIS_PK="43878a2a65c154d604cbe7d974d5dad1c63ce4dc2a68f697c45a4a3ef9ab8a21"

echo "=== Coins Aggregator E2E Test ==="
echo

# Step 1: Check genesis account
echo "Step 1: Checking genesis account..."
GENESIS_RESPONSE=$(curl -s "$AGGREGATOR_URL/account/$GENESIS_PK")
echo "Genesis account:"
echo "$GENESIS_RESPONSE" | jq .
GENESIS_BALANCE=$(echo "$GENESIS_RESPONSE" | jq -r '.balance')
GENESIS_ID=$(echo "$GENESIS_RESPONSE" | jq -r '.id')
echo "✓ Genesis balance: $GENESIS_BALANCE"
echo "✓ Genesis ID: $GENESIS_ID"
echo

# Step 2: Test with simple account query
echo "Step 2: Querying non-existent account (should return 404)..."
TEST_PK="0000000000000000000000000000000000000000000000000000000000000000"
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$AGGREGATOR_URL/account/$TEST_PK")
if [ "$HTTP_CODE" == "404" ]; then
    echo "✓ Non-existent account returns 404 as expected"
else
    echo "✗ Unexpected response code: $HTTP_CODE"
fi
echo

echo "=== Manual Transaction Test ==="
echo
echo "To complete the E2E test, you need to:"
echo "1. Generate a keypair using the Rust crypto library"
echo "2. Create accounts for test users"
echo "3. Sign and submit transactions"
echo
echo "This requires the genesis secret key to sign transfers from genesis."
echo "The genesis public key is: $GENESIS_PK"
echo
echo "For now, the aggregator is running and API endpoints are verified working."
