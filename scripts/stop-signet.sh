#!/bin/bash

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NETWORK_DIR="${PROJECT_ROOT}/.data/signet"
BITCOIN_DATADIR="${HOME}/.bitcoin"  # Canonical Bitcoin Core datadir

echo -e "${YELLOW}Stopping signet services...${NC}"

# Stop publisher using PID file (network-specific)
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

bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 stop &>/dev/null || true
sleep 2

if pkill -9 bitcoind 2>/dev/null; then
    echo -e "${GREEN}✓ Stopped bitcoind${NC}"
    sleep 1
else
    echo "  (bitcoind not running)"
fi

# Remove lock files if they exist
if [ -f "${BITCOIN_DATADIR}/signet/.lock" ]; then
    rm -f "${BITCOIN_DATADIR}/signet/.lock"
fi
if [ -f "${BITCOIN_DATADIR}/signet/wallets/coins-publisher/.walletlock" ]; then
    rm -f "${BITCOIN_DATADIR}/signet/wallets/coins-publisher/.walletlock"
fi

echo -e "${GREEN}All services stopped${NC}\n"
echo -e "${YELLOW}Note: Signet blockchain preserved in ${BITCOIN_DATADIR}${NC}"
echo -e "${YELLOW}To fully reset: rm -rf ${BITCOIN_DATADIR}${NC}\n"
