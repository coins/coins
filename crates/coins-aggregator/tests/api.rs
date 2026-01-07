use axum::{http::{Request, StatusCode}, body::Body};
use tower::util::ServiceExt;
use std::sync::Arc;
use tempfile::TempDir;

// bring server-side types directly from source since crate is bin-only
#[path = "../src/api.rs"]
mod api;
use api::{AppState, router};

use coins_types::Transaction;
use coins_crypto::{G1, SecretKey};
use coins_core::State;
use coins_indexer::Indexer;
use hex;
use bincode;

#[tokio::test]
async fn account_query_and_tx_submit() {
    // initialize state with one account
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("state.db");
    let indexer_path = temp_dir.path().join("indexer.db");

    let state = Arc::new(State::open(&state_path).unwrap());
    let indexer = Arc::new(Indexer::open(&indexer_path, state.clone()).unwrap());

    let test_sk = SecretKey::random();
    let test_pk = G1(test_sk.public_key());
    let mut acct = state.create_account(test_pk).unwrap();
    acct.balance = 100;
    state.insert_account(&acct).unwrap();

    let app_state = AppState {
        state: state.clone(),
        indexer: indexer.clone(),
        mempool: Arc::new(std::sync::Mutex::new(Vec::new())),
    };

    let app = router(app_state.clone());

    // query account
    let pk_hex = hex::encode(test_pk.0);
    let uri = format!("/account/{}", pk_hex);
    let resp = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // submit tx
    let recipient_sk = SecretKey::random();
    let recipient_pk = G1(recipient_sk.public_key());
    let tx = Transaction {
        sender_id: acct.id.0,
        recipient_pk,
        amount: 50,
        fee: 1
    };

    // Serialize transaction
    let tx_bytes = bincode::serde::encode_to_vec(&tx, bincode::config::standard()).unwrap();
    let tx_hex = hex::encode(tx_bytes);

    // Create dummy signature (64 bytes of zeros)
    let sig_hex = hex::encode([0u8; 64]);

    let json = serde_json::json!({
        "tx": tx_hex,
        "signature": sig_hex
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tx")
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // mempool should have 1 item
    assert_eq!(app_state.mempool.lock().unwrap().len(), 1);
}
