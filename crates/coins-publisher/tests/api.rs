use axum::body::Body;
use axum::http::{Request, StatusCode};
use coins_crypto::{sign, G1, G2, G2_SIZE, SecretKey};
use coins_indexer::IndexerClient;
use coins_publisher::api::{AppState, router};
use coins_types::Transaction;
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_test_state(indexer_url: &str) -> AppState {
    AppState {
        indexer: Arc::new(IndexerClient::new(indexer_url.to_string())),
        mempool: Arc::new(Mutex::new(Vec::new())),
        fee_addr: Arc::new(Mutex::new("bc1qtest".to_string())),
        recently_broadcast: Arc::new(Mutex::new(Vec::new())),
    }
}

#[tokio::test]
async fn test_get_address() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    let response = app
        .oneshot(Request::builder().uri("/address").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["address"], "bc1qtest");
}

#[tokio::test]
async fn test_submit_tx_valid() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());
    let app = router(state.clone());

    // Create a valid transaction
    let sk = SecretKey::random();
    let recipient_pk = SecretKey::random().public_key();
    let tx = Transaction {
        sender_id: 1,
        recipient_pk: G1(recipient_pk),
        amount: 100,
        fee: 1,
    };

    // Sign the transaction
    let tx_bytes = bincode::serde::encode_to_vec(&tx, bincode::config::standard()).unwrap();
    let mut msg = tx_bytes.clone();
    msg.extend_from_slice(&0u32.to_le_bytes()); // nonce = 0
    let signature = sign(&sk, &msg);

    let body = serde_json::json!({
        "tx": hex::encode(&tx_bytes),
        "signature": hex::encode(signature.0)
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tx")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Verify tx was added to mempool
    let mempool = state.mempool.lock().unwrap();
    assert_eq!(mempool.len(), 1);
    assert_eq!(mempool[0].0.sender_id, 1);
    assert_eq!(mempool[0].0.amount, 100);
}

#[tokio::test]
async fn test_submit_tx_duplicate_rejected() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());

    // Create and sign a transaction
    let sk = SecretKey::random();
    let recipient_pk = SecretKey::random().public_key();
    let tx = Transaction {
        sender_id: 1,
        recipient_pk: G1(recipient_pk),
        amount: 100,
        fee: 1,
    };

    let tx_bytes = bincode::serde::encode_to_vec(&tx, bincode::config::standard()).unwrap();
    let mut msg = tx_bytes.clone();
    msg.extend_from_slice(&0u32.to_le_bytes());
    let signature = sign(&sk, &msg);

    let body = serde_json::json!({
        "tx": hex::encode(&tx_bytes),
        "signature": hex::encode(signature.0)
    });

    // First submission should succeed
    let app = router(state.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tx")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Second submission with same signature should be rejected
    let app = router(state.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tx")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_submit_tx_invalid_hex() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    let body = serde_json::json!({
        "tx": "not_valid_hex",
        "signature": "0102030405"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tx")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_mempool_empty() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    let response = app
        .oneshot(Request::builder().uri("/mempool").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(json.is_empty());
}

#[tokio::test]
async fn test_get_mempool_with_txs() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());

    // Add a tx to mempool directly
    let recipient_pk = SecretKey::random().public_key();
    let tx = Transaction {
        sender_id: 42,
        recipient_pk: G1(recipient_pk),
        amount: 500,
        fee: 2,
    };
    let sig = G2([0u8; G2_SIZE]);
    state.mempool.lock().unwrap().push((tx, sig));

    let app = router(state);
    let response = app
        .oneshot(Request::builder().uri("/mempool").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["sender_id"], 42);
    assert_eq!(json[0]["amount"], 500);
}

#[tokio::test]
async fn test_get_mempool_filtered_by_sender() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());

    // Add txs from different senders
    let recipient_pk = SecretKey::random().public_key();
    let tx1 = Transaction {
        sender_id: 1,
        recipient_pk: G1(recipient_pk),
        amount: 100,
        fee: 1,
    };
    let tx2 = Transaction {
        sender_id: 2,
        recipient_pk: G1(recipient_pk),
        amount: 200,
        fee: 1,
    };
    let sig = G2([0u8; G2_SIZE]);
    {
        let mut mempool = state.mempool.lock().unwrap();
        mempool.push((tx1, sig));
        mempool.push((tx2, G2([1u8; G2_SIZE])));
    }

    let app = router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/mempool?sender_id=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["sender_id"], 1);
}

#[tokio::test]
async fn test_get_account_not_found() {
    let mock_server = MockServer::start().await;

    // Mock indexer returning 404
    let pk = SecretKey::random().public_key();
    let pk_hex = hex::encode(pk);
    Mock::given(method("GET"))
        .and(path(format!("/accounts/{}", pk_hex)))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/account/{}", pk_hex))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_account_found() {
    use coins_types::{Account, AccountId};

    let mock_server = MockServer::start().await;

    let pk = SecretKey::random().public_key();
    let pk_hex = hex::encode(pk);

    // Create a real Account object and serialize it
    let account = Account {
        id: AccountId(1),
        pk: G1(pk),
        balance: 1000,
        nonce: 5,
    };

    Mock::given(method("GET"))
        .and(path(format!("/accounts/{}", pk_hex)))
        .respond_with(ResponseTemplate::new(200).set_body_json(&account))
        .mount(&mock_server)
        .await;

    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/account/{}", pk_hex))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["balance"], 1000);
    assert_eq!(json["nonce"], 5);
}

#[tokio::test]
async fn test_get_account_invalid_pk() {
    let mock_server = MockServer::start().await;
    let state = create_test_state(&mock_server.uri());
    let app = router(state);

    // Invalid hex
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/account/not_hex")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Valid hex but wrong length
    let app = router(create_test_state(&mock_server.uri()));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/account/0102030405")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
