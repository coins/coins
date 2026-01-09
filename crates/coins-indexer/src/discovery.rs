//! Autonomous block discovery by watching subchain address on Bitcoin

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use bitcoin::{OutPoint, Transaction, Txid};
use coins_core::State;
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
        tokio::time::sleep(Duration::from_secs(10)).await;

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
                    // Anchor exists but not spent yet
                    tracing::debug!(anchor_idx = idx, "Anchor not spent yet");
                    break; // Stop checking further anchors
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

    // Validate and apply state changes (copied from submit_block handler)
    let mut updated_accounts = Vec::new();

    for tx in &sub_block.txs {
        // Get sender
        let sender_account_id = AccountId(tx.sender_id);
        let mut sender = state.get_account(sender_account_id)
            .map_err(|_| anyhow::anyhow!("State error getting sender"))?
            .ok_or_else(|| anyhow::anyhow!("Sender account {} not found", tx.sender_id))?;

        // Validate balance
        let total_cost = (tx.amount as u64) + (tx.fee as u64);
        if sender.balance < total_cost {
            return Err(anyhow::anyhow!(
                "Insufficient balance: {} < {}",
                sender.balance,
                total_cost
            ));
        }

        // Update sender
        sender.balance -= total_cost;
        sender.nonce += 1;
        updated_accounts.push(sender);

        // Get or create recipient
        let recipient = match state.get_by_pk(&tx.recipient_pk)
            .map_err(|_| anyhow::anyhow!("State error getting recipient"))?
        {
            Some(mut acc) => {
                acc.balance += tx.amount as u64;
                acc
            }
            None => {
                let mut new_acc = state.create_account(tx.recipient_pk)
                    .map_err(|_| anyhow::anyhow!("Failed to create recipient account"))?;
                new_acc.balance = tx.amount as u64;
                new_acc
            }
        };
        updated_accounts.push(recipient);
    }

    // Apply state changes atomically
    state.apply_batch(&updated_accounts)
        .map_err(|_| anyhow::anyhow!("Failed to apply state batch"))?;

    // Index the block
    indexer.index_block(data_txid, btc_height, sub_block)
        .map_err(|e| anyhow::anyhow!("Failed to index block: {:?}", e))?;

    tracing::info!(
        btc_txid = %data_txid,
        btc_height = btc_height,
        tx_count = updated_accounts.len() / 2,
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
    let anchor_txs = subchain.reconstruct_txs();
    let mut indexed_count = 0;
    let mut skipped_count = 0;

    for (idx, anchor_tx) in anchor_txs.iter().enumerate() {
        let anchor_txid = anchor_tx.compute_txid();
        let anchor_outpoint = OutPoint::new(anchor_txid, 1);

        // Check if anchor exists and is spent
        let status = rpc_backend.get_output_status(&anchor_txid, 1).await?;

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
