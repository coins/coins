pub mod tx_index;
pub mod cache;

use std::sync::Arc;
use anyhow::Result;
use coins_core::State;
use coins_indexer::Indexer;
use crate::bitcoin::BitcoinClient;
use crate::config::ExplorerConfig;

pub use tx_index::TxIndex;
pub use cache::Cache;

pub struct ExplorerIndexer {
    pub state: Arc<State>,
    pub indexer: Arc<Indexer>,
    pub tx_index: Arc<TxIndex>,
    pub cache: Arc<Cache>,
    pub bitcoin_client: Arc<BitcoinClient>,
}

impl ExplorerIndexer {
    pub fn open(config: &ExplorerConfig) -> Result<Self> {
        tracing::info!("Opening explorer indexer...");

        // Open publisher's databases in read-only mode
        tracing::info!("Opening state database: {:?}", config.database.state_db);
        let state = Arc::new(State::open(&config.database.state_db)?);

        tracing::info!("Opening indexer database: {:?}", config.database.indexer_db);
        let indexer = Arc::new(Indexer::open(&config.database.indexer_db, state.clone())?);

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
            state,
            indexer,
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

            self.tx_index.build_from_indexer(&self.indexer, &self.state).await?;

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
