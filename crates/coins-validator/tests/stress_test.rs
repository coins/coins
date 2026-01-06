//! Stress test with many transactions mixed from multiple senders

use coins_crypto::{SecretKey, G1, G2, aggregate, sign};
use coins_state::State;
use coins_types::{Transaction, SubBlock};
use coins_validator::validate_subblock;
use tempfile::tempdir;
use std::collections::HashMap;

#[test]
fn validate_100_mixed_transactions() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    const NUM_ACCOUNTS: usize = 10;
    const NUM_TXS: usize = 100;

    println!("\n=== Stress Test: {} Mixed Transactions from {} Accounts ===\n", NUM_TXS, NUM_ACCOUNTS);

    // Create accounts
    let mut accounts = Vec::new();
    let mut secret_keys = Vec::new();

    for i in 0..NUM_ACCOUNTS {
        let sk = SecretKey::random();
        let acct = state.create_account(G1(sk.public_key())).unwrap();

        // Fund each account with 100,000 tokens
        let mut funded = acct.clone();
        funded.balance = 100_000;
        state.insert_account(&funded).unwrap();

        println!("Account {}: ID={}, balance={}", i, acct.id.0, funded.balance);

        accounts.push(funded);
        secret_keys.push(sk);
    }

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    println!("\nCreating {} transactions...", NUM_TXS);

    // Track nonces for each account
    let mut nonces: HashMap<u32, u32> = HashMap::new();
    for acct in &accounts {
        nonces.insert(acct.id.0, 0);
    }

    // Create mixed transactions
    let mut mempool: Vec<(Transaction, G2)> = Vec::new();

    use rand::Rng;
    let mut rng = rand::thread_rng();

    for i in 0..NUM_TXS {
        // Pick random sender and recipient (different accounts)
        let sender_idx = rng.gen_range(0..NUM_ACCOUNTS);
        let mut recipient_idx = rng.gen_range(0..NUM_ACCOUNTS);
        while recipient_idx == sender_idx {
            recipient_idx = rng.gen_range(0..NUM_ACCOUNTS);
        }

        let sender = &accounts[sender_idx];
        let sender_sk = &secret_keys[sender_idx];
        let recipient = &accounts[recipient_idx];

        // Random amount between 1-100
        let amount = rng.gen_range(1..=100);
        let fee = rng.gen_range(1..=5);

        let tx = Transaction {
            sender_id: sender.id.0,
            recipient_pk: recipient.pk,
            amount,
            fee,
        };

        // Get current nonce for this sender
        let nonce = *nonces.get(&sender.id.0).unwrap();

        // Sign with current nonce
        let msg = tx.message_to_sign(nonce);
        let sig = sign(sender_sk, &msg);

        if i < 10 {
            println!("  TX{:3}: Account{} -> Account{} (amount={}, fee={}, nonce={})",
                     i, sender_idx, recipient_idx, amount, fee, nonce);
        } else if i == 10 {
            println!("  ... ({} more transactions)", NUM_TXS - 10);
        }

        mempool.push((tx, sig));

        // Increment nonce for sender
        *nonces.get_mut(&sender.id.0).unwrap() += 1;
    }

    println!("\nAggregating {} signatures...", NUM_TXS);
    let (transactions, signatures): (Vec<_>, Vec<_>) = mempool.into_iter().unzip();
    let aggregated_signature = aggregate(signatures.iter());

    println!("Creating sub-block...");
    let sub_block = SubBlock {
        txs: transactions,
        sigma: aggregated_signature,
        aggregator_pk: agg_pk,
    };

    println!("Validating sub-block with {} transactions...", NUM_TXS);
    let start = std::time::Instant::now();

    match validate_subblock(&sub_block, &state) {
        Ok(_) => {
            let elapsed = start.elapsed();
            println!("✓ Validation SUCCEEDED in {:.2?}", elapsed);
        }
        Err(e) => {
            println!("✗ Validation FAILED: {:?}", e);
            panic!("Validation should succeed but got error: {:?}", e);
        }
    }

    // Verify all accounts have correct final state
    println!("\nVerifying final account states...");

    let mut total_sent: HashMap<u32, u64> = HashMap::new();
    let mut total_received: HashMap<u32, u64> = HashMap::new();
    let mut total_fees: u64 = 0;

    for tx in &sub_block.txs {
        *total_sent.entry(tx.sender_id).or_insert(0) += tx.amount as u64 + tx.fee as u64;

        // Find recipient ID
        let recipient = state.get_by_pk(&tx.recipient_pk).unwrap().unwrap();
        *total_received.entry(recipient.id.0).or_insert(0) += tx.amount as u64;

        total_fees += tx.fee as u64;
    }

    for (i, original_acct) in accounts.iter().enumerate() {
        let final_acct = state.get_account(original_acct.id).unwrap().unwrap();

        let sent = total_sent.get(&original_acct.id.0).copied().unwrap_or(0);
        let received = total_received.get(&original_acct.id.0).copied().unwrap_or(0);
        let expected_balance = 100_000 - sent + received;
        let expected_nonce = nonces.get(&original_acct.id.0).unwrap();

        assert_eq!(final_acct.balance, expected_balance,
                   "Account {} balance mismatch", i);
        assert_eq!(final_acct.nonce, *expected_nonce,
                   "Account {} nonce mismatch", i);

        if i < 5 {
            println!("  Account {}: balance={} (sent={}, recv={}), nonce={}",
                     i, final_acct.balance, sent, received, final_acct.nonce);
        }
    }

    // Verify aggregator got all fees
    let agg_final = state.get_by_pk(&agg_pk).unwrap().unwrap();
    assert_eq!(agg_final.balance, total_fees,
               "Aggregator should have all fees");

    println!("\n✓ All {} accounts verified correctly!", NUM_ACCOUNTS);
    println!("✓ Aggregator collected {} total fees", total_fees);
    println!("\n=== Stress Test PASSED ===\n");
}

#[test]
fn validate_50_sequential_from_one_account() {
    // Test case: one account sends 50 transactions in a row
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    const NUM_TXS: usize = 50;

    println!("\n=== Sequential Test: {} Transactions from One Account ===\n", NUM_TXS);

    // Create sender and recipients
    let sender_sk = SecretKey::random();
    let sender = state.create_account(G1(sender_sk.public_key())).unwrap();

    let mut sender_funded = sender.clone();
    sender_funded.balance = 100_000;
    state.insert_account(&sender_funded).unwrap();

    println!("Sender: ID={}, balance={}", sender.id.0, sender_funded.balance);

    // Create 10 recipient accounts
    let mut recipients = Vec::new();
    for i in 0..10 {
        let recipient_sk = SecretKey::random();
        let recipient = state.create_account(G1(recipient_sk.public_key())).unwrap();
        println!("Recipient {}: ID={}", i, recipient.id.0);
        recipients.push(recipient);
    }

    let agg_sk = SecretKey::random();
    let agg_pk = G1(agg_sk.public_key());

    println!("\nCreating {} sequential transactions...", NUM_TXS);

    let mut mempool: Vec<(Transaction, G2)> = Vec::new();

    for i in 0..NUM_TXS {
        let recipient = &recipients[i % recipients.len()];
        let amount = 100 + i as u32;
        let fee = 1;

        let tx = Transaction {
            sender_id: sender.id.0,
            recipient_pk: recipient.pk,
            amount,
            fee,
        };

        // Nonce = i (sequential)
        let msg = tx.message_to_sign(i as u32);
        let sig = sign(&sender_sk, &msg);

        if i < 5 {
            println!("  TX{}: Sender -> Recipient{} (amount={}, nonce={})",
                     i, i % recipients.len(), amount, i);
        } else if i == 5 {
            println!("  ... ({} more)", NUM_TXS - 5);
        }

        mempool.push((tx, sig));
    }

    println!("\nAggregating {} signatures...", NUM_TXS);
    let (transactions, signatures): (Vec<_>, Vec<_>) = mempool.into_iter().unzip();
    let aggregated_signature = aggregate(signatures.iter());

    let sub_block = SubBlock {
        txs: transactions,
        sigma: aggregated_signature,
        aggregator_pk: agg_pk,
    };

    println!("Validating...");
    validate_subblock(&sub_block, &state).expect("should validate");

    let sender_final = state.get_account(sender.id).unwrap().unwrap();

    // Calculate expected
    let total_sent: u64 = (0..NUM_TXS).map(|i| (100 + i) as u64 + 1).sum();
    let expected_balance = 100_000 - total_sent;

    assert_eq!(sender_final.balance, expected_balance);
    assert_eq!(sender_final.nonce, NUM_TXS as u32);

    println!("\n✓ Sender final: balance={}, nonce={}", sender_final.balance, sender_final.nonce);
    println!("✓ Sent {} total (including fees)", total_sent);
    println!("\n=== Sequential Test PASSED ===\n");
}
