# Configuration Templates

This directory contains configuration templates for different Bitcoin networks.

## Quick Start

### For Regtest (Local Testing)

```bash
# 1. Copy regtest templates
cp config/aggregator-regtest.toml config/aggregator.toml
cp config/spacechain-regtest.toml config/spacechain.toml

# 2. Generate spacechain
cargo run --bin spacechain-setup

# 3. Start aggregator
cargo run --bin coins-aggregator
```

### For Signet (Public Testnet)

```bash
# 1. Copy signet templates
cp config/aggregator-signet.toml config/aggregator.toml
cp config/spacechain-signet.toml config/spacechain.toml

# 2. Edit config/aggregator.toml and set your genesis_pk

# 3. Generate spacechain
cargo run --bin spacechain-setup

# 4. Start aggregator
cargo run --bin coins-aggregator
```

## Configuration Files

### Aggregator Configurations

- **`aggregator-regtest.toml`** - Bitcoin RPC backend for local regtest
  - Uses Bitcoin Core RPC directly
  - Requires running `bitcoind -regtest`
  - Fast, local testing

- **`aggregator-signet.toml`** - Esplora backend for signet testnet
  - Uses public Esplora API
  - No Bitcoin node required
  - Good for public testing

### Spacechain Configurations

- **`spacechain-regtest.toml`** - Generate spacechain for regtest
- **`spacechain-signet.toml`** - Generate spacechain for signet

## Backend Auto-Selection

The aggregator automatically selects the blockchain backend based on the `network` parameter:

- `network = "regtest"` → **RPC Backend** (requires `rpc_url`, `rpc_user`, `rpc_pass`)
- `network = "signet"` → **Esplora Backend** (requires `esplora` URL)
- `network = "bitcoin"` → **Esplora Backend** (requires `esplora` URL)

## File Paths

All generated files use the new `.data/` directory structure:

```
.data/
├── keys/
│   ├── aggregator_sk.hex      # Bitcoin ECDSA key (fee payments)
│   ├── aggregator_bls_sk.hex  # BLS key (sub-block signing)
│   └── client_sk.hex          # Client wallet key
├── spacechains/
│   ├── spacechain_regtest.bin # Regtest spacechain
│   └── spacechain_signet.bin  # Signet spacechain
└── db/
    ├── state.db/              # Account state database
    └── indexer.db/            # Indexed blocks database
```

## Important Notes

- **Never commit `aggregator.toml` or `spacechain.toml`** to git (they are gitignored)
- **Never commit keys or database files** (in `.data/` - gitignored)
- Templates are safe to commit and share
- Production configurations should use different genesis keys and higher security
