# Integration Test Scripts

Foolproof scripts for running Coins on Bitcoin regtest.

## Quick Start

```bash
# Setup everything (takes ~30 seconds)
./scripts/setup-regtest.sh

# Run tests
./scripts/test-regtest.sh

# Stop services
./scripts/stop-regtest.sh
```

## What `setup-regtest.sh` Does

1. **Cleanup**: Stops old processes, removes stale data
2. **Bitcoin Core**: Starts fresh regtest node
3. **Wallet**: Creates watch-only wallet for aggregator
4. **Build**: Compiles subchain-setup and aggregator
5. **Subchain**:
   - Generates random keypair
   - Mines 101 blocks to that address
   - Creates subchain file with 1000 pre-signed transactions
6. **Funding**: Mines 50 blocks to fee address
7. **Accounts**: Sets up Alice and Bob test accounts
8. **Aggregator**: Starts the aggregator service
9. **Test**: Runs a quick smoke test

## What `test-regtest.sh` Does

Runs comprehensive integration tests:
- Transaction submission and mining
- Package relay to blockchain
- Account balance persistence
- Indexer functionality
- RPC connectivity
- Wallet UTXO management

## What `stop-regtest.sh` Does

Cleanly stops:
- Coins aggregator
- Bitcoin Core regtest node

## Logs

- Aggregator: `/tmp/aggregator.log`
- Bitcoin: `~/.bitcoin/regtest/debug.log`

## Manual Testing

Once setup is complete, you can:

```bash
# Submit transactions
cargo run --release --example submit_txs

# Query accounts (Alice)
curl http://localhost:8080/account/2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f | jq

# Query accounts (Bob)
curl http://localhost:8080/account/33f90e60f449b2f1d54dc04ecb4a805d67bfe6668482283c78d45b5e50af3940 | jq

# Mine a block
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    generatetoaddress 1 bcrt1qxl767gvfrpcf4lclag3w5707xdk0j7hxnyj02g

# Check mempool
bitcoin-cli -regtest -rpcuser=user -rpcpassword=password -rpcport=18443 \
    getrawmempool
```

## Configuration

The setup script uses:
- 1000 pre-signed connector transactions (can be changed by editing SUBCHAIN_COUNT)
- 30-second mining interval
- RPC port 18443
- User: `user`, Password: `password`

## Troubleshooting

**Aggregator won't start:**
```bash
tail -50 /tmp/aggregator.log
```

**Package relay not working:**
```bash
grep "Package mempool" /tmp/aggregator.log
```

**Bitcoin Core issues:**
```bash
tail -50 ~/.bitcoin/regtest/debug.log
```

**Clean restart:**
```bash
./scripts/stop-regtest.sh
./scripts/setup-regtest.sh
```
