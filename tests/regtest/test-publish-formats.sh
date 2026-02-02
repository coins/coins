#!/usr/bin/env bash
# Integration test for publishing formats (OP_RETURN, Taproot annex)

set -e

# Increase file descriptor limit
ulimit -n 2048 2>/dev/null || true

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Publishing Formats E2E Test${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Configuration
RPC_USER="user"
RPC_PASS="password"
RPC_PORT="18443"
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
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
    pkill -f "coins-publisher" 2>/dev/null || true
    pkill -f "coins-indexer" 2>/dev/null || true
    rm -rf /tmp/format_test 2>/dev/null || true
}

trap cleanup EXIT

# Helper function to test a specific format
test_format() {
    local format=$1
    local test_name=$2
    local publisher_port=$3
    local indexer_port=$4
    local test_dir="/tmp/format_test/${format}"
    local log_dir="${test_dir}/logs"

    mkdir -p "$test_dir" "$log_dir"

    echo -e "\n${BLUE}=== Testing: $test_name ===${NC}"

    # Create indexer config
    cat > "${test_dir}/indexer.toml" <<EOF
subchain = "${SUBCHAIN_FILE}"
wallet_name = "coins-indexer"

[database]
state_db = "${test_dir}/state.db"
indexer_db = "${test_dir}/indexer.db"

[bitcoin]
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
network = "regtest"

[server]
api_port = ${indexer_port}
host = "0.0.0.0"

[genesis]
genesis_pk = "${GENESIS_PK}"
genesis_balance = 1000000000000
EOF

    # Create publisher config with specific format
    cat > "${test_dir}/publisher.toml" <<EOF
rpc_url = "http://localhost:${RPC_PORT}"
rpc_user = "${RPC_USER}"
rpc_pass = "${RPC_PASS}"
rpc_wallet = "coins-publisher"
subchain = "${SUBCHAIN_FILE}"
keyfile = "${KEYS_DIR}/publisher_sk.hex"
interval = 30
network = "regtest"
indexer_url = "http://localhost:${indexer_port}"
api_port = ${publisher_port}
publish_format = "${format}"
fee_rate_sat_per_vb = 4
EOF

    # Start publisher briefly to get fee address
    echo -e "  ${BLUE}→ Getting publisher address...${NC}"
    ./target/release/coins-publisher --config "${test_dir}/publisher.toml" > "${log_dir}/publisher_temp.log" 2>&1 &
    local TEMP_PID=$!
    sleep 5

    local FEE_ADDR=$(grep "Publisher initialized" "${log_dir}/publisher_temp.log" | grep -oE 'bcrt1[a-z0-9]+' | head -1)
    kill $TEMP_PID 2>/dev/null || true
    sleep 2

    if [ -z "$FEE_ADDR" ]; then
        echo -e "${RED}✗ Could not determine publisher address${NC}"
        cat "${log_dir}/publisher_temp.log"
        return 1
    fi
    echo -e "  Publisher address: $FEE_ADDR"

    # Fund publisher
    echo -e "  ${BLUE}→ Funding publisher...${NC}"
    bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
        generatetoaddress 101 "$FEE_ADDR" &>/dev/null

    # Start indexer
    echo -n "  Starting indexer"
    ./target/release/coins-indexer --config "${test_dir}/indexer.toml" > "${log_dir}/indexer.log" 2>&1 &
    local INDEXER_PID=$!
    for i in {1..30}; do
        if curl -s "http://localhost:${indexer_port}/health" &>/dev/null; then
            echo ""
            break
        fi
        echo -n "."
        sleep 1
    done

    if ! curl -s "http://localhost:${indexer_port}/health" &>/dev/null; then
        echo -e "\n${RED}✗ Indexer failed to start${NC}"
        tail -20 "${log_dir}/indexer.log"
        return 1
    fi
    echo -e "  ${GREEN}✓ Indexer running (port ${indexer_port})${NC}"

    # Start publisher
    echo -n "  Starting publisher"
    ./target/release/coins-publisher --config "${test_dir}/publisher.toml" > "${log_dir}/publisher.log" 2>&1 &
    local PUBLISHER_PID=$!
    for i in {1..30}; do
        if curl -s "http://localhost:${publisher_port}/health" &>/dev/null; then
            echo ""
            break
        fi
        echo -n "."
        sleep 1
    done

    if ! curl -s "http://localhost:${publisher_port}/health" &>/dev/null; then
        echo -e "\n${RED}✗ Publisher failed to start${NC}"
        tail -20 "${log_dir}/publisher.log"
        kill $INDEXER_PID 2>/dev/null || true
        return 1
    fi
    echo -e "  ${GREEN}✓ Publisher running (port ${publisher_port}, format: ${format})${NC}"

    # Submit test transaction
    echo -e "  ${BLUE}→ Submitting transaction (Genesis -> Alice)...${NC}"
    ./target/release/coins-client --keyfile /tmp/format_test/genesis_sk.hex \
        --publisher-url "http://localhost:${publisher_port}" \
        send --recipient-pk "$ALICE_PK" --amount 100 2>&1 | grep -E "success|fail|error" || echo "    Transaction submitted"

    # Wait for broadcast
    echo -n "  Waiting for broadcast"
    for i in {1..60}; do
        if grep -q "Package broadcasted successfully" "${log_dir}/publisher.log" 2>/dev/null; then
            echo ""
            break
        fi
        echo -n "."
        sleep 1
    done

    if ! grep -q "Package broadcasted successfully" "${log_dir}/publisher.log" 2>/dev/null; then
        echo -e "\n${RED}✗ Package was not broadcast within 60 seconds${NC}"
        tail -50 "${log_dir}/publisher.log"
        kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
        return 1
    fi
    echo -e "  ${GREEN}✓ Package broadcast detected${NC}"

    # Extract TXIDs
    local PACKAGE_LINE=$(grep "Package broadcasted successfully" "${log_dir}/publisher.log" | tail -1)
    local DATA_TXID=$(echo "$PACKAGE_LINE" | grep -o '[a-f0-9]\{64\}' | tail -1)

    if [ -z "$DATA_TXID" ]; then
        echo -e "${RED}✗ Could not extract data TXID${NC}"
        kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
        return 1
    fi
    echo -e "  Data TXID: $DATA_TXID"

    # Mine block to confirm
    bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
        generatetoaddress 1 "$FEE_ADDR" &>/dev/null
    sleep 2

    # Verify transaction is confirmed
    local TX_CONFIRMATIONS=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
        getrawtransaction "$DATA_TXID" true | jq -r '.confirmations // 0')

    if [ "$TX_CONFIRMATIONS" -lt 1 ]; then
        echo -e "${RED}✗ Transaction not confirmed (confirmations: $TX_CONFIRMATIONS)${NC}"
        kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
        return 1
    fi

    # Verify format detection via locktime
    local TX_RAW=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
        getrawtransaction "$DATA_TXID" true)
    local LOCKTIME=$(echo "$TX_RAW" | jq -r '.locktime')
    local LOCKTIME_BIT0=$((LOCKTIME & 1))

    echo -e "  Locktime: $LOCKTIME (bit 0: $LOCKTIME_BIT0)"

    # Verify locktime encoding matches format
    case "$format" in
        "op_return")
            if [ "$LOCKTIME_BIT0" -ne 0 ]; then
                echo -e "${RED}✗ OP_RETURN format should have locktime bit 0 = 0${NC}"
                kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
                return 1
            fi
            ;;
        "taproot_annex")
            if [ "$LOCKTIME_BIT0" -ne 1 ]; then
                echo -e "${RED}✗ Taproot annex format should have locktime bit 0 = 1${NC}"
                kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
                return 1
            fi
            ;;
    esac

    # Save data TXID for IBD test
    echo "$DATA_TXID" >> /tmp/format_test/data_txids.txt

    # Stop services
    kill $PUBLISHER_PID $INDEXER_PID 2>/dev/null || true
    sleep 2

    echo -e "${GREEN}✓ $test_name test passed${NC}"
    return 0
}

# Stop any running processes and reset environment
echo -e "${YELLOW}[1/7] Resetting test environment...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
pkill -9 coins-indexer 2>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT stop 2>/dev/null || true
sleep 3

rm -rf "${NETWORK_DIR}" 2>/dev/null || true
rm -rf /tmp/format_test 2>/dev/null || true
mkdir -p "${SUBCHAIN_DIR}" "${KEYS_DIR}" "${NETWORK_DIR}/logs"
mkdir -p /tmp/format_test

# Start bitcoind
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

echo -e "${YELLOW}[2/7] Creating wallets...${NC}"
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    createwallet "coins-publisher" true false "" false true &>/dev/null || true
bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    createwallet "coins-indexer" true false "" false true &>/dev/null || true
echo -e "${GREEN}✓ Wallets created${NC}\n"

echo -e "${YELLOW}[3/7] Building project...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher --bin coins-indexer --bin coins-client \
    --example get_pk &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}\n"

echo -e "${YELLOW}[4/7] Generating subchain...${NC}"
cat > /tmp/subchain_config.toml <<EOF
count = 100
network = "regtest"
output = "${SUBCHAIN_FILE}"
EOF

SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_config.toml 2>&1 | \
    grep "Generated one-time address:" | awk '{print $4}'
)

if [ -z "$SUBCHAIN_ADDR" ]; then
    echo -e "${RED}✗ Failed to generate subchain address${NC}"
    exit 1
fi
echo -e "  Subchain address: $SUBCHAIN_ADDR"

bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    generatetoaddress 101 "$SUBCHAIN_ADDR" &>/dev/null

UTXO_INFO=$(bitcoin-cli -regtest -rpcuser=$RPC_USER -rpcpassword=$RPC_PASS -rpcport=$RPC_PORT \
    scantxoutset start "[\"addr($SUBCHAIN_ADDR)\"]" | \
    jq -r '.unspents[0] | "\(.txid):\(.vout) \((.amount*100000000)|floor)"')
UTXO_OUTPOINT=$(echo "$UTXO_INFO" | awk '{print $1}')
UTXO_VALUE=$(echo "$UTXO_INFO" | awk '{print $2}')

printf "%s\n%s\n" "$UTXO_OUTPOINT" "$UTXO_VALUE" | \
    ./target/release/subchain-setup --config /tmp/subchain_config.toml &>/dev/null

if [ ! -f "$SUBCHAIN_FILE" ]; then
    echo -e "${RED}✗ Subchain file not created${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Subchain generated (100 transactions)${NC}\n"

echo -e "${YELLOW}[5/7] Creating test keypairs...${NC}"
echo "$GENESIS_SK" > /tmp/format_test/genesis_sk.hex
./target/release/coins-client --keyfile /tmp/format_test/alice_sk.hex init &>/dev/null
ALICE_PK=$(./target/release/examples/get_pk /tmp/format_test/alice_sk.hex)
echo -e "  Genesis PK: ${GENESIS_PK}"
echo -e "  Alice PK: ${ALICE_PK}"
echo -e "${GREEN}✓ Keypairs ready${NC}\n"

# Test OP_RETURN format
echo -e "${YELLOW}[6/7] Testing OP_RETURN format...${NC}"
test_format "op_return" "OP_RETURN" 8080 8083 || exit 1

# Test Taproot annex format
echo -e "${YELLOW}[7/7] Testing Taproot annex format...${NC}"
test_format "taproot_annex" "Taproot Annex" 8081 8084 || exit 1

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   All Publishing Format Tests Passed!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "Summary:"
echo -e "  ✓ OP_RETURN publishing works (locktime bit 0 = 0)"
echo -e "  ✓ Taproot annex publishing works (locktime bit 0 = 1)"
echo -e "  ✓ Format detection from locktime encoding verified"
echo ""
