use axum::{http::{Request, StatusCode}, body::Body};
use tower::util::ServiceExt;

// bring server-side types directly from source since crate is bin-only
#[path = "../src/api.rs"]
mod api;
use api::{AppState, router};

use coins_types::{Account, AccountId};
use coins_crypto::G1;
use hex;

#[tokio::test]
async fn account_query_and_tx_submit() {
    // initialize state with one account
    let state = AppState::default();
    let test_pk = G1::default();
    {
        let mut accs = state.accounts.lock().unwrap();
        accs.push(Account {id:coins_types::AccountId(1),balance:100,nonce:0, pk: test_pk });
    }

    let app = router(state.clone());

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
    let json = serde_json::json!({ "tx": "deadbeef" });
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
    assert_eq!(state.mempool.lock().unwrap().len(), 1);
} 