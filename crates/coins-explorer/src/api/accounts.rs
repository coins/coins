use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    response::Json,
};
use crate::api::AppState;
use crate::models::*;
use coins_crypto::G1;
use coins_indexer::FINALITY_DEPTH;
use coins_types::SubBlockState;

pub async fn get_account(
    State(app_state): State<AppState>,
    Path(pk_hex): Path<String>,
) -> Result<Json<AccountResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    // Check cache first
    if let Some(cached) = indexer.cache.get_account(&pk_hex).await {
        let current_btc_height = indexer.bitcoin_client.get_block_height()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

        return Ok(Json(AccountResponse {
            account: Some(cached),
            current_btc_height,
        }));
    }

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    // Parse public key
    let pk_bytes = hex::decode(&pk_hex)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid hex public key".to_string()))?;

    if pk_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "Public key must be 32 bytes".to_string()));
    }

    let mut pk_array = [0u8; 32];
    pk_array.copy_from_slice(&pk_bytes);
    let pk = G1(pk_array);

    // Get account from state
    let account = indexer.state.get_by_pk(&pk)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))?;

    if let Some(account) = account {
        // Get transaction count
        let sent_refs = indexer.tx_index.get_sender_refs(account.id.0)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?;
        let recv_refs = indexer.tx_index.get_recipient_refs(&pk_array)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?;

        let tx_count = sent_refs.len() + recv_refs.len();

        let account_detail = AccountDetail {
            id: account.id.0,
            pk: pk_hex.clone(),
            balance: account.balance,
            nonce: account.nonce,
            tx_count,
        };

        // Cache it
        indexer.cache.put_account(pk_hex, account_detail.clone()).await;

        Ok(Json(AccountResponse {
            account: Some(account_detail),
            current_btc_height,
        }))
    } else {
        Ok(Json(AccountResponse {
            account: None,
            current_btc_height,
        }))
    }
}

pub async fn get_account_transactions(
    State(app_state): State<AppState>,
    Path(pk_hex): Path<String>,
    Query(query): Query<AccountTxQuery>,
) -> Result<Json<AccountTxResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    // Parse public key
    let pk_bytes = hex::decode(&pk_hex)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid hex public key".to_string()))?;

    if pk_bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "Public key must be 32 bytes".to_string()));
    }

    let mut pk_array = [0u8; 32];
    pk_array.copy_from_slice(&pk_bytes);
    let pk = G1(pk_array);

    // Get account
    let account = indexer.state.get_by_pk(&pk)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Account not found".to_string()))?;

    let page = query.page();
    let limit = query.limit();
    let tx_type_filter = query.tx_type_filter();

    // Collect sent transactions
    let mut all_txs = Vec::new();

    if matches!(tx_type_filter, TxTypeFilter::All | TxTypeFilter::Sent) {
        let sent_refs = indexer.tx_index.get_sender_refs(account.id.0)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?;

        for tx_ref in sent_refs {
            if let Some(tx) = indexer.tx_index.get_transaction(tx_ref.btc_height, tx_ref.tx_offset)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?
            {
                let confirmations = current_btc_height.saturating_sub(tx_ref.btc_height) + 1;

                // Get btc_txid for this height
                let key = tx_ref.btc_height.to_le_bytes();
                let value = indexer.indexer.blocks.get(&key)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

                let btc_txid = if let Some(value) = value {
                    if let Some(chain_block) = coins_indexer::ChainBlock::deserialize(&value, &indexer.state) {
                        chain_block.btc_txid.to_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };

                let sender_pk = indexer.state.get_account(account.id)
                    .ok()
                    .flatten()
                    .map(|acc| hex::encode(&acc.pk.0));

                let recipient_id = indexer.state.get_account_id_by_pk(&tx.recipient_pk)
                    .ok()
                    .flatten();

                all_txs.push(AccountTransaction {
                    btc_height: tx_ref.btc_height,
                    btc_txid,
                    btc_confirmations: confirmations,
                    tx_type: "sent".to_string(),
                    sender_id: tx.sender_id,
                    sender_pk,
                    recipient_pk: hex::encode(&tx.recipient_pk.0),
                    recipient_id,
                    amount: tx.amount,
                    fee: tx.fee,
                    finalized: confirmations >= FINALITY_DEPTH,
                });
            }
        }
    }

    // Collect received transactions
    if matches!(tx_type_filter, TxTypeFilter::All | TxTypeFilter::Received) {
        let recv_refs = indexer.tx_index.get_recipient_refs(&pk_array)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?;

        for tx_ref in recv_refs {
            if let Some(tx) = indexer.tx_index.get_transaction(tx_ref.btc_height, tx_ref.tx_offset)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?
            {
                let confirmations = current_btc_height.saturating_sub(tx_ref.btc_height) + 1;

                // Get btc_txid for this height
                let key = tx_ref.btc_height.to_le_bytes();
                let value = indexer.indexer.blocks.get(&key)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

                let btc_txid = if let Some(value) = value {
                    if let Some(chain_block) = coins_indexer::ChainBlock::deserialize(&value, &indexer.state) {
                        chain_block.btc_txid.to_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                };

                let sender_pk = indexer.state.get_account(coins_types::AccountId(tx.sender_id))
                    .ok()
                    .flatten()
                    .map(|acc| hex::encode(&acc.pk.0));

                let recipient_id = indexer.state.get_account_id_by_pk(&tx.recipient_pk)
                    .ok()
                    .flatten();

                all_txs.push(AccountTransaction {
                    btc_height: tx_ref.btc_height,
                    btc_txid,
                    btc_confirmations: confirmations,
                    tx_type: "received".to_string(),
                    sender_id: tx.sender_id,
                    sender_pk,
                    recipient_pk: hex::encode(&tx.recipient_pk.0),
                    recipient_id,
                    amount: tx.amount,
                    fee: tx.fee,
                    finalized: confirmations >= FINALITY_DEPTH,
                });
            }
        }
    }

    // Sort by height (chronological)
    all_txs.sort_by_key(|tx| (tx.btc_height, tx.tx_type.clone()));

    let total_count = all_txs.len();

    // Apply pagination
    let start = (page * limit) as usize;
    let end = start + limit as usize;
    let transactions = all_txs[start..end.min(total_count)].to_vec();

    Ok(Json(AccountTxResponse {
        transactions,
        total_count,
        page,
        limit,
    }))
}
