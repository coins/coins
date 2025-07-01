# <<WORKING TITLE>>

<<WORKING TITLE>> is an embedded-Consensus Token Protocol on Bitcoin. It's a **compact layer‑2 protcol** that batches 38-byte transactions into “sub‑blocks” stored inside Bitcoin transactions.

---

## 1 Cryptography

* **Curve**BN‑254 .  ~100‑bit security.
* **Groups**G₁ (32 B compressed) and G₂ (64 B compressed) with optimal‑ate pairing `e : G1×G2 → GT`.
* **Signature scheme***Minimal‑public‑key* BLS multi‑signature
  * Hash‑to‑curve: RFC 9380 `hash_to_G2(msg, DST="EC‑TOKEN")`.
  * Per‑tx signature`σᵢ=hash_to_G2(mᵢ)^{xᵢ}∈G₂`, where `xᵢ` is the sender’s secret key.
  * Block aggregate`Σ=Σ_i σᵢ∈G₂` (64 B compressed).
  * Verification`e(G1_gen,Σ)==∏ e(Pᵢ,hash_to_G2(mᵢ))`.
  * **No proof‑of‑possession needed** because every `mᵢ` carries a unique `(sender_id,nonce)` pair.
* **Subgroup/infinity checks**required on every `Pᵢ` and on `Σ`.

---

## 2 State Table

```
Account {
  account_id : u32  // incremental counter uniquely identifying accounts
  public_key : G1   // 32 B compressed
  balance    : u64
  nonce      : u32  // number of outgoing tx processed
}
```

Key‑value map `public_key → (balance, nonce)`.

---

## 3 Transaction (wire‑format)

```
+--------------+------+--------------------------------+
| field        | size | notes                          |
+--------------+------+--------------------------------+
| sender_id    | 4 B  | little‑endian index            |
| recipient_pk | 32 B | compressed G1 point            |
| amount       | 4 B  | unsigned                       |
| fee          | 1 B  | paid to aggregator in‑token    |
+--------------+------+--------------------------------+
```

*There is **no nonce field** in the wire format.*

### Message to sign

```
m = sender_id || recipient_pk || amount || fee || nonce
```

`nonce` is read from state at signing/verification time.

---

## 4 Sub‑block format

```
SubBlock {
  Σ          : 64 B           // aggregated G₂ signature
  txs        : tx_count × Transaction
}
```

The tx_count is implicit, because the TX size is static and the size of the SubBlock is known.
Entire SubBlock goes into a Taproot witness/inscription blob.

---

## 5 Anchoring & consensus

1. A pre‑signed chained‑UTXO sequence on Bitcoin hands “next block rights” to whoever first spends the current UTXO.
2. Aggregator builds a SubBlock, pays the BTC fee, publishes.
3. Canonical order = Bitcoin’s longest‑chain ordering.  Finality after **6 BTC blocks**.

---

## 6 Validation loop

```rust
GT lhs = 1;
for tx in block.txs {
    Account acct = state[tx.sender_id];               // must exist
    assert acct.balance >= tx.amount + tx.fee;

    Msg m = concat(tx.sender_id, tx.recipient_pk,
                   tx.amount, tx.fee, acct.nonce);
    lhs *= e(acct.public_key_G1, hash_to_G2(m));

    staged[acct].balance  -= tx.amount + tx.fee;
    staged[acct].nonce    += 1;
    staged[tx.recipient_pk].balance += tx.amount;
}
assert e(G1_GENERATOR, block.Sigma_G2) == lhs;        // one final pairing
apply staged updates;
```

Runs in O(#tx) MS plus **1 pairing** for the aggregate.

---

## 7 Wallet workflow

1. Query an indexer for `public_key → (sender_id, balance, nonce)`.
2. Build `m`, sign with BN‑254 key, send `(Transaction, σ)` to aggregators.

---


