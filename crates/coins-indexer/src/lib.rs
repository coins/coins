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

pub use finality::FINALITY_DEPTH;

/// A sub-block anchored to a Bitcoin transaction
#[derive(Debug, Clone)]
pub struct ChainBlock {
    /// Bitcoin transaction ID that anchored this sub-block
    pub btc_txid: Txid,
    /// The sub-block content
    pub sub_block: SubBlock,
}

impl ChainBlock {
    /// Serialize ChainBlock to bytes
    fn serialize(&self, state: &State) -> Vec<u8> {
        let mut v = Vec::new();
        // Txid (32 bytes)
        v.extend_from_slice(self.btc_txid.as_ref());
        // Sub-block (variable length)
        let sub_block_bytes = self.sub_block.serialize(state);
        v.extend_from_slice(&(sub_block_bytes.len() as u32).to_le_bytes());
        v.extend_from_slice(&sub_block_bytes);
        v
    }

    /// Deserialize ChainBlock from bytes
    fn deserialize(data: &[u8], state: &State) -> Option<Self> {
        if data.len() < 36 { return None; } // 32 + 4 minimum

        let txid_bytes: [u8; 32] = data[0..32].try_into().ok()?;
        let btc_txid = Txid::from_byte_array(txid_bytes);

        let sub_block_len = u32::from_le_bytes(data[32..36].try_into().ok()?) as usize;
        if data.len() < 36 + sub_block_len { return None; }

        let sub_block = SubBlock::deserialize(&data[36..36 + sub_block_len], state)?;

        Some(Self {
            btc_txid,
            sub_block,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("Database error: {0}")]
    Db(#[from] sled::Error),

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
    #[allow(dead_code)]
    db: sled::Db,
    /// Tree: btc_height (u32) -> ChainBlock
    blocks: sled::Tree,
    /// Tree: btc_txid (Txid) -> btc_height (u32)
    txid_index: sled::Tree,
    /// Reference to account state (for querying accounts by ID)
    state: Arc<State>,
}

impl Indexer {
    /// Open or create indexer database
    pub fn open<P: AsRef<Path>>(path: P, state: Arc<State>) -> Result<Self, IndexerError> {
        let db = sled::open(path)?;
        let blocks = db.open_tree(b"blocks")?;
        let txid_index = db.open_tree(b"txid_index")?;

        Ok(Self {
            db,
            blocks,
            txid_index,
            state,
        })
    }

    /// Index a new sub-block at the given Bitcoin height
    pub fn index_block(
        &self,
        btc_txid: Txid,
        btc_height: u32,
        sub_block: SubBlock,
    ) -> Result<(), IndexerError> {
        tracing::info!(
            btc_height = btc_height,
            btc_txid = %btc_txid,
            tx_count = sub_block.txs.len(),
            "Indexing sub-block"
        );

        let chain_block = ChainBlock {
            btc_txid,
            sub_block,
        };

        // Serialize and store
        let block_bytes = chain_block.serialize(&self.state);

        self.blocks.insert(&btc_height.to_le_bytes(), block_bytes)?;

        // Index by txid
        let txid_key: &[u8] = btc_txid.as_ref();
        self.txid_index.insert(
            txid_key,
            &btc_height.to_le_bytes()
        )?;

        Ok(())
    }

    /// Get all finalized blocks (6+ confirmations)
    pub fn get_finalized_blocks(&self, current_btc_height: u32) -> Result<Vec<(u32, ChainBlock)>, IndexerError> {
        let mut finalized = Vec::new();

        for item in self.blocks.iter() {
            let (key, value) = item?;
            let height = u32::from_le_bytes(key.as_ref().try_into().unwrap());
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            let confirmations = current_btc_height.saturating_sub(height) + 1;
            if confirmations >= FINALITY_DEPTH {
                finalized.push((height, chain_block));
            }
        }

        finalized.sort_by_key(|(h, _)| *h);
        Ok(finalized)
    }

    /// Get block by Bitcoin height
    pub fn get_block_by_height(&self, height: u32) -> Result<Option<ChainBlock>, IndexerError> {
        let key = height.to_le_bytes();

        if let Some(value) = self.blocks.get(&key)? {
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

    /// Handle blockchain reorganization by removing blocks from the given height onward
    pub fn handle_reorg(&self, reorg_height: u32) -> Result<(), IndexerError> {
        tracing::warn!(reorg_height = reorg_height, "Handling blockchain reorganization");

        // Remove all blocks at or after reorg_height
        let mut keys_to_remove = Vec::new();

        for item in self.blocks.iter() {
            let (key, _) = item?;
            let height = u32::from_le_bytes(key.as_ref().try_into().unwrap());

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
            let (key, value) = item?;
            let height = u32::from_le_bytes(key.as_ref().try_into().unwrap());
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            // Only include finalized blocks
            let confirmations = current_btc_height.saturating_sub(height) + 1;
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
}
