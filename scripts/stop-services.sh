#!/usr/bin/env bash

# Stop all coins services (publisher, indexer, explorer)
# Works on NixOS and macOS

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Stopping coins services...${NC}"

# Kill by process name - works on both Linux and macOS
kill_by_name() {
    local name="$1"
    local pids

    # pgrep works on both platforms
    pids=$(pgrep -f "$name" 2>/dev/null)

    if [ -n "$pids" ]; then
        echo "$pids" | while read -r pid; do
            if kill -9 "$pid" 2>/dev/null; then
                echo -e "${GREEN}✓ Killed $name (PID: $pid)${NC}"
            fi
        done
        return 0
    fi
    return 1
}

# Stop publisher
if ! kill_by_name "coins-publisher"; then
    echo "  (no publisher running)"
fi

# Stop indexer
if ! kill_by_name "coins-indexer"; then
    echo "  (no indexer running)"
fi

# Stop explorer
if ! kill_by_name "coins-explorer"; then
    echo "  (no explorer running)"
fi

# Clean up PID files from all networks
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
for network in regtest mutinynet; do
    rm -f "${PROJECT_ROOT}/.data/${network}/publisher.pid" 2>/dev/null
    rm -f "${PROJECT_ROOT}/.data/${network}/indexer.pid" 2>/dev/null
    rm -f "${PROJECT_ROOT}/.data/${network}/explorer.pid" 2>/dev/null
done

echo -e "${GREEN}All coins services stopped${NC}"
