// Simplified explorer API that proxies to indexer service
use axum::{
    Router,
    routing::get,
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use coins_indexer::IndexerClient;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct SimpleAppState {
    pub indexer_client: Arc<IndexerClient>,
}

#[derive(Deserialize)]
struct BlocksRangeQuery {
    from: Option<u32>,
    to: Option<u32>,
}

pub fn simple_router(indexer_client: Arc<IndexerClient>) -> Router {
    let state = SimpleAppState { indexer_client };

    Router::new()
        .route("/health", get(health))
        .route("/api/v1/accounts/:pk", get(get_account))
        .route("/api/v1/blocks/latest", get(get_latest_block))
        .route("/api/v1/blocks/:height", get(get_block))
        .route("/api/v1/blocks", get(get_blocks_range))
        .route("/api/v1/stats", get(get_stats))
        .nest_service("/", ServeDir::new("crates/coins-explorer/src/web/static"))
        .with_state(state)
}

async fn health() -> &'static str {
    "OK"
}

async fn get_account(
    State(state): State<SimpleAppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let pk_bytes = hex::decode(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    if pk_bytes.len() != 32 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut pk_array = [0u8; 32];
    pk_array.copy_from_slice(&pk_bytes);
    let pk = coins_crypto::G1(pk_array);

    match state.indexer_client.get_account(&pk).await {
        Ok(Some(account)) => {
            serde_json::to_value(account)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_latest_block(
    State(state): State<SimpleAppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.indexer_client.get_latest_block().await {
        Ok(Some(block)) => {
            serde_json::to_value(block)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_block(
    State(state): State<SimpleAppState>,
    Path(height): Path<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.indexer_client.get_block_by_height(height).await {
        Ok(Some(block)) => {
            serde_json::to_value(block)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_blocks_range(
    State(state): State<SimpleAppState>,
    Query(query): Query<BlocksRangeQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.indexer_client.get_blocks_range(query.from, query.to).await {
        Ok(blocks) => {
            serde_json::to_value(blocks)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_stats(
    State(state): State<SimpleAppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.indexer_client.get_stats().await {
        Ok(stats) => {
            serde_json::to_value(stats)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
