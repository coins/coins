use std::{path::PathBuf, fs};

use bitcoin::{Amount, Network};
use clap::Parser;
use coins_aggregator::engine::Engine;
use coins_aggregator::api::{router, AppState};
use coins_types::{Account, AccountId};
use coins_crypto::G1;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use hex;

#[derive(Parser, Debug)]
#[command(name="coins-aggregator", about="Run the Coins aggregator service")]
struct Opts {
    /// Path to the configuration file
    #[arg(short, long, default_value = "aggregator.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    esplora: String,
    spacechain: PathBuf,
    keyfile: PathBuf,
    interval: u64,
    network: Network,
    genesis_pk: String,
    genesis_balance: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let config_str = fs::read_to_string(opts.config)?;
    let config: Config = toml::from_str(&config_str)?;

    // spawn http server
    let pk_bytes = hex::decode(&config.genesis_pk)?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| anyhow::anyhow!("invalid genesis_pk length"))?;
    let genesis_pk = G1(pk_arr);

    let genesis_account = Account {
        id: AccountId(0),
        pk: genesis_pk,
        balance: config.genesis_balance,
        nonce: 0,
    };
    let app_state = AppState {
        accounts: std::sync::Arc::new(std::sync::Mutex::new(vec![genesis_account])),
        ..Default::default()
    };
    let router = router(app_state.clone());
    tokio::spawn(async move {
        let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
        axum::serve(listener, router).await.unwrap();
    });

    let mut engine = Engine::new(
        &config.esplora, 
        config.spacechain.clone(), 
        config.network, 
        Some(config.keyfile.clone()), 
        app_state
    ).await?;

    // Perform Initial Block Downsync to find current anchor and fee UTXOs
    engine.ibd().await?;

    println!("Fee address: {} – {} sats in {} utxos", engine.fee_addr, engine.fee_utxos.iter().map(|u| u.value).sum::<Amount>(), engine.fee_utxos.len());

    loop {
        println!("Main loop iteration started.");
        engine.refresh_anchor().await?;
        println!("Anchor refreshed.");
        engine.refresh_fee_utxos().await?;
        println!("Fee UTXOs refreshed.");
        engine.try_mine_subblock().await?;
        println!("Sub-block mining attempt finished.");
        println!("Anchor {}:{} | fee_utxos={} ", engine.current_anchor.txid, engine.current_anchor.vout, engine.fee_utxos.len());
        sleep(Duration::from_secs(config.interval)).await;
    }
} 