//! Get public key from a secret key file
//!
//! Usage: cargo run --example get_pk <secret_key_file>
//!
//! Outputs the hex-encoded public key to stdout.

use ark_bn254::Fr;
use ark_ff::PrimeField;
use coins_crypto::SecretKey;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <secret_key_file>", args[0]);
        std::process::exit(1);
    }

    let key_path = Path::new(&args[1]);
    if !key_path.exists() {
        eprintln!("Error: File not found: {}", args[1]);
        std::process::exit(1);
    }

    let hex = std::fs::read_to_string(key_path)?;
    let bytes = hex::decode(hex.trim())?;
    let fr = Fr::from_le_bytes_mod_order(&bytes);
    let sk = SecretKey(fr);
    let pk_bytes = sk.public_key();

    println!("{}", hex::encode(pk_bytes));
    Ok(())
}
