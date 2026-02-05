//! Coins client library
//!
//! Provides utilities for interacting with the Coins publisher API.

use coins_crypto::G1;
use coins_types::Account;
use reqwest::Client;

/// Error type for recipient resolution
#[derive(Debug, thiserror::Error)]
pub enum RecipientError {
    #[error("Account #{0} not found")]
    AccountNotFound(u32),
    #[error("Error fetching account #{0}: {1}")]
    FetchError(u32, reqwest::StatusCode),
    #[error("Invalid recipient: expected account ID or 64 hex chars")]
    InvalidFormat,
    #[error("Request failed: {0}")]
    RequestError(#[from] reqwest::Error),
}

/// Resolves a recipient string to a public key.
///
/// The recipient can be either:
/// - A numeric account ID (e.g., "42") - will be looked up via the publisher API
/// - A 64-character hex public key (e.g., "abc123...")
///
/// # Arguments
/// * `client` - HTTP client for making API requests
/// * `publisher_url` - Base URL of the publisher service
/// * `recipient` - The recipient string to resolve
///
/// # Returns
/// The resolved public key (G1)
pub async fn resolve_recipient(
    client: &Client,
    publisher_url: &str,
    recipient: &str,
) -> Result<G1, RecipientError> {
    if let Ok(account_id) = recipient.parse::<u32>() {
        // It's an account ID - fetch the account to get the public key
        let url = format!("{}/account/by-id/{}", publisher_url, account_id);
        let res = client.get(&url).send().await?;

        if res.status().is_success() {
            let account: Account = res.json().await?;
            Ok(account.pk)
        } else if res.status() == reqwest::StatusCode::NOT_FOUND {
            Err(RecipientError::AccountNotFound(account_id))
        } else {
            Err(RecipientError::FetchError(account_id, res.status()))
        }
    } else {
        // It's a public key (hex string)
        G1::from_hex(recipient).map_err(|_| RecipientError::InvalidFormat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_hex_public_key() {
        // A valid 64-char hex string should be parsed directly without API call
        let valid_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // We can test the parsing logic directly
        let result = G1::from_hex(valid_hex);
        assert!(result.is_ok(), "Valid 64-char hex should parse");
    }

    #[test]
    fn test_invalid_hex_rejected() {
        // Invalid hex should be rejected
        let invalid_hex = "not_a_valid_hex_string";
        let result = G1::from_hex(invalid_hex);
        assert!(result.is_err(), "Invalid hex should fail");
    }

    #[test]
    fn test_short_hex_rejected() {
        // Too short hex should be rejected
        let short_hex = "0123456789abcdef";
        let result = G1::from_hex(short_hex);
        assert!(result.is_err(), "Short hex should fail");
    }
}
