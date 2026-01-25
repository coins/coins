#!/usr/bin/env bash

# Stop mutinynet services and delete local data

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NETWORK_DIR="${PROJECT_ROOT}/.data/mutinynet"

# Stop all coins services
"${PROJECT_ROOT}/scripts/stop-services.sh"

# Delete local mutinynet data
if [ -d "$NETWORK_DIR" ]; then
    rm -rf "$NETWORK_DIR"
    echo -e "${GREEN}✓ Deleted ${NETWORK_DIR}${NC}"
else
    echo -e "${YELLOW}No mutinynet data to delete${NC}"
fi
