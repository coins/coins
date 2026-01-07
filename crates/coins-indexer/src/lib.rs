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
    /// Bitcoin block height where the anchor tx was confirmed
    pub btc_height: u32,
    /// Number of confirmations (updated as chain grows)
    pub btc_confirmations: u32,
    /// The sub-block content
    pub sub_block: SubBlock,
    /// State root hash after applying this sub-block (for future use)
    pub state_root: [u8; 32],
}

impl ChainBlock {
    /// Serialize ChainBlock to bytes
    fn serialize(&self, state: &State) -> Vec<u8> {
        let mut v = Vec::new();
        // Txid (32 bytes)
        v.extend_from_slice(self.btc_txid.as_ref());
        // Heights and confirmations (12 bytes)
        v.extend_from_slice(&self.btc_height.to_le_bytes());
        v.extend_from_slice(&self.btc_confirmations.to_le_bytes());
        // Sub-block (variable length)
        let sub_block_bytes = self.sub_block.serialize(state);
        v.extend_from_slice(&(sub_block_bytes.len() as u32).to_le_bytes());
        v.extend_from_slice(&sub_block_bytes);
        // State root (32 bytes)
        v.extend_from_slice(&self.state_root);
        v
    }

    /// Deserialize ChainBlock from bytes
    fn deserialize(data: &[u8], state: &State) -> Option<Self> {
        if data.len() < 80 { return None; } // 32 + 12 + 4 + 32 minimum

        let txid_bytes: [u8; 32] = data[0..32].try_into().ok()?;
        let btc_txid = Txid::from_byte_array(txid_bytes);

        let btc_height = u32::from_le_bytes(data[32..36].try_into().ok()?);
        let btc_confirmations = u32::from_le_bytes(data[36..40].try_into().ok()?);

        let sub_block_len = u32::from_le_bytes(data[40..44].try_into().ok()?) as usize;
        if data.len() < 44 + sub_block_len + 32 { return None; }

        let sub_block = SubBlock::deserialize(&data[44..44 + sub_block_len], state)?;

        let state_root: [u8; 32] = data[44 + sub_block_len..44 + sub_block_len + 32]
            .try_into()
            .ok()?;

        Some(Self {
            btc_txid,
            btc_height,
            btc_confirmations,
            sub_block,
            state_root,
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
    #[allow(dead_code)]
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
            btc_height,
            btc_confirmations: 0,
            sub_block,
            state_root: [0u8; 32], // TODO: compute actual state root
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

    /// Update confirmation counts based on current Bitcoin height
    pub fn update_confirmations(&self, current_btc_height: u32) -> Result<(), IndexerError> {
        for item in self.blocks.iter() {
            let (key, value) = item?;

            let height = u32::from_le_bytes(key.as_ref().try_into().unwrap());
            let mut chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            if current_btc_height >= height {
                let new_confirmations = current_btc_height - height + 1;
                if chain_block.btc_confirmations != new_confirmations {
                    chain_block.btc_confirmations = new_confirmations;

                    // Update in database
                    let updated_bytes = chain_block.serialize(&self.state);
                    self.blocks.insert(&key, updated_bytes)?;
                }
            }
        }

        Ok(())
    }

    /// Get all finalized blocks (6+ confirmations)
    pub fn get_finalized_blocks(&self) -> Result<Vec<ChainBlock>, IndexerError> {
        let mut finalized = Vec::new();

        for item in self.blocks.iter() {
            let (_, value) = item?;
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            if chain_block.btc_confirmations >= FINALITY_DEPTH {
                finalized.push(chain_block);
            }
        }

        finalized.sort_by_key(|b| b.btc_height);
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
    pub fn get_account_history(&self, pk: &G1) -> Result<Vec<Transaction>, IndexerError> {
        let mut history = Vec::new();

        for item in self.blocks.iter() {
            let (_, value) = item?;
            let chain_block = ChainBlock::deserialize(&value, &self.state)
                .ok_or(IndexerError::Serialization)?;

            // Only include finalized blocks
            if chain_block.btc_confirmations < FINALITY_DEPTH {
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
