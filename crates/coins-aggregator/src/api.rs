use axum::{Router, routing::get, routing::post, Json, extract::{Path, State}};
use serde::{Serialize, Deserialize};
use std::sync::{Arc,Mutex};
use axum::response::IntoResponse;
use axum::http::StatusCode;
use coins_types::{Account, Transaction};
use coins_crypto::{G1, G2};
use hex;
use bincode::serde::decode_from_slice;
use bincode::config::standard as bincode_config;

#[derive(Clone)]
pub struct AppState {
    pub accounts: Arc<Mutex<Vec<Account>>>,   // simple Vec for demo
    pub mempool: Arc<Mutex<Vec<(Transaction, G2)>>>,   // raw tx bytes for now
}

impl Default for AppState {
    fn default() -> Self {
        Self { accounts: Arc::new(Mutex::new(Vec::new())), mempool: Arc::new(Mutex::new(Vec::new())) }
    }
}

async fn get_account_by_pk(Path(pk_hex): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    let pk_bytes = match hex::decode(pk_hex) {
        Ok(bytes) => bytes,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid hex public key").into_response(),
    };

    let pk_arr: [u8; 32] = match pk_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid public key length").into_response(),
    };
    let pk = G1(pk_arr);

    let accs = state.accounts.lock().unwrap();
    if let Some(acc) = accs.iter().find(|a| a.pk == pk).cloned() {
        Json(acc).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
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

    let sig_arr: [u8; 64] = match sig_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid signature length").into_response(),
    };
    let signature = G2(sig_arr);
    
    state.mempool.lock().unwrap().push((tx, signature));
    (StatusCode::OK, "accepted").into_response()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/account/:pk", get(get_account_by_pk))
        .route("/tx", post(submit_tx))
        .with_state(state)
} 