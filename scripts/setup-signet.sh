#!/bin/bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=======================================${NC}"
echo -e "${BLUE}   Coins Signet Integration Setup${NC}"
echo -e "${BLUE}=======================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.data"
NETWORK_DIR="${DATA_DIR}/signet"  # Network-specific directory
BITCOIN_DATADIR="${NETWORK_DIR}/bitcoin"  # Bitcoin signet data (local, isolated)
SUBCHAIN_DIR="${NETWORK_DIR}/subchains"
KEYS_DIR="${NETWORK_DIR}/keys"

RPC_USER="user"
RPC_PASS="password"
RPC_PORT="38332"

SUBCHAIN_COUNT=1000
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
SUBCHAIN_FILE="${SUBCHAIN_DIR}/subchain_signet_${TIMESTAMP}.bin"
SUBCHAIN_LINK="${SUBCHAIN_DIR}/subchain_signet.bin"

cd "$PROJECT_ROOT"

echo -e "${YELLOW}[1/9] Cleaning up...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
sleep 2

# Clean up old signet-specific data (preserve other networks)
rm -rf "${NETWORK_DIR}" 2>/dev/null || true
rm -rf indexer.db state.db explorer-tx-index.db 2>/dev/null || true  # Legacy paths

# Create network-specific directory structure (all signet data isolated)
mkdir -p "${SUBCHAIN_DIR}" "${KEYS_DIR}"
echo -e "${GREEN}✓ Cleanup complete${NC}\n"

echo -e "${YELLOW}[2/9] Starting Bitcoin Core (signet)...${NC}"

# Clean signet data (use local datadir for isolation)
mkdir -p "${BITCOIN_DATADIR}"

# Check if bitcoind is already running on signet
if bitcoin-cli -signet -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
    echo -e "${BLUE}→ Bitcoin Core already running on signet${NC}"
else
    # Use bash wrapper for ulimit to ensure it applies to bitcoind
    bash -c 'ulimit -n 4096; bitcoind \
        -signet \
        -daemon \
        -datadir="'"${BITCOIN_DATADIR}"'" \
        -prune=2048 \
        -fallbackfee=0.00001 \
        -datacarriersize=10000 \
        -rpcuser="'"${RPC_USER}"'" \
        -rpcpassword="'"${RPC_PASS}"'" \
        -rpcport="'"${RPC_PORT}"'" \
        -maxconnections=10 \
        -dbcache=300 \
        -txindex=0' 2>&1 | grep -v "file descriptors" || true

    sleep 5

    echo -n "Waiting for bitcoind"
    for i in {1..30}; do
        if bitcoin-cli -signet -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockchaininfo &>/dev/null; then
            echo ""; break
        fi
        echo -n "."; sleep 1
    done

    echo -e "${GREEN}✓ Bitcoin Core started${NC}"
fi

echo -e "${YELLOW}Waiting for sync...${NC}\n"

# Wait for sync
while true; do
    CHAIN_INFO=$(bitcoin-cli -signet -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" getblockchaininfo 2>/dev/null)
    BLOCKS=$(echo "$CHAIN_INFO" | jq -r '.blocks' 2>/dev/null || echo "0")
    HEADERS=$(echo "$CHAIN_INFO" | jq -r '.headers' 2>/dev/null || echo "0")
    VERIFIED=$(echo "$CHAIN_INFO" | jq -r '.verificationprogress' 2>/dev/null || echo "0")

    if (( $(echo "$VERIFIED >= 0.9999" | bc -l 2>/dev/null || echo "0") )); then
        echo -e "${GREEN}✓ Synced (${BLOCKS} blocks)${NC}\n"
        break
    fi

    PERCENT=$(echo "$VERIFIED * 100" | bc -l 2>/dev/null | xargs printf "%.1f" 2>/dev/null || echo "0.0")
    echo -ne "\rSyncing: ${PERCENT}% (${BLOCKS}/${HEADERS})"
    sleep 5
done

echo -e "${YELLOW}[3/7] Building...${NC}"
cargo build --release --bin subchain-setup --bin coins-publisher &>/dev/null
echo -e "${GREEN}✓ Build complete${NC}\n"

echo -e "${YELLOW}[4/7] Generating subchain...${NC}"
cat > /tmp/subchain_signet.toml <<EOF
count = ${SUBCHAIN_COUNT}
network = "signet"
output = "${SUBCHAIN_FILE}"
EOF

SUBCHAIN_ADDR=$(
    (echo "") | ./target/release/subchain-setup --config /tmp/subchain_signet.toml 2>&1 | \
    grep "Generated one-time address:" | awk '{print $4}'
)

echo -e "${GREEN}✓ Subchain address: ${SUBCHAIN_ADDR}${NC}\n"

echo -e "${YELLOW}⚠️  MANUAL FUNDING REQUIRED${NC}"
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "Fund this address from signet faucet:\n"
echo -e "${BLUE}  Address: ${SUBCHAIN_ADDR}${NC}\n"
echo -e "${BLUE}  Faucet: https://signetfaucet.com${NC}"
echo -e "${BLUE}  Amount: 0.001 BTC (minimum)${NC}\n"
echo -e "Waiting for confirmation (~1 min)..."
echo -e "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"

# Poll for funding (one-time setup operation)
WAIT_COUNT=0
MAX_WAIT=120

while true; do
    UTXO_INFO=$(bitcoin-cli -signet -rpcuser="${RPC_USER}" -rpcpassword="${RPC_PASS}" -rpcport="${RPC_PORT}" \
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
    ./target/release/subchain-setup --config /tmp/subchain_signet.toml &>/dev/null

# Create symlink to latest subchain
ln -sf "$(basename ${SUBCHAIN_FILE})" "${SUBCHAIN_LINK}"

echo -e "${GREEN}✓ Subchain created: $(basename ${SUBCHAIN_FILE})${NC}"
echo -e "${BLUE}→ Linked to: subchain_signet.bin${NC}\n"

echo -e "${YELLOW}[5/7] Setting up test accounts...${NC}"
cargo run --release --example setup_test_accounts &>/dev/null
echo -e "${GREEN}✓ Test accounts created${NC}\n"

echo -e "${YELLOW}[6/7] Starting publisher...${NC}"
mkdir -p "${NETWORK_DIR}/logs"
./target/release/coins-publisher --config config/publisher-signet.toml > "${NETWORK_DIR}/logs/publisher.log" 2>&1 &
PUB_PID=$!
echo "$PUB_PID" > "${NETWORK_DIR}/publisher.pid"

echo -n "Waiting for publisher"
for i in {1..30}; do
    if curl -s http://localhost:8080/health &>/dev/null; then
        echo ""; break
    fi
    echo -n "."; sleep 1
done

echo -e "${GREEN}✓ Publisher running (PID: ${PUB_PID})${NC}\n"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Setup Complete!${NC}"
echo -e "${GREEN}========================================${NC}\n"
echo -e "Network:      Signet (~1 min blocks)"
echo -e "Subchain:     $(basename ${SUBCHAIN_FILE})"
echo -e "Link:         subchain_signet.bin -> $(basename ${SUBCHAIN_FILE})"
echo -e "Logs:         ${NETWORK_DIR}/logs/publisher.log\n"
echo -e "${YELLOW}NEXT STEP:${NC}"
echo -e "${YELLOW}Check ${NETWORK_DIR}/logs/publisher.log for the publisher address${NC}"
echo -e "${YELLOW}Fund it from https://signetfaucet.com (~0.01 BTC)${NC}\n"
echo -e "${BLUE}List subchains: ls -lh ${SUBCHAIN_DIR}/subchain_signet_*.bin${NC}"
echo -e "${BLUE}Monitor logs:   tail -f ${NETWORK_DIR}/logs/publisher.log${NC}"
echo -e "${BLUE}Run tests:      ./scripts/test-signet.sh${NC}\n"
