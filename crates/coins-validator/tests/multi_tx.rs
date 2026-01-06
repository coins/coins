//! Test multiple transactions in a single sub-block
//! This mimics what the aggregator does when mining.

use coins_crypto::{SecretKey, G1, G2, aggregate, sign};
use coins_state::State;
use coins_types::{Transaction, SubBlock};
use coins_validator::validate_subblock;
use tempfile::tempdir;

#[test]
fn validate_two_txs_from_same_sender() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // Create accounts
    let alice_sk = SecretKey::random();
    let alice_acct = state.create_account(G1(alice_sk.public_key())).unwrap();

    let bob_sk = SecretKey::random();
    let bob_acct = state.create_account(G1(bob_sk.public_key())).unwrap();

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Fund Alice
    let mut alice_funded = alice_acct.clone();
    alice_funded.balance = 10000;
    state.insert_account(&alice_funded).unwrap();

    println!("Alice ID: {}, nonce: {}, balance: {}", alice_acct.id.0, alice_acct.nonce, alice_funded.balance);
    println!("Bob ID: {}, nonce: {}, balance: {}", bob_acct.id.0, bob_acct.nonce, 0);

    // Create TX1: Alice -> Bob (100, fee 1)
    let tx1 = Transaction {
        sender_id: alice_acct.id.0,
        recipient_pk: bob_acct.pk,
        amount: 100,
        fee: 1,
    };

    // Sign with Alice's current nonce (0)
    let msg1 = tx1.message_to_sign(alice_acct.nonce);
    let sig1 = sign(&alice_sk, &msg1);

    println!("TX1: Alice({}) -> Bob({}), amount=100, fee=1, nonce={}",
             alice_acct.id.0, bob_acct.id.0, alice_acct.nonce);

    // Create TX2: Alice -> Bob (50, fee 1)
    let tx2 = Transaction {
        sender_id: alice_acct.id.0,
        recipient_pk: bob_acct.pk,
        amount: 50,
        fee: 1,
    };

    // Sign with Alice's NEXT nonce (1)
    let msg2 = tx2.message_to_sign(alice_acct.nonce + 1);
    let sig2 = sign(&alice_sk, &msg2);

    println!("TX2: Alice({}) -> Bob({}), amount=50, fee=1, nonce={}",
             alice_acct.id.0, bob_acct.id.0, alice_acct.nonce + 1);

    // Aggregate signatures (mimicking what the aggregator does)
    let mempool: Vec<(Transaction, G2)> = vec![(tx1.clone(), sig1), (tx2.clone(), sig2)];
    let (transactions, signatures): (Vec<_>, Vec<_>) = mempool.into_iter().unzip();
    let aggregated_signature = aggregate(signatures.iter());

    println!("Aggregated {} signatures", signatures.len());

    // Create sub-block
    let sub_block = SubBlock {
        txs: transactions,
        sigma: aggregated_signature,
        aggregator_pk: agg_pk,
    };

    // Validate (this is where it fails in the aggregator)
    println!("Validating sub-block...");
    match validate_subblock(&sub_block, &state) {
        Ok(_) => {
            println!("✓ Validation succeeded!");

            // Verify final balances
            let alice_after = state.get_account(alice_acct.id).unwrap().unwrap();
            let bob_after = state.get_account(bob_acct.id).unwrap().unwrap();
            let agg_after = state.get_by_pk(&agg_pk).unwrap().unwrap();

            println!("After validation:");
            println!("  Alice: balance={}, nonce={}", alice_after.balance, alice_after.nonce);
            println!("  Bob: balance={}, nonce={}", bob_after.balance, bob_after.nonce);
            println!("  Aggregator: balance={}", agg_after.balance);

            assert_eq!(alice_after.balance, 10000 - 100 - 50 - 1 - 1, "Alice balance");
            assert_eq!(alice_after.nonce, 2, "Alice nonce");
            assert_eq!(bob_after.balance, 150, "Bob balance");
            assert_eq!(agg_after.balance, 2, "Aggregator fees");
        }
        Err(e) => {
            println!("✗ Validation failed: {:?}", e);
            panic!("Validation should succeed but got error: {:?}", e);
        }
    }
}

#[test]
fn validate_two_txs_from_different_senders() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // Create accounts
    let alice_sk = SecretKey::random();
    let alice_acct = state.create_account(G1(alice_sk.public_key())).unwrap();

    let bob_sk = SecretKey::random();
    let bob_acct = state.create_account(G1(bob_sk.public_key())).unwrap();

    let charlie_sk = SecretKey::random();
    let charlie_acct = state.create_account(G1(charlie_sk.public_key())).unwrap();

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Fund Alice and Bob
    let mut alice_funded = alice_acct.clone();
    alice_funded.balance = 1000;
    state.insert_account(&alice_funded).unwrap();

    let mut bob_funded = bob_acct.clone();
    bob_funded.balance = 500;
    state.insert_account(&bob_funded).unwrap();

    // TX1: Alice -> Charlie (100, fee 1)
    let tx1 = Transaction {
        sender_id: alice_acct.id.0,
        recipient_pk: charlie_acct.pk,
        amount: 100,
        fee: 1,
    };
    let msg1 = tx1.message_to_sign(alice_acct.nonce);
    let sig1 = sign(&alice_sk, &msg1);

    // TX2: Bob -> Charlie (50, fee 1)
    let tx2 = Transaction {
        sender_id: bob_acct.id.0,
        recipient_pk: charlie_acct.pk,
        amount: 50,
        fee: 1,
    };
    let msg2 = tx2.message_to_sign(bob_acct.nonce);
    let sig2 = sign(&bob_sk, &msg2);

    // Aggregate
    let mempool: Vec<(Transaction, G2)> = vec![(tx1, sig1), (tx2, sig2)];
    let (transactions, signatures): (Vec<_>, Vec<_>) = mempool.into_iter().unzip();
    let aggregated_signature = aggregate(signatures.iter());

    let sub_block = SubBlock {
        txs: transactions,
        sigma: aggregated_signature,
        aggregator_pk: agg_pk,
    };

    // Validate
    validate_subblock(&sub_block, &state).expect("should validate");

    // Verify
    let alice_after = state.get_account(alice_acct.id).unwrap().unwrap();
    let bob_after = state.get_account(bob_acct.id).unwrap().unwrap();
    let charlie_after = state.get_account(charlie_acct.id).unwrap().unwrap();
    let agg_after = state.get_by_pk(&agg_pk).unwrap().unwrap();

    assert_eq!(alice_after.balance, 1000 - 100 - 1);
    assert_eq!(alice_after.nonce, 1);
    assert_eq!(bob_after.balance, 500 - 50 - 1);
    assert_eq!(bob_after.nonce, 1);
    assert_eq!(charlie_after.balance, 150);
    assert_eq!(agg_after.balance, 2);
}
