//! Coins Wallet Server
//!
//! A web-based wallet service for the Coins protocol.
//! Serves static files for the wallet UI and WASM module.
//! Includes WebSocket support for real-time updates.

use axum::{routing::get, Router};
use coins_wallet::api::{self, AppState, WsMessage};
use reqwest::Client;
use std::env;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Application configuration read from environment variables
struct Config {
    /// Port to listen on (default: 8085)
    port: u16,
    /// Indexer service URL
    indexer_url: String,
    /// Publisher service URL
    publisher_url: String,
    /// Explorer service URL
    explorer_url: String,
    /// Faucet URL (for linking to faucet from wallet UI)
    faucet_url: String,
}

impl Config {
    fn from_env() -> Self {
        Self {
            port: env::var("WALLET_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8085),
            indexer_url: env::var("INDEXER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8083".to_string()),
            publisher_url: env::var("PUBLISHER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8082".to_string()),
            explorer_url: env::var("EXPLORER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
            faucet_url: env::var("FAUCET_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8086".to_string()),
        }
    }
}

async fn health() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_wallet=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration from environment
    let config = Config::from_env();

    info!("Starting Coins Wallet Server...");
    info!("Indexer URL: {}", config.indexer_url);
    info!("Publisher URL: {}", config.publisher_url);
    info!("Explorer URL: {}", config.explorer_url);
    info!("Faucet URL: {}", config.faucet_url);

    // Create WebSocket broadcast channel
    let (ws_tx, _) = broadcast::channel::<WsMessage>(100);

    // Create HTTP client for API proxying
    let client = Client::new();
    let app_state = AppState {
        client: client.clone(),
        indexer_url: config.indexer_url.clone(),
        publisher_url: config.publisher_url.clone(),
        explorer_url: config.explorer_url.clone(),
        faucet_url: config.faucet_url.clone(),
    };

    // Spawn background task to monitor indexer and explorer for changes
    let monitor_client = client.clone();
    let monitor_indexer_url = config.indexer_url.clone();
    let monitor_explorer_url = config.explorer_url.clone();
    let monitor_ws_tx = ws_tx.clone();
    tokio::spawn(async move {
        api::monitor_indexer_changes(monitor_client, monitor_indexer_url, monitor_explorer_url, monitor_ws_tx).await;
    });

    // Build router with API endpoints, WebSocket, and static file serving
    let app = Router::new()
        .route("/health", get(health))
        // Merge API proxy routes with WebSocket support
        .merge(api::create_router_with_ws(app_state, ws_tx))
        // Serve WASM files at /wasm path
        .nest_service("/wasm", ServeDir::new("crates/coins-wallet/wasm/pkg"))
        // Serve shared static files
        .nest_service("/shared", ServeDir::new("shared/static"))
        // Serve static files at root (must be last)
        .fallback_service(ServeDir::new("crates/coins-wallet/static"));

    // Start server
    let addr = format!("127.0.0.1:{}", config.port);
    info!("Listening on http://{}", addr);
    info!("Web UI: http://{}", addr);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
