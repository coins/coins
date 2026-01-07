# Coins Protocol

A compact Layer-2 Bitcoin token protocol using BLS signatures and embedded consensus.

⚠️ **Regtest only** - This implementation currently supports Bitcoin regtest for development and testing purposes only.

## Overview

Coins is an embedded-consensus token system built on Bitcoin that uses:
- **Subchain** architecture: pre-signed Bitcoin transaction chains for anchoring
- **Minimal on-chain footprint**: 64-byte aggregate BLS signatures for entire blocks
- **OP_RETURN publishing**: Compressed sub-blocks published via Bitcoin Core v30's 100KB OP_RETURN limit

## Architecture

### Components

- **coins-crypto**: BLS signature primitives
- **coins-types**: Core protocol data structures (Transaction, Account, SubBlock)
- **coins-core**: State management and transaction validation with BLS signature verification
- **coins-indexer**: Chain indexing with 6-block finality tracking
- **coins-aggregator**: Sub-block aggregation service using Bitcoin Core RPC (regtest only)
- **coins-subchain**: Pre-signed UTXO chain generation and management
- **coins-client**: User-facing wallet CLI

### How It Works

1. **Users** create 41-byte transactions and sign them with BLS signatures
2. **Aggregator** collects transactions, aggregates signatures into sub-blocks
3. **Sub-blocks** are compressed and published to Bitcoin via OP_RETURN in anchor transactions
4. **Validators** verify aggregate signatures and update account state
5. **Finality** is achieved after 6 Bitcoin block confirmations (~1 hour)

## Quick Start (Regtest)

### Prerequisites

- Rust toolchain (2024 edition)
- Bitcoin Core (for regtest)

### 1. Start Bitcoin Core (Regtest)

```bash
# Start Bitcoin Core in regtest mode
bitcoind -regtest -daemon -rpcuser=user -rpcpassword=pass

# Create a wallet
bitcoin-cli -regtest -rpcuser=user -rpcpassword=pass createwallet "test"

# Generate some blocks and get an address for mining
ADDR=$(bitcoin-cli -regtest -rpcuser=user -rpcpassword=pass getnewaddress)
bitcoin-cli -regtest -rpcuser=user -rpcpassword=pass generatetoaddress 101 $ADDR
```

### 2. Generate Subchain

```bash
# Build the subchain setup tool
cargo build --release --bin subchain-setup

# Generate subchain (10,000 pre-signed transactions)
./target/release/subchain-setup \
    --count 10000 \
    --network regtest \
    --output subchain_regtest.bin

# This creates subchain_regtest.bin (~795 KB)
```

### 3. Configure Aggregator

Copy the regtest configuration templates:

```bash
cp config/aggregator-regtest.toml config/aggregator.toml
cp config/subchain-regtest.toml config/subchain.toml
```

The configuration will use these paths:
- Subchain: `.data/subchains/subchain_regtest.bin`
- Keys: `.data/keys/aggregator_sk.hex`
- Databases: `.data/db/state.db/` and `.data/db/indexer.db/`


### 4. Run Aggregator

```bash
# Build and run the aggregator (uses config/aggregator.toml by default)
cargo run --bin coins-aggregator

# Or specify a different config:
cargo run --bin coins-aggregator -- --config config/aggregator.toml

# The aggregator will:
# - Initialize persistent state (.data/db/state.db/)
# - Initialize indexer (.data/db/indexer.db/)
# - Start HTTP API on http://localhost:8080
# - Begin mining sub-blocks every 30 seconds
```

### 5. Use the Client

```bash
# Initialize your wallet (generates BLS keypair)
cargo run --bin coins-client init

# This creates client_sk.hex with your secret key
# Your public key is displayed - fund this account first!

# Check your balance
cargo run --bin coins-client balance

# Send tokens
cargo run --bin coins-client send \
    --recipient-pk <recipient-hex-public-key> \
    --amount 100
```

## API Endpoints

The aggregator exposes a REST API on port 8080:

### `GET /account/:pk`

Query account balance and nonce by public key (hex-encoded).

**Example:**
```bash
curl http://localhost:8080/account/<32-byte-hex-pk>
```

**Response:**
```json
{
  "id": 0,
  "pk": "...",
  "balance": 1000000,
  "nonce": 5
}
```

### `POST /tx`

Submit a signed transaction to the mempool.

**Body:**
```json
{
  "tx": "<41-byte-transaction-hex>",
  "sig": "<64-byte-signature-hex>"
}
```

## Transaction Format

Transactions are 41 bytes:

| Field | Size | Description |
|-------|------|-------------|
| sender_id | 4 bytes | Sender account ID |
| recipient_pk | 32 bytes | Recipient public key (G1) |
| amount | 4 bytes | Amount to transfer |
| fee | 1 byte | Transaction fee |

Message to sign: `tx_bytes || nonce` (45 bytes total)


## Testing

```bash
# Run all unit tests
cargo test

# Run specific crate tests
cargo test -p coins-crypto
cargo test -p coins-core
cargo test -p coins-indexer

# Run with logging
RUST_LOG=debug cargo test
```

## Logging

The aggregator uses structured logging via `tracing`:

```bash
# Info level (default)
cargo run --bin coins-aggregator

# Debug level
RUST_LOG=coins_aggregator=debug cargo run --bin coins-aggregator

# Trace level for all coins crates
RUST_LOG=coins=trace cargo run --bin coins-aggregator
```

## Generated Files

The aggregator creates files in the `.data/` directory:

```
.data/
├── keys/
│   ├── aggregator_sk.hex      # Bitcoin ECDSA secret key (fee payments)
│   ├── aggregator_bls_sk.hex  # BLS secret key (sub-block signing)
│   └── client_sk.hex          # Client wallet key
├── subchains/
│   └── subchain_regtest.bin # Pre-signed anchor transactions
└── db/
    ├── state.db/              # Persistent account state (sled database)
    └── indexer.db/            # Indexed sub-blocks with finality tracking
```

All files in `.data/` are gitignored and created automatically when needed.

## Security Warnings

⚠️ **THIS IS DEMO-QUALITY CODE - NOT PRODUCTION READY**

**Known limitations:**
1. **Hash-to-curve**: Uses simplified Blake2s approach, NOT RFC 9380 compliant
2. **No subgroup checks**: Curve points not explicitly validated
3. **No DoS protection**: Mempool and API have no rate limiting
4. **Simplified key management**: Keys stored in plain hex files
5. **No network p2p**: Single aggregator, no peer discovery
6. **Limited reorg handling**: Basic reorganization detection only

**DO NOT use this code:**
- On Bitcoin mainnet
- With real funds
- In production environments
- Without comprehensive security audit

## Configuration Files

Configuration templates are located in the `config/` directory:

- **`config/aggregator-regtest.toml`** - Aggregator configuration for regtest (Bitcoin RPC)
- **`config/subchain-regtest.toml`** - Subchain generation configuration for regtest

### Quick Setup

```bash
# Copy regtest configuration templates
cp config/aggregator-regtest.toml config/aggregator.toml
cp config/subchain-regtest.toml config/subchain.toml
```

### Configuration Options

The aggregator requires Bitcoin Core RPC configuration:
- `rpc_url` - Bitcoin Core RPC endpoint (e.g., "http://127.0.0.1:18443")
- `rpc_user` - RPC username
- `rpc_pass` - RPC password
- `network` - Must be "regtest"

See `config/README.md` for detailed documentation.

## Troubleshooting

### "Subchain exhausted"

The pre-signed transaction chain has been fully used. Generate a new subchain with more transactions (`--count` parameter).

### "No fee UTXOs available"

The aggregator's Bitcoin wallet has no confirmed UTXOs. Send Bitcoin to the fee address displayed at startup.

### "Package relay failed"

Bitcoin node doesn't support package relay. Update your Bitcoin node to 0.30 or later.

### State database corruption

Delete `.data/db/` directory to reset. You'll lose all state - only do this on regtest/testnet:

```bash
rm -rf .data/db/
```

## Development

```bash
# Build all crates
cargo build --all

# Build specific binary
cargo build --bin coins-aggregator
cargo build --bin coins-client
cargo build --bin subchain-setup

# Run with specific features
cargo build --release
cargo build --bin coins-aggregator --release

# Check code
cargo clippy --all
cargo fmt --all
```

## Project Structure

```
coins/
├── Cargo.toml                 # Workspace root
├── README.md                  # This file
├── spec.md                    # Technical specification
├── config/                    # Configuration files
│   ├── README.md
│   ├── aggregator.toml        # Active config (gitignored)
│   ├── subchain.toml          # Active config (gitignored)
│   ├── aggregator-regtest.toml  # Template
│   └── subchain-regtest.toml    # Template
├── .data/                     # Generated files (gitignored)
│   ├── keys/                  # Bitcoin & BLS keys
│   ├── subchains/             # Pre-signed chains
│   └── db/                    # Databases
├── test-data/                 # Test fixtures (gitignored)
│   └── keys/
├── crates/
│   ├── coins-crypto/         # BLS signatures (BN-254)
│   ├── coins-types/          # Data structures
│   ├── coins-core/           # State management & validation
│   ├── coins-indexer/        # Chain indexing & finality
│   ├── coins-aggregator/     # Aggregator service (RPC backend)
│   ├── coins-subchain/       # Subchain setup
│   └── coins-client/         # Wallet CLI
└── target/                    # Build outputs
```

## Contributing

This is demo code for educational purposes. Contributions are welcome but please note this is not intended for production use.
