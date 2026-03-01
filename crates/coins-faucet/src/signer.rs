//! Transaction signing logic for the faucet.
//!
//! Handles loading the faucet secret key, building transactions,
//! signing them with BLS, and submitting to the publisher.

use anyhow::{anyhow, Result};
use ark_bn254::Fr;
use ark_ff::PrimeField;
use coins_crypto::{sign, G1, SecretKey};
use coins_types::Transaction;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Faucet signer that holds the secret key and handles transaction signing.
#[derive(Clone)]
pub struct FaucetSigner {
    /// BLS secret key
    sk: SecretKey,
    /// Public key (derived from sk)
    pub pk: G1,
    /// Account ID (fetched from indexer)
    pub account_id: Option<u32>,
}

/// Transaction submission request to the publisher.
#[derive(Debug, Serialize)]
struct TxSubmission {
    /// Transaction bytes as hex string
    tx: String,
    /// Signature bytes as hex string
    signature: String,
}

/// Account response from the indexer.
#[derive(Debug, Deserialize)]
pub struct AccountResponse {
    pub id: AccountIdWrapper,
    #[allow(dead_code)]
    pub pk: String,
    pub balances: std::collections::HashMap<String, u64>,
    pub nonce: u32,
}

#[derive(Debug, Deserialize)]
pub struct AccountIdWrapper(pub u32);

impl FaucetSigner {
    /// Create a new FaucetSigner from a hex-encoded secret key.
    ///
    /// The secret key should be 32 bytes encoded as 64 hex characters.
    pub fn from_hex(sk_hex: &str) -> Result<Self> {
        let bytes = hex::decode(sk_hex.trim())
            .map_err(|e| anyhow!("Invalid hex in secret key: {}", e))?;

        if bytes.len() != 32 {
            return Err(anyhow!(
                "Invalid secret key length: expected 32 bytes, got {}",
                bytes.len()
            ));
        }

        // Convert bytes to Fr (field element) using little-endian encoding
        let fr = Fr::from_le_bytes_mod_order(&bytes);
        let sk = SecretKey(fr);
        let pk_bytes = sk.public_key();
        let pk = G1(pk_bytes);

        info!(pk = %pk.to_hex(), "Faucet signer initialized");

        Ok(Self {
            sk,
            pk,
            account_id: None,
        })
    }

    /// Load secret key from a file.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let hex = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read secret key file: {}", e))?;
        Self::from_hex(&hex)
    }

    /// Get the faucet account details from the indexer.
    pub async fn fetch_account(&mut self, client: &Client, indexer_url: &str) -> Result<AccountResponse> {
        let url = format!("{}/accounts/{}", indexer_url, self.pk.to_hex());
        debug!(url = %url, "Fetching faucet account");

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to indexer: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Indexer returned error {}: {}",
                status,
                body
            ));
        }

        let account: AccountResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse account response: {}", e))?;

        self.account_id = Some(account.id.0);
        Ok(account)
    }

    /// Build, sign, and submit a transaction to send tokens.
    ///
    /// # Arguments
    /// * `client` - HTTP client for API calls
    /// * `indexer_url` - URL of the indexer service
    /// * `publisher_url` - URL of the publisher service
    /// * `recipient_pk` - Hex-encoded public key of the recipient
    /// * `token_id` - Token ID to send (0-255)
    /// * `amount` - Amount to send
    ///
    /// # Returns
    /// The transaction hash on success.
    pub async fn send_tokens(
        &mut self,
        client: &Client,
        indexer_url: &str,
        publisher_url: &str,
        recipient_pk: &str,
        token_id: u16,
        amount: u32,
    ) -> Result<String> {
        // Fetch current account state to get nonce
        let account = self.fetch_account(client, indexer_url).await?;
        let sender_id = account.id.0;
        let nonce = account.nonce;

        // Parse recipient public key
        let recipient = G1::from_hex(recipient_pk)
            .map_err(|e| anyhow!("Invalid recipient public key: {}", e))?;

        // Build transaction
        let tx = Transaction {
            sender_id,
            recipient_pk: recipient,
            token_id,
            amount,
            fee: 0, // Faucet doesn't charge fees
        };

        // Sign the transaction
        let msg = tx.message_to_sign(nonce);
        let sig = sign(&self.sk, &msg);

        // Serialize for submission
        let tx_bytes = tx.serialize();
        let tx_hex = hex::encode(tx_bytes);
        let sig_hex = hex::encode(sig.0);

        debug!(
            sender_id = sender_id,
            recipient = recipient_pk,
            token_id = token_id,
            amount = amount,
            nonce = nonce,
            "Submitting transaction"
        );

        // Submit to publisher
        let url = format!("{}/tx", publisher_url);
        let submission = TxSubmission {
            tx: tx_hex.clone(),
            signature: sig_hex,
        };

        let response = client
            .post(&url)
            .json(&submission)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to publisher: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Publisher returned error {}: {}",
                status,
                body
            ));
        }

        let body = response.text().await.unwrap_or_default();
        info!(
            recipient = recipient_pk,
            token_id = token_id,
            amount = amount,
            response = %body,
            "Transaction submitted successfully"
        );

        // Return a simple hash (first 16 hex chars of tx)
        Ok(tx_hex[..16].to_string())
    }

    /// Export public key as hex string.
    pub fn public_key_hex(&self) -> String {
        self.pk.to_hex()
    }
}
