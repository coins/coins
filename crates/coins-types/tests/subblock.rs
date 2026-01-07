use coins_types::{Transaction, SubBlock, TX_SIZE, SubBlockState};
use coins_crypto::{SecretKey, G1, G2};
use std::collections::HashMap;

/// Mock state for testing - no accounts exist, so all TXs will be canonical
struct MockState {
    accounts: HashMap<G1, u32>,
}

impl SubBlockState for MockState {
    fn get_account_id_by_pk(&self, pk: &G1) -> Result<Option<u32>, ()> {
        Ok(self.accounts.get(pk).copied())
    }

    fn get_pk_by_account_id(&self, id: u32) -> Option<G1> {
        self.accounts.iter()
            .find(|(_, account_id)| **account_id == id)
            .map(|(pk, _)| *pk)
    }
}

#[test]
fn subblock_roundtrip_no_compression() {
    // Mock state with no accounts (all TXs will be canonical)
    let state = MockState {
        accounts: HashMap::new(),
    };

    // build transactions
    let mut txs = Vec::new();
    for i in 0..5u32 {
        let sk = SecretKey::random();
        txs.push(Transaction {
            sender_id: i,
            recipient_pk: G1(sk.public_key()),
            amount: 100 + i,
            fee: i as u8,
        });
    }
    // dummy sigma (all-zero 64 bytes for test)
    let sigma = G2([0u8; 64]);
    let pub_sk = SecretKey::random();
    let sb = SubBlock { sigma, publisher_pk: G1(pub_sk.public_key()), txs: txs.clone() };

    let bytes = sb.serialize(&state);

    // size check with new format: 64 (sigma) + 32 (pk) + 2 (count) + 1 (format_bits) + n*41
    // For 5 TXs: ceil(5/8) = 1 byte for format bits
    // Header: 98 bytes (publisher NOT compressed to avoid bootstrapping issues)
    assert_eq!(bytes.len(), 98 + 1 + txs.len()*TX_SIZE);

    let parsed = SubBlock::deserialize(&bytes, &state).expect("parse");
    assert_eq!(parsed.txs.len(), txs.len());
    assert_eq!(parsed.publisher_pk.0, sb.publisher_pk.0);
    for (orig, parsed_tx) in txs.iter().zip(parsed.txs.iter()) {
        assert_eq!(orig.sender_id, parsed_tx.sender_id);
        assert_eq!(orig.amount, parsed_tx.amount);
    }
}

#[test]
fn subblock_roundtrip_with_compression() {
    // Create transactions and register recipients in mock state
    let mut txs = Vec::new();
    let mut accounts = HashMap::new();

    for i in 0..5u32 {
        let sk = SecretKey::random();
        let recipient_pk = G1(sk.public_key());

        // Register recipients in state (so they can be compressed)
        accounts.insert(recipient_pk, 100 + i);

        txs.push(Transaction {
            sender_id: i,
            recipient_pk,
            amount: 100 + i,
            fee: i as u8,
        });
    }

    let state = MockState { accounts };

    // dummy sigma (all-zero 64 bytes for test)
    let sigma = G2([0u8; 64]);
    let pub_sk = SecretKey::random();
    let sb = SubBlock { sigma, publisher_pk: G1(pub_sk.public_key()), txs: txs.clone() };

    let bytes = sb.serialize(&state);

    // size check with compression: 64 (sigma) + 32 (pk) + 2 (count) + 1 (format_bits) + n*13
    // All TXs should be compressed to 13 bytes each
    assert_eq!(bytes.len(), 98 + 1 + txs.len() * 13);

    let parsed = SubBlock::deserialize(&bytes, &state).expect("parse");
    assert_eq!(parsed.txs.len(), txs.len());
    assert_eq!(parsed.publisher_pk.0, sb.publisher_pk.0);
    for (orig, parsed_tx) in txs.iter().zip(parsed.txs.iter()) {
        assert_eq!(orig.sender_id, parsed_tx.sender_id);
        assert_eq!(orig.recipient_pk.0, parsed_tx.recipient_pk.0);
        assert_eq!(orig.amount, parsed_tx.amount);
        assert_eq!(orig.fee, parsed_tx.fee);
    }
}

#[test]
fn subblock_hybrid_format() {
    // Mix of existing accounts (compressible) and new recipients (not compressible)
    let mut txs = Vec::new();
    let mut accounts = HashMap::new();

    // First 3 TXs: recipients exist (will be compressed)
    for i in 0..3u32 {
        let sk = SecretKey::random();
        let recipient_pk = G1(sk.public_key());
        accounts.insert(recipient_pk, 200 + i);

        txs.push(Transaction {
            sender_id: i,
            recipient_pk,
            amount: 100 + i,
            fee: i as u8,
        });
    }

    // Last 2 TXs: new recipients (will stay canonical)
    for i in 3..5u32 {
        let sk = SecretKey::random();
        txs.push(Transaction {
            sender_id: i,
            recipient_pk: G1(sk.public_key()),
            amount: 100 + i,
            fee: i as u8,
        });
    }

    let state = MockState { accounts };

    let sigma = G2([0u8; 64]);
    let pub_sk = SecretKey::random();
    let sb = SubBlock { sigma, publisher_pk: G1(pub_sk.public_key()), txs: txs.clone() };

    let bytes = sb.serialize(&state);

    // Expected: 98 (header) + 1 (format_bits for 5 TXs) + 3*13 (compact) + 2*41 (canonical)
    assert_eq!(bytes.len(), 98 + 1 + 3 * 13 + 2 * 41);

    let parsed = SubBlock::deserialize(&bytes, &state).expect("parse");
    assert_eq!(parsed.txs.len(), txs.len());
    assert_eq!(parsed.publisher_pk.0, sb.publisher_pk.0);
    for (orig, parsed_tx) in txs.iter().zip(parsed.txs.iter()) {
        assert_eq!(orig.sender_id, parsed_tx.sender_id);
        assert_eq!(orig.recipient_pk.0, parsed_tx.recipient_pk.0);
        assert_eq!(orig.amount, parsed_tx.amount);
        assert_eq!(orig.fee, parsed_tx.fee);
    }
}