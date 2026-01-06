# Coins Protocol

A compact Layer-2 Bitcoin token protocol using BLS signatures and embedded consensus.

## Overview

Coins is an embedded-consensus token system built on Bitcoin that uses:
- **BLS signatures** on the BN-254 curve for compact multi-signature aggregation
- **Space-chain** architecture: pre-signed Bitcoin transaction chains for anchoring
- **6-block finality**: Sub-blocks achieve finality after 6 Bitcoin confirmations
- **Minimal on-chain footprint**: 64-byte aggregate signatures for entire blocks

## Architecture

### Components

- **coins-crypto**: BLS signature primitives (BN-254 curve) - *demo quality*
- **coins-types**: Core protocol data structures (Transaction, Account, SubBlock)
- **coins-state**: Persistent account state using sled database
- **coins-validator**: State transition validation with BLS signature verification
- **coins-indexer**: Chain indexing with 6-block finality tracking
- **coins-aggregator**: Sub-block aggregation service with dual blockchain backend support:
  - **RPC backend**: Bitcoin Core RPC for regtest (fast, local testing)
  - **Esplora backend**: Public Esplora API for signet/mainnet (no node required)
- **coins-spacechain**: Pre-signed UTXO chain generation and management
- **coins-client**: User-facing wallet CLI

### How It Works

1. **Users** create 41-byte transactions and sign them with BLS signatures
2. **Aggregator** collects transactions, aggregates signatures into sub-blocks
3. **Sub-blocks** are inscribed to Bitcoin via pre-signed connector transactions
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

### 2. Generate Spacechain

```bash
# Build the spacechain setup tool
cargo build --release --bin spacechain-setup

# Generate spacechain (10,000 pre-signed transactions)
./target/release/spacechain-setup \
    --count 10000 \
    --network regtest \
    --output spacechain_regtest.bin

# This creates spacechain_regtest.bin (~795 KB)
```

### 3. Configure Aggregator

Copy the regtest configuration templates:

```bash
cp config/aggregator-regtest.toml config/aggregator.toml
cp config/spacechain-regtest.toml config/spacechain.toml
```

The configuration will use these paths:
- Spacechain: `.data/spacechains/spacechain_regtest.bin`
- Keys: `.data/keys/aggregator_sk.hex`
- Databases: `.data/db/state.db/` and `.data/db/indexer.db/`

**Note:** For regtest, the aggregator uses Bitcoin Core RPC directly. For signet/mainnet, use the signet templates (see Configuration Files section).

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

## Finality

Sub-blocks achieve **finality after 6 Bitcoin confirmations**:
- ~1 hour on mainnet
- ~60 seconds on regtest (with fast mining)
- ~10 minutes on testnet

The indexer tracks confirmation counts and only serves finalized data.

## Testing

```bash
# Run all unit tests
cargo test

# Run specific crate tests
cargo test -p coins-crypto
cargo test -p coins-validator
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
├── spacechains/
│   └── spacechain_regtest.bin # Pre-signed connector transactions
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

- **`config/aggregator-regtest.toml`** - Regtest configuration (Bitcoin RPC)
- **`config/aggregator-signet.toml`** - Signet configuration (Esplora API)
- **`config/spacechain-regtest.toml`** - Spacechain for regtest
- **`config/spacechain-signet.toml`** - Spacechain for signet

### Quick Setup

```bash
# For regtest
cp config/aggregator-regtest.toml config/aggregator.toml
cp config/spacechain-regtest.toml config/spacechain.toml

# For signet
cp config/aggregator-signet.toml config/aggregator.toml
cp config/spacechain-signet.toml config/spacechain.toml
```

### Backend Auto-Selection

- `network = "regtest"` → Uses Bitcoin RPC (requires `rpc_url`, `rpc_user`, `rpc_pass`)
- `network = "signet"` or `"bitcoin"` → Uses Esplora (requires `esplora` URL)

See `config/README.md` for detailed documentation.

## Troubleshooting

### "Spacechain exhausted"

The pre-signed transaction chain has been fully used. Generate a new spacechain with more transactions (`--count` parameter).

### "No fee UTXOs available"

The aggregator's Bitcoin wallet has no confirmed UTXOs. Send Bitcoin to the fee address displayed at startup.

### "Package relay failed"

Bitcoin node doesn't support package relay. The aggregator will fall back to individual transaction broadcasts.

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
cargo build --bin spacechain-setup

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
│   ├── spacechain.toml        # Active config (gitignored)
│   ├── aggregator-regtest.toml  # Template
│   ├── aggregator-signet.toml   # Template
│   ├── spacechain-regtest.toml  # Template
│   └── spacechain-signet.toml   # Template
├── .data/                     # Generated files (gitignored)
│   ├── keys/                  # Bitcoin & BLS keys
│   ├── spacechains/           # Pre-signed chains
│   └── db/                    # Databases
├── test-data/                 # Test fixtures (gitignored)
│   └── keys/
├── crates/
│   ├── coins-crypto/         # BLS signatures (BN-254)
│   ├── coins-types/          # Data structures
│   ├── coins-state/          # Persistent state (sled)
│   ├── coins-validator/      # Transaction validation
│   ├── coins-indexer/        # Chain indexing & finality
│   ├── coins-aggregator/     # Aggregator service
│   ├── coins-spacechain/     # Spacechain setup
│   └── coins-client/         # Wallet CLI
└── target/                    # Build outputs
```

## License

[Add your license here]

## Contributing

This is demo code for educational purposes. Contributions are welcome but please note this is not intended for production use.

## References

- [BLS Signatures](https://en.wikipedia.org/wiki/BLS_digital_signature)
- [BN-254 Curve](https://hackmd.io/@jpw/bn254)
- [Bitcoin Inscriptions](https://docs.ordinals.com/inscriptions.html)
- [Embedded Consensus Systems](https://bitcoin.stackexchange.com/questions/109513/what-is-an-embedded-consensus-system)
