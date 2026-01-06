//! Query API for indexed blocks and transactions

use crate::{Indexer, IndexerError};
use bitcoin::Txid;
use serde::{Serialize, Deserialize};

/// Transaction status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxStatus {
    /// Bitcoin transaction ID that contains this transaction
    pub btc_txid: Option<Txid>,
    /// Bitcoin height where it was confirmed
    pub btc_height: Option<u32>,
    /// Number of confirmations
    pub confirmations: u32,
    /// Whether the transaction is finalized (6+ confirmations)
    pub finalized: bool,
}

impl Indexer {
    /// Query transaction status (for future implementation)
    pub fn get_tx_status(&self, _tx_hash: &[u8; 32]) -> Result<Option<TxStatus>, IndexerError> {
        // TODO: Implement transaction hash indexing
        // For now, return None
        Ok(None)
    }

    /// Get block count
    pub fn get_block_count(&self) -> Result<usize, IndexerError> {
        Ok(self.blocks.len())
    }

    /// Get finalized block count
    pub fn get_finalized_count(&self) -> Result<usize, IndexerError> {
        let finalized = self.get_finalized_blocks()?;
        Ok(finalized.len())
    }
}
