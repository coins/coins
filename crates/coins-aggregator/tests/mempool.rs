use tempfile::TempDir;
use std::sync::Arc;
use coins_core::State;
use coins_aggregator::mempool::{Mempool, MempoolError};
use coins_crypto::{SecretKey, G1};
use coins_types::{Transaction};

#[tokio::test]
async fn valid_tx_gets_enqueued() {
    // temp db
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());

    // create sender account
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let mut sender = state.create_account(G1(pk)).unwrap();
    sender.balance = 1_000;
    state.insert_account(&sender).unwrap();

    // recipient
    let r_sk = SecretKey::random();
    let r_pk = r_sk.public_key();

    let tx = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 100, fee: 1 };

    let mp = Mempool::new(state.clone());
    assert!(mp.validate_and_enqueue(tx.clone()).await.is_ok());
    assert_eq!(mp.pop_batch(10).await.len(), 1);

    // send second, different tx – should be accepted with updated nonce/balance
    let tx2 = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 200, fee: 1 };
    assert!(mp.validate_and_enqueue(tx2.clone()).await.is_ok());

    // duplicate of tx2 should be rejected
    let dup = tx2.clone();
    let err = mp.validate_and_enqueue(dup).await.err().unwrap();
    assert!(matches!(err, MempoolError::Duplicate));
}

// --- additional tests ---------------------------------------------------------

#[tokio::test]
async fn unknown_sender_rejected() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());
    let mp = Mempool::new(state);

    // build tx from non-existent sender id 999
    let r_pk = SecretKey::random().public_key();
    let tx = Transaction { sender_id: 999, recipient_pk: G1(r_pk), amount: 1, fee: 1 };

    let err = mp.validate_and_enqueue(tx).await.err().unwrap();
    assert!(matches!(err, MempoolError::UnknownSender));
}

#[tokio::test]
async fn insufficient_balance_rejected() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());

    // create sender with tiny balance
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let mut acct = state.create_account(G1(pk)).unwrap();
    acct.balance = 10; // 10 sats
    state.insert_account(&acct).unwrap();

    let mp = Mempool::new(state);
    let r_pk = SecretKey::random().public_key();
    let tx = Transaction { sender_id: acct.id.0, recipient_pk: G1(r_pk), amount: 20, fee: 1 };

    let err = mp.validate_and_enqueue(tx).await.err().unwrap();
    assert!(matches!(err, MempoolError::InsufficientBalance));
}

#[tokio::test]
async fn preserves_insertion_order() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());

    // rich sender
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let mut acct = state.create_account(G1(pk)).unwrap();
    acct.balance = 1_000;
    state.insert_account(&acct).unwrap();

    let mp = Mempool::new(state.clone());
    let r_pk = SecretKey::random().public_key();
    let tx1 = Transaction { sender_id: acct.id.0, recipient_pk: G1(r_pk), amount: 10, fee: 1 };
    let tx2 = Transaction { sender_id: acct.id.0, recipient_pk: G1(r_pk), amount: 20, fee: 1 };

    mp.validate_and_enqueue(tx1.clone()).await.unwrap();
    mp.validate_and_enqueue(tx2.clone()).await.unwrap();

    let batch = mp.pop_batch(10).await;
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].amount, 10);
    assert_eq!(batch[1].amount, 20);
}

#[tokio::test]
async fn nonce_is_incremented() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());

    // create sender with sufficient balance
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let mut sender = state.create_account(G1(pk)).unwrap();
    sender.balance = 1_000;
    state.insert_account(&sender).unwrap();

    let mp = Mempool::new(state.clone());
    let r_pk = SecretKey::random().public_key();

    // first tx
    let tx1 = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 50, fee: 1 };
    mp.validate_and_enqueue(tx1).await.unwrap();
    assert_eq!(mp.nonce_of(sender.id).await, Some(1));

    // second tx
    let tx2 = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 70, fee: 1 };
    mp.validate_and_enqueue(tx2).await.unwrap();
    assert_eq!(mp.nonce_of(sender.id).await, Some(2));
}

#[tokio::test]
async fn refresh_removes_executed_txs() {
    let dir = TempDir::new().unwrap();
    let state = Arc::new(State::open(dir.path()).unwrap());

    // sender with enough balance
    let pk = SecretKey::random().public_key();
    let mut sender = state.create_account(G1(pk)).unwrap();
    sender.balance = 1_000;
    state.insert_account(&sender).unwrap();

    let r_pk = SecretKey::random().public_key();

    // two txs queued
    let tx1 = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 100, fee: 1 };
    let tx2 = Transaction { sender_id: sender.id.0, recipient_pk: G1(r_pk), amount: 150, fee: 1 };

    let mp = Mempool::new(state.clone());
    mp.validate_and_enqueue(tx1.clone()).await.unwrap();
    mp.validate_and_enqueue(tx2.clone()).await.unwrap();

    // Simulate execution of tx1 in a sub-block: update state and then inform mempool
    {
        let mut acct = state.get_account(sender.id).unwrap().unwrap();
        acct.balance -= 100 + 1;
        acct.nonce += 1;
        state.insert_account(&acct).unwrap();
    }

    mp.apply_subblock(&[tx1.clone()]).await;

    let remaining = mp.pop_batch(10).await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].amount, 150);
} 