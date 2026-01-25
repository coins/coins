#!/usr/bin/env bash

# Stop regtest services (coins + bitcoind)

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BITCOIN_DATADIR="${PROJECT_ROOT}/.data/regtest/bitcoin"

# Stop all coins services
"${PROJECT_ROOT}/scripts/stop-services.sh"

echo -e "${YELLOW}Stopping bitcoind...${NC}"

# Stop bitcoind gracefully first
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 stop &>/dev/null || true
sleep 2

# Force kill if still running (pgrep works on both Linux and macOS)
if pgrep -f "bitcoind.*regtest" >/dev/null 2>&1; then
    pkill -9 -f "bitcoind.*regtest" 2>/dev/null
    echo -e "${GREEN}✓ Stopped bitcoind${NC}"
else
    echo "  (bitcoind not running)"
fi

# Remove lock files if they exist
rm -f "${BITCOIN_DATADIR}/regtest/.lock" 2>/dev/null
rm -f "${BITCOIN_DATADIR}/regtest/wallets/coins-publisher/.walletlock" 2>/dev/null
rm -f "${BITCOIN_DATADIR}/regtest/wallets/coins-indexer/.walletlock" 2>/dev/null

echo -e "${GREEN}Regtest environment stopped${NC}"
