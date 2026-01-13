use axum::{Router, routing::get, routing::post, Json, extract::{Path, Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::{Arc,Mutex};
use axum::response::IntoResponse;
use axum::http::StatusCode;
use coins_types::Transaction;
use coins_crypto::{G1, G2, G1_SIZE, G2_SIZE};
use coins_indexer::IndexerClient;

#[derive(Clone)]
pub struct AppState {
    pub indexer: Arc<IndexerClient>,
    pub mempool: Arc<Mutex<Vec<(Transaction, G2)>>>,
    pub fee_addr: Arc<Mutex<String>>,
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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/account/:pk", get(get_account_by_pk))
        .route("/tx", post(submit_tx))
        .route("/mempool", get(get_mempool))
        .route("/address", get(get_address))
        .with_state(state)
} 