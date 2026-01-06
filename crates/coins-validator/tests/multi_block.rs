//! Test multiple sub-blocks where coins are forwarded through chains
//! This tests the realistic scenario where received coins are spent in later blocks

use coins_crypto::{SecretKey, G1, G2, aggregate, sign};
use coins_state::State;
use coins_types::{Transaction, SubBlock};
use coins_validator::validate_subblock;
use tempfile::tempdir;

#[test]
fn validate_coin_forwarding_chain() {
    // Test: A -> B -> C -> D across multiple blocks
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    println!("\n=== Coin Forwarding Chain Test ===\n");

    // Create accounts A, B, C, D
    let a_sk = SecretKey::random();
    let a = state.create_account(G1(a_sk.public_key())).unwrap();

    let b_sk = SecretKey::random();
    let b = state.create_account(G1(b_sk.public_key())).unwrap();

    let c_sk = SecretKey::random();
    let c = state.create_account(G1(c_sk.public_key())).unwrap();

    let d_sk = SecretKey::random();
    let d = state.create_account(G1(d_sk.public_key())).unwrap();

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Fund A with 10000
    let mut a_funded = a.clone();
    a_funded.balance = 10000;
    state.insert_account(&a_funded).unwrap();

    println!("Initial state:");
    println!("  A: ID={}, balance=10000", a.id.0);
    println!("  B: ID={}, balance=0", b.id.0);
    println!("  C: ID={}, balance=0", c.id.0);
    println!("  D: ID={}, balance=0", d.id.0);

    // BLOCK 1: A sends 5000 to B
    println!("\n--- Block 1: A -> B (5000) ---");
    let tx1 = Transaction {
        sender_id: a.id.0,
        recipient_pk: b.pk,
        amount: 5000,
        fee: 10,
    };
    let msg1 = tx1.message_to_sign(0);
    let sig1 = sign(&a_sk, &msg1);

    let block1 = SubBlock {
        txs: vec![tx1],
        sigma: aggregate([&sig1]),
        aggregator_pk: agg_pk,
    };

    validate_subblock(&block1, &state).expect("block 1 should validate");

    let a_after_b1 = state.get_account(a.id).unwrap().unwrap();
    let b_after_b1 = state.get_account(b.id).unwrap().unwrap();
    println!("  A: balance={}, nonce={}", a_after_b1.balance, a_after_b1.nonce);
    println!("  B: balance={}, nonce={}", b_after_b1.balance, b_after_b1.nonce);
    assert_eq!(a_after_b1.balance, 4990); // 10000 - 5000 - 10
    assert_eq!(b_after_b1.balance, 5000);

    // BLOCK 2: B forwards 3000 to C
    println!("\n--- Block 2: B -> C (3000) ---");
    let tx2 = Transaction {
        sender_id: b.id.0,
        recipient_pk: c.pk,
        amount: 3000,
        fee: 10,
    };
    let msg2 = tx2.message_to_sign(0); // B's nonce is 0
    let sig2 = sign(&b_sk, &msg2);

    let block2 = SubBlock {
        txs: vec![tx2],
        sigma: aggregate([&sig2]),
        aggregator_pk: agg_pk,
    };

    validate_subblock(&block2, &state).expect("block 2 should validate");

    let b_after_b2 = state.get_account(b.id).unwrap().unwrap();
    let c_after_b2 = state.get_account(c.id).unwrap().unwrap();
    println!("  B: balance={}, nonce={}", b_after_b2.balance, b_after_b2.nonce);
    println!("  C: balance={}, nonce={}", c_after_b2.balance, c_after_b2.nonce);
    assert_eq!(b_after_b2.balance, 1990); // 5000 - 3000 - 10
    assert_eq!(c_after_b2.balance, 3000);

    // BLOCK 3: C forwards 2000 to D
    println!("\n--- Block 3: C -> D (2000) ---");
    let tx3 = Transaction {
        sender_id: c.id.0,
        recipient_pk: d.pk,
        amount: 2000,
        fee: 10,
    };
    let msg3 = tx3.message_to_sign(0); // C's nonce is 0
    let sig3 = sign(&c_sk, &msg3);

    let block3 = SubBlock {
        txs: vec![tx3],
        sigma: aggregate([&sig3]),
        aggregator_pk: agg_pk,
    };

    validate_subblock(&block3, &state).expect("block 3 should validate");

    let c_after_b3 = state.get_account(c.id).unwrap().unwrap();
    let d_after_b3 = state.get_account(d.id).unwrap().unwrap();
    println!("  C: balance={}, nonce={}", c_after_b3.balance, c_after_b3.nonce);
    println!("  D: balance={}, nonce={}", d_after_b3.balance, d_after_b3.nonce);
    assert_eq!(c_after_b3.balance, 990); // 3000 - 2000 - 10
    assert_eq!(d_after_b3.balance, 2000);

    // Verify aggregator collected fees
    let agg_final = state.get_by_pk(&agg_pk).unwrap().unwrap();
    assert_eq!(agg_final.balance, 30); // 10 + 10 + 10

    println!("\n✓ Final balances:");
    println!("  A: {}", a_after_b1.balance);
    println!("  B: {}", b_after_b2.balance);
    println!("  C: {}", c_after_b3.balance);
    println!("  D: {}", d_after_b3.balance);
    println!("  Aggregator: {}", agg_final.balance);
    println!("✓ Coin forwarding chain verified across 3 blocks!\n");
}

#[test]
fn validate_multiple_senders_multiple_blocks() {
    // Test with 5 accounts, each sending in different blocks
    // and some receiving and forwarding coins
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    const NUM_ACCOUNTS: usize = 5;

    println!("\n=== Multiple Senders Across Multiple Blocks ===\n");

    let mut accounts = Vec::new();
    let mut secret_keys = Vec::new();

    for i in 0..NUM_ACCOUNTS {
        let sk = SecretKey::random();
        let acct = state.create_account(G1(sk.public_key())).unwrap();

        // Fund each with 10000
        let mut funded = acct.clone();
        funded.balance = 10000;
        state.insert_account(&funded).unwrap();

        println!("Account {}: ID={}, balance=10000", i, acct.id.0);
        accounts.push(acct);
        secret_keys.push(sk);
    }

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Track nonces for each account
    let mut nonces = vec![0u32; NUM_ACCOUNTS];

    // BLOCK 1: Multiple senders
    println!("\n--- Block 1: Mixed senders ---");
    let mut txs_b1 = Vec::new();
    let mut sigs_b1 = Vec::new();

    // Account 0 -> Account 1 (100)
    let tx = Transaction {
        sender_id: accounts[0].id.0,
        recipient_pk: accounts[1].pk,
        amount: 100,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[0]);
    sigs_b1.push(sign(&secret_keys[0], &msg));
    txs_b1.push(tx);
    nonces[0] += 1;

    // Account 2 -> Account 3 (200)
    let tx = Transaction {
        sender_id: accounts[2].id.0,
        recipient_pk: accounts[3].pk,
        amount: 200,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[2]);
    sigs_b1.push(sign(&secret_keys[2], &msg));
    txs_b1.push(tx);
    nonces[2] += 1;

    // Account 4 -> Account 0 (150)
    let tx = Transaction {
        sender_id: accounts[4].id.0,
        recipient_pk: accounts[0].pk,
        amount: 150,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[4]);
    sigs_b1.push(sign(&secret_keys[4], &msg));
    txs_b1.push(tx);
    nonces[4] += 1;

    let block1 = SubBlock {
        txs: txs_b1,
        sigma: aggregate(sigs_b1.iter()),
        aggregator_pk: agg_pk,
    };

    println!("  3 transactions from different senders");
    validate_subblock(&block1, &state).expect("block 1 should validate");

    // BLOCK 2: Recipients from Block 1 forward their coins
    println!("\n--- Block 2: Recipients forward coins ---");
    let mut txs_b2 = Vec::new();
    let mut sigs_b2 = Vec::new();

    // Account 1 forwards what they received: Account 1 -> Account 2 (50)
    let tx = Transaction {
        sender_id: accounts[1].id.0,
        recipient_pk: accounts[2].pk,
        amount: 50,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[1]);
    sigs_b2.push(sign(&secret_keys[1], &msg));
    txs_b2.push(tx);
    nonces[1] += 1;

    // Account 3 forwards: Account 3 -> Account 4 (100)
    let tx = Transaction {
        sender_id: accounts[3].id.0,
        recipient_pk: accounts[4].pk,
        amount: 100,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[3]);
    sigs_b2.push(sign(&secret_keys[3], &msg));
    txs_b2.push(tx);
    nonces[3] += 1;

    // Account 0 sends again (they received in block 1)
    let tx = Transaction {
        sender_id: accounts[0].id.0,
        recipient_pk: accounts[4].pk,
        amount: 75,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[0]);
    sigs_b2.push(sign(&secret_keys[0], &msg));
    txs_b2.push(tx);
    nonces[0] += 1;

    let block2 = SubBlock {
        txs: txs_b2,
        sigma: aggregate(sigs_b2.iter()),
        aggregator_pk: agg_pk,
    };

    println!("  3 transactions forwarding received coins");
    validate_subblock(&block2, &state).expect("block 2 should validate");

    // BLOCK 3: More complex mixing
    println!("\n--- Block 3: Complex mixing ---");
    let mut txs_b3 = Vec::new();
    let mut sigs_b3 = Vec::new();

    // Account 2 sends to 0 and 1 (two transactions from same sender)
    let tx = Transaction {
        sender_id: accounts[2].id.0,
        recipient_pk: accounts[0].pk,
        amount: 30,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[2]);
    sigs_b3.push(sign(&secret_keys[2], &msg));
    txs_b3.push(tx);
    nonces[2] += 1;

    let tx = Transaction {
        sender_id: accounts[2].id.0,
        recipient_pk: accounts[1].pk,
        amount: 40,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[2]);
    sigs_b3.push(sign(&secret_keys[2], &msg));
    txs_b3.push(tx);
    nonces[2] += 1;

    // Account 4 sends
    let tx = Transaction {
        sender_id: accounts[4].id.0,
        recipient_pk: accounts[3].pk,
        amount: 60,
        fee: 1,
    };
    let msg = tx.message_to_sign(nonces[4]);
    sigs_b3.push(sign(&secret_keys[4], &msg));
    txs_b3.push(tx);
    nonces[4] += 1;

    let block3 = SubBlock {
        txs: txs_b3,
        sigma: aggregate(sigs_b3.iter()),
        aggregator_pk: agg_pk,
    };

    println!("  3 transactions with account 2 sending twice");
    validate_subblock(&block3, &state).expect("block 3 should validate");

    // Verify final state
    println!("\n✓ Final state after 3 blocks:");
    for i in 0..NUM_ACCOUNTS {
        let acct = state.get_account(accounts[i].id).unwrap().unwrap();
        println!("  Account {}: balance={}, nonce={}", i, acct.balance, acct.nonce);
        assert_eq!(acct.nonce, nonces[i], "Account {} nonce mismatch", i);
    }

    let agg_final = state.get_by_pk(&agg_pk).unwrap().unwrap();
    println!("  Aggregator: balance={} (total fees)", agg_final.balance);
    assert_eq!(agg_final.balance, 9); // 3 + 3 + 3 fees

    println!("✓ All 3 blocks validated successfully!\n");
}

#[test]
fn validate_circular_transfers_across_blocks() {
    // Test: Coins go in a circle across multiple blocks
    // Block 1: A -> B
    // Block 2: B -> C
    // Block 3: C -> A (coins return to A)
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    println!("\n=== Circular Transfers Test ===\n");

    let a_sk = SecretKey::random();
    let a = state.create_account(G1(a_sk.public_key())).unwrap();

    let b_sk = SecretKey::random();
    let b = state.create_account(G1(b_sk.public_key())).unwrap();

    let c_sk = SecretKey::random();
    let c = state.create_account(G1(c_sk.public_key())).unwrap();

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    // Fund A
    let mut a_funded = a.clone();
    a_funded.balance = 1000;
    state.insert_account(&a_funded).unwrap();

    println!("Initial: A=1000, B=0, C=0");

    // Block 1: A -> B (500)
    println!("\n--- Block 1: A -> B (500) ---");
    let tx1 = Transaction {
        sender_id: a.id.0,
        recipient_pk: b.pk,
        amount: 500,
        fee: 10,
    };
    let block1 = SubBlock {
        txs: vec![tx1.clone()],
        sigma: aggregate([&sign(&a_sk, &tx1.message_to_sign(0))]),
        aggregator_pk: agg_pk,
    };
    validate_subblock(&block1, &state).unwrap();

    let a1 = state.get_account(a.id).unwrap().unwrap();
    let b1 = state.get_account(b.id).unwrap().unwrap();
    println!("After: A={}, B={}", a1.balance, b1.balance);
    assert_eq!(a1.balance, 490);
    assert_eq!(b1.balance, 500);

    // Block 2: B -> C (300)
    println!("\n--- Block 2: B -> C (300) ---");
    let tx2 = Transaction {
        sender_id: b.id.0,
        recipient_pk: c.pk,
        amount: 300,
        fee: 10,
    };
    let block2 = SubBlock {
        txs: vec![tx2.clone()],
        sigma: aggregate([&sign(&b_sk, &tx2.message_to_sign(0))]),
        aggregator_pk: agg_pk,
    };
    validate_subblock(&block2, &state).unwrap();

    let b2 = state.get_account(b.id).unwrap().unwrap();
    let c2 = state.get_account(c.id).unwrap().unwrap();
    println!("After: B={}, C={}", b2.balance, c2.balance);
    assert_eq!(b2.balance, 190);
    assert_eq!(c2.balance, 300);

    // Block 3: C -> A (200) - completing the circle
    println!("\n--- Block 3: C -> A (200) - Circle complete! ---");
    let tx3 = Transaction {
        sender_id: c.id.0,
        recipient_pk: a.pk,
        amount: 200,
        fee: 10,
    };
    let block3 = SubBlock {
        txs: vec![tx3.clone()],
        sigma: aggregate([&sign(&c_sk, &tx3.message_to_sign(0))]),
        aggregator_pk: agg_pk,
    };
    validate_subblock(&block3, &state).unwrap();

    let a3 = state.get_account(a.id).unwrap().unwrap();
    let c3 = state.get_account(c.id).unwrap().unwrap();
    println!("After: A={}, C={}", a3.balance, c3.balance);
    assert_eq!(a3.balance, 690); // 490 + 200
    assert_eq!(c3.balance, 90);

    println!("\n✓ Circular transfer complete:");
    println!("  A sent 500, received 200 back");
    println!("  Net flow: A lost 300 (+ 30 fees) = 690");
    println!("  B forwarded 300 from their 500 = 190");
    println!("  C forwarded 200 from their 300 = 90");

    let agg_final = state.get_by_pk(&agg_pk).unwrap().unwrap();
    assert_eq!(agg_final.balance, 30);
    println!("  Aggregator collected 30 total fees\n");
}
