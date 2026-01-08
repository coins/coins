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

    // Create application state
    let app_state = AppState::new(state, indexer);

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
