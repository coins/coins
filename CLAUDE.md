# Development Guide

## Project Structure

Cargo workspace with crates:

- `coins-crypto` - BLS cryptography (BN-254 curve)
- `coins-types` - Core data structures, invoice module
- `coins-core` - State management & validation
- `coins-bitcoin-rpc` - Bitcoin RPC integration
- `coins-indexer` - Transaction indexing service (port 8083)
- `coins-publisher` - Transaction publisher (port 8082)
- `coins-subchain` - Subchain compression & publishing
- `coins-client` - CLI client
- `coins-wallet` - Web wallet server (port 8085) + WASM module
- `coins-explorer` - Block explorer (port 3000)
- `shared/` - Shared CSS styles

## Starting Services

### Regtest (local)

```bash
# Setup and start regtest environment
bash tests/regtest/setup-regtest.sh

# Stop regtest
bash scripts/stop-regtest.sh
```

### Mutinynet

```bash
# First-time setup
bash tests/mutinynet/setup-mutinynet.sh

# Resume after stopping
bash scripts/resume-mutinynet.sh

# Stop
bash scripts/stop-mutinynet.sh
```

### Wallet Server

```bash
# Build WASM + start wallet server
bash scripts/setup-wallet.sh
```

## Sending Transactions

### Via CLI

```bash
# Initialize a new wallet
cargo run -p coins-client -- init

# Check balance
cargo run -p coins-client -- balance

# Send tokens
cargo run -p coins-client -- send --recipient <hex> --amount 100 --token-id 0

# Create an invoice
cargo run -p coins-client -- invoice --amount 100 --memo "Payment"

# Send from an invoice
cargo run -p coins-client -- send --invoice "coins://pay?addr=abc123&amount=100"
```

### Via Web Wallet

Open `http://localhost:8085` after starting the wallet server. Create or import a wallet, then use the Send form.

## Running Tests

```bash
# Unit tests (all crates)
cargo test

# Specific crate tests
cargo test -p coins-types
cargo test -p coins-crypto
cargo test -p coins-core
cargo test -p coins-publisher

# Invoice module tests
cargo test -p coins-types -- invoice

# Regtest integration tests
bash tests/regtest/test-regtest.sh
bash tests/regtest/test-ibd.sh
bash tests/regtest/test-publish-formats.sh
bash tests/regtest/test-wallet-regtest.sh
bash tests/regtest/test-explorer.sh

# Mutinynet integration tests
bash tests/mutinynet/test-mutinynet.sh
bash tests/mutinynet/test-wallet-mutinynet.sh
```

## Build

```bash
# Check all crates compile
cargo check

# Build all
cargo build

# Build WASM for wallet
cd crates/coins-wallet/wasm && wasm-pack build --target web
```

## Configuration

Config files in `config/` directory, organized by network:
- `*-regtest.toml` - Local regtest
- `*-signet.toml` - Bitcoin Signet
- `*-mutinynet.toml` - Mutinynet testnet
