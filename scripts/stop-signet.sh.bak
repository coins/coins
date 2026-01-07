#!/bin/bash

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Stopping signet services...${NC}"

if pkill -9 coins-publisher 2>/dev/null; then
    echo -e "${GREEN}✓ Stopped publisher${NC}"
else
    echo "  (publisher not running)"
fi

bitcoin-cli -signet -rpcuser=user -rpcpassword=password -rpcport=38332 stop &>/dev/null || true
sleep 2

if pkill -9 bitcoind 2>/dev/null; then
    echo -e "${GREEN}✓ Stopped bitcoind${NC}"
else
    echo "  (bitcoind not running)"
fi

echo -e "${GREEN}All services stopped${NC}\n"
echo -e "${YELLOW}Note: Signet blockchain preserved for future runs${NC}"
echo -e "${YELLOW}To fully reset: rm -rf ~/.bitcoin/signet${NC}\n"
