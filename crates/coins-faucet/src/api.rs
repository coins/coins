//! Faucet API routes and handlers.
//!
//! Provides REST endpoints for claiming tokens and checking faucet status.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::signer::FaucetSigner;

/// Application state shared across API handlers.
#[derive(Clone)]
pub struct AppState {
    /// HTTP client for API calls
    pub client: Client,
    /// Faucet signer (locked for thread safety)
    pub signer: Arc<RwLock<FaucetSigner>>,
    /// Indexer service URL
    pub indexer_url: String,
    /// Publisher service URL
    pub publisher_url: String,
    /// Explorer URL (user-facing, for linking from the faucet UI)
    pub explorer_url: String,
    /// Amount of tokens to dispense per claim
    pub amount_per_claim: u32,
    /// Cooldown period between claims for the same address
    pub cooldown: Duration,
    /// Track last claim time per recipient address
    pub claim_times: Arc<RwLock<HashMap<String, Instant>>>,
    /// Total claims counter
    pub total_claims: Arc<RwLock<u64>>,
}

/// Request body for claiming tokens.
#[derive(Debug, Deserialize)]
pub struct ClaimRequest {
    /// Recipient: account ID (numeric) or public key (64-char hex string)
    pub recipient: String,
    /// Token ID to claim (0-255)
    #[serde(default)]
    pub token_id: u16,
}

/// Account response from indexer (for resolving account IDs)
#[derive(Debug, Deserialize)]
struct IndexerAccountResponse {
    pk: String,
}

/// Response for a claim request.
#[derive(Debug, Serialize)]
pub struct ClaimResponse {
    /// Whether the claim was successful
    pub success: bool,
    /// Transaction hash if successful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    /// Amount of tokens sent
    pub amount: u32,
    /// Human-readable message
    pub message: String,
}

/// Response for faucet stats.
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    /// Faucet public key
    pub faucet_address: String,
    /// Available balances by token ID
    pub balances: HashMap<String, u64>,
    /// Total claims made
    pub total_claims: u64,
    /// Amount per claim
    pub amount_per_claim: u32,
    /// Cooldown in seconds
    pub cooldown_seconds: u64,
    /// Explorer URL for linking to transactions
    pub explorer_url: String,
}

/// Create the API router with all faucet endpoints.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/claim", post(claim))
        .route("/api/stats", get(stats))
        .with_state(state)
}

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    "OK"
}

/// Claim tokens from the faucet.
async fn claim(
    State(state): State<AppState>,
    Json(body): Json<ClaimRequest>,
) -> impl IntoResponse {
    let recipient_input = body.recipient.trim();
    let token_id = body.token_id;

    // Resolve recipient: can be account ID (numeric) or public key (64 hex chars)
    let recipient_pk = if recipient_input.chars().all(|c| c.is_ascii_digit()) {
        // It's an account ID - fetch the account to get the public key
        let account_id = recipient_input;
        let url = format!("{}/accounts/by-id/{}", state.indexer_url, account_id);

        match state.client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<IndexerAccountResponse>().await {
                        Ok(account) => account.pk.to_lowercase(),
                        Err(e) => {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(ClaimResponse {
                                    success: false,
                                    tx_hash: None,
                                    amount: 0,
                                    message: format!("Failed to parse account response: {}", e),
                                }),
                            );
                        }
                    }
                } else if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ClaimResponse {
                            success: false,
                            tx_hash: None,
                            amount: 0,
                            message: format!("Account #{} not found", account_id),
                        }),
                    );
                } else {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(ClaimResponse {
                            success: false,
                            tx_hash: None,
                            amount: 0,
                            message: format!("Error fetching account: {}", response.status()),
                        }),
                    );
                }
            }
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(ClaimResponse {
                        success: false,
                        tx_hash: None,
                        amount: 0,
                        message: format!("Failed to connect to indexer: {}", e),
                    }),
                );
            }
        }
    } else {
        // Assume it's a public key - validate format
        let pk = recipient_input.to_lowercase();
        if pk.len() != 64 || hex::decode(&pk).is_err() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ClaimResponse {
                    success: false,
                    tx_hash: None,
                    amount: 0,
                    message: "Invalid recipient: must be account number or 64-char hex public key".to_string(),
                }),
            );
        }
        pk
    };

    // Check cooldown (using resolved public key)
    {
        let claim_times = state.claim_times.read().await;
        if let Some(last_claim) = claim_times.get(&recipient_pk) {
            let elapsed = last_claim.elapsed();
            if elapsed < state.cooldown {
                let remaining = (state.cooldown - elapsed).as_secs();
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(ClaimResponse {
                        success: false,
                        tx_hash: None,
                        amount: 0,
                        message: format!(
                            "Please wait {} seconds before claiming again",
                            remaining
                        ),
                    }),
                );
            }
        }
    }

    info!(
        recipient = %recipient_pk,
        token_id = token_id,
        amount = state.amount_per_claim,
        "Processing claim request"
    );

    // Send the tokens
    let result = {
        let mut signer = state.signer.write().await;
        signer
            .send_tokens(
                &state.client,
                &state.indexer_url,
                &state.publisher_url,
                &recipient_pk,
                token_id,
                state.amount_per_claim,
            )
            .await
    };

    match result {
        Ok(tx_hash) => {
            // Update claim time
            {
                let mut claim_times = state.claim_times.write().await;
                claim_times.insert(recipient_pk.clone(), Instant::now());
            }

            // Increment total claims
            {
                let mut total = state.total_claims.write().await;
                *total += 1;
            }

            info!(
                recipient = %recipient_pk,
                token_id = token_id,
                tx_hash = %tx_hash,
                "Claim successful"
            );

            (
                StatusCode::OK,
                Json(ClaimResponse {
                    success: true,
                    tx_hash: Some(tx_hash),
                    amount: state.amount_per_claim,
                    message: format!(
                        "Successfully sent {} tokens (token ID {})",
                        state.amount_per_claim, token_id
                    ),
                }),
            )
        }
        Err(e) => {
            error!(error = %e, recipient = %recipient_pk, "Claim failed");

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ClaimResponse {
                    success: false,
                    tx_hash: None,
                    amount: 0,
                    message: format!("Failed to send tokens: {}", e),
                }),
            )
        }
    }
}

/// Get faucet statistics.
async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    // Fetch current account state
    let (faucet_address, balances) = {
        let mut signer = state.signer.write().await;
        let pk_hex = signer.public_key_hex();

        match signer.fetch_account(&state.client, &state.indexer_url).await {
            Ok(account) => (pk_hex, account.balances),
            Err(e) => {
                warn!(error = %e, "Failed to fetch faucet account for stats");
                (pk_hex, HashMap::new())
            }
        }
    };

    let total_claims = *state.total_claims.read().await;

    Json(StatsResponse {
        faucet_address,
        balances,
        total_claims,
        amount_per_claim: state.amount_per_claim,
        cooldown_seconds: state.cooldown.as_secs(),
        explorer_url: state.explorer_url.clone(),
    })
}
