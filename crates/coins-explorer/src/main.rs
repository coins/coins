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
    let mut last_btc_height: Option<u32> = None;
    let mut last_pending_state: Option<PendingState> = None;
    let mut last_stats: Option<(u32, u64, u64)> = None;
    let mut is_first_check = true;

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check for new blocks
        if let Ok(Some(latest_block)) = indexer_client.get_latest_block().await {
            let current_height = latest_block.height;
            if last_block_height != Some(current_height) {
                if !is_first_check {
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
                if !is_first_check {
                    let _ = ws_tx.send(WsMessage::StatsUpdate {
                        total_blocks: stats.total_blocks,
                        total_accounts: stats.total_accounts,
                        total_supply: stats.total_supply,
                    });
                    tracing::debug!("Broadcast stats update");
                }
                last_stats = Some(current_stats);
            }

            // Check for Bitcoin block height changes (affects confirmations)
            let current_btc_height = stats.btc_height;
            if current_btc_height > 0 && last_btc_height != Some(current_btc_height) {
                if !is_first_check {
                    let _ = ws_tx.send(WsMessage::ConfirmationUpdate {
                        btc_height: current_btc_height,
                    });
                    tracing::debug!("Broadcast confirmation update: btc_height={}", current_btc_height);
                }
                last_btc_height = Some(current_btc_height);
            }
        }

        // Check for pending transaction changes (count AND status distribution)
        if let Some(ref pub_url) = publisher_url {
            let current_state = fetch_pending_state(pub_url, &indexer_client).await;
            if last_pending_state.as_ref() != Some(&current_state) {
                if !is_first_check {
                    let _ = ws_tx.send(WsMessage::PendingTxsUpdate { count: current_state.total() });
                    tracing::debug!(
                        "Broadcast pending txs update: publishing={}, broadcasting={}, unconfirmed={}",
                        current_state.publishing, current_state.broadcasting, current_state.unconfirmed
                    );
                }
                last_pending_state = Some(current_state);
            }
        }

        is_first_check = false;
    }
}

/// Tracks the distribution of pending transactions across states
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingState {
    publishing: usize,
    broadcasting: usize,
    unconfirmed: usize,
}

impl PendingState {
    fn total(&self) -> usize {
        self.publishing + self.broadcasting + self.unconfirmed
    }
}

/// Fetch pending transaction state from publisher
/// Returns the distribution of transactions across publishing, broadcasting, and unconfirmed states
async fn fetch_pending_state(publisher_url: &str, indexer_client: &IndexerClient) -> PendingState {
    let client = reqwest::Client::new();
    let mut state = PendingState {
        publishing: 0,
        broadcasting: 0,
        unconfirmed: 0,
    };

    // Count mempool transactions (publishing)
    if let Ok(response) = client.get(format!("{}/mempool", publisher_url)).send().await {
        if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
            state.publishing = txs.len();
        }
    }

    // Count recently broadcast transactions and check their indexer status
    if let Ok(response) = client.get(format!("{}/recently-broadcast", publisher_url)).send().await {
        if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
            for tx in txs {
                if let Some(btc_txid) = tx.get("btc_txid").and_then(|v| v.as_str()) {
                    // Check if indexed
                    match indexer_client.get_block_confirmation(btc_txid).await {
                        Ok(Some(_)) => {
                            // Indexed - unconfirmed (or confirmed, but still in recently-broadcast)
                            state.unconfirmed += 1;
                        }
                        _ => {
                            // Not indexed yet - still broadcasting
                            state.broadcasting += 1;
                        }
                    }
                } else {
                    // No btc_txid - still broadcasting
                    state.broadcasting += 1;
                }
            }
        }
    }

    state
}
