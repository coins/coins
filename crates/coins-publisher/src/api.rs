use axum::{Router, routing::get, routing::post, Json, extract::{Path, State}};
use serde::Deserialize;
use std::sync::{Arc,Mutex};
use axum::response::IntoResponse;
use axum::http::StatusCode;
use coins_types::Transaction;
use coins_crypto::{G1, G2, G1_SIZE, G2_SIZE};
use coins_indexer::IndexerClient;
use hex;

#[derive(Clone)]
pub struct AppState {
    pub indexer: Arc<IndexerClient>,
    pub mempool: Arc<Mutex<Vec<(Transaction, G2)>>>,
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

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/account/:pk", get(get_account_by_pk))
        .route("/tx", post(submit_tx))
        .with_state(state)
} 