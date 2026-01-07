use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    response::Json,
};
use crate::api::AppState;
use crate::models::*;
use coins_indexer::{ChainBlock, FINALITY_DEPTH};
use coins_types::SubBlockState;

pub async fn list_blocks(
    State(app_state): State<AppState>,
    Query(query): Query<BlockListQuery>,
) -> Result<Json<BlockListResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    let page = query.page();
    let limit = query.limit();
    let finalized_only = query.finalized_only();

    // Collect all blocks
    let mut all_blocks: Vec<(u32, ChainBlock)> = Vec::new();

    for item in indexer.indexer.blocks.iter().rev() {
        let (key, value) = item
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

        let btc_height = u32::from_le_bytes(key.as_ref().try_into().unwrap());

        if let Some(chain_block) = ChainBlock::deserialize(&value, &indexer.state) {
            let confirmations = current_btc_height.saturating_sub(btc_height) + 1;
            let finalized = confirmations >= FINALITY_DEPTH;

            if !finalized_only || finalized {
                all_blocks.push((btc_height, chain_block));
            }
        }
    }

    let total_count = all_blocks.len();

    // Apply pagination
    let start = (page * limit) as usize;
    let end = start + limit as usize;
    let page_blocks = &all_blocks[start..end.min(total_count)];

    // Convert to BlockSummary
    let blocks: Vec<BlockSummary> = page_blocks
        .iter()
        .map(|(btc_height, chain_block)| {
            let confirmations = current_btc_height.saturating_sub(*btc_height) + 1;
            let timestamp = indexer.bitcoin_client.get_block_timestamp(*btc_height).ok().flatten();

            BlockSummary {
                btc_height: *btc_height,
                btc_txid: chain_block.btc_txid.to_string(),
                btc_confirmations: confirmations,
                tx_count: chain_block.sub_block.txs.len(),
                publisher_pk: hex::encode(&chain_block.sub_block.publisher_pk.0),
                timestamp,
                finalized: confirmations >= FINALITY_DEPTH,
            }
        })
        .collect();

    Ok(Json(BlockListResponse {
        blocks,
        total_count,
        page,
        limit,
        current_btc_height,
    }))
}

pub async fn get_block(
    State(app_state): State<AppState>,
    Path(height): Path<u32>,
) -> Result<Json<BlockDetail>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    // Check cache first
    if let Some(cached) = indexer.cache.get_block(height).await {
        return Ok(Json(cached));
    }

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    let key = height.to_le_bytes();
    let value = indexer.indexer.blocks.get(&key)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?
        .ok_or((StatusCode::NOT_FOUND, "Block not found".to_string()))?;

    let chain_block = ChainBlock::deserialize(&value, &indexer.state)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Failed to deserialize block".to_string()))?;

    let confirmations = current_btc_height.saturating_sub(height) + 1;
    let timestamp = indexer.bitcoin_client.get_block_timestamp(height).ok().flatten();
    let bitcoin_links = indexer.bitcoin_client.get_explorer_links(&chain_block.btc_txid.to_string());

    // Convert transactions
    let txs: Vec<TransactionDetail> = chain_block.sub_block.txs.iter().map(|tx| {
        let sender_pk = indexer.state.get_account(coins_types::AccountId(tx.sender_id))
            .ok()
            .flatten()
            .map(|acc| hex::encode(&acc.pk.0))
            .unwrap_or_else(|| "unknown".to_string());

        let recipient_id = indexer.state.get_account_id_by_pk(&tx.recipient_pk)
            .ok()
            .flatten();

        TransactionDetail {
            btc_height: height,
            btc_txid: chain_block.btc_txid.to_string(),
            btc_confirmations: confirmations,
            sender_id: tx.sender_id,
            sender_pk,
            recipient_pk: hex::encode(&tx.recipient_pk.0),
            recipient_id,
            amount: tx.amount,
            fee: tx.fee,
            finalized: confirmations >= FINALITY_DEPTH,
        }
    }).collect();

    let block_detail = BlockDetail {
        btc_height: height,
        btc_txid: chain_block.btc_txid.to_string(),
        btc_confirmations: confirmations,
        btc_timestamp: timestamp,
        publisher_pk: hex::encode(&chain_block.sub_block.publisher_pk.0),
        sigma: hex::encode(bincode::serialize(&chain_block.sub_block.sigma).unwrap()),
        txs,
        finalized: confirmations >= FINALITY_DEPTH,
        bitcoin_links,
    };

    // Cache the result
    indexer.cache.put_block(height, block_detail.clone()).await;

    Ok(Json(block_detail))
}

pub async fn latest_block(
    State(app_state): State<AppState>,
) -> Result<Json<LatestBlockResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    // Get the latest block
    let latest = indexer.indexer.get_latest_block()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

    if let Some(_chain_block) = latest {
        // Find the height by iterating backwards
        let mut latest_height = 0u32;
        for item in indexer.indexer.blocks.iter().rev() {
            let (key, _) = item
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;
            latest_height = u32::from_le_bytes(key.as_ref().try_into().unwrap());
            break;
        }

        // Use get_block to get the full detail with caching
        let block_json = get_block(State(app_state), Path(latest_height)).await?;

        Ok(Json(LatestBlockResponse {
            block: Some(block_json.0),
            current_btc_height,
        }))
    } else {
        Ok(Json(LatestBlockResponse {
            block: None,
            current_btc_height,
        }))
    }
}
