use anyhow::Result;
use sled::Db;
use std::path::Path;
use coins_core::State;
use coins_indexer::Indexer;
use coins_types::Transaction;

pub struct TxIndex {
    #[allow(dead_code)]
    db: Db,
    // Tree: sender_id (u32) -> Vec<(btc_height, tx_offset)>
    sender_index: sled::Tree,
    // Tree: recipient_pk (32 bytes) -> Vec<(btc_height, tx_offset)>
    recipient_index: sled::Tree,
    // Tree: (btc_height, tx_offset) -> Transaction
    tx_data: sled::Tree,
}

#[derive(Debug, Clone, Copy)]
pub struct TxRef {
    pub btc_height: u32,
    pub tx_offset: u32,
}

impl TxIndex {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?;
        let sender_index = db.open_tree(b"sender_index")?;
        let recipient_index = db.open_tree(b"recipient_index")?;
        let tx_data = db.open_tree(b"tx_data")?;

        Ok(Self {
            db,
            sender_index,
            recipient_index,
            tx_data,
        })
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.tx_data.is_empty())
    }

    /// Build index from existing blocks in the indexer
    pub async fn build_from_indexer(&self, indexer: &Indexer, state: &State) -> Result<()> {
        tracing::info!("Building transaction index from indexer...");

        // Get all blocks from the indexer
        let mut block_count = 0;
        let mut tx_count = 0;

        // Iterate through all blocks in the indexer
        for item in indexer.blocks.iter() {
            let (key, value) = item?;
            let btc_height = u32::from_le_bytes(key.as_ref().try_into()?);

            // Deserialize the chain block
            if let Some(chain_block) = coins_indexer::ChainBlock::deserialize(&value, state) {
                // Index each transaction in the sub-block
                for (tx_offset, tx) in chain_block.sub_block.txs.iter().enumerate() {
                    self.index_transaction(tx, btc_height, tx_offset as u32)?;
                    tx_count += 1;
                }

                block_count += 1;
                if block_count % 100 == 0 {
                    tracing::debug!(blocks = block_count, txs = tx_count, "Indexing progress...");
                }
            }
        }

        tracing::info!(
            blocks = block_count,
            transactions = tx_count,
            "Transaction index built"
        );

        Ok(())
    }

    /// Index a single transaction
    pub fn index_transaction(&self, tx: &Transaction, btc_height: u32, tx_offset: u32) -> Result<()> {
        let tx_ref = TxRef { btc_height, tx_offset };

        // Index by sender
        self.add_to_sender_index(tx.sender_id, &tx_ref)?;

        // Index by recipient
        self.add_to_recipient_index(&tx.recipient_pk.0, &tx_ref)?;

        // Store transaction data
        let key = Self::tx_key(btc_height, tx_offset);
        let value = bincode::serialize(&tx)?;
        self.tx_data.insert(&key, value)?;

        Ok(())
    }

    fn add_to_sender_index(&self, sender_id: u32, tx_ref: &TxRef) -> Result<()> {
        let key = sender_id.to_le_bytes();
        let mut refs = self.get_sender_refs(sender_id)?;
        refs.push(*tx_ref);
        let value = bincode::serialize(&refs)?;
        self.sender_index.insert(&key, value)?;
        Ok(())
    }

    fn add_to_recipient_index(&self, recipient_pk: &[u8; 32], tx_ref: &TxRef) -> Result<()> {
        let mut refs = self.get_recipient_refs(recipient_pk)?;
        refs.push(*tx_ref);
        let value = bincode::serialize(&refs)?;
        self.recipient_index.insert(recipient_pk, value)?;
        Ok(())
    }

    pub fn get_sender_refs(&self, sender_id: u32) -> Result<Vec<TxRef>> {
        let key = sender_id.to_le_bytes();
        if let Some(value) = self.sender_index.get(&key)? {
            let refs: Vec<TxRef> = bincode::deserialize(&value)?;
            Ok(refs)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn get_recipient_refs(&self, recipient_pk: &[u8; 32]) -> Result<Vec<TxRef>> {
        if let Some(value) = self.recipient_index.get(recipient_pk)? {
            let refs: Vec<TxRef> = bincode::deserialize(&value)?;
            Ok(refs)
        } else {
            Ok(Vec::new())
        }
    }

    pub fn get_transaction(&self, btc_height: u32, tx_offset: u32) -> Result<Option<Transaction>> {
        let key = Self::tx_key(btc_height, tx_offset);
        if let Some(value) = self.tx_data.get(&key)? {
            let tx: Transaction = bincode::deserialize(&value)?;
            Ok(Some(tx))
        } else {
            Ok(None)
        }
    }

    fn tx_key(btc_height: u32, tx_offset: u32) -> [u8; 8] {
        let mut key = [0u8; 8];
        key[0..4].copy_from_slice(&btc_height.to_le_bytes());
        key[4..8].copy_from_slice(&tx_offset.to_le_bytes());
        key
    }
}

// Make TxRef serializable
impl serde::Serialize for TxRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (self.btc_height, self.tx_offset).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for TxRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let (btc_height, tx_offset) = <(u32, u32)>::deserialize(deserializer)?;
        Ok(Self { btc_height, tx_offset })
    }
}
