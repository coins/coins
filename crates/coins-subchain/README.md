# Coins Spacechain – Trusted Setup

This crate ships an executable that performs the **one-time trusted setup** for the Coins space-chain: it

1.  Generates a fresh one-time keypair.
2.  Displays the Bech32 **address** that must receive the setup funds.
3.  Waits for you to paste the **funding out-point** once the transaction is confirmed.
4.  Builds the *connector transaction* chain and writes it to a compact binary file.

> ⚠️  Run this program **once** and **offline**. The one-time secret key lives only in RAM during
> execution and is automatically dropped when the program terminates. Re-running the setup would
> create a different space-chain and invalidate the previous one.

---

## Building the tool

```bash
# from the workspace root
cargo build -p coins-spacechain --bin spacechain-setup --release
```

The resulting binary is at

```
./target/release/spacechain-setup
```


---

## Running the setup

Example for a regtest deployment that creates **1 000 000** connector transactions carrying the
minimum relayable amount (546 sat) each:

```bash
./target/release/spacechain-setup \
  --count 1000000 \
  --value 546 \
  --network regtest \
  --output spacechain.bin
```

### Step-by-step interaction

1. **Key generation**  – the tool prints something like:

   ```text
   Generated one-time address: bcrt1qxy…
   ```

   *(The secret key is kept in-memory only and not displayed for security reasons.)*

2. **Fund the address**  with the required amount and wait until the funding transaction is in a
   block.
3. **Provide the out-point**  – paste `<txid>:<vout>` when prompted:

   ```text
   Send the required funds to bcrt1qxy… then enter the funding outpoint <txid>:<vout>:
   > 1a2b…:0
   ```

4. **Connector chain generation** – the program calculates signatures and stores the result in the
   specified file.

   ```text
   Building spacechain…
   Wrote 1000000 connectorTxs (75.00 MB) to spacechain.bin
   ```

---

## Output format

`spacechain.bin` is the compact **bincode** representation of the `Spacechain` struct (with
fixed-integer encoding). It is read by `Spacechain::decode()` and can reconstruct the full connector
transactions on demand.

---

## Troubleshooting

* **Wrong network** – make sure you pass the same `--network` throughout.
* **Invalid out-point** – double-check the txid and vout index.
* **File too large** – lower `--count` or `--value`.

For any issues open a ticket in the repository.

---

## Verifying file integrity

Because `spacechain.bin` is deterministic for a given funding out-point, its
authenticity can be verified by comparing its SHA-256 hash across machines.

Generate the hash on Unix-like systems:

```bash
sha256sum spacechain.bin   # Linux
# or
shasum -a 256 spacechain.bin   # macOS / BSD
# or with OpenSSL
openssl dgst -sha256 spacechain.bin
```

