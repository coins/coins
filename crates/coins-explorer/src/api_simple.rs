// Simplified explorer API that proxies to indexer service
use axum::{
    Router,
    routing::get,
    extract::{State, Path, Query, ws::{Message, WebSocket, WebSocketUpgrade}},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::broadcast;
use coins_indexer::IndexerClient;
use tower_http::services::ServeDir;

/// WebSocket message types for live updates
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// New block has been indexed
    NewBlock {
        height: u32,
        btc_txid: String,
        tx_count: usize,
    },
    /// Network stats have been updated
    StatsUpdate {
        total_blocks: u32,
        total_accounts: u64,
        total_supply: u64,
    },
    /// Pending transactions count changed
    PendingTxsUpdate {
        count: usize,
    },
    /// Transaction confirmation status changed
    ConfirmationUpdate {
        btc_height: u32,
    },
}

#[derive(Clone)]
pub struct SimpleAppState {
    pub indexer_client: Arc<IndexerClient>,
    pub publisher_url: Option<String>,
    pub ws_tx: broadcast::Sender<WsMessage>,
    pub http_client: reqwest::Client,
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

/// Status of a pending transaction
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PendingTxStatus {
    /// In publisher mempool, waiting to be mined into a sub-block
    Publishing,
    /// Broadcast to Bitcoin, waiting for indexer to discover
    Broadcasting,
    /// Indexed by indexer, in Bitcoin mempool (btc_height = 0)
    Unconfirmed,
}

/// A pending transaction with its status
#[derive(Debug, Clone, Serialize)]
pub struct PendingTransaction {
    pub sender_id: u32,
    pub sender_pk: String,
    pub recipient_pk: String,
    pub token_id: u16,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u32,
    pub status: PendingTxStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub btc_txid: Option<String>,
    pub btc_height: u32,
    pub confirmations: u32,
    pub finalized: bool,
}

pub fn simple_router(
    indexer_client: Arc<IndexerClient>,
    publisher_url: Option<String>,
    ws_tx: broadcast::Sender<WsMessage>,
    http_client: reqwest::Client,
) -> Router {
    let state = SimpleAppState {
        indexer_client,
        publisher_url,
        ws_tx,
        http_client,
    };

    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/api/v1/accounts/by-id/:id", get(get_account_by_id))
        .route("/api/v1/accounts/:pk", get(get_account))
        .route("/api/v1/accounts/:pk/transactions", get(get_account_transactions))
        .route("/api/v1/blocks/latest", get(get_latest_block))
        .route("/api/v1/blocks/:height", get(get_block))
        .route("/api/v1/blocks", get(get_blocks_range))
        .route("/api/v1/stats", get(get_stats))
        .route("/api/v1/mempool", get(get_mempool))
        .route("/api/v1/recently-broadcast", get(get_recently_broadcast))
        .route("/api/v1/pending-transactions", get(get_pending_transactions))
        .route("/api/publisher/status", get(get_publisher_status))
        .route("/api/publisher/address", get(get_publisher_address))
        // Serve shared static files
        .nest_service("/shared", ServeDir::new("shared/static"))
        // Serve explorer static files at root
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

async fn get_account_by_id(
    State(state): State<SimpleAppState>,
    Path(id): Path<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.indexer_client.get_account_by_id(id).await {
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
            let mut block_json = serde_json::to_value(&block)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Enrich with confirmation data
            if let Ok(Some(conf_info)) = state.indexer_client.get_block_confirmation(&block.btc_txid).await {
                if let Some(obj) = block_json.as_object_mut() {
                    obj.insert("confirmations".to_string(), serde_json::json!(conf_info.confirmations));
                    obj.insert("finalized".to_string(), serde_json::json!(conf_info.finalized));
                }
            }

            Ok(Json(block_json))
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
            let mut block_json = serde_json::to_value(&block)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Enrich with confirmation data
            if let Ok(Some(conf_info)) = state.indexer_client.get_block_confirmation(&block.btc_txid).await {
                if let Some(obj) = block_json.as_object_mut() {
                    obj.insert("confirmations".to_string(), serde_json::json!(conf_info.confirmations));
                    obj.insert("finalized".to_string(), serde_json::json!(conf_info.finalized));
                }
            }

            Ok(Json(block_json))
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
            // Enrich each block with confirmation data
            let mut enriched_blocks = Vec::new();
            for block in blocks {
                let mut block_json = serde_json::to_value(&block)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                if let Ok(Some(conf_info)) = state.indexer_client.get_block_confirmation(&block.btc_txid).await {
                    if let Some(obj) = block_json.as_object_mut() {
                        obj.insert("confirmations".to_string(), serde_json::json!(conf_info.confirmations));
                        obj.insert("finalized".to_string(), serde_json::json!(conf_info.finalized));
                    }
                }

                enriched_blocks.push(block_json);
            }

            Ok(Json(serde_json::json!(enriched_blocks)))
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

    let response = state.http_client.get(&url).send().await
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

    let response = state.http_client.get(&url).send().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = response.json().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(body))
}

/// Proxy to publisher status endpoint
async fn get_publisher_status(
    State(state): State<SimpleAppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher_url = state.publisher_url.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let url = format!("{}/status", publisher_url);

    let response = state.http_client.get(&url).send().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = response.json().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(body))
}

/// Proxy to publisher address endpoint
async fn get_publisher_address(
    State(state): State<SimpleAppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let publisher_url = state.publisher_url.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let url = format!("{}/address", publisher_url);

    let response = state.http_client.get(&url).send().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let body: serde_json::Value = response.json().await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(body))
}

/// Get all pending transactions with deduplicated status
/// Returns transactions in three states: publishing, broadcasting, unconfirmed
async fn get_pending_transactions(
    State(state): State<SimpleAppState>,
    Query(query): Query<MempoolQuery>,
) -> Result<Json<Vec<PendingTransaction>>, StatusCode> {
    let publisher_url = state.publisher_url.as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut results: Vec<PendingTransaction> = Vec::new();
    let mut seen_btc_txids: HashSet<String> = HashSet::new();

    // Build query params
    let mut params = Vec::new();
    if let Some(sender_id) = query.sender_id {
        params.push(format!("sender_id={}", sender_id));
    }
    if let Some(ref recipient_pk) = query.recipient_pk {
        params.push(format!("recipient_pk={}", recipient_pk));
    }
    let query_string = if params.is_empty() {
        String::new()
    } else {
        format!("?{}", params.join("&"))
    };

    // 1. Fetch recently-broadcast transactions from publisher
    //    These have been broadcast to Bitcoin but may or may not be indexed yet
    let broadcast_url = format!("{}/recently-broadcast{}", publisher_url, query_string);
    if let Ok(response) = state.http_client.get(&broadcast_url).send().await {
        if response.status().is_success() {
            if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
                for tx in txs {
                    let btc_txid = tx.get("btc_txid")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    // Get actual confirmation status from indexer
                    let (status, btc_height, confirmations, finalized) = if let Some(ref txid) = btc_txid {
                        match state.indexer_client.get_block_confirmation(txid).await {
                            Ok(Some(info)) => {
                                // Transaction is indexed - use actual confirmation data
                                (PendingTxStatus::Unconfirmed, info.btc_height, info.confirmations, info.finalized)
                            }
                            _ => {
                                // Not indexed yet - still broadcasting
                                (PendingTxStatus::Broadcasting, 0, 0, false)
                            }
                        }
                    } else {
                        (PendingTxStatus::Broadcasting, 0, 0, false)
                    };

                    // Track seen btc_txids to avoid duplicates
                    if let Some(ref txid) = btc_txid {
                        seen_btc_txids.insert(txid.clone());
                    }

                    results.push(PendingTransaction {
                        sender_id: tx.get("sender_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        sender_pk: tx.get("sender_pk").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        recipient_pk: tx.get("recipient_pk").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        token_id: tx.get("token_id").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
                        amount: tx.get("amount").and_then(|v| v.as_u64()).unwrap_or(0),
                        fee: tx.get("fee").and_then(|v| v.as_u64()).unwrap_or(0),
                        nonce: tx.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        status,
                        btc_txid,
                        btc_height,
                        confirmations,
                        finalized,
                    });
                }
            }
        }
    }

    // 2. Fetch mempool transactions from publisher
    //    These haven't been broadcast to Bitcoin yet
    let mempool_url = format!("{}/mempool{}", publisher_url, query_string);
    if let Ok(response) = state.http_client.get(&mempool_url).send().await {
        if response.status().is_success() {
            if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
                for tx in txs {
                    results.push(PendingTransaction {
                        sender_id: tx.get("sender_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        sender_pk: tx.get("sender_pk").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        recipient_pk: tx.get("recipient_pk").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        token_id: tx.get("token_id").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
                        amount: tx.get("amount").and_then(|v| v.as_u64()).unwrap_or(0),
                        fee: tx.get("fee").and_then(|v| v.as_u64()).unwrap_or(0),
                        nonce: tx.get("nonce").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        status: PendingTxStatus::Publishing,
                        btc_txid: None,
                        btc_height: 0,
                        confirmations: 0,
                        finalized: false,
                    });
                }
            }
        }
    }

    Ok(Json(results))
}

// WebSocket handler for live updates
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SimpleAppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: SimpleAppState) {
    let mut rx = state.ws_tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Spawn task to forward broadcasts to this client
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages (ping/pong, close)
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Close(_)) => break,
            Err(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}
