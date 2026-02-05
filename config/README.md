# Configuration Templates

This directory contains configuration templates for different Bitcoin networks.

## Quick Start

### For Regtest (Local Testing)

```bash
# 1. Copy regtest templates
cp config/publisher-regtest.toml config/publisher.toml
cp config/subchain-regtest.toml config/subchain.toml
cp config/client-regtest.toml config/client.toml

# 2. Generate subchain
cargo run --bin subchain-setup

# 3. Start publisher
cargo run --bin coins-publisher

# 4. Initialize client
cargo run --bin coins-client init

# 5. Use client
cargo run --bin coins-client balance
```

### For Signet (Public Testnet)

```bash
# 1. Copy signet templates
cp config/publisher-signet.toml config/publisher.toml
cp config/subchain-signet.toml config/subchain.toml
cp config/client-signet.toml config/client.toml

# 2. Edit config/publisher.toml and set your genesis_pk
# 3. Edit config/client.toml and set your publisher_url

# 4. Generate subchain
cargo run --bin subchain-setup

# 5. Start publisher
cargo run --bin coins-publisher

# 6. Initialize client
cargo run --bin coins-client init

# 7. Use client
cargo run --bin coins-client balance
```

## Configuration Files

### Publisher Configurations

- **`publisher-regtest.toml`** - Bitcoin RPC backend for local regtest
  - Uses Bitcoin Core RPC directly
  - Requires running `bitcoind -regtest`
  - Fast, local testing

### Subchain Configurations

- **`subchain-regtest.toml`** - Generate subchain for regtest

### Client Configurations

- **`client-default.toml`** - General template with default values
- **`client-regtest.toml`** - Client config for local regtest

## Backend

The publisher uses Bitcoin Core RPC backend for regtest:

- `network = "regtest"` → **RPC Backend** (requires `rpc_url`, `rpc_user`, `rpc_pass`)

**Note:** Only regtest is currently supported. Signet and mainnet support have been removed.

## File Paths

All generated files use the new `.data/` directory structure:

```
.data/
├── keys/
│   ├── publisher_sk.hex      # Bitcoin ECDSA key (fee payments)
│   ├── publisher_bls_sk.hex  # BLS key (sub-block signing)
│   ├── client_sk.hex          # Default client wallet key
│   ├── alice_sk.hex           # Example: Alice's client key
│   └── bob_sk.hex             # Example: Bob's client key
├── subchains/
│   ├── subchain_regtest.bin # Regtest subchain
│   └── subchain_signet.bin  # Signet subchain
└── db/
    ├── state.db/              # Account state database
    └── indexer.db/            # Indexed blocks database
```

## Running Multiple Clients in Parallel

The client supports running multiple instances simultaneously using different configs or CLI overrides:

### Using Config Files

```bash
# Create separate config files for each client
cp config/client-regtest.toml config/client-alice.toml
cp config/client-regtest.toml config/client-bob.toml

# Edit each config to use different keyfiles
# client-alice.toml: keyfile = ".data/keys/alice_sk.hex"
# client-bob.toml: keyfile = ".data/keys/bob_sk.hex"

# Run clients in parallel (different terminals)
cargo run --bin coins-client --config config/client-alice.toml balance
cargo run --bin coins-client --config config/client-bob.toml balance
```

### Using CLI Overrides

```bash
# Override keyfile without config file
cargo run --bin coins-client --keyfile .data/keys/alice_sk.hex init
cargo run --bin coins-client --keyfile .data/keys/bob_sk.hex init

# Override publisher URL to test against different servers
cargo run --bin coins-client --publisher-url http://localhost:8081 balance

# Combine overrides
cargo run --bin coins-client \
  --keyfile .data/keys/alice_sk.hex \
  --publisher-url http://signet.example.com:8080 \
  send --recipient abc123... --amount 100
```

### Priority Order

Configuration values are applied in this order (later overrides earlier):

1. Hardcoded defaults (`http://127.0.0.1:8080`, `.data/keys/client_sk.hex`)
2. Config file values (if `--config` file exists)
3. CLI flag overrides (`--keyfile`, `--publisher-url`)

## Important Notes

- **Never commit `publisher.toml`, `subchain.toml`, or `client.toml`** to git (they are gitignored)
- **Never commit keys or database files** (in `.data/` - gitignored)
- Templates are safe to commit and share
- Production configurations should use different genesis keys and higher security
