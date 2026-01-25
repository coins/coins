#!/usr/bin/env bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=======================================${NC}"
echo -e "${BLUE}   Resuming Mutinynet Services${NC}"
echo -e "${BLUE}=======================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NETWORK_DIR="${PROJECT_ROOT}/.data/mutinynet"

cd "$PROJECT_ROOT"

# Check if setup was run
if [ ! -d "$NETWORK_DIR" ]; then
    echo -e "${RED}✗ Mutinynet not set up. Run setup-mutinynet first.${NC}"
    exit 1
fi

echo -e "${YELLOW}[1/3] Stopping existing services...${NC}"
pkill -9 coins-publisher 2>/dev/null || true
pkill -9 coins-indexer 2>/dev/null || true
sleep 2
echo -e "${GREEN}✓ Services stopped${NC}\n"

echo -e "${YELLOW}[2/3] Starting indexer...${NC}"
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

if ! curl -s http://localhost:8083/health &>/dev/null; then
    echo ""
    echo -e "${RED}✗ Indexer failed to start${NC}"
    echo -e "${YELLOW}Last 20 lines of indexer log:${NC}"
    tail -20 "${NETWORK_DIR}/logs/indexer.log"
    exit 1
fi

echo -e "${GREEN}✓ Indexer running (PID: ${IDX_PID})${NC}\n"

echo -e "${YELLOW}[3/3] Starting publisher...${NC}"
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

if ! curl -s http://localhost:8082/health &>/dev/null; then
    echo ""
    echo -e "${RED}✗ Publisher failed to start${NC}"
    echo -e "${YELLOW}Last 20 lines of publisher log:${NC}"
    tail -20 "${NETWORK_DIR}/logs/publisher.log"
    exit 1
fi

echo -e "${GREEN}✓ Publisher running (PID: ${PUB_PID})${NC}\n"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Mutinynet Services Resumed!${NC}"
echo -e "${GREEN}========================================${NC}\n"
echo -e "Indexer:    http://localhost:8083 (PID: ${IDX_PID})"
echo -e "Publisher:  http://localhost:8082 (PID: ${PUB_PID})"
echo -e "Logs:       ${NETWORK_DIR}/logs/"
echo -e ""
echo -e "${BLUE}Monitor logs:   tail -f ${NETWORK_DIR}/logs/publisher.log${NC}"
echo -e "${BLUE}Start explorer: explorer-mutinynet${NC}"
