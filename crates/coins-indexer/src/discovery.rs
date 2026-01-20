//! Autonomous block discovery by watching subchain address on Bitcoin

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use bitcoin::{OutPoint, Transaction, Txid};
use coins_core::{State, validate_subblock, ValidationError};
use coins_crypto::G1;
use coins_subchain::{parse_blob_from_tx, decompress, Subchain};
use coins_types::{SubBlock, SubBlockState, AccountId};
use crate::Indexer;
use coins_bitcoin_rpc::RpcBackend;

/// State adapter for SubBlock deserialization
struct IndexerStateAdapter {
    state: Arc<State>,
}

impl IndexerStateAdapter {
    fn new(state: Arc<State>) -> Self {
        Self { state }
    }
}

impl SubBlockState for IndexerStateAdapter {
    fn get_account_id_by_pk(&self, pk: &G1) -> Result<Option<u32>, ()> {
        match self.state.get_by_pk(pk) {
            Ok(Some(account)) => Ok(Some(account.id.0)),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    fn get_pk_by_account_id(&self, id: u32) -> Option<G1> {
        match self.state.get_account(AccountId(id)) {
            Ok(Some(account)) => Some(account.pk),
            _ => None,
        }
    }
}

/// Continuously discover new blocks by watching subchain address
pub async fn discover_blocks_loop(
    indexer: Arc<Indexer>,
    state: Arc<State>,
    rpc_backend: Arc<RpcBackend>,
    subchain: Arc<Subchain>,
) -> Result<()> {
    let subchain_address = subchain.address();
    let mut last_checked_anchor_idx = 0;

    // Import subchain address into wallet (one-time, with rescan)
    tracing::info!(
        address = %subchain_address,
        "Importing subchain address for watching"
    );
    if let Some(genesis_height) = subchain.genesis_height {
        rpc_backend.ensure_address_imported_from_height(
            &subchain_address,
            genesis_height
        ).await?;
    } else {
        tracing::warn!("No genesis_height in subchain, full rescan may take a while");
        rpc_backend.ensure_address_imported(&subchain_address).await?;
    }

    tracing::info!("Starting continuous block discovery loop");

    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Get all anchor transactions
        let anchor_txs = subchain.reconstruct_txs();

        // Check each anchor from our last checkpoint
        for (idx, anchor_tx) in anchor_txs.iter().enumerate().skip(last_checked_anchor_idx) {
            let anchor_txid = anchor_tx.compute_txid();
            let anchor_outpoint = OutPoint::new(anchor_txid, 1);  // Anchor is output[1]

            // Check if anchor is spent
            let status = rpc_backend.get_output_status(&anchor_txid, 1).await?;

            match status {
                None => {
                    // Anchor not yet broadcast
                    tracing::debug!(anchor_idx = idx, "Anchor not yet broadcast");
                    break; // Stop checking further anchors
                }
                Some(output_status) if !output_status.spent => {
                    // Anchor exists but not spent yet - publisher may skip it
                    tracing::debug!(anchor_idx = idx, "Anchor not spent yet, checking next");
                    continue; // Check next anchor (publisher may skip anchors)
                }
                Some(_) => {
                    // Anchor is spent! Find the spending transaction
                    tracing::debug!(anchor_idx = idx, "Anchor spent, searching for data_tx");

                    if let Some((data_txid, data_tx, btc_height)) =
                        rpc_backend.get_spending_tx(&anchor_outpoint).await?
                    {
                        // Check if we've already indexed this block
                        let txid_bytes: &[u8] = data_txid.as_ref();
                        if indexer.txid_index.get(txid_bytes)?.is_none() {
                            // New block discovered!
                            tracing::info!(
                                anchor_idx = idx,
                                data_txid = %data_txid,
                                btc_height = btc_height,
                                "Discovered new block autonomously"
                            );

                            if let Err(e) = process_anchor_spend(
                                data_tx,
                                data_txid,
                                btc_height,
                                &indexer,
                                &state,
                            ).await {
                                tracing::error!(
                                    anchor_idx = idx,
                                    data_txid = %data_txid,
                                    error = ?e,
                                    "Failed to process discovered block"
                                );
                                // Continue to next anchor despite error
                            }
                        } else {
                            tracing::debug!(
                                anchor_idx = idx,
                                data_txid = %data_txid,
                                "Block already indexed, skipping"
                            );
                        }
                        // Only advance checkpoint when we found and processed the spending tx
                        last_checked_anchor_idx = idx + 1;
                    } else {
                        // Spending tx not found yet (may be in mempool or not confirmed)
                        // Don't advance checkpoint - retry on next iteration
                        tracing::debug!(
                            anchor_idx = idx,
                            "Spending tx not found yet, will retry"
                        );
                        break;
                    }
                }
            }
        }
    }
}

/// Process a discovered anchor spend transaction
async fn process_anchor_spend(
    data_tx: Transaction,
    data_txid: Txid,
    btc_height: u32,
    indexer: &Indexer,
    state: &State,
) -> Result<()> {
    // Auto-detect format and extract blob
    let (compressed_blob, format) = parse_blob_from_tx(&data_tx)
        .ok_or_else(|| anyhow::anyhow!("No blob found in transaction"))?;

    tracing::debug!(
        data_txid = %data_txid,
        format = ?format,
        compressed_size = compressed_blob.len(),
        "Extracted compressed blob"
    );

    // Decompress
    let blob = decompress(&compressed_blob)
        .map_err(|e| anyhow::anyhow!("Decompression failed: {}", e))?;

    tracing::debug!(
        data_txid = %data_txid,
        decompressed_size = blob.len(),
        "Decompressed blob"
    );

    // Create state adapter for deserialization
    let state_adapter = IndexerStateAdapter::new(indexer.state.clone());

    // Deserialize SubBlock
    let sub_block = SubBlock::deserialize(&blob, &state_adapter)
        .ok_or_else(|| anyhow::anyhow!("Failed to deserialize SubBlock"))?;

    tracing::debug!(
        data_txid = %data_txid,
        tx_count = sub_block.txs.len(),
        "Deserialized SubBlock"
    );

    // Validate and apply state changes using the canonical validator
    // This verifies BLS signatures, handles multiple txs from same sender,
    // credits publisher fees, and applies state atomically.
    validate_subblock(&sub_block, state).map_err(|e| match e {
        ValidationError::UnknownSender(id) => anyhow::anyhow!("Unknown sender account: {}", id),
        ValidationError::Balance => anyhow::anyhow!("Insufficient balance"),
        ValidationError::BadSignature => anyhow::anyhow!("Signature verification failed"),
        ValidationError::Nonce => anyhow::anyhow!("Nonce mismatch"),
        ValidationError::Db => anyhow::anyhow!("Database error"),
        ValidationError::Overflow => anyhow::anyhow!("Balance overflow"),
    })?;

    let tx_count = sub_block.txs.len();

    // Index the block
    indexer.index_block(data_txid, btc_height, sub_block)
        .map_err(|e| anyhow::anyhow!("Failed to index block: {:?}", e))?;

    tracing::info!(
        btc_txid = %data_txid,
        btc_height = btc_height,
        tx_count = tx_count,
        format = ?format,
        "Autonomously discovered and indexed block"
    );

    Ok(())
}

/// Scan for historical blocks during Initial Block Download
pub async fn scan_historical_anchors(
    indexer: &Indexer,
    state: &State,
    rpc_backend: &RpcBackend,
    subchain: &Arc<Subchain>,
) -> Result<()> {
    tracing::info!("Starting historical block discovery (IBD)");

    let subchain_address = subchain.address();

    // Import address with rescan from genesis height
    // This is the most time-consuming part of IBD
    if let Some(genesis_height) = subchain.genesis_height {
        tracing::info!(
            genesis_height = genesis_height,
            "Importing address with rescan from genesis height"
        );
        rpc_backend.ensure_address_imported_from_height(
            &subchain_address,
            genesis_height
        ).await?;
    } else {
        tracing::warn!("No genesis_height in subchain, scanning from block 0 (this will be slow)");
        // Full blockchain scan - very slow on mainnet/signet
        rpc_backend.ensure_address_imported(&subchain_address).await?;
    }

    // Check each anchor transaction
    // Anchors are always used sequentially, so we can stop at the first unbroadcast one
    let anchor_txs = subchain.reconstruct_txs();
    let mut indexed_count = 0;
    let mut skipped_count = 0;

    for (idx, anchor_tx) in anchor_txs.iter().enumerate() {
        let anchor_txid = anchor_tx.compute_txid();
        let anchor_outpoint = OutPoint::new(anchor_txid, 1);

        // Check if anchor exists and is spent
        let status = rpc_backend.get_output_status(&anchor_txid, 1).await?;

        // If anchor was never broadcast, stop scanning (anchors are sequential)
        if status.is_none() {
            tracing::debug!(
                anchor_idx = idx,
                "Anchor not broadcast yet, stopping IBD scan (anchors are sequential)"
            );
            break;
        }

        if let Some(output_status) = status {
            if output_status.spent {
                // Find spending transaction
                if let Some((data_txid, data_tx, btc_height)) =
                    rpc_backend.get_spending_tx(&anchor_outpoint).await?
                {
                    // Check if already indexed
                    let txid_bytes: &[u8] = data_txid.as_ref();
                    if indexer.txid_index.get(txid_bytes)?.is_none() {
                        tracing::info!(
                            anchor_idx = idx,
                            data_txid = %data_txid,
                            btc_height = btc_height,
                            "Processing historical block"
                        );

                        if let Err(e) = process_anchor_spend(
                            data_tx,
                            data_txid,
                            btc_height,
                            indexer,
                            state,
                        ).await {
                            tracing::error!(
                                anchor_idx = idx,
                                data_txid = %data_txid,
                                error = ?e,
                                "Failed to process historical block"
                            );
                            // Continue despite error
                        } else {
                            indexed_count += 1;
                        }
                    } else {
                        tracing::debug!(
                            anchor_idx = idx,
                            data_txid = %data_txid,
                            "Historical block already indexed"
                        );
                        skipped_count += 1;
                    }
                }
            }
        }
    }

    tracing::info!(
        indexed_count = indexed_count,
        skipped_count = skipped_count,
        total_anchors = anchor_txs.len(),
        "Historical block discovery complete (IBD)"
    );

    Ok(())
}

/// Background task to update btc_height for blocks that were initially discovered in mempool
pub async fn update_confirmation_statuses(
    indexer: Arc<Indexer>,
    rpc_backend: Arc<RpcBackend>,
    subchain: Arc<Subchain>,
) -> Result<()> {
    tracing::info!("Starting confirmation status update task");

    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        // Get all blocks from indexer
        let latest_height = match indexer.get_latest_height() {
            Ok(Some(h)) => h,
            Ok(None) => {
                tracing::debug!("No blocks indexed yet");
                continue;
            }
            Err(e) => {
                tracing::warn!(error = ?e, "Failed to get latest height");
                continue;
            }
        };

        // Reconstruct anchor transactions from subchain
        let anchor_txs = subchain.reconstruct_txs();

        // Check each block for confirmation updates
        let mut updated_count = 0;
        let mut pending_count = 0;

        for sub_chain_height in 0..=latest_height {
            let chain_block = match indexer.get_block_by_height(sub_chain_height) {
                Ok(Some(block)) => block,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        sub_chain_height = sub_chain_height,
                        error = ?e,
                        "Failed to read block"
                    );
                    continue;
                }
            };

            // Only process unconfirmed blocks (btc_height == 0)
            if chain_block.btc_height == 0 {
                let data_txid = chain_block.btc_txid;

                // Anchors are always used sequentially, so sub_chain_height == anchor_idx
                let anchor_idx = sub_chain_height as usize;

                if anchor_idx >= anchor_txs.len() {
                    tracing::warn!(
                        sub_chain_height = sub_chain_height,
                        data_txid = %data_txid,
                        "Anchor index out of bounds"
                    );
                    pending_count += 1;
                    continue;
                }

                let anchor_tx = &anchor_txs[anchor_idx];
                let anchor_txid = anchor_tx.compute_txid();
                let anchor_outpoint = OutPoint::new(anchor_txid, 1);

                // Query for the spending transaction of this anchor
                match rpc_backend.get_spending_tx(&anchor_outpoint).await {
                    Ok(Some((queried_txid, _tx, btc_height))) => {
                        // Verify this is the expected transaction
                        if queried_txid == data_txid {
                            if btc_height > 0 {
                                // Transaction is now confirmed! Update the height
                                if let Err(e) = indexer.update_btc_height(sub_chain_height, btc_height) {
                                    tracing::error!(
                                        sub_chain_height = sub_chain_height,
                                        btc_height = btc_height,
                                        data_txid = %data_txid,
                                        anchor_idx = anchor_idx,
                                        error = ?e,
                                        "Failed to update btc_height"
                                    );
                                } else {
                                    tracing::info!(
                                        sub_chain_height = sub_chain_height,
                                        btc_height = btc_height,
                                        data_txid = %data_txid,
                                        anchor_idx = anchor_idx,
                                        "Updated btc_height for confirmed block"
                                    );
                                    updated_count += 1;
                                }
                            } else {
                                // Still in mempool
                                tracing::debug!(
                                    sub_chain_height = sub_chain_height,
                                    data_txid = %data_txid,
                                    anchor_idx = anchor_idx,
                                    "Transaction still in mempool"
                                );
                                pending_count += 1;
                            }
                        } else {
                            tracing::warn!(
                                sub_chain_height = sub_chain_height,
                                expected_txid = %data_txid,
                                actual_txid = %queried_txid,
                                "Anchor spent by unexpected transaction"
                            );
                            pending_count += 1;
                        }
                    }
                    Ok(None) => {
                        // Anchor not spent yet - data_tx still in mempool
                        tracing::debug!(
                            sub_chain_height = sub_chain_height,
                            data_txid = %data_txid,
                            anchor_idx = anchor_idx,
                            "Anchor not spent yet"
                        );
                        pending_count += 1;
                    }
                    Err(e) => {
                        tracing::debug!(
                            sub_chain_height = sub_chain_height,
                            anchor_idx = anchor_idx,
                            error = ?e,
                            "Failed to query anchor spending status"
                        );
                        pending_count += 1;
                    }
                }
            }
        }

        if updated_count > 0 || pending_count > 0 {
            tracing::info!(
                updated_count = updated_count,
                pending_count = pending_count,
                "Confirmation status update cycle complete"
            );
        }
    }
}
