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
    pub publisher_url: Option<String>,
}

#[derive(Deserialize)]
struct BlocksRangeQuery {
    from: Option<u32>,
    to: Option<u32>,
}

#[derive(Deserialize)]
struct MempoolQuery {
    sender_id: Option<u32>,
    recipient_pk: Option<String>,
}

pub fn simple_router(indexer_client: Arc<IndexerClient>, publisher_url: Option<String>) -> Router {
    let state = SimpleAppState { indexer_client, publisher_url };

    Router::new()
        .route("/health", get(health))
        .route("/api/v1/accounts/:pk", get(get_account))
        .route("/api/v1/accounts/:pk/transactions", get(get_account_transactions))
        .route("/api/v1/blocks/latest", get(get_latest_block))
        .route("/api/v1/blocks/:height", get(get_block))
        .route("/api/v1/blocks", get(get_blocks_range))
        .route("/api/v1/stats", get(get_stats))
        .route("/api/v1/mempool", get(get_mempool))
        .route("/api/v1/recently-broadcast", get(get_recently_broadcast))
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
    let pk = coins_crypto::G1::from_hex(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;

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

async fn get_account_transactions(
    State(state): State<SimpleAppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let pk = coins_crypto::G1::from_hex(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;

    match state.indexer_client.get_account_transactions(&pk).await {
        Ok(transactions) => {
            serde_json::to_value(transactions)
                .map(Json)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        }
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

async fn get_mempool(
    State(state): State<SimpleAppState>,
    Query(query): Query<MempoolQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher_url = state.publisher_url.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut url = format!("{}/mempool", publisher_url);
    let mut params = Vec::new();
    if let Some(sender_id) = query.sender_id {
        params.push(format!("sender_id={}", sender_id));
    }
    if let Some(ref recipient_pk) = query.recipient_pk {
        params.push(format!("recipient_pk={}", recipient_pk));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = response.json().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(body))
}

async fn get_recently_broadcast(
    State(state): State<SimpleAppState>,
    Query(query): Query<MempoolQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher_url = state.publisher_url.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut url = format!("{}/recently-broadcast", publisher_url);
    let mut params = Vec::new();
    if let Some(sender_id) = query.sender_id {
        params.push(format!("sender_id={}", sender_id));
    }
    if let Some(ref recipient_pk) = query.recipient_pk {
        params.push(format!("recipient_pk={}", recipient_pk));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = response.json().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(body))
}
