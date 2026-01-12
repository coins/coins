#!/usr/bin/env bash

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NETWORK_DIR="${PROJECT_ROOT}/.data/mutinynet"

echo -e "${YELLOW}Stopping mutinynet services...${NC}"

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
    # Fallback: kill any coins-publisher on port 8082
    if lsof -ti:8082 2>/dev/null | xargs kill -9 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped publisher${NC}"
    else
        echo "  (publisher not running)"
    fi
fi

# Stop indexer using PID file (network-specific)
if [ -f "${NETWORK_DIR}/indexer.pid" ]; then
    PID=$(cat "${NETWORK_DIR}/indexer.pid")
    if kill -9 "$PID" 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped indexer (PID: $PID)${NC}"
    else
        echo "  (indexer not running)"
    fi
    rm -f "${NETWORK_DIR}/indexer.pid"
else
    # Fallback: kill any coins-indexer on port 8083
    if lsof -ti:8083 2>/dev/null | xargs kill -9 2>/dev/null; then
        echo -e "${GREEN}✓ Stopped indexer${NC}"
    else
        echo "  (indexer not running)"
    fi
fi

echo -e "${GREEN}All local services stopped${NC}\n"
echo -e "${YELLOW}Note: Using remote mutinynet node (no local bitcoind to stop)${NC}"
echo -e "${YELLOW}To reset local data: rm -rf ${NETWORK_DIR}${NC}\n"
