//! Submit many transactions at once with incrementing nonces
//!
//! Usage: cargo run --example submit_many_txs <count>

use coins_crypto::{SecretKey, G1, G2, sign};
use coins_types::{Transaction, Account};
use std::path::Path;
use std::fs;
use reqwest::blocking::Client;
use serde_json::json;
use ark_ff::PrimeField;
use ark_bn254::Fr;
use ark_serialize::CanonicalSerialize;

const PUBLISHER_URL: &str = "http://localhost:8080";

fn load_key(path: &str) -> Result<SecretKey, Box<dyn std::error::Error>> {
    let key_path = Path::new(path);
    let hex = fs::read_to_string(key_path)?;
    let bytes = hex::decode(hex.trim())?;
    let fr = Fr::from_le_bytes_mod_order(&bytes);
    Ok(SecretKey(fr))
}

fn get_account(pk_hex: &str) -> Result<Account, Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("{}/account/{}", PUBLISHER_URL, pk_hex);
    let resp = client.get(&url).send()?;
    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()).into());
    }
    Ok(resp.json()?)
}

fn submit_tx(tx: &Transaction, sig: &G2) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = format!("{}/tx", PUBLISHER_URL);
    let tx_bytes = bincode::serde::encode_to_vec(&tx, bincode::config::standard())?;
    let sig_bytes = bincode::serde::encode_to_vec(&sig, bincode::config::standard())?;
    let body = json!({
        "tx": hex::encode(tx_bytes),
        "signature": hex::encode(sig_bytes)
    });
    let resp = client.post(&url).json(&body).send()?;
    if !resp.status().is_success() {
        let error_text = resp.text().unwrap_or_else(|_| "unknown error".to_string());
        return Err(format!("Failed: {}", error_text).into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    println!("\n=== Submitting {} Transactions ===\n", count);

    let alice_sk = load_key(".data/test-keys/alice_sk.hex")?;
    let bob_sk = load_key(".data/test-keys/bob_sk.hex")?;

    let alice_pk_hex = hex::encode(alice_sk.public_key());
    let bob_pk_bytes = bob_sk.public_key();

    let alice_account = get_account(&alice_pk_hex)?;
    println!("Alice (ID {}): balance={}, nonce={}\n",
        alice_account.id.0, alice_account.balance, alice_account.nonce);

    let mut current_nonce = alice_account.nonce;
    let amount = 10u32;
    let fee = 1u8;

    for i in 0..count {
        let tx = Transaction {
            sender_id: alice_account.id.0,
            recipient_pk: G1(bob_pk_bytes),
            amount,
            fee,
        };
        let msg = tx.message_to_sign(current_nonce);
        let sig = sign(&alice_sk, &msg);

        match submit_tx(&tx, &sig) {
            Ok(_) => println!("  [{}] ✓ Submitted (nonce={})", i + 1, current_nonce),
            Err(e) => println!("  [{}] ✗ Error: {}", i + 1, e),
        }

        current_nonce += 1;
    }

    println!("\n=== Done! {} transactions submitted ===\n", count);
    Ok(())
}
