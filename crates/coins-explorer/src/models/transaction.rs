use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSearchQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub account_id: Option<u32>,
    pub min_amount: Option<u32>,
    pub max_amount: Option<u32>,
}

impl TxSearchQuery {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(0)
    }

    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(20).min(100)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSearchResponse {
    pub transactions: Vec<TransactionDetail>,
    pub total_count: usize,
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionDetail {
    pub btc_height: u32,
    pub btc_txid: String,
    pub btc_confirmations: u32,
    pub sender_id: u32,
    pub sender_pk: String,
    pub recipient_pk: String,
    pub recipient_id: Option<u32>,
    pub amount: u32,
    pub fee: u8,
    pub finalized: bool,
}
