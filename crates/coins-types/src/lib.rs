//! Core protocol data structures (skeleton).
//!
//! These are pure value types with minimal helper methods so that other
//! crates can already depend on them while the detailed serialization logic
//! is filled in later phases.

use coins_crypto::{G1, G2, G1_SIZE, G2_SIZE};
use serde::{Serialize, Deserialize};
use bincode::serde::{encode_to_vec as bincode_serialize, decode_from_slice as bincode_deserialize};
use bincode::config::{standard, Config};

/// Wire-size constant for canonical transaction
pub const TX_SIZE: usize = 41;

/// Wire-size constant for compact transaction
pub const COMPACT_TX_SIZE: usize = 13;

/// SubBlock header size (G2 signature + G1 publisher key)
pub const SUBBLOCK_HEADER_SIZE: usize = G2_SIZE + G1_SIZE;

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

    /// Convert to compact format (requires recipient to have an account ID)
    pub fn compact(&self, recipient_id: u32) -> CompactTransaction {
        CompactTransaction {
            sender_id: self.sender_id,
            recipient_id,
            amount: self.amount,
            fee: self.fee,
        }
    }
}

/// Compact wire-format transaction (13 bytes).
/// Uses recipient_id instead of recipient_pk to reduce size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactTransaction {
    pub sender_id: u32,
    pub recipient_id: u32,
    pub amount: Amount,
    pub fee: Fee,
}

impl CompactTransaction {
    /// Serialize to 13-byte wire format
    pub fn serialize(&self) -> [u8; COMPACT_TX_SIZE] {
        let vec: Vec<u8> = bincode_serialize(self, bin_config()).expect("bincode serialize");
        debug_assert_eq!(vec.len(), COMPACT_TX_SIZE, "compact tx size mismatch");
        let mut arr = [0u8; COMPACT_TX_SIZE];
        arr.copy_from_slice(&vec);
        arr
    }

    /// Deserialize from 13-byte wire format
    pub fn deserialize(data: &[u8; COMPACT_TX_SIZE]) -> Result<Self, String> {
        let (tx, _): (Self, usize) = bincode_deserialize(data, bin_config())
            .map_err(|e| format!("bincode deserialize: {}", e))?;
        Ok(tx)
    }

    /// Expand to canonical Transaction by providing recipient's public key
    pub fn expand(&self, recipient_pk: G1) -> Transaction {
        Transaction {
            sender_id: self.sender_id,
            recipient_pk,
            amount: self.amount,
            fee: self.fee,
        }
    }
}

/// Hybrid transaction format - either compact or canonical
#[derive(Debug, Clone)]
pub enum TxFormat {
    Compact(CompactTransaction),   // 13 bytes
    Canonical(Transaction),         // 41 bytes
}

/// A batch of transactions together with an aggregated signature.
#[derive(Debug, Clone)]
pub struct SubBlock {
    pub sigma: G2,
    pub publisher_pk: G1,
    pub txs: Vec<Transaction>,
}

impl SubBlock {
    /// Serialize SubBlock with hybrid format compression.
    /// Uses compact format (13 bytes) when recipient has an account, canonical (41 bytes) otherwise.
    /// Requires State to lookup account IDs for compression.
    ///
    /// Note: Publisher PK is NOT compressed to avoid bootstrapping issues (publisher account
    /// doesn't exist during first sub-block, causing deserialization to fail during IBD).
    ///
    /// Format: [sigma: 64] [publisher_pk: 32] [count: 2] [format_bits: ceil(count/8)] [txs: variable]
    pub fn serialize<S>(&self, state: &S) -> Vec<u8>
    where
        S: SubBlockState,
    {
        let tx_count = self.txs.len() as u16;
        let format_bytes = (tx_count as usize + 7) / 8; // ceil(tx_count/8)

        let mut v = Vec::new();

        // Sigma (64 bytes)
        v.extend_from_slice(&bincode_serialize(&self.sigma, bin_config()).expect("sigma"));

        // Publisher PK (32 bytes) - always canonical to avoid bootstrapping issues
        v.extend_from_slice(&bincode_serialize(&self.publisher_pk, bin_config()).expect("pk"));

        // TX count (2 bytes)
        v.extend_from_slice(&tx_count.to_le_bytes());

        // Format bitfield (ceil(tx_count/8) bytes) - 0 = compact, 1 = canonical
        let mut format_bits = vec![0u8; format_bytes];
        let mut tx_formats = Vec::with_capacity(tx_count as usize);

        for (i, tx) in self.txs.iter().enumerate() {
            // Try to compress: lookup recipient account ID
            if let Ok(Some(account_id)) = state.get_account_id_by_pk(&tx.recipient_pk) {
                // Can compress - recipient has an account
                tx_formats.push(TxFormat::Compact(tx.compact(account_id)));
            } else {
                // Cannot compress - new recipient
                format_bits[i / 8] |= 1 << (i % 8);  // Set bit for canonical
                tx_formats.push(TxFormat::Canonical(tx.clone()));
            }
        }
        v.extend_from_slice(&format_bits);

        // Transactions (ordered, variable size)
        for tx_format in &tx_formats {
            match tx_format {
                TxFormat::Compact(compact_tx) => {
                    v.extend_from_slice(&compact_tx.serialize());
                }
                TxFormat::Canonical(canonical_tx) => {
                    v.extend_from_slice(&canonical_tx.serialize());
                }
            }
        }

        v
    }

    /// Deserialize sub-block from bytes, expanding compressed transactions.
    /// Requires State to lookup recipient PKs from account IDs.
    pub fn deserialize<S>(data: &[u8], state: &S) -> Option<Self>
    where
        S: SubBlockState,
    {
        if data.len() < 98 { return None; } // Minimum: 64 + 32 + 2 = 98 bytes

        let (sigma_bytes, rest) = data.split_at(G2_SIZE);
        let (pk_bytes, rest) = rest.split_at(G1_SIZE);
        let (count_bytes, rest) = rest.split_at(2);

        let sigma: G2 = {
            let (s, _): (G2, usize) = bincode_deserialize(sigma_bytes, bin_config()).ok()?;
            s
        };
        let publisher_pk: G1 = {
            let (pk, _): (G1, usize) = bincode_deserialize(pk_bytes, bin_config()).ok()?;
            pk
        };

        let tx_count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]) as usize;
        let format_bytes = (tx_count + 7) / 8;

        if rest.len() < format_bytes { return None; }
        let (format_bits, mut tx_data) = rest.split_at(format_bytes);

        // Parse transactions using format bitfield, expand all to canonical
        let mut txs = Vec::with_capacity(tx_count);
        for i in 0..tx_count {
            let is_canonical = (format_bits[i / 8] >> (i % 8)) & 1 == 1;

            let canonical_tx = if is_canonical {
                if tx_data.len() < TX_SIZE { return None; }
                let tx = Transaction::deserialize(&tx_data[..TX_SIZE]);
                tx_data = &tx_data[TX_SIZE..];
                tx
            } else {
                if tx_data.len() < COMPACT_TX_SIZE { return None; }
                let tx_array: [u8; COMPACT_TX_SIZE] = tx_data[..COMPACT_TX_SIZE].try_into().ok()?;
                let compact_tx = CompactTransaction::deserialize(&tx_array).ok()?;
                tx_data = &tx_data[COMPACT_TX_SIZE..];

                // Expand to canonical by looking up recipient PK
                let recipient_pk = state.get_pk_by_account_id(compact_tx.recipient_id)?;
                compact_tx.expand(recipient_pk)
            };

            txs.push(canonical_tx);
        }

        Some(Self { sigma, publisher_pk, txs })
    }
}

/// Trait for state access required by SubBlock serialization.
/// Allows SubBlock to compress/decompress without directly depending on coins-core.
pub trait SubBlockState {
    /// Get account ID for a given public key (for compression)
    fn get_account_id_by_pk(&self, pk: &G1) -> Result<Option<u32>, ()>;

    /// Get public key for a given account ID (for decompression)
    fn get_pk_by_account_id(&self, id: u32) -> Option<G1>;
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

