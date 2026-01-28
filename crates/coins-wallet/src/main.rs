//! Coins Wallet Server
//!
//! A web-based wallet service for the Coins protocol.
//! Serves static files for the wallet UI and WASM module.

use axum::{routing::get, Router};
use std::env;
use tokio::net::TcpListener;
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
}

impl Config {
    fn from_env() -> Self {
        Self {
            port: env::var("WALLET_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8085),
            indexer_url: env::var("INDEXER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
            publisher_url: env::var("PUBLISHER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8082".to_string()),
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

    // Build router with static file serving
    let app = Router::new()
        .route("/health", get(health))
        // Serve WASM files at /wasm path
        .nest_service("/wasm", ServeDir::new("crates/coins-wallet/wasm/pkg"))
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
