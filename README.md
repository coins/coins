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
- **coins-publisher**: Sub-block publishing service using Bitcoin Core RPC (regtest only)
- **coins-subchain**: Pre-signed UTXO chain generation and management
- **coins-client**: User-facing wallet CLI

### How It Works

1. **Users** create 43-byte transactions and sign them with BLS signatures
2. **Publisher** collects transactions, aggregates signatures into sub-blocks
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

### 3. Configure Publisher

Copy the regtest configuration templates:

```bash
cp config/publisher-regtest.toml config/publisher.toml
cp config/subchain-regtest.toml config/subchain.toml
```

The configuration will use these paths:
- Subchain: `.data/subchains/subchain_regtest.bin`
- Keys: `.data/keys/publisher_sk.hex`
- Databases: `.data/regtest/state.db/` and `.data/regtest/indexer.db/`


### 4. Run Publisher

```bash
# Build and run the publisher (uses config/publisher.toml by default)
cargo run --bin coins-publisher

# Or specify a different config:
cargo run --bin coins-publisher -- --config config/publisher.toml

# The publisher will:
# - Initialize persistent state (.data/regtest/state.db/)
# - Initialize indexer (.data/regtest/indexer.db/)
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

The publisher exposes a REST API (default ports: regtest=8080, signet=8081):

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
  "tx": "<43-byte-transaction-hex>",
  "sig": "<64-byte-signature-hex>"
}
```

## Transaction Format

Transactions use a **hybrid format** in sub-blocks to minimize on-chain footprint:

### Canonical Format (43 bytes)
Used when the recipient is **new** (no existing account):

| Field | Size | Description |
|-------|------|-------------|
| sender_id | 4 bytes | Sender account ID |
| recipient_pk | 32 bytes | Recipient public key (G1) |
| token_id | 2 bytes | Token identifier (0 = native) |
| amount | 4 bytes | Amount to transfer |
| fee | 1 byte | Transaction fee |

### Compact Format (13 bytes)
Used when the recipient **already has an account**:

| Field | Size | Description |
|-------|------|-------------|
| sender_id | 4 bytes | Sender account ID |
| recipient_id | 4 bytes | Recipient account ID |
| amount | 4 bytes | Amount to transfer |
| fee | 1 byte | Transaction fee |

The compact format saves 28 bytes per transaction by replacing the 32-byte public key with a 4-byte account ID. Sub-blocks automatically use compact format when possible, with a bitfield indicating which format each transaction uses.

**Signing**: Transactions are always signed in canonical format. Message to sign: `tx_bytes || nonce` (47 bytes total). The nonce comes from the sender's current account state.

### Signature Aggregation and Verification

The compact format only affects transaction *serialization* - signatures are always computed and verified against the canonical 43-byte format.

Clients submit a 43-byte canonical transaction along with its BLS signature to the publisher. The signature covers the transaction bytes concatenated with the sender's current nonce (45 bytes total). The publisher stores transactions and their individual signatures in a mempool until mining.

When mining a sub-block, the publisher aggregates all individual BLS signatures into a single 64-byte aggregate signature and serializes the transactions using compact format where possible. The sub-block (transactions + aggregate signature + publisher public key) is then published to Bitcoin.

Validators deserialize the sub-block, which expands any compact transactions back to canonical format using the recipient's public key from state. For each transaction, the validator reconstructs the 45-byte signing message using the sender's current nonce from state, then verifies the aggregate BLS signature against all (public_key, message) pairs.

### Data Transaction Validity Rule

A data transaction (containing sub-block data) is only valid if it is included in the **same Bitcoin block** as its corresponding anchor transaction. This ensures:

- **Determinism**: All nodes agree on which data transactions count
- **No withheld data attacks**: Publishers cannot hold back a data transaction and publish it later to rewrite history
- **Incentive alignment**: Publishers must use package relay (TRUC) to ensure both transactions are mined together

If a data transaction ends up in a different block than its anchor (due to miner behavior, network issues, etc.), it is ignored. Publishers bear the risk of losing fees if their package is not mined atomically.

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

The publisher uses structured logging via `tracing`:

```bash
# Info level (default)
cargo run --bin coins-publisher

# Debug level
RUST_LOG=coins_publisher=debug cargo run --bin coins-publisher

# Trace level for all coins crates
RUST_LOG=coins=trace cargo run --bin coins-publisher
```

## Generated Files

The publisher creates files in the `.data/` directory:

```
.data/
├── keys/
│   ├── publisher_sk.hex       # Bitcoin ECDSA secret key (fee payments)
│   ├── publisher_bls_sk.hex   # BLS secret key (sub-block signing)
│   └── client_sk.hex          # Client wallet key
├── subchains/
│   └── subchain_regtest.bin   # Pre-signed anchor transactions
├── regtest/                   # Regtest-specific databases
│   ├── state.db/              # Persistent account state (RocksDB)
│   └── indexer.db/            # Indexed sub-blocks with finality tracking
└── signet/                    # Signet-specific databases (when using signet)
    ├── state.db/              # Persistent account state (RocksDB)
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
5. **No network p2p**: Single publisher, no peer discovery
6. **Limited reorg handling**: Basic reorganization detection only

**DO NOT use this code:**
- On Bitcoin mainnet
- With real funds
- In production environments
- Without comprehensive security audit

## Configuration Files

Configuration templates are located in the `config/` directory:

- **`config/publisher-regtest.toml`** - Publisher configuration for regtest (Bitcoin RPC)
- **`config/subchain-regtest.toml`** - Subchain generation configuration for regtest

### Quick Setup

```bash
# Copy regtest configuration templates
cp config/publisher-regtest.toml config/publisher.toml
cp config/subchain-regtest.toml config/subchain.toml
```

### Configuration Options

The publisher requires Bitcoin Core RPC configuration:
- `rpc_url` - Bitcoin Core RPC endpoint (e.g., "http://127.0.0.1:18443")
- `rpc_user` - RPC username
- `rpc_pass` - RPC password
- `network` - Must be "regtest"

See `config/README.md` for detailed documentation.

## Troubleshooting

### "Subchain exhausted"

The pre-signed transaction chain has been fully used. Generate a new subchain with more transactions (`--count` parameter).

### "No fee UTXOs available"

The publisher's Bitcoin wallet has no confirmed UTXOs. Send Bitcoin to the fee address displayed at startup.

### "Package relay failed"

Bitcoin node doesn't support package relay. Update your Bitcoin node to 0.30 or later.

### State database corruption

Delete `.data/regtest/` (or `.data/signet/`) directory to reset. You'll lose all state - only do this on regtest/testnet:

```bash
rm -rf .data/regtest/  # For regtest
# or
rm -rf .data/signet/   # For signet
```

## Development

```bash
# Build all crates
cargo build --all

# Build specific binary
cargo build --bin coins-publisher
cargo build --bin coins-client
cargo build --bin subchain-setup

# Run with specific features
cargo build --release
cargo build --bin coins-publisher --release

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
│   ├── publisher.toml        # Active config (gitignored)
│   ├── subchain.toml          # Active config (gitignored)
│   ├── publisher-regtest.toml  # Template
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
│   ├── coins-publisher/     # Publisher service (RPC backend)
│   ├── coins-subchain/       # Subchain setup
│   └── coins-client/         # Wallet CLI
└── target/                    # Build outputs
```

## Contributing

This is demo code for educational purposes. Contributions are welcome but please note this is not intended for production use.
