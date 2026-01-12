#!/usr/bin/env bash

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BITCOIN_DATADIR="${PROJECT_ROOT}/.data/regtest/bitcoin"

echo -e "${YELLOW}Stopping regtest services...${NC}"

# Stop publisher using PID file (network-specific)
NETWORK_DIR="${PROJECT_ROOT}/.data/regtest"
if [ -f "${NETWORK_DIR}/publisher.pid" ]; then
    PID=$(cat "${NETWORK_DIR}/publisher.pid")
    if kill -9 "$PID" 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped publisher (PID: $PID)${NC}"
    else
        echo "  (publisher not running)"
    fi
    rm -f "${NETWORK_DIR}/publisher.pid"
else
    # Fallback: kill any publisher (legacy)
    if pkill -9 coins-publisher 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped publisher${NC}"
    else
        echo "  (publisher not running)"
    fi
fi

# Stop indexer using PID file
if [ -f "${NETWORK_DIR}/indexer.pid" ]; then
    PID=$(cat "${NETWORK_DIR}/indexer.pid")
    if kill -9 "$PID" 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped indexer (PID: $PID)${NC}"
    else
        echo "  (indexer not running)"
    fi
    rm -f "${NETWORK_DIR}/indexer.pid"
else
    # Fallback: kill any indexer
    if pkill -9 coins-indexer 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped indexer${NC}"
    else
        echo "  (indexer not running)"
    fi
fi

# Stop bitcoind gracefully first
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 stop &>/dev/null || true
sleep 3

# Force kill if still running
if pkill -9 bitcoind 2>/dev/null; then
    echo -e "${GREEN}✓ Stopped bitcoind${NC}"
    sleep 1
else
    echo "  (bitcoind not running)"
fi

# Remove lock files if they exist
if [ -f "${BITCOIN_DATADIR}/regtest/.lock" ]; then
    rm -f "${BITCOIN_DATADIR}/regtest/.lock"
fi
if [ -f "${BITCOIN_DATADIR}/regtest/wallets/coins-publisher/.walletlock" ]; then
    rm -f "${BITCOIN_DATADIR}/regtest/wallets/coins-publisher/.walletlock"
fi
if [ -f "${BITCOIN_DATADIR}/regtest/wallets/coins-indexer/.walletlock" ]; then
    rm -f "${BITCOIN_DATADIR}/regtest/wallets/coins-indexer/.walletlock"
fi

echo -e "${GREEN}All services stopped${NC}"
