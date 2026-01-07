#!/bin/bash

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Stopping regtest services...${NC}"

# Stop publisher
if pkill -9 coins-publisher 2>/dev/null; then
    echo -e "${GREEN}✓ Stopped publisher${NC}"
else
    echo "  (publisher not running)"
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
if [ -f "${HOME}/.bitcoin/regtest/.lock" ]; then
    rm -f "${HOME}/.bitcoin/regtest/.lock"
fi
if [ -f "${HOME}/.bitcoin/regtest/wallets/coins-publisher/.walletlock" ]; then
    rm -f "${HOME}/.bitcoin/regtest/wallets/coins-publisher/.walletlock"
fi

echo -e "${GREEN}All services stopped${NC}"
