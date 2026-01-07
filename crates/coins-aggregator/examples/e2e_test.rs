//! E2E test for Coins aggregator
//!
//! This creates test accounts, submits transactions, and verifies they are mined.
//!
//! Usage: cargo run --example e2e_test

use coins_crypto::{SecretKey, G1, G2, sign};
use coins_types::{Transaction, Account};
use coins_core::State;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use reqwest::blocking::Client;
use serde_json::json;

const AGGREGATOR_URL: &str = "http://localhost:8080";

struct TestUser {
    name: String,
    sk: SecretKey,
    pk: G1,
    pk_hex: String,
}

impl TestUser {
    fn new(name: &str) -> Self {
        let sk = SecretKey::random();
        let pk_bytes = sk.public_key();
        let pk = G1(pk_bytes);
        let pk_hex = hex::encode(pk_bytes);

        println!("Generated {} - PK: {}", name, pk_hex);

        Self {
            name: name.to_string(),
            sk,
            pk,
            pk_hex,
        }
    }

    fn get_account(&self) -> Result<Account, Box<dyn std::error::Error>> {
        let client = Client::new();
        let url = format!("{}/account/{}", AGGREGATOR_URL, self.pk_hex);
        let resp = client.get(&url).send()?;

        if resp.status() == 404 {
            return Err(format!("Account {} not found", self.name).into());
        }

        if !resp.status().is_success() {
            return Err(format!("HTTP error: {}", resp.status()).into());
        }

        let account: Account = resp.json()?;
        Ok(account)
    }

    fn create_transfer(&self, sender_id: u32, recipient_pk: G1, amount: u32, fee: u8, nonce: u32) -> (Transaction, G2) {
        let tx = Transaction {
            sender_id,
            recipient_pk,
            amount,
            fee,
        };

        let msg = tx.message_to_sign(nonce);
        let sig = sign(&self.sk, &msg);

        (tx, sig)
    }

    fn submit_tx(&self, tx: &Transaction, sig: &G2) -> Result<(), Box<dyn std::error::Error>> {
        let client = Client::new();
        let url = format!("{}/tx", AGGREGATOR_URL);

        let tx_bytes = bincode::serde::encode_to_vec(&tx, bincode::config::standard())?;
        let sig_bytes = bincode::serde::encode_to_vec(&sig, bincode::config::standard())?;

        let body = json!({
            "tx": hex::encode(tx_bytes),
            "signature": hex::encode(sig_bytes)
        });

        let resp = client.post(&url).json(&body).send()?;

        if !resp.status().is_success() {
            let error_text = resp.text().unwrap_or_else(|_| "unknown error".to_string());
            return Err(format!("Failed to submit tx: {}", error_text).into());
        }

        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Coins E2E Test ===\n");

    // Step 1: Create test users
    println!("Step 1: Generating test users...");
    let alice = TestUser::new("Alice");
    let bob = TestUser::new("Bob");
    println!();

    // Step 2: Setup accounts in state database
    println!("Step 2: Creating accounts in state database...");
    let state_path = PathBuf::from(".data/db/state.db");
    let state = State::open(&state_path)?;

    // Create Alice's account
    let alice_account = state.create_account(alice.pk)?;
    println!("  Created Alice account - ID: {}", alice_account.id.0);

    // Give Alice some balance
    let mut alice_acc = alice_account.clone();
    alice_acc.balance = 1000;
    state.apply_batch(&[alice_acc])?;
    println!("  Funded Alice with 1000 tokens");

    // Create Bob's account
    let bob_account = state.create_account(bob.pk)?;
    println!("  Created Bob account - ID: {}", bob_account.id.0);
    println!();

    // Step 3: Verify accounts via API
    println!("Step 3: Verifying accounts via API...");
    sleep(Duration::from_millis(500)); // Give aggregator time to see the updates

    let alice_info = alice.get_account()?;
    println!("  Alice - ID: {}, Balance: {}, Nonce: {}",
             alice_info.id.0, alice_info.balance, alice_info.nonce);

    let bob_info = bob.get_account()?;
    println!("  Bob - ID: {}, Balance: {}, Nonce: {}",
             bob_info.id.0, bob_info.balance, bob_info.nonce);
    println!();

    // Step 4: Create and submit transaction: Alice -> Bob (100 tokens)
    println!("Step 4: Submitting transfer: Alice -> Bob (100 tokens, fee 1)...");
    let (tx1, sig1) = alice.create_transfer(
        alice_info.id.0,
        bob.pk,
        100,
        1,
        alice_info.nonce,
    );

    alice.submit_tx(&tx1, &sig1)?;
    println!("  ✓ Transaction submitted to mempool");
    println!();

    // Step 5: Submit another transaction: Alice -> Bob (50 tokens)
    println!("Step 5: Submitting second transfer: Alice -> Bob (50 tokens, fee 1)...");
    let (tx2, sig2) = alice.create_transfer(
        alice_info.id.0,
        bob.pk,
        50,
        1,
        alice_info.nonce + 1,  // increment nonce
    );

    alice.submit_tx(&tx2, &sig2)?;
    println!("  ✓ Transaction submitted to mempool");
    println!();

    // Step 6: Wait for mining
    println!("Step 6: Waiting for aggregator to mine sub-block (30 seconds)...");
    for i in 0..6 {
        sleep(Duration::from_secs(5));
        print!("  {} seconds...", (i + 1) * 5);
        if (i + 1) * 5 >= 30 {
            println!(" ready!");
        } else {
            println!();
        }
    }
    println!();

    // Step 7: Verify balances updated
    println!("Step 7: Verifying balances after mining...");
    let alice_after = alice.get_account()?;
    let bob_after = bob.get_account()?;

    println!("  Alice - Balance: {} (was {}), Nonce: {} (was {})",
             alice_after.balance, alice_info.balance,
             alice_after.nonce, alice_info.nonce);
    println!("  Bob - Balance: {} (was {}), Nonce: {} (was {})",
             bob_after.balance, bob_info.balance,
             bob_after.nonce, bob_info.nonce);
    println!();

    // Verify expected balances
    let expected_alice = 1000 - 100 - 50 - 1 - 1; // initial - transfers - fees
    let expected_bob = 100 + 50; // received amounts

    println!("Step 8: Validating results...");
    if alice_after.balance == expected_alice {
        println!("  ✓ Alice balance correct: {}", alice_after.balance);
    } else {
        println!("  ✗ Alice balance incorrect: got {}, expected {}",
                 alice_after.balance, expected_alice);
    }

    if bob_after.balance == expected_bob {
        println!("  ✓ Bob balance correct: {}", bob_after.balance);
    } else {
        println!("  ✗ Bob balance incorrect: got {}, expected {}",
                 bob_after.balance, expected_bob);
    }

    if alice_after.nonce == alice_info.nonce + 2 {
        println!("  ✓ Alice nonce incremented correctly: {}", alice_after.nonce);
    } else {
        println!("  ✗ Alice nonce incorrect: got {}, expected {}",
                 alice_after.nonce, alice_info.nonce + 2);
    }

    println!("\n=== E2E Test Complete ===\n");

    Ok(())
}
