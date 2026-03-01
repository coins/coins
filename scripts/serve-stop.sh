#!/usr/bin/env bash

# Stop all serve-mode services (nginx, wallet, explorer, publisher, indexer)

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVE_DIR="${PROJECT_ROOT}/.data/serve"
PID_DIR="${SERVE_DIR}/pids"

echo -e "${YELLOW}Stopping serve-mode services...${NC}"

# Stop nginx first (it's the front door)
if [ -f "${SERVE_DIR}/nginx.pid" ]; then
    PID=$(cat "${SERVE_DIR}/nginx.pid" 2>/dev/null)
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        nginx -c "${SERVE_DIR}/nginx.conf" -s stop 2>/dev/null || kill "$PID" 2>/dev/null
        echo -e "${GREEN}✓ Stopped nginx (PID: $PID)${NC}"
    else
        echo "  (nginx not running)"
    fi
    rm -f "${SERVE_DIR}/nginx.pid"
else
    echo "  (no nginx PID file)"
fi

# Stop each service by PID file
for service in wallet faucet explorer publisher indexer; do
    PIDFILE="${PID_DIR}/${service}.pid"
    if [ -f "$PIDFILE" ]; then
        PID=$(cat "$PIDFILE" 2>/dev/null)
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
            kill "$PID" 2>/dev/null
            # Wait briefly for graceful shutdown
            for i in $(seq 1 5); do
                kill -0 "$PID" 2>/dev/null || break
                sleep 0.5
            done
            # Force kill if still alive
            if kill -0 "$PID" 2>/dev/null; then
                kill -9 "$PID" 2>/dev/null
            fi
            echo -e "${GREEN}✓ Stopped ${service} (PID: $PID)${NC}"
        else
            echo "  (${service} not running)"
        fi
        rm -f "$PIDFILE"
    else
        echo "  (no ${service} PID file)"
    fi
done

echo -e "${GREEN}All serve-mode services stopped${NC}"
