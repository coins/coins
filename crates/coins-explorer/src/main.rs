use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use coins_explorer::{ExplorerConfig, ExplorerIndexer};
use coins_explorer::api;

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

    tracing::info!("Starting Coins Block Explorer...");
    tracing::info!("Configuration: {:#?}", config);

    // Initialize explorer indexer
    let indexer = Arc::new(ExplorerIndexer::open(&config)?);

    // Build transaction index if needed
    indexer.build_tx_index_if_needed().await?;

    // Create API router
    let app = api::create_router(indexer.clone());

    // Start background update task
    let indexer_clone = indexer.clone();
    let app_state_for_bg = api::AppState {
        indexer: indexer_clone.clone(),
        ws_state: Arc::new(api::websocket::WebSocketState::new()),
    };

    tokio::spawn(async move {
        background_updater(indexer_clone, app_state_for_bg.ws_state).await;
    });

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

async fn background_updater(
    indexer: Arc<ExplorerIndexer>,
    ws_state: Arc<api::websocket::WebSocketState>,
) {
    let mut last_block_count = 0usize;

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Check for new blocks
        match get_block_count(&indexer).await {
            Ok(current_count) => {
                if current_count > last_block_count {
                    tracing::info!(
                        old_count = last_block_count,
                        new_count = current_count,
                        "New blocks detected"
                    );

                    // Get the latest block and broadcast
                    if let Ok(Some(latest_height)) = get_latest_block_height(&indexer).await {
                        if let Ok(current_btc_height) = indexer.bitcoin_client.get_block_height() {
                            if let Ok(Some(chain_block)) = get_chain_block(&indexer, latest_height).await {
                                let confirmations = current_btc_height.saturating_sub(latest_height) + 1;
                                let timestamp = indexer.bitcoin_client.get_block_timestamp(latest_height).ok().flatten();

                                let block_summary = coins_explorer::models::BlockSummary {
                                    btc_height: latest_height,
                                    btc_txid: chain_block.btc_txid.to_string(),
                                    btc_confirmations: confirmations,
                                    tx_count: chain_block.sub_block.txs.len(),
                                    publisher_pk: hex::encode(&chain_block.sub_block.publisher_pk.0),
                                    timestamp,
                                    finalized: confirmations >= coins_indexer::FINALITY_DEPTH,
                                };

                                ws_state.broadcast(api::websocket::ServerMessage::NewBlock {
                                    block: block_summary,
                                });

                                tracing::debug!("Broadcasted new block update via WebSocket");
                            }
                        }
                    }

                    last_block_count = current_count;

                    // Update transaction index with new blocks
                    if let Err(e) = update_tx_index_incrementally(&indexer, last_block_count, current_count).await {
                        tracing::error!(error = ?e, "Failed to update transaction index");
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to get block count");
            }
        }
    }
}

async fn get_block_count(indexer: &ExplorerIndexer) -> Result<usize> {
    let mut count = 0;
    for _ in indexer.indexer.blocks.iter() {
        count += 1;
    }
    Ok(count)
}

async fn get_latest_block_height(indexer: &ExplorerIndexer) -> Result<Option<u32>> {
    let mut latest = None;
    for item in indexer.indexer.blocks.iter().rev() {
        let (key, _) = item?;
        latest = Some(u32::from_le_bytes(key.as_ref().try_into()?));
        break;
    }
    Ok(latest)
}

async fn get_chain_block(indexer: &ExplorerIndexer, height: u32) -> Result<Option<coins_indexer::ChainBlock>> {
    let key = height.to_le_bytes();
    if let Some(value) = indexer.indexer.blocks.get(&key)? {
        Ok(coins_indexer::ChainBlock::deserialize(&value, &indexer.state))
    } else {
        Ok(None)
    }
}

async fn update_tx_index_incrementally(
    indexer: &ExplorerIndexer,
    _old_count: usize,
    _new_count: usize,
) -> Result<()> {
    // Get the latest blocks and index their transactions
    // For simplicity, we scan the last few blocks
    let mut indexed = 0;

    for item in indexer.indexer.blocks.iter().rev().take(10) {
        let (key, value) = item?;
        let btc_height = u32::from_le_bytes(key.as_ref().try_into()?);

        if let Some(chain_block) = coins_indexer::ChainBlock::deserialize(&value, &indexer.state) {
            for (tx_offset, tx) in chain_block.sub_block.txs.iter().enumerate() {
                // Only index if not already indexed
                if indexer.tx_index.get_transaction(btc_height, tx_offset as u32)?.is_none() {
                    indexer.tx_index.index_transaction(tx, btc_height, tx_offset as u32)?;
                    indexed += 1;
                }
            }
        }
    }

    if indexed > 0 {
        tracing::debug!(count = indexed, "Indexed new transactions");
    }

    Ok(())
}
