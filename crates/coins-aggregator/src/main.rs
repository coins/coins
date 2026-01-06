use std::{path::PathBuf, fs, sync::Arc};

use bitcoin::Network;
use clap::Parser;
use coins_aggregator::engine::Engine;
use coins_aggregator::api::{router, AppState};
use coins_aggregator::blockchain_backend::BlockchainBackend;
use coins_aggregator::rpc_backend::RpcBackend;
use coins_aggregator::esplora_backend::EsploraBackend;
use coins_crypto::G1;
use coins_state::State;
use coins_indexer::Indexer;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use hex;

#[derive(Parser, Debug)]
#[command(name="coins-aggregator", about="Run the Coins aggregator service")]
struct Opts {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config/aggregator.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    // Esplora config (for signet/mainnet)
    #[serde(default)]
    esplora: Option<String>,

    // RPC config (for regtest)
    #[serde(default)]
    rpc_url: Option<String>,
    #[serde(default)]
    rpc_user: Option<String>,
    #[serde(default)]
    rpc_pass: Option<String>,
    #[serde(default = "default_wallet_name")]
    rpc_wallet: String,

    // Common config
    subchain: PathBuf,
    keyfile: PathBuf,
    interval: u64,
    network: Network,
    genesis_pk: String,
    genesis_balance: u64,
}

fn default_wallet_name() -> String {
    "coins-aggregator".to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_aggregator=info,coins_validator=info,coins_crypto=debug".into())
        )
        .init();

    let opts = Opts::parse();

    let config_str = fs::read_to_string(opts.config)?;
    let config: Config = toml::from_str(&config_str)?;

    // Initialize persistent state
    let state_path = PathBuf::from("./state.db");
    let state = Arc::new(State::open(&state_path)?);
    tracing::info!("Opened persistent state database");

    // Parse genesis public key
    let pk_bytes = hex::decode(&config.genesis_pk)?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| anyhow::anyhow!("invalid genesis_pk length"))?;
    let genesis_pk = G1(pk_arr);

    // Create or load genesis account
    if state.get_by_pk(&genesis_pk)?.is_none() {
        let genesis_account = state.create_account(genesis_pk)?;
        tracing::info!(id = genesis_account.id.0, "Created genesis account");

        // Set genesis balance (this is a demo - in production this would be more controlled)
        let mut acc = genesis_account;
        acc.balance = config.genesis_balance;
        state.apply_batch(&[acc])?;
        tracing::info!(balance = config.genesis_balance, "Set genesis balance");
    }

    // Initialize indexer
    let indexer_path = PathBuf::from("./indexer.db");
    let indexer = Arc::new(Indexer::open(&indexer_path, state.clone())?);
    tracing::info!("Opened indexer database");

    // Create app state
    let app_state = AppState {
        state: state.clone(),
        indexer: indexer.clone(),
        mempool: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let router = router(app_state.clone());
    tokio::spawn(async move {
        let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
        axum::serve(listener, router).await.unwrap();
    });

    // ===== BACKEND SELECTION =====
    let backend: Arc<dyn BlockchainBackend> = match config.network {
        Network::Regtest => {
            // Validate RPC config exists
            let rpc_url = config.rpc_url
                .ok_or_else(|| anyhow::anyhow!("rpc_url required for network=regtest"))?;
            let rpc_user = config.rpc_user
                .ok_or_else(|| anyhow::anyhow!("rpc_user required for network=regtest"))?;
            let rpc_pass = config.rpc_pass
                .ok_or_else(|| anyhow::anyhow!("rpc_pass required for network=regtest"))?;

            tracing::info!(
                rpc_url = %rpc_url,
                wallet = %config.rpc_wallet,
                "Using Bitcoin RPC backend"
            );

            let rpc_backend = RpcBackend::new(
                rpc_url,
                rpc_user,
                rpc_pass,
                config.rpc_wallet,
            )?;

            Arc::new(rpc_backend) as Arc<dyn BlockchainBackend>
        }

        Network::Signet | Network::Bitcoin | Network::Testnet => {
            // Validate Esplora config exists
            let esplora_url = config.esplora
                .ok_or_else(|| anyhow::anyhow!("esplora URL required for network={:?}", config.network))?;

            tracing::info!(
                esplora_url = %esplora_url,
                "Using Esplora backend"
            );

            let esplora_backend = EsploraBackend::new(&esplora_url)?;
            Arc::new(esplora_backend) as Arc<dyn BlockchainBackend>
        }

        other => return Err(anyhow::anyhow!("Unsupported network: {:?}", other)),
    };

    // ===== ENGINE INITIALIZATION =====
    let mut engine = Engine::new(
        backend,
        config.subchain.clone(),
        config.network,
        Some(config.keyfile.clone()),
        app_state
    ).await?;

    // Perform Initial Block Downsync to find current anchor and fee UTXOs
    engine.ibd().await?;

    let total_sats: bitcoin::Amount = engine.fee_utxos.iter().map(|u| u.value).sum();
    tracing::info!(
        fee_address = %engine.fee_addr,
        total_sats = %total_sats,
        utxo_count = engine.fee_utxos.len(),
        "Aggregator initialized"
    );

    loop {
        tracing::debug!("Main loop iteration started");
        engine.refresh_anchor().await?;
        tracing::debug!("Anchor refreshed");
        engine.refresh_fee_utxos().await?;
        tracing::debug!("Fee UTXOs refreshed");
        engine.try_mine_subblock().await?;
        tracing::debug!("Sub-block mining attempt finished");
        tracing::info!(
            anchor_txid = %engine.current_anchor.txid,
            anchor_vout = engine.current_anchor.vout,
            fee_utxos = engine.fee_utxos.len(),
            "Main loop iteration complete"
        );
        sleep(Duration::from_secs(config.interval)).await;
    }
} 