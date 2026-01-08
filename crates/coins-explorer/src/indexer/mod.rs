pub mod tx_index;
pub mod cache;

use std::sync::Arc;
use anyhow::Result;
use coins_indexer::IndexerClient;
use crate::bitcoin::BitcoinClient;
use crate::config::ExplorerConfig;

pub use tx_index::TxIndex;
pub use cache::Cache;

pub struct ExplorerIndexer {
    pub indexer_client: Arc<IndexerClient>,
    pub tx_index: Arc<TxIndex>,
    pub cache: Arc<Cache>,
    pub bitcoin_client: Arc<BitcoinClient>,
}

impl ExplorerIndexer {
    pub fn open(config: &ExplorerConfig) -> Result<Self> {
        tracing::info!("Opening explorer indexer...");

        // Connect to indexer service via HTTP client
        tracing::info!("Connecting to indexer service: {}", config.indexer.url);
        let indexer_client = Arc::new(IndexerClient::new(config.indexer.url.clone()));

        // Create explorer-specific transaction index
        tracing::info!("Opening transaction index: {:?}", config.database.tx_index_db);
        let tx_index = Arc::new(TxIndex::open(&config.database.tx_index_db)?);

        // Initialize cache
        let cache = Arc::new(Cache::new(config.cache.clone()));

        // Bitcoin RPC client
        let bitcoin_client = Arc::new(BitcoinClient::new(
            config.bitcoin.rpc_url.clone(),
            config.bitcoin.rpc_user.clone(),
            config.bitcoin.rpc_pass.clone(),
            config.bitcoin.network,
        )?);

        tracing::info!("Explorer indexer opened successfully");

        Ok(Self {
            indexer_client,
            tx_index,
            cache,
            bitcoin_client,
        })
    }

    /// Build transaction index from existing blocks (run on startup if index is empty)
    pub async fn build_tx_index_if_needed(&self) -> Result<()> {
        if self.tx_index.is_empty()? {
            tracing::info!("Transaction index is empty, building from existing blocks...");
            let start = std::time::Instant::now();

            self.tx_index.build_from_indexer_client(&self.indexer_client).await?;

            tracing::info!(
                duration_ms = start.elapsed().as_millis(),
                "Transaction index built successfully"
            );
        } else {
            tracing::info!("Transaction index already exists");
        }
        Ok(())
    }
}
