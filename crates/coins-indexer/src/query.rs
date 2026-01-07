//! Query API for indexed blocks and transactions

use crate::{Indexer, IndexerError};

impl Indexer {
    /// Get block count
    pub fn get_block_count(&self) -> Result<usize, IndexerError> {
        Ok(self.blocks.len())
    }

    /// Get finalized block count
    pub fn get_finalized_count(&self, current_btc_height: u32) -> Result<usize, IndexerError> {
        let finalized = self.get_finalized_blocks(current_btc_height)?;
        Ok(finalized.len())
    }
}
