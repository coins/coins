//! Setup test accounts for e2e testing
//!
//! This generates Alice and Bob keypairs, saves the secret keys,
//! and creates accounts in the state database with initial balances.
//!
//! Usage: cargo run --example setup_test_accounts [state_db_path]
//!
//! The secret keys are saved to a `test-keys/` subdirectory next to the state.db.
//! For example, if state_db_path is `.data/mutinynet/state.db`, keys go to
//! `.data/mutinynet/test-keys/alice_sk.hex` and `.data/mutinynet/test-keys/bob_sk.hex`.

use ark_ff::{BigInteger, PrimeField};
use coins_crypto::{SecretKey, G1};
use coins_core::State;
use std::path::PathBuf;

fn load_or_generate_key(path: &PathBuf) -> Result<SecretKey, Box<dyn std::error::Error>> {
    if path.exists() {
        let hex = std::fs::read_to_string(path)?;
        let bytes = hex::decode(hex.trim())?;
        let fr = ark_bn254::Fr::from_le_bytes_mod_order(&bytes);
        Ok(SecretKey(fr))
    } else {
        let sk = SecretKey::random();
        // Save using little-endian bytes (consistent with client)
        let sk_bytes = sk.0.into_bigint().to_bytes_le();
        std::fs::write(path, hex::encode(&sk_bytes))?;
        Ok(sk)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Setup Test Accounts ===\n");

    // Open state database (use argument or default to regtest)
    let state_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".data/regtest/state.db"));
    println!("Using state database: {}", state_path.display());

    // Determine keys directory (sibling to state.db)
    let keys_dir = state_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("test-keys");
    std::fs::create_dir_all(&keys_dir)?;
    println!("Using keys directory: {}", keys_dir.display());

    let alice_key_path = keys_dir.join("alice_sk.hex");
    let bob_key_path = keys_dir.join("bob_sk.hex");

    // Load or generate keypairs
    println!("\nStep 1: Loading/generating keypairs...");

    let alice_sk = load_or_generate_key(&alice_key_path)?;
    let alice_pk_bytes = alice_sk.public_key();
    let alice_pk = G1(alice_pk_bytes);
    let alice_pk_hex = hex::encode(alice_pk_bytes);
    println!("  Alice SK: {}", alice_key_path.display());
    println!("  Alice PK: {}", alice_pk_hex);

    let bob_sk = load_or_generate_key(&bob_key_path)?;
    let bob_pk_bytes = bob_sk.public_key();
    let bob_pk = G1(bob_pk_bytes);
    let bob_pk_hex = hex::encode(bob_pk_bytes);
    println!("  Bob SK:   {}", bob_key_path.display());
    println!("  Bob PK:   {}", bob_pk_hex);

    // Open state and create accounts
    println!("\nStep 2: Creating accounts in state database...");
    let state = State::open(&state_path)?;

    // Check if accounts already exist
    if let Some(existing) = state.get_by_pk(&alice_pk)? {
        println!("  Alice account already exists:");
        println!("    ID: {}", existing.id.0);
        println!("    Balance: {}", existing.balance);
        println!("    Nonce: {}", existing.nonce);
    } else {
        // Create Alice's account
        let alice_account = state.create_account(alice_pk)?;
        println!("  Created Alice account - ID: {}", alice_account.id.0);

        // Give Alice initial balance
        let mut alice_acc = alice_account.clone();
        alice_acc.balance = 10000;
        state.apply_batch(&[alice_acc])?;
        println!("  Funded Alice with 10000 tokens");
    }

    if let Some(existing) = state.get_by_pk(&bob_pk)? {
        println!("  Bob account already exists:");
        println!("    ID: {}", existing.id.0);
        println!("    Balance: {}", existing.balance);
        println!("    Nonce: {}", existing.nonce);
    } else {
        // Create Bob's account
        let bob_account = state.create_account(bob_pk)?;
        println!("  Created Bob account - ID: {}", bob_account.id.0);
        println!("  Bob starts with 0 balance");
    }

    println!("\n=== Setup Complete ===\n");
    println!("Keys saved to: {}", keys_dir.display());
    println!("You can now run:");
    println!("  KEYS_DIR={} cargo run --example submit_txs", keys_dir.display());

    Ok(())
}
