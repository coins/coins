use std::{fs, path::PathBuf, str::FromStr};

use clap::Parser;
use coins_spacechain::Spacechain;
use bitcoin::{Amount, Network, OutPoint, PrivateKey, Txid};
use serde::Deserialize;

/// spacechain-setup: interactive trusted setup generator for anchor transactions.
#[derive(Debug, Parser)]
#[command(name = "spacechain-setup", version, author, about = "Trusted setup generator for Coins spacechain anchor transactions")]
struct Opts {
    /// Path to the configuration file
    #[arg(short, long, default_value = "spacechain.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    count: usize,
    value: u64,
    network: String,
    output: PathBuf,
}

fn parse_network(s: &str) -> Option<Network> {
    match s.to_lowercase().as_str() {
        "mainnet" | "main" | "bitcoin" => Some(Network::Bitcoin),
        "testnet" | "test" => Some(Network::Testnet),
        "signet" => Some(Network::Signet),
        "regtest" | "reg" => Some(Network::Regtest),
        _ => None,
    }
}

fn parse_outpoint(s: &str) -> Result<OutPoint, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err("outpoint must be <txid>:<vout>".into());
    }
    let txid = Txid::from_str(parts[0]).map_err(|e| format!("invalid txid: {e}"))?;
    let vout: u32 = parts[1]
        .parse()
        .map_err(|_| "vout must be a number".to_string())?;
    Ok(OutPoint::new(txid, vout))
}

fn main() {
    let opts = Opts::parse();

    let config_str = fs::read_to_string(opts.config).expect("failed to read config file");
    let config: Config = toml::from_str(&config_str).expect("failed to parse config file");

    let network = parse_network(&config.network).expect("invalid network");

    // always create new key
    let (sk, _pk, addr) = Spacechain::new_key(network);
    println!("Generated one-time address: {}", addr);

    // prompt for outpoint
    println!("\nSend the required funds to {} then enter the funding outpoint <txid>:<vout>:", addr);
    use std::io::{self, Write};
    print!("> ");
    io::stdout().flush().unwrap();
    let mut line = String::new();
    io::stdin().read_line(&mut line).expect("stdin");
    let outpoint_str = line.trim().to_string();

    let outpoint = parse_outpoint(&outpoint_str).expect("outpoint format");

    println!("\nBuilding spacechain…");
    let value = Amount::from_sat(config.value);
    let pk = PrivateKey::new(sk, network);
    let sc = Spacechain::generate_with_private_key(config.count, outpoint, value, network, &pk);

    let blob = sc.encode();
    if let Err(e) = fs::write(&config.output, &blob) {
        eprintln!("Failed to write {}: {e}", config.output.display());
        std::process::exit(1);
    }

    let size_mb = (blob.len() as f64) / 1_048_576.0_f64;
    println!(
        "Wrote {} anchorTxs ({:.2} MB) to {}",
        sc.sigs.len(),
        size_mb,
        config.output.display()
    );
} 