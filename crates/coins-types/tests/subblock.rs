use coins_types::{Transaction, SubBlock, TX_SIZE};
use coins_crypto::{SecretKey, G2};

#[test]
fn subblock_roundtrip() {
    // build transactions
    let mut txs = Vec::new();
    for i in 0..5u32 {
        let sk = SecretKey::random();
        txs.push(Transaction {
            sender_id: i,
            recipient_pk: sk.public_key(),
            amount: 100 + i,
            fee: i as u8,
        });
    }
    // dummy sigma (all-zero 64 bytes for test)
    let sigma = G2([0u8; 64]);
    let agg_sk = SecretKey::random();
    let sb = SubBlock { sigma, aggregator_pk: agg_sk.public_key(), txs: txs.clone() };
    let bytes = sb.serialize();
    // size check: 64 +32+ n*41
    assert_eq!(bytes.len(), 96 + txs.len()*TX_SIZE);
    let parsed = SubBlock::deserialize(&bytes).expect("parse");
    assert_eq!(parsed.txs.len(), txs.len());
    for (orig, parsed_tx) in txs.iter().zip(parsed.txs.iter()) {
        assert_eq!(orig.sender_id, parsed_tx.sender_id);
        assert_eq!(orig.amount, parsed_tx.amount);
    }
} 