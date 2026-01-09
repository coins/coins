use std::{path::PathBuf, fs, sync::Arc};

use bitcoin::Network;
use clap::Parser;
use coins_publisher::engine::Engine;
use coins_publisher::api::{router, AppState};
use coins_bitcoin_rpc::RpcBackend;
use coins_publisher::state_adapter::StateAdapter;
use coins_indexer::IndexerClient;
use coins_subchain::{PublishMode, PublishFormat};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};

#[derive(Parser, Debug)]
#[command(name="coins-publisher", about="Run the Coins publisher service")]
struct Opts {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config/publisher.toml")]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    // Bitcoin Core RPC config (regtest only)
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

    // Optional runtime config
    #[serde(default = "default_api_port")]
    api_port: u16,
    #[serde(default = "default_indexer_url")]
    indexer_url: String,
    #[serde(default = "default_bls_keyfile")]
    bls_keyfile: PathBuf,

    // Publishing config
    #[serde(default = "default_publish_format")]
    publish_format: String,
    #[serde(default = "default_fee_rate")]
    fee_rate_sat_per_vb: u64,
    #[serde(default)]
    dual_primary: Option<String>,
}

impl Config {
    /// Parse the publish_format string into a PublishMode
    fn parse_publish_mode(&self) -> anyhow::Result<PublishMode> {
        match self.publish_format.as_str() {
            "op_return" => Ok(PublishMode::Single(PublishFormat::OpReturn)),
            "taproot_annex" => Ok(PublishMode::Single(PublishFormat::TaprootAnnex)),
            "dual" => {
                let primary = match self.dual_primary.as_deref() {
                    Some("op_return") => PublishFormat::OpReturn,
                    Some("taproot_annex") => PublishFormat::TaprootAnnex,
                    Some(other) => {
                        return Err(anyhow::anyhow!("Invalid dual_primary: '{}'. Use 'op_return' or 'taproot_annex'", other));
                    }
                    None => PublishFormat::OpReturn, // default to OP_RETURN if not specified
                };
                let secondary = primary.other();
                Ok(PublishMode::Dual { primary, secondary })
            }
            other => {
                Err(anyhow::anyhow!(
                    "Invalid publish_format: '{}'. Use 'op_return', 'taproot_annex', or 'dual'",
                    other
                ))
            }
        }
    }
}

fn default_wallet_name() -> String {
    "coins-publisher".to_string()
}

fn default_api_port() -> u16 {
    8080  // Regtest default; signet uses 8081 (set in config)
}

fn default_indexer_url() -> String {
    "http://localhost:8083".to_string()
}

fn default_bls_keyfile() -> PathBuf {
    PathBuf::from(".data/regtest/keys/publisher_bls_sk.hex")
}

fn default_publish_format() -> String {
    // Can be overridden via environment variable
    std::env::var("DEFAULT_PUBLISH_FORMAT")
        .unwrap_or_else(|_| "op_return".to_string())
}

fn default_fee_rate() -> u64 {
    4
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_publisher=info,coins_validator=info,coins_crypto=debug".into())
        )
        .init();

    let opts = Opts::parse();

    let config_str = fs::read_to_string(opts.config)?;
    let config: Config = toml::from_str(&config_str)?;

    // Initialize indexer client
    let indexer_client = Arc::new(IndexerClient::new(config.indexer_url.clone()));
    tracing::info!(url = %config.indexer_url, "Connected to indexer service");

    // Create state adapter for validation/serialization
    let state_adapter = StateAdapter::new(indexer_client.clone());

    // Create app state
    let app_state = AppState {
        indexer: indexer_client.clone(),
        mempool: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let router = router(app_state.clone());
    let api_addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!(addr = %api_addr, "Starting API server");
    tokio::spawn(async move {
        let listener = TcpListener::bind(&api_addr).await.unwrap();
        axum::serve(listener, router).await.unwrap();
    });

    // ===== PARSE PUBLISHING CONFIG FIRST (before config is moved) =====
    let publish_mode = config.parse_publish_mode()?;
    let fee_rate = config.fee_rate_sat_per_vb;
    tracing::info!(
        mode = ?publish_mode,
        fee_rate = fee_rate,
        "Parsed publishing configuration"
    );

    // ===== BACKEND SELECTION =====
    let backend: Arc<RpcBackend> = match config.network {
        Network::Regtest | Network::Signet => {
            // Validate RPC config exists
            let rpc_url = config.rpc_url
                .ok_or_else(|| anyhow::anyhow!("rpc_url required for network={:?}", config.network))?;
            let rpc_user = config.rpc_user
                .ok_or_else(|| anyhow::anyhow!("rpc_user required for network={:?}", config.network))?;
            let rpc_pass = config.rpc_pass
                .ok_or_else(|| anyhow::anyhow!("rpc_pass required for network={:?}", config.network))?;

            tracing::info!(
                network = ?config.network,
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

            Arc::new(rpc_backend)
        }

        other => {
            return Err(anyhow::anyhow!(
                "Network {:?} not supported. Use 'regtest' or 'signet' with RPC backend.",
                other
            ));
        }
    };

    // ===== ENGINE INITIALIZATION =====
    let mut engine = Engine::new(
        backend,
        config.subchain.clone(),
        config.network,
        Some(config.keyfile.clone()),
        config.bls_keyfile.clone(),
        app_state,
        state_adapter,
        publish_mode,
        fee_rate,
    ).await?;

    // Perform Initial Block Downsync to find current anchor and fee UTXOs
    engine.ibd().await?;

    let total_sats: bitcoin::Amount = engine.fee_utxos.iter().map(|u| u.value).sum();
    tracing::info!(
        publisher_address = %engine.fee_addr,
        total_sats = %total_sats,
        utxo_count = engine.fee_utxos.len(),
        "Publisher initialized"
    );

    loop {
        tracing::debug!("Main loop iteration started");

        // Sync with indexer to catch any blocks we missed (e.g. after restart)
        engine.sync_from_indexer().await?;
        tracing::debug!("Synced with indexer");

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