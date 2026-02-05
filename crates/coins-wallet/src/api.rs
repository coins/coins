//! Wallet API proxy endpoints
//!
//! Proxies requests to the indexer and publisher services.
//! Includes WebSocket support for real-time updates.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// WebSocket message types for live updates
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Balance has been updated
    BalanceUpdate,
    /// New transaction received or status changed
    TransactionUpdate,
    /// Pending transaction count changed
    PendingTxsUpdate { count: u32 },
    /// Connection established
    Connected,
}

/// Application state shared across API handlers
#[derive(Clone)]
pub struct AppState {
    /// HTTP client for proxying requests
    pub client: Client,
    /// Indexer service URL
    pub indexer_url: String,
    /// Publisher service URL
    pub publisher_url: String,
    /// Explorer service URL
    pub explorer_url: String,
}

/// Extended application state with WebSocket support
#[derive(Clone)]
pub struct AppStateWithWs {
    /// Base app state
    pub inner: AppState,
    /// WebSocket broadcast channel
    pub ws_tx: broadcast::Sender<WsMessage>,
}

/// Create the API router for wallet proxy endpoints
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/account/by-id/:id", get(get_account_by_id))
        .route("/api/account/:pk", get(get_account))
        .route("/api/account/:pk/transactions", get(get_account_transactions))
        .route("/api/tx/submit", post(submit_tx))
        .route("/api/publisher/status", get(get_publisher_status))
        .route("/api/pending-transactions", get(get_pending_transactions))
        .route("/api/explorer/health", get(get_explorer_health))
        .with_state(state)
}

/// Create the API router with WebSocket support
pub fn create_router_with_ws(
    state: AppState,
    ws_tx: broadcast::Sender<WsMessage>,
) -> Router {
    let state_with_ws = AppStateWithWs {
        inner: state.clone(),
        ws_tx,
    };

    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state_with_ws)
        .merge(create_router(state))
}

/// Proxy GET /api/account/by-id/:id to indexer /accounts/by-id/:id
async fn get_account_by_id(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> impl IntoResponse {
    debug!(id = %id, "Proxying account-by-id request to indexer");

    let url = format!("{}/accounts/by-id/{}", state.indexer_url, id);

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read indexer response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to indexer");
            (StatusCode::BAD_GATEWAY, "Failed to connect to indexer").into_response()
        }
    }
}

/// Proxy GET /api/account/:pk to indexer /accounts/:pk
async fn get_account(
    State(state): State<AppState>,
    Path(pk): Path<String>,
) -> impl IntoResponse {
    debug!(pk = %pk, "Proxying account request to indexer");

    let url = format!("{}/accounts/{}", state.indexer_url, pk);

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    // Forward the status code and body from indexer
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read indexer response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to indexer");
            (StatusCode::BAD_GATEWAY, "Failed to connect to indexer").into_response()
        }
    }
}

/// Proxy GET /api/account/:pk/transactions to indexer /accounts/:pk/transactions
async fn get_account_transactions(
    State(state): State<AppState>,
    Path(pk): Path<String>,
) -> impl IntoResponse {
    debug!(pk = %pk, "Proxying transactions request to indexer");

    let url = format!("{}/accounts/{}/transactions", state.indexer_url, pk);

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read indexer response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to indexer");
            (StatusCode::BAD_GATEWAY, "Failed to connect to indexer").into_response()
        }
    }
}

/// Transaction submission request body
#[derive(Debug, Deserialize, Serialize)]
pub struct TxSubmission {
    /// Transaction bytes as hex string
    pub tx: String,
    /// Signature bytes as hex string
    pub signature: String,
}

/// Query parameters for pending transactions endpoint
#[derive(Debug, Deserialize)]
pub struct PendingTxQuery {
    /// Filter by sender account ID
    pub sender_id: Option<u32>,
}

/// Proxy GET /api/publisher/status to publisher /status
async fn get_publisher_status(State(state): State<AppState>) -> impl IntoResponse {
    debug!("Proxying publisher status request");

    let url = format!("{}/status", state.publisher_url);

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read publisher response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to publisher");
            (StatusCode::BAD_GATEWAY, "Failed to connect to publisher").into_response()
        }
    }
}

/// Proxy POST /api/tx/submit to publisher /tx
async fn submit_tx(
    State(state): State<AppState>,
    Json(body): Json<TxSubmission>,
) -> impl IntoResponse {
    debug!("Proxying transaction submission to publisher");

    let url = format!("{}/tx", state.publisher_url);

    match state.client.post(&url).json(&body).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    // Forward the status code and body from publisher
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read publisher response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to publisher");
            (StatusCode::BAD_GATEWAY, "Failed to connect to publisher").into_response()
        }
    }
}

/// Proxy GET /api/pending-transactions to explorer /api/v1/pending-transactions
async fn get_pending_transactions(
    State(state): State<AppState>,
    Query(query): Query<PendingTxQuery>,
) -> impl IntoResponse {
    debug!(sender_id = ?query.sender_id, "Proxying pending transactions request to explorer");

    let mut url = format!("{}/api/v1/pending-transactions", state.explorer_url);
    if let Some(sender_id) = query.sender_id {
        url.push_str(&format!("?sender_id={}", sender_id));
    }

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        [("content-type", "application/json")],
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read explorer response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to explorer");
            (StatusCode::BAD_GATEWAY, "Failed to connect to explorer").into_response()
        }
    }
}

/// Proxy GET /api/explorer/health to explorer /health
async fn get_explorer_health(State(state): State<AppState>) -> impl IntoResponse {
    debug!("Proxying health check request to explorer");

    let url = format!("{}/health", state.explorer_url);

    match state.client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        body,
                    )
                        .into_response()
                }
                Err(e) => {
                    error!(error = ?e, "Failed to read explorer response body");
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            error!(error = ?e, url = %url, "Failed to connect to explorer");
            (StatusCode::BAD_GATEWAY, "Failed to connect to explorer").into_response()
        }
    }
}

/// WebSocket handler for live updates
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppStateWithWs>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppStateWithWs) {
    let mut rx = state.ws_tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Send connected message
    if let Ok(json) = serde_json::to_string(&WsMessage::Connected) {
        let _ = sender.send(Message::Text(json.into())).await;
    }

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

/// Monitor indexer and explorer for state changes and broadcast updates
pub async fn monitor_indexer_changes(
    client: Client,
    indexer_url: String,
    explorer_url: String,
    ws_tx: broadcast::Sender<WsMessage>,
) {
    let mut last_block_height: Option<u32> = None;
    let mut last_pending_count: Option<u32> = None;

    loop {
        // Check for new blocks every 2 seconds
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Get latest block from indexer
        let url = format!("{}/blocks/latest", indexer_url);
        if let Ok(response) = client.get(&url).send().await {
            if response.status().is_success() {
                if let Ok(body) = response.json::<serde_json::Value>().await {
                    if let Some(height) = body.get("height").and_then(|h| h.as_u64()) {
                        let height = height as u32;
                        if last_block_height.map_or(true, |h| height > h) {
                            if last_block_height.is_some() {
                                info!("New block detected at height {}", height);
                                // Broadcast balance and transaction updates
                                let _ = ws_tx.send(WsMessage::BalanceUpdate);
                                let _ = ws_tx.send(WsMessage::TransactionUpdate);
                            }
                            last_block_height = Some(height);
                        }
                    }
                }
            }
        }

        // Check pending transactions from explorer
        let pending_url = format!("{}/api/v1/pending-transactions", explorer_url);
        if let Ok(response) = client.get(&pending_url).send().await {
            if response.status().is_success() {
                if let Ok(txs) = response.json::<Vec<serde_json::Value>>().await {
                    let count = txs.len() as u32;
                    if last_pending_count.map_or(true, |c| c != count) {
                        if last_pending_count.is_some() {
                            debug!("Pending transaction count changed: {}", count);
                            let _ = ws_tx.send(WsMessage::PendingTxsUpdate { count });
                        }
                        last_pending_count = Some(count);
                    }
                }
            }
        }
    }
}
