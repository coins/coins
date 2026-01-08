mod api;
mod config;

use anyhow::Result;
use std::sync::Arc;
use coins_core::State;
use coins_crypto::G1;
use coins_indexer::Indexer;
use config::IndexerConfig;
use api::{AppState, create_router};
use tower_http::cors::{CorsLayer, Any};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coins_indexer=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .get(1)
        .and_then(|arg| {
            if arg == "--config" {
                args.get(2).map(|s| s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("config/indexer-regtest.toml");

    tracing::info!("Loading configuration from: {}", config_path);
    let config = IndexerConfig::from_file(config_path)?;
    tracing::info!("Configuration loaded: {:#?}", config);

    // Open databases
    tracing::info!("Opening state database: {:?}", config.database.state_db);
    let state = Arc::new(State::open(&config.database.state_db)?);

    // Initialize genesis account if needed
    let genesis_pk_bytes = hex::decode(&config.genesis.genesis_pk)?;
    let genesis_pk_arr: [u8; 32] = genesis_pk_bytes.try_into()
        .map_err(|_| anyhow::anyhow!("Invalid genesis public key"))?;
    let genesis_pk = G1(genesis_pk_arr);

    if state.get_by_pk(&genesis_pk)?.is_none() {
        tracing::info!("Creating genesis account");
        let mut genesis = state.create_account(genesis_pk)?;
        genesis.balance = config.genesis.genesis_balance;
        let genesis_id = genesis.id.0;
        state.apply_batch(&[genesis])?;
        tracing::info!(
            id = genesis_id,
            balance = config.genesis.genesis_balance,
            "Genesis account created"
        );
    } else {
        tracing::info!("Genesis account already exists");
    }

    tracing::info!("Opening indexer database: {:?}", config.database.indexer_db);
    let indexer = Arc::new(Indexer::open(&config.database.indexer_db, state.clone())?);

    // Create application state with Bitcoin RPC access
    let rpc_auth = bitcoincore_rpc::Auth::UserPass(
        config.bitcoin.rpc_user.clone(),
        config.bitcoin.rpc_pass.clone(),
    );
    let app_state = AppState::new(state, indexer.clone(), config.bitcoin.rpc_url.clone(), rpc_auth.clone());

    // Start background task to update Bitcoin block heights
    let indexer_bg = indexer.clone();
    let rpc_url_bg = config.bitcoin.rpc_url.clone();
    let rpc_auth_bg = rpc_auth.clone();
    tokio::spawn(async move {
        // Do initial historical scan for blocks with btc_height=0
        if let Err(e) = scan_historical_blocks(&indexer_bg, &rpc_url_bg, &rpc_auth_bg).await {
            tracing::warn!("Historical block scan failed: {}", e);
        }
        // Then start continuous monitoring
        update_btc_heights_loop(indexer_bg, rpc_url_bg, rpc_auth_bg).await;
    });

    // Create router
    let app = create_router(app_state)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.api_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Indexer API server listening on {}", addr);
    tracing::info!("Endpoints:");
    tracing::info!("  GET  /health");
    tracing::info!("  GET  /accounts/:pk");
    tracing::info!("  GET  /accounts/:pk/transactions");
    tracing::info!("  GET  /blocks/latest");
    tracing::info!("  GET  /blocks/:height");
    tracing::info!("  GET  /blocks?from=X&to=Y");
    tracing::info!("  POST /blocks");
    tracing::info!("  GET  /stats");

    axum::serve(listener, app).await?;

    Ok(())
}

/// Scan recent Bitcoin blocks to find and update blocks with btc_height=0
async fn scan_historical_blocks(
    indexer: &Arc<Indexer>,
    rpc_url: &str,
    rpc_auth: &bitcoincore_rpc::Auth,
) -> Result<()> {
    use bitcoincore_rpc::RpcApi;

    tracing::info!("Starting historical scan for blocks with unknown Bitcoin height");

    // Create RPC client
    let client = bitcoincore_rpc::Client::new(rpc_url, rpc_auth.clone())?;

    // Get current Bitcoin height
    let current_height = client.get_block_count()? as u32;

    // Find all coins blocks with btc_height = 0
    let mut blocks_to_find = Vec::new();
    for item in indexer.blocks.iter() {
        let (key, value) = item?;
        let sub_chain_height = u32::from_le_bytes(key.as_ref().try_into().unwrap());
        let chain_block = coins_indexer::ChainBlock::deserialize(&value, &indexer.state)
            .ok_or_else(|| anyhow::anyhow!("Failed to deserialize block"))?;

        if chain_block.btc_height == 0 {
            blocks_to_find.push((sub_chain_height, chain_block.btc_txid));
        }
    }

    if blocks_to_find.is_empty() {
        tracing::info!("No blocks with unknown Bitcoin height found");
        return Ok(());
    }

    tracing::info!(
        count = blocks_to_find.len(),
        "Found coins blocks with unknown Bitcoin height, scanning recent blocks"
    );

    // Scan backwards from current height (last 1000 blocks should be enough)
    let start_height = current_height.saturating_sub(1000);
    let mut found_count = 0;

    for height in (start_height..=current_height).rev() {
        if blocks_to_find.is_empty() {
            break; // Found all blocks
        }

        if let Err(e) = scan_bitcoin_block(&client, indexer, height).await {
            tracing::warn!(height = height, error = ?e, "Failed to scan historical block");
            continue;
        }

        // Check if we found any blocks at this height
        let mut remaining = Vec::new();
        for (sub_height, txid) in &blocks_to_find {
            // Check if this block still has btc_height = 0
            if let Ok(Some(block)) = indexer.get_block_by_height(*sub_height) {
                if block.btc_height == 0 {
                    remaining.push((*sub_height, *txid));
                } else {
                    found_count += 1;
                    tracing::info!(
                        sub_chain_height = sub_height,
                        btc_height = block.btc_height,
                        btc_txid = %txid,
                        "Found Bitcoin height during historical scan"
                    );
                }
            }
        }
        blocks_to_find = remaining;
    }

    tracing::info!(
        found = found_count,
        remaining = blocks_to_find.len(),
        "Historical scan complete"
    );

    Ok(())
}

/// Background task that monitors new Bitcoin blocks and updates btc_height for confirmed transactions
async fn update_btc_heights_loop(
    indexer: Arc<Indexer>,
    rpc_url: String,
    rpc_auth: bitcoincore_rpc::Auth,
) {
    use bitcoincore_rpc::RpcApi;

    let mut last_checked_height = 0u32;

    loop {
        // Wait 10 seconds between checks (mutinynet has ~1 min block time)
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        // Create RPC client
        let client = match bitcoincore_rpc::Client::new(&rpc_url, rpc_auth.clone()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create RPC client for block monitoring: {}", e);
                continue;
            }
        };

        // Get current Bitcoin block height
        let current_height = match client.get_block_count() {
            Ok(h) => h as u32,
            Err(e) => {
                tracing::warn!("Failed to get block count: {}", e);
                continue;
            }
        };

        // Initialize last_checked_height if this is the first run
        if last_checked_height == 0 {
            last_checked_height = current_height;
            tracing::info!(
                current_height = current_height,
                "Initialized Bitcoin block monitoring"
            );
            continue;
        }

        // Check for new blocks
        if current_height > last_checked_height {
            tracing::info!(
                from = last_checked_height + 1,
                to = current_height,
                "Scanning new Bitcoin blocks"
            );

            // Scan each new block
            for height in (last_checked_height + 1)..=current_height {
                if let Err(e) = scan_bitcoin_block(&client, &indexer, height).await {
                    tracing::warn!(height = height, error = ?e, "Failed to scan Bitcoin block");
                }
            }

            last_checked_height = current_height;
        }
    }
}

/// Scan a Bitcoin block for coins sub-chain transactions and update their heights
async fn scan_bitcoin_block(
    client: &bitcoincore_rpc::Client,
    indexer: &Arc<Indexer>,
    btc_height: u32,
) -> Result<()> {
    use bitcoincore_rpc::RpcApi;

    // Get block hash at this height
    let block_hash = client.get_block_hash(btc_height as u64)?;

    // Get block with transactions (verbosity 1)
    let block = client.get_block(&block_hash)?;

    // Check each transaction in the block
    for tx in &block.txdata {
        let txid = tx.compute_txid();

        // Check if this txid is in our indexer
        let txid_bytes: &[u8] = txid.as_ref();
        if let Ok(Some(height_bytes)) = indexer.txid_index.get(txid_bytes) {
            let sub_chain_height = u32::from_le_bytes(height_bytes.as_ref().try_into().unwrap());

            // Update the btc_height for this coins block
            if let Err(e) = indexer.update_btc_height(sub_chain_height, btc_height) {
                tracing::warn!(
                    sub_chain_height = sub_chain_height,
                    btc_height = btc_height,
                    btc_txid = %txid,
                    error = ?e,
                    "Failed to update Bitcoin height"
                );
            } else {
                tracing::info!(
                    sub_chain_height = sub_chain_height,
                    btc_height = btc_height,
                    btc_txid = %txid,
                    "Updated Bitcoin height for coins block"
                );
            }
        }
    }

    Ok(())
}
