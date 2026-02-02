# Testing Guide

## Quick Start (Regtest)

```bash
./tests/regtest/setup-regtest.sh    # Start local environment
./tests/regtest/test-regtest.sh     # Run tests
./scripts/stop-regtest.sh           # Stop services
```

## Prerequisites

- Bitcoin Core 24+
- Rust toolchain (stable)
- wasm-pack (for wallet tests)

## Test Scripts

### Regtest (`tests/regtest/`)

| Script | Tests |
|--------|-------|
| `setup-regtest.sh` | Starts bitcoind, publisher (8080), indexer (8084), funds test accounts |
| `test-regtest.sh` | Account queries, transaction submission, package relay, balance persistence |
| `test-wallet-regtest.sh` | Wallet server, nonce increments, token transfers (native + token_id=1) |
| `test-publish-formats.sh` | OP_RETURN and Taproot annex publishing formats, locktime encoding |
| `test-ibd.sh` | Initial block download - fresh node syncs historical sub-blocks |
| `test-explorer.sh` | Explorer API, confirmation status, pending/broadcast tracking, finalization |

### Mutinynet (`tests/mutinynet/`)

| Script | Tests |
|--------|-------|
| `setup-mutinynet.sh` | Connects to remote signet node, requires faucet funding |
| `test-mutinynet.sh` | Remote node connectivity, transaction submission, block confirmations |
| `test-wallet-mutinynet.sh` | Wallet operations on live signet (~1 min block times) |

## Service Ports

| Network | Publisher | Indexer | Bitcoin RPC |
|---------|-----------|---------|-------------|
| Regtest | 8080 | 8084 | 18443 |
| Mutinynet | 8082 | 8083 | 38332 (remote) |
