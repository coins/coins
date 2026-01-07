use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use crate::api::AppState;
use crate::models::*;
use coins_indexer::{ChainBlock, FINALITY_DEPTH};

pub async fn get_stats(
    State(app_state): State<AppState>,
) -> Result<Json<StatsResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    let finality_height = current_btc_height.saturating_sub(FINALITY_DEPTH);

    // Count blocks and transactions
    let mut total_blocks = 0usize;
    let mut finalized_blocks = 0usize;
    let mut total_transactions = 0usize;
    let mut recent_blocks = Vec::new();

    for item in indexer.indexer.blocks.iter().rev() {
        let (key, value) = item
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

        let btc_height = u32::from_le_bytes(key.as_ref().try_into().unwrap());

        if let Some(chain_block) = ChainBlock::deserialize(&value, &indexer.state) {
            total_blocks += 1;
            total_transactions += chain_block.sub_block.txs.len();

            let confirmations = current_btc_height.saturating_sub(btc_height) + 1;
            let finalized = confirmations >= FINALITY_DEPTH;

            if finalized {
                finalized_blocks += 1;
            }

            // Collect recent blocks (last 10)
            if recent_blocks.len() < 10 {
                let timestamp = indexer.bitcoin_client.get_block_timestamp(btc_height).ok().flatten();

                recent_blocks.push(BlockSummary {
                    btc_height,
                    btc_txid: chain_block.btc_txid.to_string(),
                    btc_confirmations: confirmations,
                    tx_count: chain_block.sub_block.txs.len(),
                    publisher_pk: hex::encode(&chain_block.sub_block.publisher_pk.0),
                    timestamp,
                    finalized,
                });
            }
        }
    }

    // Count accounts
    let total_accounts = indexer.state.account_count();

    // Calculate total supply (sum of all balances)
    let total_supply = indexer.state.total_supply()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to calculate supply: {}", e)))?;

    Ok(Json(StatsResponse {
        network: NetworkStats {
            total_blocks,
            finalized_blocks,
            total_accounts,
            total_transactions,
            total_supply,
        },
        bitcoin: BitcoinStats {
            current_height: current_btc_height,
            finality_height,
            finality_depth: FINALITY_DEPTH,
        },
        recent_blocks,
    }))
}
