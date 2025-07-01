use coins_crypto::{SecretKey, aggregate, sign};
use coins_state::State;
use coins_types::{Transaction, SubBlock};
use coins_validator::validate_subblock;
use tempfile::tempdir;

#[test]
fn validate_single_tx() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // accounts
    let sender_sk = SecretKey::random();
    let sender_acct = state.create_account(sender_sk.public_key()).unwrap();
    let recipient_sk = SecretKey::random();
    let recipient_acct = state.create_account(recipient_sk.public_key()).unwrap();
    let agg_sk = SecretKey::random();
    let _agg_acct = state.create_account(agg_sk.public_key()).unwrap();

    // fund sender
    let mut rich = sender_acct.clone();
    rich.balance = 500;
    state.insert_account(&rich).unwrap();

    // build tx
    let tx = Transaction {
        sender_id: sender_acct.id.0,
        recipient_pk: recipient_acct.pk,
        amount: 100,
        fee: 2,
    };
    let msg = tx.message_to_sign(sender_acct.nonce);
    let sig = sign(&sender_sk, &msg);
    let sigma = aggregate([&sig]);

    let sb = SubBlock { sigma, aggregator_pk: agg_sk.public_key(), txs: vec![tx] };
    validate_subblock(&sb, &state).expect("validate");

    // verify balances
    let sender_new = state.get_account(sender_acct.id).unwrap().unwrap();
    assert_eq!(sender_new.balance, 398);
    assert_eq!(sender_new.nonce, 1);
    let recipient_new = state.get_account(recipient_acct.id).unwrap().unwrap();
    assert_eq!(recipient_new.balance, 100);
    let agg_new = state.get_by_pk(&agg_sk.public_key()).unwrap().unwrap();
    assert_eq!(agg_new.balance, 2);
}

// insufficient balance should fail
#[test]
fn validate_insufficient_balance() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    let sender_sk = SecretKey::random();
    let sender = state.create_account(sender_sk.public_key()).unwrap();
    // sender balance left at 0
    let recipient_sk = SecretKey::random();
    let recipient = state.create_account(recipient_sk.public_key()).unwrap();

    let tx = Transaction {
        sender_id: sender.id.0,
        recipient_pk: recipient.pk,
        amount: 1,
        fee: 1,
    };
    let msg = tx.message_to_sign(0);
    let sig = sign(&sender_sk, &msg);
    let sigma = aggregate([&sig]);
    let sb = SubBlock { sigma, aggregator_pk: recipient.pk, txs: vec![tx] };

    let err = validate_subblock(&sb, &state).expect_err("should fail");
    assert!(matches!(err, coins_validator::ValidationError::Balance));
}

// bad signature should fail
#[test]
fn validate_bad_signature() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    let sender_sk = SecretKey::random();
    let sender = state.create_account(sender_sk.public_key()).unwrap();
    // fund sender
    let mut rich = sender.clone();
    rich.balance = 10;
    state.insert_account(&rich).unwrap();

    let recipient_sk = SecretKey::random();
    let recipient = state.create_account(recipient_sk.public_key()).unwrap();

    let tx = Transaction { sender_id: sender.id.0, recipient_pk: recipient.pk, amount: 5, fee: 1 };
    // sign wrong message (nonce 1 instead of 0) to make signature invalid
    let wrong_msg = tx.message_to_sign(1);
    let sig = sign(&sender_sk, &wrong_msg);
    let sigma = aggregate([&sig]);
    let sb = SubBlock { sigma, aggregator_pk: recipient.pk, txs: vec![tx] };

    let err = validate_subblock(&sb, &state).expect_err("bad sig should fail");
    assert!(matches!(err, coins_validator::ValidationError::BadSignature));
} 