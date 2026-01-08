//! Submit test transactions to the publisher
//!
//! This script:
//! 1. Loads Alice/Bob keys (or generates them)
//! 2. Submits transactions
//! 3. Waits for mining
//! 4. Verifies balances
//!
//! Usage: cargo run --example submit_txs

use coins_crypto::{SecretKey, G1, G2, sign};
use coins_types::{Transaction, Account};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use reqwest::blocking::Client;
use serde_json::json;
use ark_ff::PrimeField;
use ark_bn254::Fr;
use ark_serialize::CanonicalSerialize;

fn get_publisher_url() -> String {
    std::env::var("PUBLISHER_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

const PUBLISHER_URL: &str = "http://localhost:8080";

// Alice PK from setup_test_accounts
const ALICE_PK: &str = "2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f";

fn load_or_generate_key(path: &str) -> Result<SecretKey, Box<dyn std::error::Error>> {
    let key_path = Path::new(path);

    if key_path.exists() {
        let hex = std::fs::read_to_string(key_path)?;
        let bytes = hex::decode(hex.trim())?;
        let fr = Fr::from_le_bytes_mod_order(&bytes);
        Ok(SecretKey(fr))
    } else {
        let sk = SecretKey::random();
        let mut bytes = Vec::new();
        sk.0.serialize_uncompressed(&mut bytes)?;
        std::fs::write(key_path, hex::encode(&bytes))?;
        println!("Generated new key: {}", path);
        let pk_hex = hex::encode(sk.public_key());
        println!("  Public key: {}", pk_hex);
        Ok(sk)
    }
}

fn get_account(pk_hex: &str) -> Result<Account, Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("{}/account/{}", get_publisher_url(), pk_hex);
    let resp = client.get(&url).send()?;

    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()).into());
    }

    let account: Account = resp.json()?;
    Ok(account)
}

fn submit_tx(tx: &Transaction, sig: &G2) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("{}/tx", get_publisher_url());

    let tx_bytes = bincode::serde::encode_to_vec(tx, bincode::config::standard())?;
    let sig_bytes = bincode::serde::encode_to_vec(sig, bincode::config::standard())?;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Submit Test Transactions ===\n");

    // Load keys
    println!("Step 1: Loading keypairs...");
    let alice_sk = load_or_generate_key(".data/test-keys/alice_sk.hex")?;
    let bob_sk = load_or_generate_key(".data/test-keys/bob_sk.hex")?;

    let alice_pk_bytes = alice_sk.public_key();
    let bob_pk_bytes = bob_sk.public_key();
    let alice_pk_hex = hex::encode(alice_pk_bytes);
    let bob_pk_hex = hex::encode(bob_pk_bytes);

    println!("  Alice PK: {}", alice_pk_hex);
    println!("  Bob PK: {}", bob_pk_hex);
    println!();

    // Get current account states
    println!("Step 2: Getting current account states...");
    let alice_before = get_account(&alice_pk_hex)?;
    let bob_before = get_account(&bob_pk_hex)?;

    println!("  Alice (ID {}): balance={}, nonce={}",
             alice_before.id.0, alice_before.balance, alice_before.nonce);
    println!("  Bob (ID {}): balance={}, nonce={}",
             bob_before.id.0, bob_before.balance, bob_before.nonce);
    println!();

    if alice_before.balance < 200 {
        println!("⚠️  Alice doesn't have enough balance ({}). Please fund Alice first.", alice_before.balance);
        println!("   Expected Alice PK: {}", ALICE_PK);
        println!("   Actual Alice PK:   {}", alice_pk_hex);
        return Ok(());
    }

    // Create and submit transaction
    println!("Step 3: Creating and submitting transaction...");

    // TX: Alice -> Bob (100 tokens, fee 1)
    let tx = Transaction {
        sender_id: alice_before.id.0,
        recipient_pk: G1(bob_pk_bytes),
        amount: 100,
        fee: 1,
    };
    let msg = tx.message_to_sign(alice_before.nonce);
    let sig = sign(&alice_sk, &msg);

    println!("  Submitting TX: Alice -> Bob (100 tokens, fee 1)...");
    submit_tx(&tx, &sig)?;
    println!("    ✓ Submitted");
    println!();

    // Wait for mining
    println!("Step 4: Waiting for publisher to mine sub-block...");
    println!("  (Publisher mines every 30 seconds)");

    for i in 1..=6 {
        sleep(Duration::from_secs(5));
        print!("  {} seconds elapsed...", i * 5);
        if i * 5 >= 30 {
            println!(" checking now!");
        } else {
            println!();
        }
    }
    println!();

    // Verify results
    println!("Step 5: Verifying balances after mining...");
    let alice_after = get_account(&alice_pk_hex)?;
    let bob_after = get_account(&bob_pk_hex)?;

    println!("  Alice (ID {}): balance={} (was {}), nonce={} (was {})",
             alice_after.id.0, alice_after.balance, alice_before.balance,
             alice_after.nonce, alice_before.nonce);
    println!("  Bob (ID {}): balance={} (was {}), nonce={} (was {})",
             bob_after.id.0, bob_after.balance, bob_before.balance,
             bob_after.nonce, bob_before.nonce);
    println!();

    // Calculate expected balances
    let alice_expected = alice_before.balance - 100 - 1;  // amount + fee
    let bob_expected = bob_before.balance + 100;  // amount

    println!("Step 6: Validation...");

    let mut success = true;

    if alice_after.balance == alice_expected {
        println!("  ✓ Alice balance correct: {} (expected {})", alice_after.balance, alice_expected);
    } else {
        println!("  ✗ Alice balance INCORRECT: got {}, expected {}", alice_after.balance, alice_expected);
        success = false;
    }

    if bob_after.balance == bob_expected {
        println!("  ✓ Bob balance correct: {} (expected {})", bob_after.balance, bob_expected);
    } else {
        println!("  ✗ Bob balance INCORRECT: got {}, expected {}", bob_after.balance, bob_expected);
        success = false;
    }

    if alice_after.nonce == alice_before.nonce + 1 {
        println!("  ✓ Alice nonce incremented correctly: {} -> {}", alice_before.nonce, alice_after.nonce);
    } else {
        println!("  ✗ Alice nonce INCORRECT: got {}, expected {}", alice_after.nonce, alice_before.nonce + 1);
        success = false;
    }

    println!();
    if success {
        println!("=== ✓ E2E Test PASSED ===\n");
    } else {
        println!("=== ✗ E2E Test FAILED ===\n");
    }

    Ok(())
}
