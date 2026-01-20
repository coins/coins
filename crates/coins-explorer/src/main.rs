use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use coins_explorer::{ExplorerConfig, simple_router, WsMessage};
use coins_indexer::IndexerClient;

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

    // Log publisher URL if configured
    if let Some(ref publisher_url) = config.indexer.publisher_url {
        tracing::info!("Publisher URL: {}", publisher_url);
    }

    // Create broadcast channel for WebSocket updates
    let (ws_tx, _) = broadcast::channel::<WsMessage>(100);

    // Spawn state monitoring task for WebSocket updates
    let ws_tx_bg = ws_tx.clone();
    let indexer_client_bg = indexer_client.clone();
    let publisher_url_bg = config.indexer.publisher_url.clone();
    tokio::spawn(async move {
        monitor_state_changes(indexer_client_bg, publisher_url_bg, ws_tx_bg).await;
    });

    // Create simple proxy router
    let app = simple_router(indexer_client, config.indexer.publisher_url.clone(), ws_tx);

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

/// Background task that monitors state changes and broadcasts WebSocket updates
async fn monitor_state_changes(
    indexer_client: Arc<IndexerClient>,
    publisher_url: Option<String>,
    ws_tx: broadcast::Sender<WsMessage>,
) {
    let mut last_block_height: Option<u32> = None;
    let mut last_pending_count: Option<usize> = None;
    let mut last_stats: Option<(u32, u64, u64)> = None;

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check for new blocks
        if let Ok(Some(latest_block)) = indexer_client.get_latest_block().await {
            let current_height = latest_block.height;
            if last_block_height != Some(current_height) {
                if last_block_height.is_some() {
                    // Only broadcast if this isn't the first check
                    let tx_count = latest_block.sub_block.txs.len();
                    let _ = ws_tx.send(WsMessage::NewBlock {
                        height: current_height,
                        btc_txid: latest_block.btc_txid.clone(),
                        tx_count,
                    });
                    tracing::debug!("Broadcast new block: height={}", current_height);
                }
                last_block_height = Some(current_height);
            }
        }

        // Check for stats changes
        if let Ok(stats) = indexer_client.get_stats().await {
            let current_stats = (stats.total_blocks, stats.total_accounts, stats.total_supply);
            if last_stats != Some(current_stats) {
                if last_stats.is_some() {
                    let _ = ws_tx.send(WsMessage::StatsUpdate {
                        total_blocks: stats.total_blocks,
                        total_accounts: stats.total_accounts,
                        total_supply: stats.total_supply,
                    });
                    tracing::debug!("Broadcast stats update");
                }
                last_stats = Some(current_stats);
            }
        }

        // Check for pending transaction changes
        if let Some(ref pub_url) = publisher_url {
            let pending_count = fetch_pending_count(pub_url).await;
            if last_pending_count != Some(pending_count) {
                if last_pending_count.is_some() {
                    let _ = ws_tx.send(WsMessage::PendingTxsUpdate { count: pending_count });
                    tracing::debug!("Broadcast pending txs update: count={}", pending_count);
                }
                last_pending_count = Some(pending_count);
            }
        }
    }
}

/// Fetch pending transaction count from publisher
async fn fetch_pending_count(publisher_url: &str) -> usize {
    let client = reqwest::Client::new();
    let mut count = 0;

    // Count mempool transactions
    if let Ok(response) = client.get(format!("{}/mempool", publisher_url)).send().await {
        if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
            count += txs.len();
        }
    }

    // Count recently broadcast transactions
    if let Ok(response) = client.get(format!("{}/recently-broadcast", publisher_url)).send().await {
        if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
            count += txs.len();
        }
    }

    count
}
