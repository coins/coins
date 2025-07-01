use coins_types::{Transaction, TX_SIZE};
use coins_crypto::SecretKey;

#[test]
fn wire_roundtrip() {
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let tx = Transaction {
        sender_id: 42,
        recipient_pk: pk,
        amount: 1_234_567,
        fee: 10,
    };
    let bytes = tx.serialize();
    assert_eq!(bytes.len(), TX_SIZE);
    let parsed = Transaction::deserialize(&bytes);
    assert_eq!(parsed.sender_id, tx.sender_id);
    assert_eq!(parsed.recipient_pk.0, tx.recipient_pk.0);
    assert_eq!(parsed.amount, tx.amount);
    assert_eq!(parsed.fee, tx.fee);
}

#[test]
fn message_to_sign_layout() {
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let tx = Transaction {
        sender_id: 1,
        recipient_pk: pk,
        amount: 2,
        fee: 3,
    };
    let nonce = 99u32;
    let msg = tx.message_to_sign(nonce);
    assert_eq!(msg.len(), 45);
    let expected = {
        let mut v = Vec::new();
        v.extend_from_slice(&tx.serialize());
        v.extend_from_slice(&nonce.to_le_bytes());
        v
    };
    assert_eq!(msg, expected);
} 