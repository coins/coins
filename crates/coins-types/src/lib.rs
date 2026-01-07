//! Core protocol data structures (skeleton).
//!
//! These are pure value types with minimal helper methods so that other
//! crates can already depend on them while the detailed serialization logic
//! is filled in later phases.

use coins_crypto::{G1, G2};
use serde::{Serialize, Deserialize};
use bincode::serde::{encode_to_vec as bincode_serialize, decode_from_slice as bincode_deserialize};
use bincode::config::{standard, Config};

/// Wire-size constant 
pub const TX_SIZE: usize = 41; 

pub type Amount = u32;
pub type Fee = u8;

/// Unique identifier for an account (monotonically increasing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub u32);

/// On-chain account state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub pk: G1,
    pub balance: u64,
    pub nonce: u32,
}

/// Wire-format transaction (41 bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub sender_id: u32,
    pub recipient_pk: G1,
    pub amount: Amount,
    pub fee: Fee,
}

impl Transaction {
    /// Serialize the transaction to fixed 41-byte array using spec bincode options.
    pub fn serialize(&self) -> [u8; TX_SIZE] {
        let vec: Vec<u8> = bincode_serialize(self, bin_config()).expect("bincode serialize");
        debug_assert_eq!(vec.len(), TX_SIZE);
        let mut arr = [0u8; TX_SIZE];
        arr.copy_from_slice(&vec);
        arr
    }

    /// Deserialize from a 41-byte slice (panics on failure, helper for tests).
    pub fn deserialize(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), TX_SIZE, "tx size mismatch");
        bincode_deserialize(bytes, bin_config()).expect("bincode deserialize").0
    }

    /// Build the canonical message that gets signed: sender_id||recipient_pk||amount||fee||nonce
    pub fn message_to_sign(&self, nonce: u32) -> Vec<u8> {
        let mut v = self.serialize().to_vec();
        v.extend_from_slice(&nonce.to_le_bytes());
        v
    }
}

/// A batch of transactions together with an aggregated signature.
#[derive(Debug, Clone)]
pub struct SubBlock {
    pub sigma: G2,
    pub publisher_pk: G1,
    pub txs: Vec<Transaction>,
}

impl SubBlock {
    /// Serialize SubBlock into Vec<u8> (64-byte sigma + 32-byte publisher_pk + N×41-byte txs).
    pub fn serialize(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(64 + 32 + self.txs.len() * TX_SIZE);
        v.extend_from_slice(&bincode_serialize(&self.sigma, bin_config()).expect("sigma ser"));
        v.extend_from_slice(&bincode_serialize(&self.publisher_pk, bin_config()).expect("pk ser"));
        for tx in &self.txs {
            v.extend_from_slice(&tx.serialize());
        }
        v
    }

    /// Deserialize sub-block from bytes; returns None if size is invalid.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 96 { return None; }
        let (sigma_bytes, rest) = data.split_at(64);
        let (pk_bytes, tx_bytes) = rest.split_at(32);
        let sigma: G2 = {
            let (s, _): (G2, usize) = bincode_deserialize(sigma_bytes, bin_config()).ok()?;
            s
        };
        let publisher_pk: G1 = {
            let (pk, _): (G1, usize) = bincode_deserialize(pk_bytes, bin_config()).ok()?;
            pk
        };
        let tx_count = (tx_bytes.len()) / TX_SIZE;
        if tx_bytes.len() != tx_count * TX_SIZE { return None; }
        let mut txs = Vec::with_capacity(tx_count);
        for i in 0..tx_count {
            let start = i * TX_SIZE;
            let end = start + TX_SIZE;
            let tx_slice = &tx_bytes[start..end];
            txs.push(Transaction::deserialize(tx_slice));
        }
        Some(Self { sigma, publisher_pk, txs })
    }
}

// -----------------------------------------------------------------------------
// Module-private helpers
// -----------------------------------------------------------------------------

/// Fixed bincode configuration: little-endian + *fixed-int* encoding ⇒ 41-byte TX.
fn bin_config() -> impl Config {
    // `standard()` already uses little-endian. We explicitly set it again for clarity.
    standard()
        .with_fixed_int_encoding()
        .with_little_endian()
}

