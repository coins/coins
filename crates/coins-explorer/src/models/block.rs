use serde::{Serialize, Deserialize};
use crate::bitcoin::BitcoinLinks;
use crate::models::TransactionDetail;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockListQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub finalized_only: Option<bool>,
}

impl BlockListQuery {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(0)
    }

    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(20).min(100)
    }

    pub fn finalized_only(&self) -> bool {
        self.finalized_only.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockListResponse {
    pub blocks: Vec<BlockSummary>,
    pub total_count: usize,
    pub page: u32,
    pub limit: u32,
    pub current_btc_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSummary {
    pub btc_height: u32,
    pub btc_txid: String,
    pub btc_confirmations: u32,
    pub tx_count: usize,
    pub publisher_pk: String,
    pub timestamp: Option<u64>,
    pub finalized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockDetail {
    pub btc_height: u32,
    pub btc_txid: String,
    pub btc_confirmations: u32,
    pub btc_timestamp: Option<u64>,
    pub publisher_pk: String,
    pub sigma: String,
    pub txs: Vec<TransactionDetail>,
    pub finalized: bool,
    pub bitcoin_links: BitcoinLinks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestBlockResponse {
    pub block: Option<BlockDetail>,
    pub current_btc_height: u32,
}
