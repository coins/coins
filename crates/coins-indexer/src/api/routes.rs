use axum::{
    extract::{Path, Query, State as AxumState},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use bitcoin::Txid;
use bitcoincore_rpc::RpcApi;
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
        .route("/blocks", get(get_blocks_range).post(submit_block))
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

    // Get current Bitcoin height and setup RPC client
    let rpc_client = bitcoincore_rpc::Client::new(&state.rpc_url, state.rpc_auth.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let current_btc_height = rpc_client.get_block_count()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? as u32;

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

#[derive(Deserialize)]
struct SubmitBlockRequest {
    pub btc_txid: String,
    pub btc_height: u32,
    pub sub_block: SubBlock,
}

/// Submit a new block for indexing (called by publisher after mining)
/// Indexer validates transactions and applies state changes.
async fn submit_block(
    AxumState(state): AxumState<AppState>,
    Json(req): Json<SubmitBlockRequest>,
) -> Result<StatusCode, StatusCode> {
    let txid: Txid = req.btc_txid.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    // Validate and apply state changes from the sub-block
    let mut updated_accounts = Vec::new();

    for tx in &req.sub_block.txs {
        // Get sender account by ID
        let sender_account_id = coins_types::AccountId(tx.sender_id);
        let mut sender = state.state
            .get_account(sender_account_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::BAD_REQUEST)?;

        // Validate balance (amount + fee)
        let total_cost = (tx.amount as u64) + (tx.fee as u64);
        if sender.balance < total_cost {
            tracing::warn!("Insufficient balance: {} < {}", sender.balance, total_cost);
            return Err(StatusCode::BAD_REQUEST);
        }

        // Update sender
        sender.balance -= total_cost;
        sender.nonce += 1;
        updated_accounts.push(sender);

        // Get or create recipient account
        let recipient = match state.state.get_by_pk(&tx.recipient_pk) {
            Ok(Some(mut acc)) => {
                acc.balance += tx.amount as u64;
                acc
            }
            Ok(None) => {
                // Create new account for recipient
                let mut new_acc = state.state.create_account(tx.recipient_pk)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                new_acc.balance = tx.amount as u64;
                new_acc
            }
            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        };
        updated_accounts.push(recipient);
    }

    // Apply state changes atomically
    state.state
        .apply_batch(&updated_accounts)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Index the block
    state.indexer
        .index_block(txid, req.btc_height, req.sub_block)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!(
        btc_height = req.btc_height,
        btc_txid = %txid,
        tx_count = updated_accounts.len() / 2,
        "Block submitted and indexed"
    );

    Ok(StatusCode::CREATED)
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
