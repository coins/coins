use serde::{Serialize, Deserialize};
use crate::models::BlockSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub network: NetworkStats,
    pub bitcoin: BitcoinStats,
    pub recent_blocks: Vec<BlockSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub total_blocks: usize,
    pub finalized_blocks: usize,
    pub total_accounts: usize,
    pub total_transactions: usize,
    pub total_supply: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinStats {
    pub current_height: u32,
    pub finality_height: u32,
    pub finality_depth: u32,
}
