use axum::{
    extract::{Path, Query, State as AxumState},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use coins_crypto::G1;
use coins_types::Account;
use super::AppState;
use coins_indexer::client::{ChainBlockResponse, NetworkStats, TransactionDirection, TransactionWithStatus};

/// Create the API router
pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/accounts/:pk", get(get_account))
        .route("/accounts/:pk/transactions", get(get_account_transactions))
        .route("/accounts/by-id/:id", get(get_account_by_id))
        .route("/blocks/latest", get(get_latest_block))
        .route("/blocks/by-txid/:txid", get(get_block_by_txid))
        .route("/blocks/:height", get(get_block_by_height))
        .route("/blocks", get(get_blocks_range))
        .route("/stats", get(get_stats))
        .with_state(app_state)
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Get block confirmation info by Bitcoin txid
/// Returns block height, btc_height, confirmations, finalized status
async fn get_block_by_txid(
    AxumState(state): AxumState<AppState>,
    Path(txid_hex): Path<String>,
) -> Result<Json<BlockConfirmationInfo>, StatusCode> {
    tracing::debug!(txid_hex = %txid_hex, "Getting block by txid");

    let txid = match txid_hex.parse::<bitcoin::Txid>() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(txid_hex = %txid_hex, error = ?e, "Invalid txid format");
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    // Find the block with this txid
    let chain_block = match state.indexer.get_block_by_txid(&txid) {
        Ok(Some(block)) => block,
        Ok(None) => {
            tracing::debug!(txid = %txid, "Txid not found in index");
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!(txid = %txid, error = ?e, "Error getting block by txid");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get current Bitcoin blockchain height for confirmation calculation
    let current_btc_height = match state.rpc_backend.get_current_height().await {
        Ok(height) => height,
        Err(_) => {
            match state.indexer.get_latest_block() {
                Ok(Some(block)) if block.btc_height > 0 => block.btc_height,
                _ => 0,
            }
        }
    };

    // Calculate confirmation status
    let (confirmations, finalized) = if chain_block.btc_height == 0 {
        (0, false)
    } else {
        let confs = if current_btc_height >= chain_block.btc_height {
            current_btc_height.saturating_sub(chain_block.btc_height) + 1
        } else {
            0
        };
        (confs, confs >= coins_indexer::FINALITY_DEPTH)
    };

    Ok(Json(BlockConfirmationInfo {
        btc_height: chain_block.btc_height,
        btc_txid: chain_block.btc_txid.to_string(),
        confirmations,
        finalized,
    }))
}

#[derive(Debug, Serialize)]
struct BlockConfirmationInfo {
    pub btc_height: u32,
    pub btc_txid: String,
    pub confirmations: u32,
    pub finalized: bool,
}

/// Get account by public key
async fn get_account(
    AxumState(state): AxumState<AppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<Account>, StatusCode> {
    let pk = G1::from_hex(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;

    match state.state.get_by_pk(&pk) {
        Ok(Some(account)) => Ok(Json(account)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Get account by ID (needed for IBD sub-block deserialization)
async fn get_account_by_id(
    AxumState(state): AxumState<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<Account>, StatusCode> {
    use coins_types::AccountId;

    match state.state.get_account(AccountId(id)) {
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
    let pk = G1::from_hex(&pk_hex).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Get account to find its ID for outgoing transaction filtering
    let account = match state.state.get_by_pk(&pk) {
        Ok(Some(acc)) => acc,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let account_id = account.id.0;

    // Get current Bitcoin blockchain height (not the latest indexed block's height)
    let current_btc_height = match state.rpc_backend.get_current_height().await {
        Ok(height) => height,
        Err(_) => {
            // Fallback to using latest indexed block if RPC fails
            match state.indexer.get_latest_block() {
                Ok(Some(block)) if block.btc_height > 0 => block.btc_height,
                _ => 0,
            }
        }
    };

    let mut history = Vec::new();

    // Iterate through all blocks (both finalized and unfinalized)
    for item in state.indexer.blocks.iter() {
        let (_, value) = item.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let chain_block = coins_indexer::ChainBlock::deserialize(&value, &state.state)
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        // Process transactions with their stored nonces
        for (tx_idx, tx) in chain_block.sub_block.txs.iter().enumerate() {
            // Check if this tx involves the current account
            let is_incoming = &tx.recipient_pk == &pk;
            let is_outgoing = tx.sender_id == account_id;

            // Skip if transaction doesn't involve this account at all
            if !is_incoming && !is_outgoing {
                continue;
            }

            // Get nonce from stored data (falls back to 0 for old entries)
            let nonce = chain_block.nonces.get(tx_idx).copied().unwrap_or(0);

            // If btc_height is 0, we don't have the actual Bitcoin block height yet
            let (confirmations, finalized, confirmations_remaining) = if chain_block.btc_height == 0 {
                (0, false, coins_indexer::FINALITY_DEPTH)
            } else {
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

            // Look up sender's public key from sender_id
            let sender_pk = match state.state.get_account(coins_types::AccountId(tx.sender_id)) {
                Ok(Some(sender_account)) => sender_account.pk,
                Ok(None) => {
                    tracing::warn!(sender_id = tx.sender_id, "Sender account not found");
                    continue;
                }
                Err(e) => {
                    tracing::error!(sender_id = tx.sender_id, error = ?e, "Failed to look up sender account");
                    continue;
                }
            };

            // For self-sends, add both incoming and outgoing entries
            // For regular transactions, add only one entry with the appropriate direction
            if is_incoming && is_outgoing {
                // Self-send: add as outgoing first
                history.push(TransactionWithStatus {
                    tx: tx.clone(),
                    nonce,
                    btc_height: chain_block.btc_height,
                    btc_txid: chain_block.btc_txid.to_string(),
                    confirmations,
                    finalized,
                    confirmations_remaining,
                    direction: TransactionDirection::Outgoing,
                    sender_pk,
                });
                // Then add as incoming
                history.push(TransactionWithStatus {
                    tx: tx.clone(),
                    nonce,
                    btc_height: chain_block.btc_height,
                    btc_txid: chain_block.btc_txid.to_string(),
                    confirmations,
                    finalized,
                    confirmations_remaining,
                    direction: TransactionDirection::Incoming,
                    sender_pk,
                });
            } else if is_outgoing {
                history.push(TransactionWithStatus {
                    tx: tx.clone(),
                    nonce,
                    btc_height: chain_block.btc_height,
                    btc_txid: chain_block.btc_txid.to_string(),
                    confirmations,
                    finalized,
                    confirmations_remaining,
                    direction: TransactionDirection::Outgoing,
                    sender_pk,
                });
            } else {
                // is_incoming (but not outgoing)
                history.push(TransactionWithStatus {
                    tx: tx.clone(),
                    nonce,
                    btc_height: chain_block.btc_height,
                    btc_txid: chain_block.btc_txid.to_string(),
                    confirmations,
                    finalized,
                    confirmations_remaining,
                    direction: TransactionDirection::Incoming,
                    sender_pk,
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


/// Get network statistics
async fn get_stats(
    AxumState(state): AxumState<AppState>,
) -> Result<Json<NetworkStats>, StatusCode> {
    // Count actual number of sub-blocks stored
    let total_blocks = state.indexer.blocks.iter().count() as u32;

    let total_accounts = state.state.account_count() as u64;
    let total_supply = state.state.total_supply()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get current Bitcoin block height
    let btc_height = state.rpc_backend.get_current_height().await.unwrap_or(0);

    Ok(Json(NetworkStats {
        total_blocks,
        total_accounts,
        total_supply,
        btc_height,
        network: state.network.clone(),
    }))
}


