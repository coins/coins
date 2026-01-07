use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountResponse {
    pub account: Option<AccountDetail>,
    pub current_btc_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountDetail {
    pub id: u32,
    pub pk: String,
    pub balance: u64,
    pub nonce: u32,
    pub tx_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTxQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub tx_type: Option<String>,
}

impl AccountTxQuery {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(0)
    }

    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(20).min(100)
    }

    pub fn tx_type_filter(&self) -> TxTypeFilter {
        match self.tx_type.as_deref() {
            Some("sent") => TxTypeFilter::Sent,
            Some("received") => TxTypeFilter::Received,
            _ => TxTypeFilter::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxTypeFilter {
    All,
    Sent,
    Received,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTxResponse {
    pub transactions: Vec<AccountTransaction>,
    pub total_count: usize,
    pub page: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTransaction {
    pub btc_height: u32,
    pub btc_txid: String,
    pub btc_confirmations: u32,
    pub tx_type: String,
    pub sender_id: u32,
    pub sender_pk: Option<String>,
    pub recipient_pk: String,
    pub recipient_id: Option<u32>,
    pub amount: u32,
    pub fee: u8,
    pub finalized: bool,
}
