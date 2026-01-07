//! Setup test accounts for e2e testing
//!
//! This adds Alice and Bob accounts to the state database with initial balances.
//!
//! Usage: cargo run --example setup_test_accounts

use coins_crypto::G1;
use coins_state::State;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Setup Test Accounts ===\n");

    // Alice and Bob public keys from submit_txs
    let alice_pk_hex = "2fa09cfde49a9c593bee32d5297a413d5ee2f8956cd8a2324fb8e523b2196d8f";
    let bob_pk_hex = "5e74734c69fbb261c4c936d375df870f2a6af117f811a5c88f8c3328f291c012";

    let alice_pk_bytes = hex::decode(alice_pk_hex)?;
    let bob_pk_bytes = hex::decode(bob_pk_hex)?;

    let alice_pk_arr: [u8; 32] = alice_pk_bytes.try_into().unwrap();
    let bob_pk_arr: [u8; 32] = bob_pk_bytes.try_into().unwrap();

    let alice_pk = G1(alice_pk_arr);
    let bob_pk = G1(bob_pk_arr);

    // Open state database (use argument or default)
    let state_path = std::env::args().nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./state.db"));
    println!("Using state database: {}", state_path.display());
    let state = State::open(&state_path)?;

    // Check if accounts already exist
    if let Some(existing) = state.get_by_pk(&alice_pk)? {
        println!("Alice account already exists:");
        println!("  ID: {}", existing.id.0);
        println!("  Balance: {}", existing.balance);
        println!("  Nonce: {}", existing.nonce);
    } else {
        // Create Alice's account
        let alice_account = state.create_account(alice_pk)?;
        println!("Created Alice account - ID: {}", alice_account.id.0);

        // Give Alice initial balance
        let mut alice_acc = alice_account.clone();
        alice_acc.balance = 10000;
        state.apply_batch(&[alice_acc])?;
        println!("Funded Alice with 10000 tokens");
    }

    println!();

    if let Some(existing) = state.get_by_pk(&bob_pk)? {
        println!("Bob account already exists:");
        println!("  ID: {}", existing.id.0);
        println!("  Balance: {}", existing.balance);
        println!("  Nonce: {}", existing.nonce);
    } else {
        // Create Bob's account
        let bob_account = state.create_account(bob_pk)?;
        println!("Created Bob account - ID: {}", bob_account.id.0);
        println!("Bob starts with 0 balance");
    }

    println!("\n=== Setup Complete ===\n");
    println!("You can now run: cargo run --example submit_txs");

    Ok(())
}
