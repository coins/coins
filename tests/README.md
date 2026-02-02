# Test Scripts

Quick reference for integration testing scripts. See [TESTING.md](../TESTING.md) for detailed usage instructions.

## Scripts Overview

| Script | Network | Purpose |
|--------|---------|---------|
| `setup-regtest.sh` | Regtest | Setup local Bitcoin test environment with publisher and indexer |
| `test-regtest.sh` | Regtest | Run 7 integration tests |
| `stop-regtest.sh` | Regtest | Stop all regtest services |
| `setup-mutinynet.sh` | Mutinynet | Setup remote Mutinynet environment (requires manual funding) |
| `test-mutinynet.sh` | Mutinynet | Run integration tests on Mutinynet |
| `stop-mutinynet.sh` | Mutinynet | Stop all Mutinynet services |

## Quick Start

```bash
# Regtest (local testing)
./scripts/setup-regtest.sh && ./scripts/test-regtest.sh

# Mutinynet (remote testing)
./scripts/setup-mutinynet.sh && ./scripts/test-mutinynet.sh
```

## Documentation

For detailed information see [TESTING.md](../TESTING.md) including:
- Prerequisites and setup details
- Configuration options
- Directory structure
- Troubleshooting guide
