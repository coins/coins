#!/usr/bin/env bash
set -e

# Start all serve-mode services (indexer, publisher, explorer, wallet, nginx)
# for production deployment behind nginx reverse proxy on port 80.

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVE_DIR="${PROJECT_ROOT}/.data/serve"
PID_DIR="${SERVE_DIR}/pids"
LOG_DIR="${SERVE_DIR}/logs"

cd "$PROJECT_ROOT"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Coins Serve Mode${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# ── Bootstrap serve data from mutinynet ──

MUTINYNET_DIR="${PROJECT_ROOT}/.data/mutinynet"

if [ ! -d "${SERVE_DIR}/keys" ] || [ ! -d "${SERVE_DIR}/subchains" ]; then
    if [ ! -d "$MUTINYNET_DIR" ]; then
        echo -e "${RED}✗ No mutinynet data found. Run setup-mutinynet.sh first.${NC}"
        exit 1
    fi
    echo -e "${YELLOW}Bootstrapping serve data from mutinynet...${NC}"
    mkdir -p "${SERVE_DIR}/keys" "${SERVE_DIR}/subchains"
    cp "${MUTINYNET_DIR}/keys/genesis_sk.hex" "${SERVE_DIR}/keys/"
    cp "${MUTINYNET_DIR}/keys/publisher_mutinynet_sk.hex" "${SERVE_DIR}/keys/"
    cp "${MUTINYNET_DIR}/keys/publisher_bls_sk.hex" "${SERVE_DIR}/keys/"
    cp -P "${MUTINYNET_DIR}/subchains/subchain_mutinynet.bin" "${SERVE_DIR}/subchains/"
    # Copy the actual subchain file the symlink points to
    cp "$(readlink -f "${MUTINYNET_DIR}/subchains/subchain_mutinynet.bin")" "${SERVE_DIR}/subchains/"
    echo -e "${GREEN}✓ Serve data bootstrapped${NC}"
    echo ""
fi

# ── Pre-checks ──

# Check for required binaries
MISSING=0
for bin in coins-indexer coins-publisher coins-explorer coins-wallet coins-faucet; do
    if [ ! -f "target/release/${bin}" ]; then
        echo -e "${RED}✗ Missing target/release/${bin}${NC}"
        MISSING=1
    fi
done

if [ "$MISSING" -eq 1 ]; then
    echo ""
    echo -e "${YELLOW}Build all binaries first:${NC}"
    echo "  cargo build --release"
    echo "  bash scripts/setup-wallet.sh  # for WASM"
    exit 1
fi

# Check WASM
if [ ! -f "crates/coins-wallet/wasm/pkg/coins_wallet_wasm_bg.wasm" ]; then
    echo -e "${RED}✗ Missing WASM module. Build with:${NC}"
    echo "  cd crates/coins-wallet/wasm && wasm-pack build --target web"
    exit 1
fi

# Check if already running
if [ -f "${PID_DIR}/nginx.pid" ] && kill -0 "$(cat "${PID_DIR}/nginx.pid")" 2>/dev/null; then
    echo -e "${YELLOW}Serve mode appears to be already running.${NC}"
    echo "  Run: serve-stop  (then try again)"
    exit 1
fi

# ── Create directories ──
mkdir -p "$PID_DIR" "$LOG_DIR" "${SERVE_DIR}/tmp/client_body" "${SERVE_DIR}/tmp/proxy" \
    "${SERVE_DIR}/tmp/fastcgi" "${SERVE_DIR}/tmp/uwsgi" "${SERVE_DIR}/tmp/scgi"

# ── Helper: wait for a service to be healthy ──
wait_for_health() {
    local name="$1"
    local url="$2"
    local max_wait="${3:-30}"
    local elapsed=0

    printf "  Waiting for %s..." "$name"
    while [ $elapsed -lt $max_wait ]; do
        if curl -sf "$url" > /dev/null 2>&1; then
            echo -e " ${GREEN}ready${NC}"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    echo -e " ${RED}TIMEOUT after ${max_wait}s${NC}"
    return 1
}

# ── 1. Start Indexer ──
echo -e "${YELLOW}[1/6] Starting indexer on port 10003...${NC}"
./target/release/coins-indexer --config config/serve-indexer.toml \
    > "${LOG_DIR}/indexer.log" 2>&1 &
echo $! > "${PID_DIR}/indexer.pid"
wait_for_health "indexer" "http://localhost:10003/health"

# ── 2. Start Publisher ──
echo -e "${YELLOW}[2/6] Starting publisher on port 10004...${NC}"
./target/release/coins-publisher --config config/serve-publisher.toml \
    > "${LOG_DIR}/publisher.log" 2>&1 &
echo $! > "${PID_DIR}/publisher.pid"
wait_for_health "publisher" "http://localhost:10004/health"

# ── 3. Start Explorer ──
echo -e "${YELLOW}[3/6] Starting explorer on port 10002...${NC}"
./target/release/coins-explorer --config config/serve-explorer.toml \
    > "${LOG_DIR}/explorer.log" 2>&1 &
echo $! > "${PID_DIR}/explorer.pid"
wait_for_health "explorer" "http://localhost:10002/health"

# ── 4. Start Wallet ──
echo -e "${YELLOW}[4/6] Starting wallet on port 10001...${NC}"
WALLET_PORT=10001 \
INDEXER_URL=http://localhost:10003 \
PUBLISHER_URL=http://localhost:10004 \
EXPLORER_URL=http://localhost:10002 \
FAUCET_URL=/faucet \
    ./target/release/coins-wallet \
    > "${LOG_DIR}/wallet.log" 2>&1 &
echo $! > "${PID_DIR}/wallet.pid"
wait_for_health "wallet" "http://localhost:10001/health"

# ── 5. Start Faucet ──
echo -e "${YELLOW}[5/6] Starting faucet on port 10005...${NC}"
./target/release/coins-faucet --config config/serve-faucet.toml \
    > "${LOG_DIR}/faucet.log" 2>&1 &
echo $! > "${PID_DIR}/faucet.pid"
wait_for_health "faucet" "http://localhost:10005/health"

# ── 6. Start nginx ──
echo -e "${YELLOW}[6/6] Starting nginx on ports 80 + 443...${NC}"

# Generate self-signed SSL cert if missing
SSL_DIR="${SERVE_DIR}/ssl"
if [ ! -f "${SSL_DIR}/server.crt" ]; then
    mkdir -p "$SSL_DIR"
    echo -e "  Generating self-signed SSL certificate..."
    openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
        -keyout "${SSL_DIR}/server.key" \
        -out "${SSL_DIR}/server.crt" \
        -subj "/CN=168.119.139.152" \
        2>/dev/null
    echo -e "  ${GREEN}Certificate created${NC}"
fi

# Find nginx mime.types
NGINX_MIME_TYPES=""
for candidate in \
    "$(dirname "$(which nginx 2>/dev/null)")/../conf/mime.types" \
    "$(dirname "$(which nginx 2>/dev/null)")/../etc/nginx/mime.types" \
    "/etc/nginx/mime.types" \
    "/nix/store/"*"-nginx-"*"/conf/mime.types"; do
    if [ -f "$candidate" ]; then
        NGINX_MIME_TYPES="$candidate"
        break
    fi
done

if [ -z "$NGINX_MIME_TYPES" ]; then
    echo -e "${RED}✗ Could not find nginx mime.types${NC}"
    echo "  Make sure nginx is installed (use: nix develop .#serve)"
    exit 1
fi

# Generate runtime nginx config from template
sed \
    -e "s|SERVE_DATA_DIR|${SERVE_DIR}|g" \
    -e "s|NGINX_MIME_TYPES|${NGINX_MIME_TYPES}|g" \
    config/serve-nginx.conf > "${SERVE_DIR}/nginx.conf"

nginx -c "${SERVE_DIR}/nginx.conf" -e "${SERVE_DIR}/nginx-error.log"
# nginx manages its own PID file at SERVE_DIR/nginx.pid

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   All services running!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${BLUE}URLs:${NC}"
echo -e "  Wallet:   ${GREEN}https://168.119.139.152/wallet/${NC}"
echo -e "  Explorer: ${GREEN}https://168.119.139.152/explorer/${NC}"
echo -e "  Faucet:   ${GREEN}https://168.119.139.152/faucet/${NC}"
echo -e "  (also available on http, but crypto.subtle requires https)"
echo ""
echo -e "${BLUE}Direct ports:${NC}"
echo -e "  Wallet:    http://localhost:10001"
echo -e "  Explorer:  http://localhost:10002"
echo -e "  Indexer:   http://localhost:10003"
echo -e "  Publisher: http://localhost:10004"
echo -e "  Faucet:    http://localhost:10005"
echo ""
echo -e "${BLUE}Logs:${NC}   ${LOG_DIR}/"
echo -e "${BLUE}PIDs:${NC}   ${PID_DIR}/"
echo ""
echo -e "${YELLOW}Stop with: serve-stop${NC}"
