//! Integration tests for recipient resolution
//!
//! Tests that the coins-client correctly resolves recipients specified as
//! account IDs or hex public keys.

use coins_client::resolve_recipient;
use coins_crypto::{G1, SecretKey};
use coins_types::{Account, AccountId, TX_SIZE};
use reqwest::Client;
use std::collections::BTreeMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test resolving a recipient by account ID
#[tokio::test]
async fn test_resolve_recipient_by_account_id() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    // Create a test account with a known public key
    let sk = SecretKey::random();
    let expected_pk = G1(sk.public_key());

    let mock_account = Account {
        id: AccountId(42),
        pk: expected_pk,
        balances: BTreeMap::new(),
        nonce: 0,
    };

    // Mock the /account/by-id/42 endpoint
    Mock::given(method("GET"))
        .and(path("/account/by-id/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&mock_account))
        .mount(&mock_server)
        .await;

    // Resolve by account ID
    let result = resolve_recipient(&client, &mock_server.uri(), "42").await;

    assert!(result.is_ok(), "Should resolve account ID successfully");
    let resolved_pk = result.unwrap();
    assert_eq!(resolved_pk.0, expected_pk.0, "Should return the correct public key");
}

/// Test resolving a recipient by hex public key (no API call needed)
#[tokio::test]
async fn test_resolve_recipient_by_hex_pubkey() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    // Create a known public key
    let sk = SecretKey::random();
    let expected_pk = G1(sk.public_key());
    let pk_hex = expected_pk.to_hex();

    // No mocks needed - hex resolution is local

    // Resolve by hex public key
    let result = resolve_recipient(&client, &mock_server.uri(), &pk_hex).await;

    assert!(result.is_ok(), "Should resolve hex public key successfully");
    let resolved_pk = result.unwrap();
    assert_eq!(resolved_pk.0, expected_pk.0, "Should return the same public key");
}

/// Test that account not found returns appropriate error
#[tokio::test]
async fn test_resolve_recipient_account_not_found() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    // Mock 404 response
    Mock::given(method("GET"))
        .and(path("/account/by-id/999"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let result = resolve_recipient(&client, &mock_server.uri(), "999").await;

    assert!(result.is_err(), "Should fail for non-existent account");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("not found"),
        "Error should mention 'not found': {}",
        err
    );
}

/// Test that invalid hex is rejected
#[tokio::test]
async fn test_resolve_recipient_invalid_hex() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    // Try to resolve an invalid recipient (not a number, not valid hex)
    let result = resolve_recipient(&client, &mock_server.uri(), "invalid_recipient").await;

    assert!(result.is_err(), "Should fail for invalid recipient format");
}

/// Test that short hex is rejected
#[tokio::test]
async fn test_resolve_recipient_short_hex() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    // Try to resolve a hex string that's too short
    let result = resolve_recipient(&client, &mock_server.uri(), "abcd1234").await;

    assert!(result.is_err(), "Should fail for short hex string");
}

/// Test that transaction size is correct (43 bytes for canonical format)
#[test]
fn test_transaction_size_is_correct() {
    use coins_types::Transaction;

    let sk = SecretKey::random();
    let recipient_pk = G1(sk.public_key());

    let tx = Transaction {
        sender_id: 1,
        recipient_pk,
        token_id: 0,
        amount: 100,
        fee: 1,
    };

    let serialized = tx.serialize();
    assert_eq!(
        serialized.len(),
        TX_SIZE,
        "Transaction should serialize to {} bytes",
        TX_SIZE
    );
    assert_eq!(TX_SIZE, 43, "TX_SIZE should be 43 bytes");
}

/// Test that we can round-trip a transaction through serialization
#[test]
fn test_transaction_serialization_roundtrip() {
    use coins_types::Transaction;

    let sk = SecretKey::random();
    let recipient_pk = G1(sk.public_key());

    let tx = Transaction {
        sender_id: 42,
        recipient_pk,
        token_id: 0,
        amount: 1000,
        fee: 5,
    };

    let serialized = tx.serialize();
    let deserialized = Transaction::deserialize(&serialized);

    assert_eq!(tx.sender_id, deserialized.sender_id);
    assert_eq!(tx.recipient_pk.0, deserialized.recipient_pk.0);
    assert_eq!(tx.token_id, deserialized.token_id);
    assert_eq!(tx.amount, deserialized.amount);
    assert_eq!(tx.fee, deserialized.fee);
}

/// Test that resolving works with leading zeros in account ID
#[tokio::test]
async fn test_resolve_recipient_account_id_with_leading_zero() {
    let mock_server = MockServer::start().await;
    let client = Client::new();

    let sk = SecretKey::random();
    let expected_pk = G1(sk.public_key());

    let mock_account = Account {
        id: AccountId(7),
        pk: expected_pk,
        balances: BTreeMap::new(),
        nonce: 0,
    };

    Mock::given(method("GET"))
        .and(path("/account/by-id/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&mock_account))
        .mount(&mock_server)
        .await;

    // "007" should parse as account ID 7
    let result = resolve_recipient(&client, &mock_server.uri(), "007").await;

    assert!(result.is_ok(), "Should resolve account ID with leading zeros");
    let resolved_pk = result.unwrap();
    assert_eq!(resolved_pk.0, expected_pk.0);
}
