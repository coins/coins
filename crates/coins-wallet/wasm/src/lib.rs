//! WASM module for BLS signing and transaction building.
//!
//! This module provides browser-compatible cryptographic operations for the
//! coins wallet, including key generation, key import, signing, and transaction
//! construction.

use wasm_bindgen::prelude::*;
use ark_bn254::Fr;
use ark_ff::{PrimeField, BigInteger};
use coins_crypto::{SecretKey, G1, G2, sign, G1_SIZE};
use coins_types::{Transaction, TX_SIZE};

/// Secret key size in bytes (BN-254 scalar field element)
const SK_SIZE: usize = 32;

/// Initialize panic hook for better error messages in browser console
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// A wallet key pair for BLS signing.
///
/// Contains both the secret key (for signing) and derived public key.
/// The secret key should be encrypted at rest in the browser.
#[wasm_bindgen]
pub struct WalletKey {
    sk: SecretKey,
    pk: G1,
}

#[wasm_bindgen]
impl WalletKey {
    /// Generate a new random key pair.
    ///
    /// Uses the browser's cryptographically secure random number generator.
    #[wasm_bindgen(constructor)]
    pub fn new() -> WalletKey {
        let sk = SecretKey::random();
        let pk_bytes = sk.public_key();
        let pk = G1(pk_bytes);
        WalletKey { sk, pk }
    }

    /// Import an existing key from secret key bytes.
    ///
    /// # Arguments
    /// * `sk_bytes` - 32-byte secret key in little-endian format
    ///
    /// # Returns
    /// A new WalletKey, or an error if the bytes are invalid
    #[wasm_bindgen]
    pub fn from_bytes(sk_bytes: &[u8]) -> Result<WalletKey, JsValue> {
        if sk_bytes.len() != SK_SIZE {
            return Err(JsValue::from_str(&format!(
                "Invalid secret key length: expected {} bytes, got {}",
                SK_SIZE,
                sk_bytes.len()
            )));
        }

        // Convert bytes to Fr (field element) using little-endian encoding
        let fr = Fr::from_le_bytes_mod_order(sk_bytes);
        let sk = SecretKey(fr);
        let pk_bytes = sk.public_key();
        let pk = G1(pk_bytes);

        Ok(WalletKey { sk, pk })
    }

    /// Get the public key as a 64-character hex string.
    ///
    /// The public key is a compressed G1 point (32 bytes = 64 hex chars).
    #[wasm_bindgen]
    pub fn public_key_hex(&self) -> String {
        self.pk.to_hex()
    }

    /// Get the secret key as a hex string for backup/export.
    ///
    /// WARNING: This exposes the raw secret key. Only use for backup purposes.
    /// The secret key should be encrypted before storage.
    #[wasm_bindgen]
    pub fn secret_key_hex(&self) -> String {
        let bytes = self.sk.0.into_bigint().to_bytes_le();
        hex::encode(bytes)
    }

    /// Get the secret key as raw bytes for encryption.
    ///
    /// Returns 32 bytes in little-endian format.
    #[wasm_bindgen]
    pub fn secret_key_bytes(&self) -> Vec<u8> {
        self.sk.0.into_bigint().to_bytes_le()
    }

    /// Sign a message and return the signature as a hex string.
    ///
    /// # Arguments
    /// * `message` - The message bytes to sign
    ///
    /// # Returns
    /// A 128-character hex string representing the 64-byte G2 signature
    #[wasm_bindgen]
    pub fn sign(&self, message: &[u8]) -> String {
        let sig: G2 = sign(&self.sk, message);
        sig.to_hex()
    }
}

/// Build a transaction in wire format.
///
/// # Arguments
/// * `sender_id` - The sender's account ID (u32)
/// * `recipient_pk_hex` - The recipient's public key as 64-char hex
/// * `token_id` - The token ID (u16, 0 for native token)
/// * `amount` - The amount to send (u32)
/// * `fee` - The transaction fee (u8)
///
/// # Returns
/// The 43-byte serialized transaction, or an error
#[wasm_bindgen]
pub fn build_transaction(
    sender_id: u32,
    recipient_pk_hex: &str,
    token_id: u16,
    amount: u32,
    fee: u8,
) -> Result<Vec<u8>, JsValue> {
    // Parse recipient public key from hex
    let recipient_pk = G1::from_hex(recipient_pk_hex)
        .map_err(|e| JsValue::from_str(&format!("Invalid recipient public key: {}", e)))?;

    let tx = Transaction {
        sender_id,
        recipient_pk,
        token_id,
        amount,
        fee,
    };

    let bytes = tx.serialize();
    Ok(bytes.to_vec())
}

/// Build the message to be signed for a transaction.
///
/// The signing message is: tx_bytes || nonce (little-endian u32)
///
/// # Arguments
/// * `tx_bytes` - The serialized transaction (43 bytes)
/// * `nonce` - The sender's current nonce
///
/// # Returns
/// The message bytes to sign (47 bytes), or an error
#[wasm_bindgen]
pub fn build_signing_message(tx_bytes: &[u8], nonce: u32) -> Result<Vec<u8>, JsValue> {
    if tx_bytes.len() != TX_SIZE {
        return Err(JsValue::from_str(&format!(
            "Invalid transaction size: expected {} bytes, got {}",
            TX_SIZE,
            tx_bytes.len()
        )));
    }

    let mut msg = tx_bytes.to_vec();
    msg.extend_from_slice(&nonce.to_le_bytes());
    Ok(msg)
}

/// Get the wire size of a transaction (43 bytes).
#[wasm_bindgen]
pub fn get_tx_size() -> usize {
    TX_SIZE
}

/// Get the public key size (32 bytes).
#[wasm_bindgen]
pub fn get_pk_size() -> usize {
    G1_SIZE
}
