mod api;
mod config;

use anyhow::Result;
use std::sync::Arc;
use coins_core::State;
use coins_crypto::G1;
use coins_types::NATIVE_TOKEN_ID;
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
    let genesis_pk = G1::from_hex(&config.genesis.genesis_pk)
        .map_err(|e| anyhow::anyhow!("Invalid genesis public key: {}", e))?;

    if state.get_by_pk(&genesis_pk)?.is_none() {
        tracing::info!("Creating genesis account");
        let mut genesis = state.create_account(genesis_pk)?;
        genesis.balances.insert(NATIVE_TOKEN_ID, config.genesis.genesis_balance);
        // For testing purposes, also give Genesis some token_id=1 balance
        genesis.balances.insert(1, 10000);
        let genesis_id = genesis.id.0;
        state.apply_batch(&[genesis])?;
        tracing::info!(
            id = genesis_id,
            balance = config.genesis.genesis_balance,
            "Genesis account created with native and token_id=1 balances"
        );
    } else {
        tracing::info!("Genesis account already exists");
    }

    tracing::info!("Opening indexer database: {:?}", config.database.indexer_db);
    let indexer = Arc::new(Indexer::open(&config.database.indexer_db, state.clone())?);

    // Load subchain for autonomous block discovery
    tracing::info!("Loading subchain from: {:?}", config.subchain);
    let subchain_bytes = std::fs::read(&config.subchain)?;
    let subchain = Arc::new(coins_subchain::Subchain::decode(&subchain_bytes)
        .ok_or_else(|| anyhow::anyhow!("Invalid subchain file"))?);
    tracing::info!(
        anchors = subchain.sigs.len(),
        address = %subchain.address(),
        genesis_height = ?subchain.genesis_height,
        "Subchain loaded successfully"
    );

    // Create RpcBackend for Bitcoin interaction
    let rpc_backend = Arc::new(coins_bitcoin_rpc::RpcBackend::new(
        config.bitcoin.rpc_url.clone(),
        config.bitcoin.rpc_user.clone(),
        config.bitcoin.rpc_pass.clone(),
        config.wallet_name.clone(),
    )?);
    tracing::info!("Bitcoin RPC backend initialized");

    // Run IBD: scan historical blocks
    tracing::info!("Starting Initial Block Download (IBD)");
    if let Err(e) = coins_indexer::discovery::scan_historical_anchors(
        &indexer,
        &state,
        &rpc_backend,
        &subchain,
    ).await {
        tracing::error!("IBD failed: {}", e);
        // Continue anyway - will catch up via continuous monitoring
    }

    // Start autonomous discovery loop
    let indexer_bg = indexer.clone();
    let state_bg = state.clone();
    let rpc_backend_bg = rpc_backend.clone();
    let subchain_bg = subchain.clone();
    tokio::spawn(async move {
        if let Err(e) = coins_indexer::discovery::discover_blocks_loop(
            indexer_bg,
            state_bg,
            rpc_backend_bg,
            subchain_bg,
        ).await {
            tracing::error!("Discovery loop failed: {}", e);
        }
    });

    // Start confirmation status update task
    let indexer_confirm = indexer.clone();
    let rpc_backend_confirm = rpc_backend.clone();
    let subchain_confirm = subchain.clone();
    tokio::spawn(async move {
        if let Err(e) = coins_indexer::discovery::update_confirmation_statuses(
            indexer_confirm,
            rpc_backend_confirm,
            subchain_confirm,
        ).await {
            tracing::error!("Confirmation status update task failed: {}", e);
        }
    });

    // Create application state
    let app_state = AppState::new(state, indexer.clone(), rpc_backend.clone(), config.bitcoin.network.clone());

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
    tracing::info!("  GET  /stats");

    axum::serve(listener, app).await?;

    Ok(())
}
