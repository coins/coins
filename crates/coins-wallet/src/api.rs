//! Wallet API proxy endpoints
//!
//! Proxies requests to the indexer and publisher services.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

/// Application state shared across API handlers
#[derive(Clone)]
pub struct AppState {
    /// HTTP client for proxying requests
    pub client: Client,
    /// Indexer service URL
    pub indexer_url: String,
    /// Publisher service URL
    pub publisher_url: String,
}

/// Create the API router for wallet proxy endpoints
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/account/:pk", get(get_account))
        .route("/api/account/:pk/transactions", get(get_account_transactions))
        .route("/api/tx/submit", post(submit_tx))
        .with_state(state)
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
