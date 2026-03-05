//! Faucet service for distributing testnet tokens.
//!
//! Provides a web UI and REST API for users to claim tokens on testnets.
//! Uses BLS signatures to sign and submit transactions to the publisher.

mod api;
mod signer;

use anyhow::Result;
use clap::Parser;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use api::{create_router, AppState};
use signer::FaucetSigner;

/// Command-line arguments.
#[derive(Parser, Debug)]
#[command(name = "coins-faucet")]
#[command(about = "Faucet service for distributing testnet tokens")]
struct Args {
    /// Path to TOML configuration file
    #[arg(short, long, default_value = "config/faucet-regtest.toml")]
    config: String,
}

/// Configuration file structure.
#[derive(Debug, Deserialize)]
struct Config {
    faucet: FaucetConfig,
    server: ServerConfig,
    services: ServicesConfig,
}

#[derive(Debug, Deserialize)]
struct FaucetConfig {
    /// Path to the secret key file (hex-encoded)
    secret_key_file: String,
    /// Amount of tokens to dispense per claim
    #[serde(default = "default_amount")]
    amount_per_claim: u32,
    /// Cooldown period between claims (in seconds)
    #[serde(default = "default_cooldown")]
    cooldown_seconds: u64,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    /// Port to listen on
    #[serde(default = "default_port")]
    port: u16,
    /// Host to bind to
    #[serde(default = "default_host")]
    host: String,
}

#[derive(Debug, Deserialize)]
struct ServicesConfig {
    /// Indexer service URL
    indexer_url: String,
    /// Publisher service URL
    publisher_url: String,
    /// Explorer URL (user-facing, for linking from the faucet UI)
    #[serde(default = "default_explorer_url")]
    explorer_url: String,
}

fn default_amount() -> u32 {
    1000
}
fn default_cooldown() -> u64 {
    60
}
fn default_port() -> u16 {
    8086
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_explorer_url() -> String {
    "http://127.0.0.1:3000".to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_faucet=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command-line arguments
    let args = Args::parse();

    // Load configuration
    let config_str = std::fs::read_to_string(&args.config)
        .map_err(|e| anyhow::anyhow!("Failed to read config file {}: {}", args.config, e))?;
    let config: Config = toml::from_str(&config_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse config file: {}", e))?;

    info!(
        config_file = %args.config,
        port = config.server.port,
        indexer = %config.services.indexer_url,
        publisher = %config.services.publisher_url,
        amount_per_claim = config.faucet.amount_per_claim,
        cooldown = config.faucet.cooldown_seconds,
        "Starting faucet service"
    );

    // Load the faucet signer
    let secret_key_path = PathBuf::from(&config.faucet.secret_key_file);
    let signer = FaucetSigner::from_file(&secret_key_path)?;

    info!(
        faucet_address = %signer.public_key_hex(),
        "Faucet signer loaded"
    );

    // Create HTTP client
    let client = Client::new();

    // Verify we can reach the indexer
    match client
        .get(format!("{}/health", config.services.indexer_url))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            info!("Indexer is reachable");
        }
        Ok(resp) => {
            warn!(
                status = %resp.status(),
                "Indexer health check returned non-success status"
            );
        }
        Err(e) => {
            warn!(error = %e, "Could not reach indexer - will retry on first request");
        }
    }

    // Build application state
    let app_state = AppState {
        client,
        signer: Arc::new(RwLock::new(signer)),
        indexer_url: config.services.indexer_url,
        publisher_url: config.services.publisher_url,
        explorer_url: config.services.explorer_url,
        amount_per_claim: config.faucet.amount_per_claim,
        cooldown: Duration::from_secs(config.faucet.cooldown_seconds),
        claim_times: Arc::new(RwLock::new(HashMap::new())),
        total_claims: Arc::new(RwLock::new(0)),
    };

    // Set up CORS for development
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build the application with API routes and static file serving
    let app = create_router(app_state)
        .nest_service("/shared", ServeDir::new("shared/static"))
        .fallback_service(ServeDir::new("crates/coins-faucet/static"))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // Start the server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;

    info!(address = %addr, "Faucet server listening");

    axum::serve(listener, app).await?;

    Ok(())
}
