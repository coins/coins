use axum::{
    extract::{Path, Query, State as AxumState},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use coins_crypto::G1;
use coins_types::{Account, SubBlock, Transaction};
use super::AppState;

/// Create the API router
pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/accounts/:pk", get(get_account))
        .route("/accounts/:pk/transactions", get(get_account_transactions))
        .route("/blocks/latest", get(get_latest_block))
        .route("/blocks/:height", get(get_block_by_height))
        .route("/blocks", get(get_blocks_range))
        .route("/stats", get(get_stats))
        .with_state(app_state)
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Get account by public key
async fn get_account(
    AxumState(state): AxumState<AppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<Account>, StatusCode> {
    let pk_bytes = hex::decode(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| StatusCode::BAD_REQUEST)?;
    let pk = G1(pk_arr);

    match state.state.get_by_pk(&pk) {
        Ok(Some(account)) => Ok(Json(account)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Get account transaction history with confirmation status
async fn get_account_transactions(
    AxumState(state): AxumState<AppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<Vec<TransactionWithStatus>>, StatusCode> {
    let pk_bytes = hex::decode(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| StatusCode::BAD_REQUEST)?;
    let pk = G1(pk_arr);

    // Get current Bitcoin height from latest indexed block
    let current_btc_height = match state.indexer.get_latest_block() {
        Ok(Some(block)) if block.btc_height > 0 => block.btc_height,
        _ => 0, // If no blocks or btc_height is 0, use 0 (all unconfirmed)
    };

    let mut history = Vec::new();

    // Iterate through all blocks (both finalized and unfinalized)
    for item in state.indexer.blocks.iter() {
        let (_, value) = item.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let chain_block = coins_indexer::ChainBlock::deserialize(&value, &state.state)
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        // Find transactions involving this public key
        for tx in &chain_block.sub_block.txs {
            if &tx.recipient_pk == &pk {
                // If btc_height is 0, we don't have the actual Bitcoin block height yet
                let (confirmations, finalized, confirmations_remaining) = if chain_block.btc_height == 0 {
                    // Show as unconfirmed/in mempool until background task updates the height
                    // This avoids showing incorrect status during the brief window between
                    // when tx confirms and when background task updates btc_height
                    (0, false, coins_indexer::FINALITY_DEPTH)
                } else {
                    // Block is confirmed on Bitcoin - calculate confirmations
                    let confs = if current_btc_height >= chain_block.btc_height {
                        current_btc_height.saturating_sub(chain_block.btc_height) + 1
                    } else {
                        0
                    };
                    let final_status = confs >= coins_indexer::FINALITY_DEPTH;
                    let remaining = if final_status {
                        0
                    } else {
                        coins_indexer::FINALITY_DEPTH.saturating_sub(confs)
                    };
                    (confs, final_status, remaining)
                };

                history.push(TransactionWithStatus {
                    tx: tx.clone(),
                    btc_height: chain_block.btc_height,
                    confirmations,
                    finalized,
                    confirmations_remaining,
                });
            }
        }
    }

    Ok(Json(history))
}

/// Get latest block
async fn get_latest_block(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<ChainBlockResponse>, StatusCode> {
    // Get the latest sub-chain height from blocks tree
    let latest_height = if let Some(item) = state.indexer.blocks.iter().last() {
        let (key, _) = item.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        u32::from_le_bytes(key.as_ref().try_into().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
    } else {
        return Err(StatusCode::NOT_FOUND);
    };

    match state.indexer.get_latest_block() {
        Ok(Some(block)) => Ok(Json(ChainBlockResponse {
            height: latest_height,
            btc_height: block.btc_height,
            btc_txid: block.btc_txid.to_string(),
            sub_block: block.sub_block,
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Get block by sub-chain height
async fn get_block_by_height(
    AxumState(state): AxumState<AppState>,
    Path(height): Path<u32>,
) -> Result<Json<ChainBlockResponse>, StatusCode> {
    match state.indexer.get_block_by_height(height) {
        Ok(Some(block)) => Ok(Json(ChainBlockResponse {
            height,  // sub-chain height
            btc_height: block.btc_height,
            btc_txid: block.btc_txid.to_string(),
            sub_block: block.sub_block,
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
struct BlocksRangeQuery {
    from: Option<u32>,
    to: Option<u32>,
}

/// Get range of blocks by sub-chain height
async fn get_blocks_range(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<BlocksRangeQuery>,
) -> Result<Json<Vec<ChainBlockResponse>>, StatusCode> {
    // Get latest sub-chain height
    let latest = if let Some(item) = state.indexer.blocks.iter().last() {
        let (key, _) = item.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        u32::from_le_bytes(key.as_ref().try_into().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
    } else {
        return Ok(Json(vec![]));
    };

    let from = query.from.unwrap_or(0);
    let to = query.to.unwrap_or(latest).min(latest);

    let mut blocks = Vec::new();
    for height in from..=to {
        if let Ok(Some(block)) = state.indexer.get_block_by_height(height) {
            blocks.push(ChainBlockResponse {
                height,  // sub-chain height
                btc_height: block.btc_height,
                btc_txid: block.btc_txid.to_string(),
                sub_block: block.sub_block,
            });
        }
    }

    Ok(Json(blocks))
}

#[derive(Serialize)]
struct ChainBlockResponse {
    pub height: u32,  // sub-chain height (sequential: 0, 1, 2, ...)
    pub btc_height: u32,  // Bitcoin block height where this was anchored
    pub btc_txid: String,
    pub sub_block: SubBlock,
}

/// Get network statistics
async fn get_stats(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<NetworkStats>, StatusCode> {
    // Count actual number of sub-blocks stored
    let total_blocks = state.indexer.blocks.iter().count() as u32;

    let total_accounts = state.state.account_count() as u64;
    let total_supply = state.state.total_supply()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(NetworkStats {
        total_blocks,
        total_accounts,
        total_supply,
    }))
}

#[derive(Serialize)]
struct NetworkStats {
    pub total_blocks: u32,
    pub total_accounts: u64,
    pub total_supply: u64,
}

#[derive(Serialize)]
struct TransactionWithStatus {
    #[serde(flatten)]
    pub tx: Transaction,
    pub btc_height: u32,
    pub confirmations: u32,
    pub finalized: bool,
    pub confirmations_remaining: u32,
}
