use coins_crypto::{SecretKey, G1, sign, aggregate};
use coins_state::State;
use coins_types::{SubBlock, Transaction};
use coins_validator::validate_subblock;
use tempfile::tempdir;

#[test]
fn test_aggregator_receives_fees() {
    // Setup
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // Create aggregator
    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Create sender
    let sender_sk = SecretKey::random();
    let sender_pk = G1(sender_sk.public_key());
    let sender_account = state.create_account(sender_pk).unwrap();

    // Give sender some balance
    let mut sender = sender_account.clone();
    sender.balance = 1000;
    state.insert_account(&sender).unwrap();

    // Create recipient
    let recipient_sk = SecretKey::random();
    let recipient_pk = G1(recipient_sk.public_key());

    // Create transaction with fee
    let tx = Transaction {
        sender_id: sender_account.id.0,
        recipient_pk,
        amount: 100,
        fee: 10, // 10 sat fee
    };

    // Sign transaction
    let msg = tx.message_to_sign(sender_account.nonce);
    let sig = sign(&sender_sk, &msg);

    // Create sub-block
    let sub_block = SubBlock {
        txs: vec![tx],
        sigma: aggregate([&sig]),
        aggregator_pk: agg_pk, // This should receive the fees
    };

    // Validate sub-block (this should credit fees to aggregator)
    validate_subblock(&sub_block, &state).expect("validation should succeed");

    // Verify aggregator received the fees
    let agg_account = state.get_by_pk(&agg_pk).unwrap().expect("aggregator account should exist");
    assert_eq!(agg_account.balance, 10, "aggregator should have received 10 sat fee");

    // Verify sender was debited
    let sender_after = state.get_account(sender_account.id).unwrap().unwrap();
    assert_eq!(sender_after.balance, 1000 - 100 - 10, "sender should be debited amount + fee");

    // Verify recipient was credited
    let recipient_account = state.get_by_pk(&recipient_pk).unwrap().expect("recipient should exist");
    assert_eq!(recipient_account.balance, 100, "recipient should receive 100 sat");
}

#[test]
fn test_aggregator_accumulates_fees_from_multiple_txs() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // Create aggregator
    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Create two senders
    let sender1_sk = SecretKey::random();
    let sender1_pk = G1(sender1_sk.public_key());
    let sender1_account = state.create_account(sender1_pk).unwrap();
    let mut s1 = sender1_account.clone();
    s1.balance = 1000;
    state.insert_account(&s1).unwrap();

    let sender2_sk = SecretKey::random();
    let sender2_pk = G1(sender2_sk.public_key());
    let sender2_account = state.create_account(sender2_pk).unwrap();
    let mut s2 = sender2_account.clone();
    s2.balance = 2000;
    state.insert_account(&s2).unwrap();

    // Create recipients
    let recipient1_pk = G1(SecretKey::random().public_key());
    let recipient2_pk = G1(SecretKey::random().public_key());

    // Create transactions with different fees
    let tx1 = Transaction {
        sender_id: sender1_account.id.0,
        recipient_pk: recipient1_pk,
        amount: 100,
        fee: 5,
    };

    let tx2 = Transaction {
        sender_id: sender2_account.id.0,
        recipient_pk: recipient2_pk,
        amount: 200,
        fee: 15,
    };

    // Sign transactions
    let sig1 = sign(&sender1_sk, &tx1.message_to_sign(sender1_account.nonce));
    let sig2 = sign(&sender2_sk, &tx2.message_to_sign(sender2_account.nonce));

    // Create sub-block with both transactions
    let sub_block = SubBlock {
        txs: vec![tx1, tx2],
        sigma: aggregate([&sig1, &sig2]),
        aggregator_pk: agg_pk,
    };

    // Validate
    validate_subblock(&sub_block, &state).expect("validation should succeed");

    // Verify aggregator received total fees (5 + 15 = 20)
    let agg_account = state.get_by_pk(&agg_pk).unwrap().expect("aggregator account should exist");
    assert_eq!(agg_account.balance, 20, "aggregator should have received 20 sat in total fees");
}
