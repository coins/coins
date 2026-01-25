use axum::{Router, routing::get, routing::post, Json, extract::{Path, Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::{Arc,Mutex};
use std::time::Instant;
use axum::response::IntoResponse;
use axum::http::StatusCode;
use bitcoin::Txid;
use coins_types::Transaction;
use coins_crypto::{G1, G2, G2_SIZE};
use coins_indexer::IndexerClient;

#[derive(Clone)]
pub struct AppState {
    pub indexer: Arc<IndexerClient>,
    /// Mempool: (tx, signature, nonce)
    pub mempool: Arc<Mutex<Vec<(Transaction, G2, u32)>>>,
    pub fee_addr: Arc<Mutex<String>>,
    /// Transactions that were recently broadcast to Bitcoin (with timestamp, btc_txid, nonce)
    pub recently_broadcast: Arc<Mutex<Vec<(Transaction, Instant, Txid, u32)>>>,
}

async fn get_account_by_pk(Path(pk_hex): Path<String>, State(app_state): State<AppState>) -> impl IntoResponse {
    let pk = match G1::from_hex(&pk_hex) {
        Ok(pk) => pk,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };

    // Query from indexer service
    match app_state.indexer.get_account(&pk).await {
        Ok(Some(acc)) => Json(acc).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct TxSubmission { 
    tx: String, // hex
    signature: String, // hex
}

async fn submit_tx(State(state): State<AppState>, Json(body): Json<TxSubmission>) -> impl IntoResponse {
    let tx_bytes = match hex::decode(body.tx.trim()) {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex in tx").into_response(),
    };
    let tx: Transaction = match bincode::serde::decode_from_slice(&tx_bytes, bincode::config::standard()) {
        Ok((tx, _)) => tx,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid transaction data").into_response(),
    };

    let sig_bytes = match hex::decode(body.signature.trim()) {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex in signature").into_response(),
    };

    let sig_arr: [u8; G2_SIZE] = match sig_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid signature length").into_response(),
    };
    let signature = G2(sig_arr);

    // Get sender's base nonce from indexer
    let base_nonce = match state.indexer.get_account_by_id(tx.sender_id).await {
        Ok(Some(acc)) => acc.nonce,
        Ok(None) => return (StatusCode::BAD_REQUEST, "sender account not found").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "indexer error").into_response(),
    };

    match state.mempool.lock() {
        Ok(mut mempool) => {
            // Check for duplicate signature (replay protection)
            if mempool.iter().any(|(_, existing_sig, _)| existing_sig.0 == signature.0) {
                return (StatusCode::CONFLICT, "duplicate transaction").into_response();
            }
            // Compute nonce: base + count of pending txs from same sender
            let pending_from_sender = mempool.iter().filter(|(t, _, _)| t.sender_id == tx.sender_id).count() as u32;
            let nonce = base_nonce + pending_from_sender;
            mempool.push((tx, signature, nonce));
            (StatusCode::OK, "accepted").into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "mempool lock failed").into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct MempoolQuery {
    sender_id: Option<u32>,
    recipient_pk: Option<String>,
    amount: Option<u32>,
    fee: Option<u8>,
}

#[derive(Debug, Serialize)]
struct MempoolTxResponse {
    sender_id: u32,
    sender_pk: String,
    recipient_pk: String,
    amount: u32,
    fee: u8,
    nonce: u32,
}

async fn get_mempool(
    Query(query): Query<MempoolQuery>,
    State(state): State<AppState>
) -> impl IntoResponse {
    // Parse recipient_pk if provided
    let recipient_pk_filter: Option<G1> = if let Some(ref pk_hex) = query.recipient_pk {
        match G1::from_hex(pk_hex) {
            Ok(pk) => Some(pk),
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else {
        None
    };

    // Lock mempool and collect filtered transactions (release lock quickly)
    let filtered_txs: Vec<(Transaction, u32)> = {
        let mempool = match state.mempool.lock() {
            Ok(mp) => mp,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "mempool lock failed").into_response(),
        };

        mempool
            .iter()
            .filter(|(tx, _sig, _nonce)| {
                if let Some(sender_id) = query.sender_id {
                    if tx.sender_id != sender_id {
                        return false;
                    }
                }
                if let Some(ref pk) = recipient_pk_filter {
                    if tx.recipient_pk != *pk {
                        return false;
                    }
                }
                if let Some(amount) = query.amount {
                    if tx.amount != amount {
                        return false;
                    }
                }
                if let Some(fee) = query.fee {
                    if tx.fee != fee {
                        return false;
                    }
                }
                true
            })
            .map(|(tx, _sig, nonce)| (tx.clone(), *nonce))
            .collect()
    };

    // Look up sender_pk for each transaction
    let mut results = Vec::new();
    for (tx, nonce) in filtered_txs {
        let sender_pk = match state.indexer.get_account_by_id(tx.sender_id).await {
            Ok(Some(acc)) => acc.pk.to_hex(),
            _ => continue, // Skip if sender not found
        };
        results.push(MempoolTxResponse {
            sender_id: tx.sender_id,
            sender_pk,
            recipient_pk: tx.recipient_pk.to_hex(),
            amount: tx.amount,
            fee: tx.fee,
            nonce,
        });
    }

    Json(results).into_response()
}

#[derive(Debug, Serialize)]
struct AddressResponse {
    address: String,
}

async fn get_address(State(state): State<AppState>) -> impl IntoResponse {
    let address = match state.fee_addr.lock() {
        Ok(addr) => addr.clone(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "failed to get address").into_response(),
    };

    Json(AddressResponse { address }).into_response()
}

#[derive(Debug, Serialize)]
struct RecentlyBroadcastTxResponse {
    sender_id: u32,
    sender_pk: String,
    recipient_pk: String,
    amount: u32,
    fee: u8,
    nonce: u32,
    btc_txid: String,
}

/// Get transactions that were recently broadcast to Bitcoin (in-flight)
async fn get_recently_broadcast(
    Query(query): Query<MempoolQuery>,
    State(state): State<AppState>
) -> impl IntoResponse {
    // Parse recipient_pk if provided
    let recipient_pk_filter: Option<G1> = if let Some(ref pk_hex) = query.recipient_pk {
        match G1::from_hex(pk_hex) {
            Ok(pk) => Some(pk),
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else {
        None
    };

    // Get candidates from recently_broadcast (release lock quickly)
    let candidates: Vec<(Transaction, Txid, u32)> = {
        let recently_broadcast = match state.recently_broadcast.lock() {
            Ok(rb) => rb,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "recently_broadcast lock failed").into_response(),
        };

        // No TTL-based removal - we'll filter by indexer status instead
        // Entries are removed when they're confirmed indexed

        recently_broadcast
            .iter()
            .filter(|(tx, _timestamp, _btc_txid, _nonce)| {
                // Filter by sender_id if provided
                if let Some(sender_id) = query.sender_id {
                    if tx.sender_id != sender_id {
                        return false;
                    }
                }
                // Filter by recipient_pk if provided
                if let Some(ref pk) = recipient_pk_filter {
                    if tx.recipient_pk != *pk {
                        return false;
                    }
                }
                true
            })
            .map(|(tx, _timestamp, btc_txid, nonce)| (tx.clone(), *btc_txid, *nonce))
            .collect()
    };

    // Filter out transactions already indexed (check async without holding lock)
    let mut results = Vec::new();
    for (tx, btc_txid, nonce) in candidates {
        // Check if indexer has already seen this txid
        let is_indexed = match state.indexer.is_txid_indexed(&btc_txid.to_string()).await {
            Ok(indexed) => {
                tracing::debug!(
                    btc_txid = %btc_txid,
                    indexed = indexed,
                    "Checked txid indexed status"
                );
                indexed
            }
            Err(e) => {
                tracing::warn!(
                    btc_txid = %btc_txid,
                    error = ?e,
                    "Failed to check if txid is indexed, assuming not indexed"
                );
                false
            }
        };
        if !is_indexed {
            // Look up sender_pk
            let sender_pk = match state.indexer.get_account_by_id(tx.sender_id).await {
                Ok(Some(acc)) => acc.pk.to_hex(),
                _ => continue, // Skip if sender not found
            };
            results.push(RecentlyBroadcastTxResponse {
                sender_id: tx.sender_id,
                sender_pk,
                recipient_pk: tx.recipient_pk.to_hex(),
                amount: tx.amount,
                fee: tx.fee,
                nonce,
                btc_txid: btc_txid.to_string(),
            });
        }
    }

    Json(results).into_response()
}

async fn health() -> &'static str {
    "OK"
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/account/:pk", get(get_account_by_pk))
        .route("/tx", post(submit_tx))
        .route("/mempool", get(get_mempool))
        .route("/recently-broadcast", get(get_recently_broadcast))
        .route("/address", get(get_address))
        .with_state(state)
} 