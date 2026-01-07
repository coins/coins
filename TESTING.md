# Integration Testing Guide

Complete guide for running Coins integration tests on Bitcoin regtest.

## Prerequisites

- Bitcoin Core 24+ installed and in PATH
- Rust toolchain
- macOS or Linux

## Quick Start (One Command)

```bash
./scripts/setup-regtest.sh
```

This single command will:
1. ✅ Clean up old processes and data
2. ✅ Start fresh Bitcoin Core regtest node
3. ✅ Create watch-only wallet
4. ✅ Build all binaries
5. ✅ Generate subchain with valid UTXOs
6. ✅ Mine blocks to fee address for funding
7. ✅ Setup test accounts (Alice & Bob)
8. ✅ Start aggregator service
9. ✅ Run smoke test

## What the Setup Script Does

### 1. Cleanup
- Kills any running `bitcoind` or `coins-aggregator` processes
- Removes old data directories
- Cleans Bitcoin regtest data

### 2. Bitcoin Core Setup
- Starts `bitcoind` in regtest mode
- Configuration:
  - RPC port: 18443
  - User: `user`
  - Password: `password`
  - txindex enabled
  - Minimal connections for performance

### 3. Subchain Generation
The script generates a subchain with pre-signed connector transactions:

1. Runs `subchain-setup` to generate a random keypair
2. Mines 101 blocks to that address (for coinbase maturity)
3. Uses a mature UTXO to fund the subchain
4. Creates 1000 pre-signed connector transactions

### 4. Fee Funding
1. Starts aggregator temporarily to discover its fee address
2. Extracts fee address from logs
3. Mines 150 blocks to that address (ensures mature coinbase UTXOs)

### 5. Test Accounts
Creates two test accounts:
- **Alice** (ID: 1): 10,000 tokens
- **Bob** (ID: 2): 0 tokens

## Running Tests

After setup, run the test suite:

```bash
./scripts/test-regtest.sh
```

This runs 6 integration tests:
1. ✅ Transaction submission and mining
2. ⚠️  Package relay to blockchain (may fail due to fee issues)
3. ✅ Account balance persistence
4. ✅ Indexer functionality
5. ✅ Bitcoin RPC connectivity
6. ✅ Wallet UTXO management

## Manual Testing

### Submit a Transaction

```bash
cargo run --release --example submit_txs
```

This:
- Creates a transaction from Alice to Bob
- Submits it to the aggregator
- Waits 30 seconds for mining
- Verifies balances updated

### Query Accounts

```bash
# Alice (replace with actual pubkey from logs)
curl http://localhost:8080/account/2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f | jq

# Bob
curl http://localhost:8080/account/33f90e60f449b2f1d54dc04ecb4a805d67bfe6668482283c78d45b5e50af3940 | jq
```

### Mine Bitcoin Blocks

```bash
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    generatetoaddress 1 $(bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getnewaddress)
```

### Check Mempool

```bash
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 getrawmempool
```

### View Logs

```bash
# Aggregator logs
tail -f /tmp/aggregator.log

# Bitcoin Core logs
tail -f ~/Library/Application\ Support/Bitcoin/regtest/debug.log
```

## Stopping Services

```bash
./scripts/stop-regtest.sh
```

Cleanly stops:
- Aggregator service
- Bitcoin Core regtest node
- Removes lock files

## Directory Structure

```
.data/
├── subchains/
│   └── subchain_regtest.bin    # 1000 pre-signed transactions (contains pubkey → address)
└── keys/
    └── aggregator_sk.hex        # Fee payment key

state.db/                         # Account state database
indexer.db/                       # Block indexer database
aggregator_bls_sk.hex            # BLS key for signatures
```

## Troubleshooting

### Aggregator Won't Start

```bash
# Check logs for errors
tail -50 /tmp/aggregator.log

# Common issues:
# - "database locked": Run ./scripts/stop-regtest.sh first
# - "No fee UTXOs": Run setup script again (mines more blocks)
```

### Package Relay Failing

```bash
# Check for rejection reasons
grep "Package mempool" /tmp/aggregator.log

# Common issues:
# - "missing-inputs": Subchain file references non-existent UTXOs (re-run setup)
# - "min relay fee not met": Fee rate too low or UTXO value too small
```

### Bitcoin Core Issues

```bash
# Check Bitcoin logs
tail -50 ~/Library/Application\ Support/Bitcoin/regtest/debug.log

# Restart Bitcoin
pkill -9 bitcoind
bitcoind -regtest -daemon -fallbackfee=0.00001 -txindex=1 \
    -rpcuser=user -rpcpassword=password -rpcport=18443
```

### Clean Restart

```bash
./scripts/stop-regtest.sh
rm -rf .data state.db indexer.db aggregator_bls_sk.hex
./scripts/setup-regtest.sh
```

## Configuration

### Setup Script Variables

Edit `scripts/setup-regtest.sh` to customize:

```bash
SUBCHAIN_COUNT=1000        # Number of pre-signed transactions
RPC_PORT="18443"           # Bitcoin RPC port
RPC_USER="user"            # Bitcoin RPC username
RPC_PASS="password"        # Bitcoin RPC password
```

### Aggregator Config

Edit `config/aggregator.toml`:

```toml
interval = 30              # Mining interval in seconds
network = "regtest"        # Bitcoin network
genesis_balance = 1000000000000  # Initial token supply
```

## Known Issues

1. **Package Relay Test Fails**: Fee calculation may result in insufficient fees for relay. This is a known issue being worked on.

2. **File Descriptor Limit (macOS)**: Bitcoin Core may report "-1 file descriptors available". This is a macOS-specific issue but bitcoind usually starts anyway. The script filters this warning.

3. **Fee UTXOs Not Mature**: If tests run immediately after setup, fee UTXOs may not be mature yet (need 100 confirmations). The setup script mines 150 blocks to address this, but timing-sensitive tests may still fail.

## Advanced Usage

### Custom Subchain Size

Create a custom subchain with more/fewer transactions:

```bash
cat > /tmp/custom_subchain.toml <<EOF
count = 10000
network = "regtest"
output = ".data/subchains/subchain_custom.bin"
EOF

cargo run --release --bin subchain-setup -- --config /tmp/custom_subchain.toml
```

### Debug Mode

Run aggregator with debug logging:

```bash
RUST_LOG=debug ./target/release/coins-aggregator --config config/aggregator.toml
```

### Package Relay Debugging

Enable detailed package relay logs:

```bash
RUST_LOG=coins_aggregator::rpc_backend=debug ./target/release/coins-aggregator --config config/aggregator.toml
```

## CI/CD Integration

For automated testing in CI:

```bash
#!/bin/bash
set -e

# Run setup
./scripts/setup-regtest.sh

# Run tests
./scripts/test-regtest.sh

# Cleanup
./scripts/stop-regtest.sh
```

## Next Steps

After successful setup:
1. Read `scripts/README.md` for script details
2. Check `crates/coins-aggregator/examples/` for more examples
3. Review `ARCHITECTURE.md` for system design

## Support

- Setup issues: Check `/tmp/aggregator.log` and this guide's troubleshooting section
- Bitcoin Core issues: Verify Bitcoin Core version is 24+
- General questions: See main README.md
