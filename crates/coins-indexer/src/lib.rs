//! Chain indexer tracking and reorg handling

use std::path::Path;
use std::sync::Arc;
use bitcoin::Txid;
use bitcoin::hashes::Hash as HashTrait;
use coins_types::{SubBlock, Transaction};
use coins_crypto::G1;
use coins_core::State;

pub mod finality;
pub mod query;
pub mod client;
pub mod discovery;

pub use finality::FINALITY_DEPTH;
pub use client::IndexerClient;

// ChainBlock serialization layout constants
/// Bitcoin txid size in bytes
const TXID_SIZE: usize = 32;
/// u32 field size in bytes
const U32_SIZE: usize = std::mem::size_of::<u32>();

/// A sub-block anchored to a Bitcoin transaction
#[derive(Debug, Clone)]
pub struct ChainBlock {
    /// Bitcoin transaction ID that anchored this sub-block
    pub btc_txid: Txid,
    /// Bitcoin block height where this sub-block was anchored
    pub btc_height: u32,
    /// The sub-block content
    pub sub_block: SubBlock,
}

impl ChainBlock {
    /// Serialize ChainBlock to bytes
    fn serialize(&self, state: &State) -> Vec<u8> {
        let mut v = Vec::new();
        // Txid (32 bytes)
        v.extend_from_slice(self.btc_txid.as_ref());
        // Bitcoin height (4 bytes)
        v.extend_from_slice(&self.btc_height.to_le_bytes());
        // Sub-block (variable length)
        let sub_block_bytes = self.sub_block.serialize(state);
        v.extend_from_slice(&(sub_block_bytes.len() as u32).to_le_bytes());
        v.extend_from_slice(&sub_block_bytes);
        v
    }

    /// Deserialize ChainBlock from bytes
    pub fn deserialize(data: &[u8], state: &State) -> Option<Self> {
        const MIN_SIZE: usize = TXID_SIZE + U32_SIZE + U32_SIZE; // txid + btc_height + length
        if data.len() < MIN_SIZE { return None; }

        let txid_bytes: [u8; TXID_SIZE] = data[0..TXID_SIZE].try_into().ok()?;
        let btc_txid = Txid::from_byte_array(txid_bytes);

        let btc_height = u32::from_le_bytes(data[TXID_SIZE..TXID_SIZE + U32_SIZE].try_into().ok()?);

        let sub_block_len_offset = TXID_SIZE + U32_SIZE;
        let sub_block_len = u32::from_le_bytes(data[sub_block_len_offset..sub_block_len_offset + U32_SIZE].try_into().ok()?) as usize;

        let sub_block_offset = sub_block_len_offset + U32_SIZE;
        if data.len() < sub_block_offset + sub_block_len { return None; }

        let sub_block = SubBlock::deserialize(&data[sub_block_offset..sub_block_offset + sub_block_len], state)?;

        Some(Self {
            btc_txid,
            btc_height,
            sub_block,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("Database error: {0}")]
    Db(#[from] sled::Error),

    #[error("Database corruption: {0}")]
    Corruption(String),

    #[error("Serialization error")]
    Serialization,

    #[error("Block not found: {0}")]
    BlockNotFound(u32),

    #[error("State error")]
    State,
}

/// Chain indexer for tracking finalized sub-blocks
pub struct Indexer {
    /// Database handle (kept alive to maintain connection)
    _db: sled::Db,
    /// Tree: sub_chain_height (u32) -> ChainBlock
    pub blocks: sled::Tree,
    /// Tree: btc_txid (Txid) -> sub_chain_height (u32)
    pub txid_index: sled::Tree,
    /// Tree: metadata (stores latest sub_chain_height)
    pub metadata: sled::Tree,
    /// Reference to account state (for querying accounts by ID)
    pub state: Arc<State>,
}

impl Indexer {
    /// Open or create indexer database
    pub fn open<P: AsRef<Path>>(path: P, state: Arc<State>) -> Result<Self, IndexerError> {
        let db = sled::open(path)?;
        let blocks = db.open_tree(b"blocks")?;
        let txid_index = db.open_tree(b"txid_index")?;
        let metadata = db.open_tree(b"metadata")?;

        Ok(Self {
            _db: db,
            blocks,
            txid_index,
            metadata,
            state,
        })
    }

    /// Get the next sub-chain height
    fn get_next_height(&self) -> Result<u32, IndexerError> {
        // Get current height from metadata, or start at 0
        let height = if let Some(bytes) = self.metadata.get(b"latest_height")? {
            let arr: [u8; 4] = bytes.as_ref().try_into().map_err(|_| {
                tracing::error!(len = bytes.len(), "Corrupted latest_height in database");
                IndexerError::Corruption("corrupted latest_height".into())
            })?;
            u32::from_le_bytes(arr) + 1
        } else {
            0
        };
        Ok(height)
    }

    /// Update the latest sub-chain height
    fn set_latest_height(&self, height: u32) -> Result<(), IndexerError> {
        self.metadata.insert(b"latest_height", &height.to_le_bytes())?;
        Ok(())
    }

    /// Get the latest sub-chain height
    pub fn get_latest_height(&self) -> Result<Option<u32>, IndexerError> {
        if let Some(bytes) = self.metadata.get(b"latest_height")? {
            let arr: [u8; 4] = bytes.as_ref().try_into().map_err(|_| {
                tracing::error!(len = bytes.len(), "Corrupted latest_height in database");
                IndexerError::Corruption("corrupted latest_height".into())
            })?;
            Ok(Some(u32::from_le_bytes(arr)))
        } else {
            Ok(None)
        }
    }

    /// Index a new sub-block at the given Bitcoin height
    pub fn index_block(
        &self,
        btc_txid: Txid,
        btc_height: u32,
        sub_block: SubBlock,
    ) -> Result<(), IndexerError> {
        // Get next sequential sub-chain height
        let sub_chain_height = self.get_next_height()?;

        tracing::info!(
            sub_chain_height = sub_chain_height,
            btc_height = btc_height,
            btc_txid = %btc_txid,
            tx_count = sub_block.txs.len(),
            "Indexing sub-block"
        );

        let chain_block = ChainBlock {
            btc_txid,
            btc_height,
            sub_block,
        };

        // Serialize and store using sub-chain height as key
        let block_bytes = chain_block.serialize(&self.state);
        self.blocks.insert(sub_chain_height.to_le_bytes(), block_bytes)?;

        // Index by txid -> sub_chain_height
        let txid_key: &[u8] = btc_txid.as_ref();
        tracing::debug!(
            btc_txid = %btc_txid,
            txid_key_hex = hex::encode(txid_key),
            sub_chain_height = sub_chain_height,
            "Indexing txid"
        );
        self.txid_index.insert(txid_key, &sub_chain_height.to_le_bytes())?;

        // Update latest height
        self.set_latest_height(sub_chain_height)?;

        Ok(())
    }

    /// Get all finalized blocks (6+ confirmations)
    pub fn get_finalized_blocks(&self, current_btc_height: u32) -> Result<Vec<(u32, ChainBlock)>, IndexerError> {
        let mut finalized = Vec::new();

        for item in self.blocks.iter() {
            let (key, value) = item?;
            let sub_chain_height = match key.as_ref().try_into() {
                Ok(arr) => u32::from_le_bytes(arr),
                Err(_) => {
                    tracing::error!(len = key.len(), "Corrupted block key in database, skipping");
                    continue;
                }
            };
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            let confirmations = current_btc_height.saturating_sub(chain_block.btc_height) + 1;
            if confirmations >= FINALITY_DEPTH {
                finalized.push((sub_chain_height, chain_block));
            }
        }

        finalized.sort_by_key(|(h, _)| *h);
        Ok(finalized)
    }

    /// Get block by sub-chain height
    pub fn get_block_by_height(&self, sub_chain_height: u32) -> Result<Option<ChainBlock>, IndexerError> {
        let key = sub_chain_height.to_le_bytes();

        if let Some(value) = self.blocks.get(key)? {
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;
            Ok(Some(chain_block))
        } else {
            Ok(None)
        }
    }

    /// Get the latest indexed block
    pub fn get_latest_block(&self) -> Result<Option<ChainBlock>, IndexerError> {
        if let Some(item) = self.blocks.iter().last() {
            let (_, value) = item?;
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;
            Ok(Some(chain_block))
        } else {
            Ok(None)
        }
    }

    /// Check if a Bitcoin txid has been indexed
    pub fn has_txid(&self, btc_txid: &Txid) -> Result<bool, IndexerError> {
        let txid_key: &[u8] = btc_txid.as_ref();
        let exists = self.txid_index.contains_key(txid_key)?;
        tracing::debug!(
            btc_txid = %btc_txid,
            txid_key_hex = hex::encode(txid_key),
            exists = exists,
            "has_txid check"
        );
        Ok(exists)
    }

    /// Handle blockchain reorganization by removing blocks from the given height onward
    pub fn handle_reorg(&self, reorg_height: u32) -> Result<(), IndexerError> {
        tracing::warn!(reorg_height = reorg_height, "Handling blockchain reorganization");

        // Remove all blocks at or after reorg_height
        let mut keys_to_remove = Vec::new();

        for item in self.blocks.iter() {
            let (key, _) = item?;
            let height = match key.as_ref().try_into() {
                Ok(arr) => u32::from_le_bytes(arr),
                Err(_) => {
                    tracing::error!(len = key.len(), "Corrupted block key in database during reorg");
                    continue;
                }
            };

            if height >= reorg_height {
                keys_to_remove.push(key.to_vec());
            }
        }

        for key in keys_to_remove {
            self.blocks.remove(&key)?;
        }

        tracing::info!(
            reorg_height = reorg_height,
            "Reorganization handled, blocks removed"
        );

        Ok(())
    }

    /// Get transaction history for an account (simple implementation for demo)
    pub fn get_account_history(&self, pk: &G1, current_btc_height: u32) -> Result<Vec<Transaction>, IndexerError> {
        let mut history = Vec::new();

        for item in self.blocks.iter() {
            let (_, value) = item?;
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            // Only include finalized blocks
            let confirmations = current_btc_height.saturating_sub(chain_block.btc_height) + 1;
            if confirmations < FINALITY_DEPTH {
                continue;
            }

            // Find transactions involving this public key
            for tx in &chain_block.sub_block.txs {
                if &tx.recipient_pk == pk {
                    history.push(tx.clone());
                }
            }
        }

        Ok(history)
    }

    /// Update the Bitcoin height for a block (when it gets confirmed)
    pub fn update_btc_height(&self, sub_chain_height: u32, new_btc_height: u32) -> Result<(), IndexerError> {
        let key = sub_chain_height.to_le_bytes();

        if let Some(value) = self.blocks.get(&key)? {
            let mut chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            // Update the btc_height
            chain_block.btc_height = new_btc_height;

            // Serialize and store back
            let block_bytes = chain_block.serialize(&self.state);
            self.blocks.insert(&key, block_bytes)?;

            tracing::info!(
                sub_chain_height = sub_chain_height,
                btc_height = new_btc_height,
                btc_txid = %chain_block.btc_txid,
                "Updated Bitcoin block height"
            );

            Ok(())
        } else {
            Err(IndexerError::BlockNotFound(sub_chain_height))
        }
    }
}
