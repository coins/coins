use axum::{
    extract::{State, Query},
    http::StatusCode,
    response::Json,
};
use crate::api::AppState;
use crate::models::*;
use coins_indexer::FINALITY_DEPTH;
use coins_types::SubBlockState;

pub async fn search_transactions(
    State(app_state): State<AppState>,
    Query(query): Query<TxSearchQuery>,
) -> Result<Json<TxSearchResponse>, (StatusCode, String)> {
    let indexer = &app_state.indexer;

    let current_btc_height = indexer.bitcoin_client.get_block_height()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Bitcoin RPC error: {}", e)))?;

    let page = query.page();
    let limit = query.limit();

    let mut all_txs = Vec::new();

    // If account_id is specified, use index
    if let Some(account_id) = query.account_id {
        let tx_refs = indexer.tx_index.get_sender_refs(account_id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?;

        for tx_ref in tx_refs {
            if let Some(tx) = indexer.tx_index.get_transaction(tx_ref.btc_height, tx_ref.tx_offset)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Index error: {}", e)))?
            {
                // Apply amount filters
                if let Some(min) = query.min_amount {
                    if tx.amount < min {
                        continue;
                    }
                }
                if let Some(max) = query.max_amount {
                    if tx.amount > max {
                        continue;
                    }
                }

                let confirmations = current_btc_height.saturating_sub(tx_ref.btc_height) + 1;

                // Get btc_txid
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
                    .map(|acc| hex::encode(&acc.pk.0))
                    .unwrap_or_else(|| "unknown".to_string());

                let recipient_id = indexer.state.get_account_id_by_pk(&tx.recipient_pk)
                    .ok()
                    .flatten();

                all_txs.push(TransactionDetail {
                    btc_height: tx_ref.btc_height,
                    btc_txid,
                    btc_confirmations: confirmations,
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
    } else {
        // No account filter - scan all blocks (expensive!)
        for item in indexer.indexer.blocks.iter() {
            let (key, value) = item
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e)))?;

            let btc_height = u32::from_le_bytes(key.as_ref().try_into().unwrap());

            if let Some(chain_block) = coins_indexer::ChainBlock::deserialize(&value, &indexer.state) {
                for tx in &chain_block.sub_block.txs {
                    // Apply filters
                    if let Some(min) = query.min_amount {
                        if tx.amount < min {
                            continue;
                        }
                    }
                    if let Some(max) = query.max_amount {
                        if tx.amount > max {
                            continue;
                        }
                    }

                    let confirmations = current_btc_height.saturating_sub(btc_height) + 1;

                    let sender_pk = indexer.state.get_account(coins_types::AccountId(tx.sender_id))
                        .ok()
                        .flatten()
                        .map(|acc| hex::encode(&acc.pk.0))
                        .unwrap_or_else(|| "unknown".to_string());

                    let recipient_id = indexer.state.get_account_id_by_pk(&tx.recipient_pk)
                        .ok()
                        .flatten();

                    all_txs.push(TransactionDetail {
                        btc_height,
                        btc_txid: chain_block.btc_txid.to_string(),
                        btc_confirmations: confirmations,
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
    }

    let total_count = all_txs.len();

    // Apply pagination
    let start = (page * limit) as usize;
    let end = start + limit as usize;
    let transactions = all_txs[start..end.min(total_count)].to_vec();

    Ok(Json(TxSearchResponse {
        transactions,
        total_count,
        page,
        limit,
    }))
}
