# Testing Guide

Complete guide for running Coins integration tests on Bitcoin regtest and mutinynet.

## Quick Start

```bash
# Setup everything (regtest)
./scripts/setup-regtest.sh

# Run tests
./scripts/test-regtest.sh

# Stop services
./scripts/stop-regtest.sh
```

## Prerequisites

- Bitcoin Core 24+ installed and in PATH
- Rust toolchain (stable)
- macOS or Linux
- ~1 minute for regtest setup

## Testing Scripts

| Script | Network | Purpose |
|--------|---------|---------|
| `setup-regtest.sh` | Regtest | Setup local Bitcoin test environment with publisher and indexer |
| `test-regtest.sh` | Regtest | Run 7 integration tests |
| `stop-regtest.sh` | Regtest | Stop all regtest services |
| `setup-mutinynet.sh` | Mutinynet | Setup remote Mutinynet environment (requires manual funding) |
| `test-mutinynet.sh` | Mutinynet | Run integration tests on Mutinynet |
| `stop-mutinynet.sh` | Mutinynet | Stop all Mutinynet services |

## Regtest Workflow

### Setup

```bash
./scripts/setup-regtest.sh
```

This script performs 11 steps:
- Cleans up old processes and data
- Starts fresh Bitcoin Core regtest node (RPC port 18443)
- Creates watch-only wallets for publisher and indexer
- Builds all project binaries
- Generates subchain with 1000 pre-signed transactions
- Mines 150 blocks to publisher address for fee funding
- Creates test keypairs (Alice, Bob, Genesis)
- Starts indexer service (port 8083)
- Starts publisher service (port 8080, 30-second mining interval)
- Funds test accounts (Alice: 10,000 tokens, Bob: 0 tokens)
- Runs smoke test

### Run Tests

```bash
./scripts/test-regtest.sh
```

Runs 7 integration tests:
1. Alice account exists
2. Submit transaction (Alice → Bob, 100 tokens)
3. Package relay to blockchain
4. Account balance persistence
5. Indexer has indexed blocks
6. Bitcoin RPC connectivity
7. Publisher wallet has UTXOs

### Cleanup

```bash
./scripts/stop-regtest.sh
```

Stops publisher, indexer, and Bitcoin Core regtest node.

### Manual Testing

Once setup is complete:

```bash
# Submit transaction (Alice → Bob)
./target/release/coins-client --keyfile .data/regtest/test-keys/alice_sk.hex \
    --publisher-url http://localhost:8080 \
    send --recipient-pk $(./target/release/examples/get_pk .data/regtest/test-keys/bob_sk.hex) --amount 100

# Query Alice's account
ALICE_PK=$(./target/release/examples/get_pk .data/regtest/test-keys/alice_sk.hex)
curl http://localhost:8080/account/${ALICE_PK} | jq

# Mine a block manually
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    -rpcwallet=coins-publisher generatetoaddress 1 $(bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 -rpcwallet=coins-publisher getnewaddress)
```

## Mutinynet Workflow (Brief)

For testing against remote Mutinynet signet node (~1 min blocks).

### Setup

```bash
./scripts/setup-mutinynet.sh
```

**IMPORTANT**: Mutinynet requires manual funding:
1. Script displays subchain address - fund it via https://faucet.mutinynet.com (0.001 BTC minimum)
2. Script waits for blockchain confirmation (~1 minute)
3. If needed, fund publisher address for transaction fees (0.001 BTC recommended)

Services run on different ports than regtest:
- Publisher: port 8082
- Indexer: port 8083

### Run Tests

```bash
./scripts/test-mutinynet.sh
```

### Cleanup

```bash
./scripts/stop-mutinynet.sh
```

## Configuration

### Network-Specific Configs

Regtest and Mutinynet use separate configuration files:

- `config/publisher-regtest.toml` - Regtest publisher config
- `config/publisher-mutinynet.toml` - Mutinynet publisher config
- `config/indexer-regtest.toml` - Regtest indexer config
- `config/indexer-mutinynet.toml` - Mutinynet indexer config

### Key Settings

```toml
# Common settings in publisher configs
interval = 30              # Mining interval (seconds)
network = "regtest"        # or "mutinynet"
genesis_balance = 1000000000000  # Initial balance
```

### Service Ports

| Network | Publisher | Indexer | Bitcoin RPC |
|---------|-----------|---------|-------------|
| Regtest | 8080 | 8083 | 18443 |
| Mutinynet | 8082 | 8083 | 38332 (remote) |

## Directory Structure

All test data is organized by network to avoid conflicts:

```
.data/
├── regtest/
│   ├── bitcoin/          # Bitcoin Core datadir
│   ├── subchains/        # Subchain files
│   ├── keys/             # Publisher keypairs
│   ├── test-keys/        # Test account keypairs (Alice, Bob, Genesis)
│   ├── logs/             # publisher.log, indexer.log
│   ├── state.db/         # Account state database
│   └── indexer.db/       # Block indexer database
└── mutinynet/
    ├── subchains/
    ├── keys/
    ├── logs/
    ├── state.db/
    └── indexer.db/
```

## Troubleshooting

### Publisher won't start

```bash
# Check publisher logs
tail -50 .data/regtest/logs/publisher.log

# Check if port is already in use
lsof -i :8080
```

### Indexer won't start

```bash
# Check indexer logs
tail -50 .data/regtest/logs/indexer.log

# Verify Bitcoin Core is running
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getblockchaininfo
```

### Tests are failing

```bash
# Clean restart
./scripts/stop-regtest.sh
./scripts/setup-regtest.sh

# Check service health
curl http://localhost:8080/health
curl http://localhost:8083/health
```

### Bitcoin Core issues

```bash
# Check Bitcoin Core logs
tail -50 .data/regtest/bitcoin/regtest/debug.log

# Verify RPC connectivity
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getblockcount
```

### Package relay not working

```bash
# Check if transactions are in mempool
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getrawmempool

# Verify publisher is broadcasting
grep "Package mempool" .data/regtest/logs/publisher.log
```
