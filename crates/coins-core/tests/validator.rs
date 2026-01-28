//! Validator tests - ensure signing and verification work end-to-end

use ark_ff::PrimeField;
use coins_crypto::{sign, aggregate, verify_aggregate, SecretKey, G1};
use coins_types::{Transaction, SubBlock, NATIVE_TOKEN_ID};

/// Test that signing a transaction and verifying with the same message works
#[test]
fn sign_and_verify_transaction() {
    // Create sender key (sk=2, matching genesis in regtest config)
    let sk_bytes = hex::decode("0200000000000000000000000000000000000000000000000000000000000000").unwrap();
    let fr = ark_bn254::Fr::from_le_bytes_mod_order(&sk_bytes);
    let sk = SecretKey(fr);
    let pk = G1(sk.public_key());

    // Verify pk matches expected value
    let expected_pk = "d3cf876dc108c2d3a81c8716a91678d9851518685b04859b021a132ee7440603";
    assert_eq!(pk.to_hex(), expected_pk, "PK should match config genesis_pk");

    // Create a recipient key
    let recipient_sk = SecretKey::random();
    let recipient_pk = G1(recipient_sk.public_key());

    // Create transaction
    let tx = Transaction {
        sender_id: 0,  // genesis is always account 0
        recipient_pk,
        token_id: NATIVE_TOKEN_ID,
        amount: 1000,
        fee: 1,
    };

    // Sign with nonce 0 (first tx from genesis)
    let nonce: u32 = 0;
    let msg = tx.message_to_sign(nonce);
    let sig = sign(&sk, &msg);

    // Verify single signature
    assert!(
        coins_crypto::verify(&pk, &msg, &sig),
        "Single signature verification should pass"
    );

    // Create SubBlock with aggregated signature (single tx case)
    let publisher_sk = SecretKey::random();
    let publisher_pk = G1(publisher_sk.public_key());

    let aggregated = aggregate([&sig]);

    let sub_block = SubBlock {
        txs: vec![tx.clone()],
        sigma: aggregated,
        publisher_pk,
    };

    // Verify aggregate (same as validate_subblock does)
    let pairs = vec![(&pk, msg.as_slice())];
    assert!(
        verify_aggregate(pairs.into_iter(), &sub_block.sigma),
        "Aggregate signature verification should pass"
    );
}

/// Test multiple transactions from same sender with sequential nonces
#[test]
fn multiple_txs_same_sender() {
    let sk = SecretKey::random();
    let pk = G1(sk.public_key());

    let recipient1 = G1(SecretKey::random().public_key());
    let recipient2 = G1(SecretKey::random().public_key());

    let tx1 = Transaction {
        sender_id: 0,
        recipient_pk: recipient1,
        token_id: NATIVE_TOKEN_ID,
        amount: 100,
        fee: 1,
    };

    let tx2 = Transaction {
        sender_id: 0,
        recipient_pk: recipient2,
        token_id: NATIVE_TOKEN_ID,
        amount: 200,
        fee: 1,
    };

    // Sign with sequential nonces
    let msg1 = tx1.message_to_sign(0);
    let msg2 = tx2.message_to_sign(1);  // nonce incremented!

    let sig1 = sign(&sk, &msg1);
    let sig2 = sign(&sk, &msg2);

    // Aggregate
    let aggregated = aggregate([&sig1, &sig2]);

    // Verify - must use same (pk, msg) pairs with correct nonces
    let pairs = vec![
        (&pk, msg1.as_slice()),
        (&pk, msg2.as_slice()),
    ];

    assert!(
        verify_aggregate(pairs.into_iter(), &aggregated),
        "Aggregate verification with sequential nonces should pass"
    );
}

/// Test that using wrong nonce fails verification
#[test]
fn wrong_nonce_fails() {
    let sk = SecretKey::random();
    let pk = G1(sk.public_key());

    let recipient = G1(SecretKey::random().public_key());

    let tx = Transaction {
        sender_id: 0,
        recipient_pk: recipient,
        token_id: NATIVE_TOKEN_ID,
        amount: 100,
        fee: 1,
    };

    // Sign with nonce 0
    let msg_sign = tx.message_to_sign(0);
    let sig = sign(&sk, &msg_sign);

    // Try to verify with nonce 1 (wrong!)
    let msg_verify = tx.message_to_sign(1);
    let aggregated = aggregate([&sig]);

    let pairs = vec![(&pk, msg_verify.as_slice())];

    assert!(
        !verify_aggregate(pairs.into_iter(), &aggregated),
        "Verification with wrong nonce should fail"
    );
}
