use std::{fs, path::PathBuf, str::FromStr};

use clap::Parser;
use coins_spacechain::Spacechain;
use bitcoin::{Amount, Network, OutPoint, PrivateKey, Txid, Address};
use serde::Deserialize;
use anyhow::{anyhow, Result};
use esplora_client::{AsyncClient, Builder};

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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let opts = Opts::parse();

    let config_str = fs::read_to_string(opts.config).expect("failed to read config file");
    let config: Config = toml::from_str(&config_str).expect("failed to parse config file");

    let network = parse_network(&config.network).expect("invalid network");

    // always create new key
    let (sk, _pk, addr) = Spacechain::new_key(network);
    println!("Generated one-time address: {}", addr);

    println!("\nSend funds to {} then wait for the UTXO to appear …", addr);

    // --- Esplora client ---
    let default_esplora = match network {
        Network::Bitcoin=>"https://mempool.space/api".to_string(),
        Network::Testnet=>"https://blockstream.info/testnet/api".to_string(),
        Network::Signet=>"https://mempool.space/signet/api".to_string(),
        Network::Regtest=>{eprintln!("No default esplora URL for regtest – set ESPLORA_URL env var");std::process::exit(1);}
        _ => todo!(),
    };

    let esplora_url = std::env::var("ESPLORA_URL").unwrap_or(default_esplora);
    let client: AsyncClient<esplora_client::r#async::DefaultSleeper> =
        AsyncClient::from_builder(Builder::new(&esplora_url))
            .expect("create esplora client");

    println!("Waiting for confirmed UTXO at {addr} … (polling Esplora {})", esplora_url);
    let funding_utxo = loop {
        let utxos = client.get_address_utxo(addr.clone()).await?;
        if let Some(u) = utxos.first() {
            break u.clone();
        }
        println!("No UTXO yet – sleeping 30s …");
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    };

    let outpoint = OutPoint::new(funding_utxo.txid, funding_utxo.vout as u32);

    println!("\nBuilding spacechain…");
    let value = funding_utxo.value;
    let pk = PrivateKey::new(sk, network);
    let sc = Spacechain::generate_with_private_key(config.count, outpoint, value, network, &pk);

    let blob = sc.encode();
    fs::write(&config.output, &blob)?;

    let size_mb = (blob.len() as f64) / 1_048_576.0_f64;
    println!(
        "Wrote {} anchorTxs ({:.2} MB, value {} sat each) to {}",
        sc.sigs.len(),
        size_mb,
        value.to_sat(),
        config.output.display()
    );

    Ok(())
} 