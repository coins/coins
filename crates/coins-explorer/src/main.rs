use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use coins_explorer::ExplorerConfig;
use coins_indexer::IndexerClient;

mod api_simple;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config/explorer-regtest.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_explorer=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command line arguments
    let args = Args::parse();

    // Load configuration
    tracing::info!("Loading configuration from: {}", args.config);
    let config = ExplorerConfig::load(&args.config)?;

    tracing::info!("Starting Coins Block Explorer (Simple Proxy Mode)...");
    tracing::info!("Indexer URL: {}", config.indexer.url);

    // Create indexer client
    let indexer_client = Arc::new(IndexerClient::new(config.indexer.url.clone()));

    // Test connection
    indexer_client.health().await?;
    tracing::info!("Connected to indexer service");

    // Create simple proxy router
    let app = api_simple::simple_router(indexer_client);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.api_port);
    tracing::info!("Listening on http://{}", addr);
    tracing::info!("Web UI: http://{}", addr);
    tracing::info!("API: http://{}/api/v1", addr);
    tracing::info!("WebSocket: ws://{}/ws", addr);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
