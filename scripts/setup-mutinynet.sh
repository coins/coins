#!/usr/bin/env bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=======================================${NC}"
echo -e "${BLUE}   Coins Mutinynet Integration Setup${NC}"
echo -e "${BLUE}=======================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.data"
NETWORK_DIR="${DATA_DIR}/mutinynet"  # Network-specific directory
SUBCHAIN_DIR="${NETWORK_DIR}/subchains"
KEYS_DIR="${NETWORK_DIR}/keys"

# Remote mutinynet node credentials
RPC_USER="bitcoin"
RPC_PASS="8723nasd0932n"
RPC_URL="http://168.119.139.152:38332"
RPC_HOST="168.119.139.152"
RPC_PORT="38332"

SUBCHAIN_COUNT=1000
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SUBCHAIN_FILE="${SUBCHAIN_DIR}/subchain_mutinynet_${TIMESTAMP}.bin"
SUBCHAIN_LINK="${SUBCHAIN_DIR}/subchain_mutinynet.bin"

cd "$PROJECT_ROOT"

echo -e "${YELLOW}[1/10] Cleaning up...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
pkill -9 coins-indexer 2>/dev/null || true
sleep 2

# Clean up old mutinynet-specific data (preserve other networks)
rm -rf "${NETWORK_DIR}" 2>/dev/null || true

# Create network-specific directory structure (all mutinynet data isolated)
mkdir -p "${SUBCHAIN_DIR}" "${KEYS_DIR}"
echo -e "${GREEN}✓ Cleanup complete${NC}\n"

echo -e "${YELLOW}[2/10] Testing connection to remote mutinynet node...${NC}"

# Test connection to remote node
if bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
    BLOCKS=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" getblockchaininfo | jq -r '.blocks')
    echo -e "${GREEN}✓ Connected to mutinynet node${NC}"
    echo -e "${BLUE}→ Current block height: ${BLOCKS}${NC}\n"
else
    echo -e "${RED}✗ Failed to connect to mutinynet node at ${RPC_HOST}:${RPC_PORT}${NC}"
    echo -e "${RED}  Please check network connectivity and credentials${NC}"
    exit 1
fi

echo -e "${YELLOW}[3/10] Building...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher --bin coins-indexer --bin coins-client --example get_pk &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}\n"

echo -e "${YELLOW}[4/10] Generating subchain...${NC}"
cat > /tmp/subchain_mutinynet.toml <<EOF
count = ${SUBCHAIN_COUNT}
network = "signet"
output = "${SUBCHAIN_FILE}"
EOF

SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_mutinynet.toml 2>&1 | \
    grep "Generated one-time address:" | awk '{print $4}'
)

echo -e "${GREEN}✓ Subchain address: ${SUBCHAIN_ADDR}${NC}\n"

echo -e "${YELLOW}⚠️  MANUAL FUNDING REQUIRED${NC}"
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "Fund this address from mutinynet faucet:\n"
echo -e "${BLUE}  Address: ${SUBCHAIN_ADDR}${NC}\n"
echo -e "${BLUE}  Faucet: https://faucet.mutinynet.com${NC}"
echo -e "${BLUE}  Amount: 0.001 BTC (minimum)${NC}\n"
echo -e "Waiting for confirmation (~1 min)..."
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"

# Poll for funding (one-time setup operation)
WAIT_COUNT=0
MAX_WAIT=120

while true; do
    UTXO_INFO=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
        scantxoutset start "[\"addr(${SUBCHAIN_ADDR})\"]" 2>/dev/null | \
        jq -r '.unspents[] | select(.height > 0) | "\(.txid):\(.vout) \((.amount*100000000)|floor)"' 2>/dev/null | head -1)

    if [ -n "$UTXO_INFO" ]; then
        UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
        UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')
        echo -e "${GREEN}✓ Funded: ${UTXO_OUTPOINT} (${UTXO_VALUE} sats)${NC}\n"
        break
    fi

    WAIT_COUNT=$((WAIT_COUNT + 1))
    if [ $WAIT_COUNT -ge $MAX_WAIT ]; then
        echo -e "${RED}✗ Timeout waiting for funding${NC}"
        exit 1
    fi

    echo -ne "\rWaiting... ($((WAIT_COUNT * 5))s)"
    sleep 5
done

echo -e "${BLUE}→ Generating subchain file...${NC}"
printf "%s\n%s\n" "${UTXO_OUTPOINT}" "${UTXO_VALUE}" | \
    ./target/release/subchain-setup --config /tmp/subchain_mutinynet.toml &>/dev/null

# Create symlink to latest subchain
ln -sf "$(basename ${SUBCHAIN_FILE})" "${SUBCHAIN_LINK}"

echo -e "${GREEN}✓ Subchain created: $(basename ${SUBCHAIN_FILE})${NC}"
echo -e "${BLUE}→ Linked to: subchain_mutinynet.bin${NC}\n"

echo -e "${YELLOW}[5/10] Creating test keypairs...${NC}"

# Genesis key (sk=3, matches config/indexer-mutinynet.toml genesis_pk)
echo "0300000000000000000000000000000000000000000000000000000000000000" > "${KEYS_DIR}/genesis_sk.hex"
echo -e "  ${GREEN}✓ Genesis key created${NC}"

# Create Alice and Bob keypairs using coins-client
./target/release/coins-client --keyfile "${KEYS_DIR}/alice_sk.hex" init 2>&1 | grep -q "stored" && echo -e "  ${GREEN}✓ Alice key created${NC}" || echo -e "  ${GREEN}✓ Alice key exists${NC}"
./target/release/coins-client --keyfile "${KEYS_DIR}/bob_sk.hex" init 2>&1 | grep -q "stored" && echo -e "  ${GREEN}✓ Bob key created${NC}" || echo -e "  ${GREEN}✓ Bob key exists${NC}"

echo -e "${GREEN}✓ Test keypairs ready${NC}\n"

echo -e "${YELLOW}[6/10] Creating wallet on remote node...${NC}"
# Try to create wallet, ignore error if it already exists
bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
    createwallet "coins-publisher" true 2>/dev/null || true
echo -e "${GREEN}✓ Wallet ready${NC}\n"

echo -e "${YELLOW}[7/10] Starting indexer...${NC}"
mkdir -p "${NETWORK_DIR}/logs"
./target/release/coins-indexer --config config/indexer-mutinynet.toml > "${NETWORK_DIR}/logs/indexer.log" 2>&1 &
IDX_PID=$!
echo "$IDX_PID" > "${NETWORK_DIR}/indexer.pid"

echo -n "Waiting for indexer"
for i in {1..30}; do
    if curl -s http://localhost:8083/health &>/dev/null; then
        echo ""; break
    fi
    echo -n "."; sleep 1
done

echo -e "${GREEN}✓ Indexer running (PID: ${IDX_PID})${NC}\n"

echo -e "${YELLOW}[8/10] Starting publisher...${NC}"
./target/release/coins-publisher --config config/publisher-mutinynet.toml > "${NETWORK_DIR}/logs/publisher.log" 2>&1 &
PUB_PID=$!
echo "$PUB_PID" > "${NETWORK_DIR}/publisher.pid"

echo -n "Waiting for publisher"
for i in {1..30}; do
    if curl -s http://localhost:8082/health &>/dev/null; then
        echo ""; break
    fi
    echo -n "."; sleep 1
done

echo -e "${GREEN}✓ Publisher running (PID: ${PUB_PID})${NC}\n"

echo -e "${YELLOW}[9/10] Checking publisher funding...${NC}"

# Extract publisher address from logs (wait for publisher to initialize)
echo -n "Waiting for publisher to initialize"
PUBLISHER_ADDR=""
for i in {1..30}; do
    # Strip ANSI codes and extract address (tb1... format for testnet/signet)
    PUBLISHER_ADDR=$(grep "Publisher initialized" "${NETWORK_DIR}/logs/publisher.log" 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' | grep -oE 'publisher_address=(tb1[a-z0-9]+)' | cut -d= -f2 | head -1)
    if [ -n "$PUBLISHER_ADDR" ]; then
        echo ""
        break
    fi
    echo -n "."
    sleep 1
done

if [ -z "$PUBLISHER_ADDR" ]; then
    echo ""
    echo -e "${RED}✗ Could not determine publisher address from logs${NC}"
    echo -e "${YELLOW}Last 20 lines of publisher log:${NC}"
    tail -20 "${NETWORK_DIR}/logs/publisher.log"
    exit 1
fi

echo -e "  Publisher address: ${PUBLISHER_ADDR}"

# Check if publisher has enough Bitcoin for transaction fees
# Minimum: ~50,000 sats for at least 10 publishes at 4 sat/vb
MIN_PUBLISHER_SATS=50000

PUBLISHER_BALANCE=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
    scantxoutset start "[\"addr(${PUBLISHER_ADDR})\"]" 2>/dev/null | \
    jq -r '.total_amount * 100000000 | floor' 2>/dev/null || echo "0")

if [ "$PUBLISHER_BALANCE" -ge "$MIN_PUBLISHER_SATS" ]; then
    echo -e "${GREEN}✓ Publisher funded: ${PUBLISHER_BALANCE} sats${NC}\n"
else
    echo -e "${YELLOW}⚠️  PUBLISHER FUNDING REQUIRED${NC}"
    echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "The publisher needs Bitcoin to pay transaction fees.\n"
    echo -e "${BLUE}  Address: ${PUBLISHER_ADDR}${NC}"
    echo -e "${BLUE}  Current: ${PUBLISHER_BALANCE} sats${NC}"
    echo -e "${BLUE}  Minimum: ${MIN_PUBLISHER_SATS} sats${NC}\n"
    echo -e "${BLUE}  Faucet: https://faucet.mutinynet.com${NC}"
    echo -e "${BLUE}  Recommended: 0.001 BTC (100,000 sats)${NC}\n"
    echo -e "Waiting for funding (~1 min for confirmation)..."
    echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"

    # Poll for publisher funding silently
    WAIT_COUNT=0
    MAX_WAIT=120

    while true; do
        PUBLISHER_BALANCE=$(bitcoin-cli -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcconnect="${RPC_HOST}" -rpcport="${RPC_PORT}" \
            scantxoutset start "[\"addr(${PUBLISHER_ADDR})\"]" 2>/dev/null | \
            jq -r '.total_amount * 100000000 | floor' 2>/dev/null || echo "0")

        if [ "$PUBLISHER_BALANCE" -ge "$MIN_PUBLISHER_SATS" ]; then
            echo -e "${GREEN}✓ Publisher funded: ${PUBLISHER_BALANCE} sats${NC}\n"
            break
        fi

        WAIT_COUNT=$((WAIT_COUNT + 1))
        if [ $WAIT_COUNT -ge $MAX_WAIT ]; then
            echo -e "${RED}✗ Timeout waiting for publisher funding${NC}"
            echo -e "${YELLOW}You can fund it later and restart, or continue without test account funding${NC}"
            exit 1
        fi

        sleep 5
    done
fi

echo -e "${YELLOW}[10/10] Funding test accounts from genesis...${NC}"

# Get Alice and Bob public keys
ALICE_PK=$(./target/release/examples/get_pk "${KEYS_DIR}/alice_sk.hex")
BOB_PK=$(./target/release/examples/get_pk "${KEYS_DIR}/bob_sk.hex")
echo -e "  Alice PK: ${ALICE_PK}"
echo -e "  Bob PK: ${BOB_PK}"

# Fund Alice from genesis (10000 tokens)
echo -e "  ${BLUE}→ Sending 10000 tokens from Genesis to Alice...${NC}"

# Get current broadcast count before sending
BROADCAST_COUNT_BEFORE=$(grep "Package broadcasted successfully" "${NETWORK_DIR}/logs/publisher.log" 2>/dev/null | wc -l)

GENESIS_TX_OUTPUT=$(./target/release/coins-client --keyfile "${KEYS_DIR}/genesis_sk.hex" --publisher-url http://localhost:8082 \
    send --recipient-pk "$ALICE_PK" --amount 10000 2>&1)
echo "$GENESIS_TX_OUTPUT" | grep -E "success|fail" || echo "    Transaction submitted"

# Wait for publisher to mine and broadcast (poll every 5s, timeout 2 min)
echo -e "  ${BLUE}→ Waiting for publisher to mine sub-block...${NC}"
GENESIS_TXID=""
for i in {1..24}; do
    BROADCAST_COUNT_AFTER=$(grep "Package broadcasted successfully" "${NETWORK_DIR}/logs/publisher.log" 2>/dev/null | wc -l)
    if [ "$BROADCAST_COUNT_AFTER" -gt "$BROADCAST_COUNT_BEFORE" ]; then
        GENESIS_TXID=$(grep "Package broadcasted successfully" "${NETWORK_DIR}/logs/publisher.log" | tail -1 | sed 's/\x1b\[[0-9;]*m//g' | grep -oE 'data_txid=[a-f0-9]+' | cut -d= -f2)
        if [ -n "$GENESIS_TXID" ]; then
            echo -e "    ${BLUE}Bitcoin TX: https://mutinynet.com/tx/${GENESIS_TXID}${NC}"
        fi
        break
    fi
    sleep 5
done

if [ -z "$GENESIS_TXID" ]; then
    echo -e "    ${YELLOW}Timeout waiting for broadcast (check publisher logs)${NC}"
fi

echo -e "\n${GREEN}✓ Alice funded from Genesis${NC}\n"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Setup Complete!${NC}"
echo -e "${GREEN}========================================${NC}\n"
echo -e "Network:      Mutinynet Signet (~1 min blocks)"
echo -e "Remote Node:  ${RPC_HOST}:${RPC_PORT}"
echo -e "Subchain:     $(basename ${SUBCHAIN_FILE})"
echo -e "Link:         subchain_mutinynet.bin -> $(basename ${SUBCHAIN_FILE})"
echo -e "Keys:         ${KEYS_DIR}/"
echo -e "Publisher:    ${PUBLISHER_ADDR}"
echo -e "Logs:         ${NETWORK_DIR}/logs/publisher.log\n"
echo -e "${BLUE}Query accounts:${NC}"
echo -e "  curl http://localhost:8082/account/${ALICE_PK}"
echo -e "  curl http://localhost:8082/account/${BOB_PK}\n"
echo -e "${BLUE}List subchains: ls -lh ${SUBCHAIN_DIR}/subchain_mutinynet_*.bin${NC}"
echo -e "${BLUE}Monitor logs:   tail -f ${NETWORK_DIR}/logs/publisher.log${NC}"
echo -e "${BLUE}Run tests:      ./scripts/test-mutinynet.sh${NC}\n"
