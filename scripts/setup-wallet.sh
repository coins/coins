#!/usr/bin/env bash
set -e

# Add cargo bin to PATH if it exists (for wasm-pack)
if [ -d "$HOME/.cargo/bin" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   Coins Wallet Development Setup${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Configuration
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM_DIR="${PROJECT_ROOT}/crates/coins-wallet/wasm"
WALLET_PORT="${WALLET_PORT:-8085}"
INDEXER_URL="${INDEXER_URL:-http://localhost:8084}"
PUBLISHER_URL="${PUBLISHER_URL:-http://localhost:8080}"

cd "$PROJECT_ROOT"

# Check if --dev flag is provided
DEV_MODE=false
if [ "$1" = "--dev" ]; then
    DEV_MODE=true
    echo -e "${YELLOW}Running in development mode with auto-rebuild${NC}"
    echo ""
fi

echo -e "${YELLOW}[1/3] Building WASM module...${NC}"

# Build WASM with wasm-pack
if ! command -v wasm-pack &> /dev/null; then
    echo -e "${YELLOW}wasm-pack not found. Installing...${NC}"
    cargo install wasm-pack
fi

cd "${WASM_DIR}"
wasm-pack build --target web

if [ ! -f "${WASM_DIR}/pkg/coins_wallet_wasm_bg.wasm" ]; then
    echo -e "${RED}✗ WASM build failed${NC}"
    exit 1
fi

WASM_SIZE=$(du -h "${WASM_DIR}/pkg/coins_wallet_wasm_bg.wasm" | awk '{print $1}')
echo -e "${GREEN}✓ WASM module built (${WASM_SIZE})${NC}"
echo ""

cd "$PROJECT_ROOT"

echo -e "${YELLOW}[2/3] Building wallet server...${NC}"
cargo build --release --bin coins-wallet

echo -e "${GREEN}✓ Wallet server built${NC}"
echo ""

echo -e "${YELLOW}[3/3] Starting wallet server...${NC}"

# Set environment variables
export WALLET_PORT="${WALLET_PORT}"
export INDEXER_URL="${INDEXER_URL}"
export PUBLISHER_URL="${PUBLISHER_URL}"

echo -e "${BLUE}Configuration:${NC}"
echo -e "  Port: ${WALLET_PORT}"
echo -e "  Indexer: ${INDEXER_URL}"
echo -e "  Publisher: ${PUBLISHER_URL}"
echo ""

if [ "$DEV_MODE" = true ]; then
    echo -e "${BLUE}Starting in development mode...${NC}"
    echo -e "${YELLOW}Note: Auto-rebuild on changes not yet implemented${NC}"
    echo -e "${YELLOW}Restart the script manually after making changes${NC}"
    echo ""
fi

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Wallet Ready!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${BLUE}Access wallet at:${NC}"
echo -e "  ${GREEN}http://localhost:${WALLET_PORT}${NC}"
echo ""
echo -e "${BLUE}Health check:${NC}"
echo -e "  curl http://localhost:${WALLET_PORT}/health"
echo ""
echo -e "${YELLOW}Press Ctrl+C to stop${NC}"
echo ""

# Run the wallet server
./target/release/coins-wallet
