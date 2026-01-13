use axum::{Router, routing::get, routing::post, Json, extract::{Path, Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::{Arc,Mutex};
use std::time::Instant;
use axum::response::IntoResponse;
use axum::http::StatusCode;
use bitcoin::Txid;
use coins_types::Transaction;
use coins_crypto::{G1, G2, G1_SIZE, G2_SIZE};
use coins_indexer::IndexerClient;

/// TTL for recently broadcast transactions (60 seconds)
pub const RECENTLY_BROADCAST_TTL_SECS: u64 = 60;

#[derive(Clone)]
pub struct AppState {
    pub indexer: Arc<IndexerClient>,
    pub mempool: Arc<Mutex<Vec<(Transaction, G2)>>>,
    pub fee_addr: Arc<Mutex<String>>,
    /// Transactions that were recently broadcast to Bitcoin (with timestamp and btc_txid)
    pub recently_broadcast: Arc<Mutex<Vec<(Transaction, Instant, Txid)>>>,
}

async fn get_account_by_pk(Path(pk_hex): Path<String>, State(app_state): State<AppState>) -> impl IntoResponse {
    let pk_bytes = match hex::decode(pk_hex) {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex public key").into_response(),
    };

    let pk_arr: [u8; G1_SIZE] = match pk_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid public key length").into_response(),
    };
    let pk = G1(pk_arr);

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

    match state.mempool.lock() {
        Ok(mut mempool) => {
            // Check for duplicate signature (replay protection)
            if mempool.iter().any(|(_, existing_sig)| existing_sig.0 == signature.0) {
                return (StatusCode::CONFLICT, "duplicate transaction").into_response();
            }
            mempool.push((tx, signature));
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
    recipient_pk: String,
    amount: u32,
    fee: u8,
    signature: String,
}

async fn get_mempool(
    Query(query): Query<MempoolQuery>,
    State(state): State<AppState>
) -> impl IntoResponse {
    // Parse recipient_pk if provided
    let recipient_pk_filter: Option<G1> = if let Some(ref pk_hex) = query.recipient_pk {
        let pk_bytes = match hex::decode(pk_hex) {
            Ok(bytes) => bytes,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex in recipient_pk").into_response(),
        };
        let pk_arr: [u8; G1_SIZE] = match pk_bytes.try_into() {
            Ok(arr) => arr,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid recipient_pk length").into_response(),
        };
        Some(G1(pk_arr))
    } else {
        None
    };

    // Lock mempool and filter transactions
    let mempool = match state.mempool.lock() {
        Ok(mp) => mp,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "mempool lock failed").into_response(),
    };

    let filtered: Vec<MempoolTxResponse> = mempool
        .iter()
        .filter(|(tx, _sig)| {
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
            // Filter by amount if provided
            if let Some(amount) = query.amount {
                if tx.amount != amount {
                    return false;
                }
            }
            // Filter by fee if provided
            if let Some(fee) = query.fee {
                if tx.fee != fee {
                    return false;
                }
            }
            true
        })
        .map(|(tx, sig)| MempoolTxResponse {
            sender_id: tx.sender_id,
            recipient_pk: hex::encode(tx.recipient_pk.0),
            amount: tx.amount,
            fee: tx.fee,
            signature: hex::encode(sig.0),
        })
        .collect();

    Json(filtered).into_response()
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
    recipient_pk: String,
    amount: u32,
    fee: u8,
    btc_txid: String,
}

/// Get transactions that were recently broadcast to Bitcoin (in-flight)
async fn get_recently_broadcast(
    Query(query): Query<MempoolQuery>,
    State(state): State<AppState>
) -> impl IntoResponse {
    // Parse recipient_pk if provided
    let recipient_pk_filter: Option<G1> = if let Some(ref pk_hex) = query.recipient_pk {
        let pk_bytes = match hex::decode(pk_hex) {
            Ok(bytes) => bytes,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex in recipient_pk").into_response(),
        };
        let pk_arr: [u8; G1_SIZE] = match pk_bytes.try_into() {
            Ok(arr) => arr,
            Err(_) => return (StatusCode::BAD_REQUEST, "invalid recipient_pk length").into_response(),
        };
        Some(G1(pk_arr))
    } else {
        None
    };

    // Lock recently_broadcast and filter + cleanup old entries
    let mut recently_broadcast = match state.recently_broadcast.lock() {
        Ok(rb) => rb,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "recently_broadcast lock failed").into_response(),
    };

    // Remove entries older than TTL
    let now = Instant::now();
    recently_broadcast.retain(|(_, timestamp, _)| {
        now.duration_since(*timestamp).as_secs() < RECENTLY_BROADCAST_TTL_SECS
    });

    let filtered: Vec<RecentlyBroadcastTxResponse> = recently_broadcast
        .iter()
        .filter(|(tx, _timestamp, _btc_txid)| {
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
        .map(|(tx, _timestamp, btc_txid)| RecentlyBroadcastTxResponse {
            sender_id: tx.sender_id,
            recipient_pk: hex::encode(tx.recipient_pk.0),
            amount: tx.amount,
            fee: tx.fee,
            btc_txid: btc_txid.to_string(),
        })
        .collect();

    Json(filtered).into_response()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/account/:pk", get(get_account_by_pk))
        .route("/tx", post(submit_tx))
        .route("/mempool", get(get_mempool))
        .route("/recently-broadcast", get(get_recently_broadcast))
        .route("/address", get(get_address))
        .with_state(state)
} 