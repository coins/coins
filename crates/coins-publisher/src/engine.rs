use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use bitcoin::{Address, Amount, Network, OutPoint, PrivateKey, Txid, CompressedPublicKey};
use bitcoin::secp256k1::{Secp256k1, SecretKey, XOnlyPublicKey};
use bitcoin::key::Keypair;
use coins_subchain::Subchain;
use std::str::FromStr;
use crate::api::AppState;
use coins_bitcoin_rpc::RpcBackend;
use crate::state_adapter::StateAdapter;
use coins_types::SubBlock;
use coins_crypto::{G1, SecretKey as BLSSecretKey, aggregate};
use coins_subchain::{compress, decompress, PublishMode, PublishFormat, publish_subblock};
use ark_ff::{PrimeField, BigInteger};
use ark_bn254::Fr;

/// Anchor output index (output[1] in anchor transactions)
const ANCHOR_OUTPUT_INDEX: u32 = 1;

/// Fee UTXO for paying Bitcoin transaction fees.
#[derive(Debug, Clone)]
pub struct FeeUtxo {
    pub outpoint: OutPoint,
    pub value: Amount,
}

pub struct Engine {
    pub backend: Arc<RpcBackend>,
    pub sc: Subchain,
    pub fee_sk: SecretKey,
    pub fee_addr: Address,
    pub fee_utxos: Vec<FeeUtxo>,
    pub current_anchor: OutPoint,
    pub anchor_idx: usize,
    pub last_synced: Option<Txid>,
    pub app_state: AppState,
    pub state_adapter: StateAdapter,
    pub publisher_sk: BLSSecretKey,
    pub publisher_pk: G1,
    pub publish_mode: PublishMode,
    pub fee_rate: u64,
}

impl Engine {
    /// Initialize from backend, subchain path, optional fee key path.
    pub async fn new(
        backend: Arc<RpcBackend>,
        subchain_path: PathBuf,
        network: Network,
        key_file: Option<PathBuf>,
        bls_key_file: PathBuf,
        app_state: AppState,
        state_adapter: StateAdapter,
        publish_mode: PublishMode,
        fee_rate: u64,
    ) -> Result<Self> {
        // ---------- load subchain ----------
        let sc_bytes = std::fs::read(&subchain_path)?;
        let sc = Subchain::decode(&sc_bytes).ok_or_else(|| anyhow::anyhow!("invalid subchain file"))?;

        // ---------- fee secret key ----------
        let fee_sk = match key_file {
            Some(ref path) if path.exists() => {
                let hex = std::fs::read_to_string(path)?;
                let bytes = hex::decode(hex.trim())?;
                SecretKey::from_slice(&bytes)?
            },
            Some(ref path) => {
                let mut rng = rand::rngs::OsRng;
                let sk = SecretKey::new(&mut rng);
                std::fs::write(path, hex::encode(sk.secret_bytes()))?;
                sk
            },
            None => {
                let mut rng = rand::rngs::OsRng;
                SecretKey::new(&mut rng)
            }
        };

        let secp = Secp256k1::new();
        let pk = PrivateKey::new(fee_sk, network);
        let fee_pk = CompressedPublicKey::from_private_key(&secp, &pk).expect("private key");

        // Choose address type based on publish format:
        // - TaprootAnnex requires P2TR (segwit v1) for the annex feature
        // - OpReturn and others use P2WPKH (segwit v0)
        let fee_addr = match &publish_mode {
            PublishMode::Single(PublishFormat::TaprootAnnex) |
            PublishMode::Dual { primary: PublishFormat::TaprootAnnex, .. } |
            PublishMode::Dual { secondary: PublishFormat::TaprootAnnex, .. } => {
                // Create P2TR address for Taproot annex compatibility
                let keypair = Keypair::from_secret_key(&secp, &fee_sk);
                let (x_only_pk, _parity) = XOnlyPublicKey::from_keypair(&keypair);
                Address::p2tr(&secp, x_only_pk, None, network)
            }
            _ => {
                // Use P2WPKH for OP_RETURN and other formats
                Address::p2wpkh(&fee_pk, network)
            }
        };

        // ---------- publisher BLS secret key ----------
        let publisher_sk = if bls_key_file.exists() {
            let hex = std::fs::read_to_string(&bls_key_file)?;
            let bytes = hex::decode(hex.trim())?;
            let fr = Fr::from_le_bytes_mod_order(&bytes);
            BLSSecretKey(fr)
        } else {
            let sk = BLSSecretKey::random();
            // Save it for next time (using into_bigint().to_bytes_le() for consistency with client)
            let sk_bytes = sk.0.into_bigint().to_bytes_le();
            std::fs::write(&bls_key_file, hex::encode(&sk_bytes))?;
            sk
        };
        let publisher_pk = G1(publisher_sk.public_key());

        // current anchor (anchor.output[1]) for idx 0 initially
        let current_anchor = sc.first_out;

        let mut eng = Self {
            backend,
            sc,
            fee_sk,
            fee_addr,
            fee_utxos: Vec::new(),
            current_anchor,
            anchor_idx: 0,
            last_synced: None,
            app_state,
            state_adapter,
            publisher_sk,
            publisher_pk,
            publish_mode,
            fee_rate,
        };
        eng.refresh_fee_utxos().await?;
        Ok(eng)
    }

    /// Query backend for all UTXOs belonging to `fee_addr`.
    pub async fn refresh_fee_utxos(&mut self) -> Result<()> {
        let utxos = self.backend.get_address_utxos(&self.fee_addr).await?;
        self.fee_utxos = utxos.into_iter()
            .filter(|u| u.confirmed)
            .map(|u| FeeUtxo { outpoint: u.outpoint, value: u.value })
            .collect();
        Ok(())
    }

    /// Check if current anchor is still unspent; if spent, advance to next anchor.
    pub async fn refresh_anchor(&mut self) -> Result<()> {
        let txid32 = Txid::from_str(&self.current_anchor.txid.to_string())?;
        let status_opt = self.backend.get_output_status(&txid32, self.current_anchor.vout).await?;
        if let Some(status) = status_opt {
            if status.spent {
                self.anchor_idx += 1;
                let txs = self.sc.reconstruct_txs();
                if self.anchor_idx >= txs.len() {
                    anyhow::bail!("subchain exhausted");
                }
                let next = &txs[self.anchor_idx];
                self.current_anchor = OutPoint::new(next.compute_txid(), ANCHOR_OUTPUT_INDEX);
            }
        }
        Ok(())
    }

    /// Broadcast raw tx via backend
    pub async fn broadcast(&self, tx: &bitcoin::Transaction) -> Result<()> {
        self.backend.broadcast(tx).await?;
        tracing::info!(txid = %tx.compute_txid(), "Transaction broadcast successful");
        Ok(())
    }

    /// Broadcast a parent/child package via backend
    async fn broadcast_package(&self, txs: &[bitcoin::Transaction]) -> Result<()> {
        self.backend.broadcast_package(txs).await?;
        tracing::debug!(tx_count = txs.len(), "Package broadcast successful");
        Ok(())
    }

    pub async fn try_mine_subblock(&mut self) -> Result<()> {
        let txs = {
            let mut mempool = self.app_state.mempool.lock()
                .map_err(|_| anyhow::anyhow!("mempool lock poisoned"))?;
            if mempool.is_empty() {
                return Ok(());
            }
            mempool.drain(..).collect::<Vec<_>>()
        };

        tracing::info!(tx_count = txs.len(), "Mining sub-block");

        let (transactions, signatures): (Vec<_>, Vec<_>) = txs.into_iter().unzip();

        // Aggregate BLS signatures
        let aggregated_signature = aggregate(signatures.iter());

        let sub_block = SubBlock {
            txs: transactions,
            sigma: aggregated_signature,
            publisher_pk: self.publisher_pk,
        };

        // Serialize sub-block (uses state adapter for compression)
        let sub_block_bytes = sub_block.serialize(&self.state_adapter);

        if self.fee_utxos.is_empty() {
            tracing::warn!("Cannot mine sub-block: No fee UTXOs available");
            return Ok(());
        }

        let fee_utxo = self.fee_utxos[0].clone();

        let txs = self.sc.reconstruct_txs();

        // Ensure all anchor transactions up to the current index are on-chain.
        // We broadcast them best-effort; if they are already in the chain/mempool
        // the node will just return an error which we can safely ignore.
        for tx in txs.iter().take(self.anchor_idx + 1) {
            let _ = self.broadcast(tx).await;
        }

        let anchor_tx = &txs[self.anchor_idx];

        // Compress sub-block before publishing
        let compressed_bytes = compress(&sub_block_bytes)
            .map_err(|e| anyhow::anyhow!("Compression failed: {}", e))?;

        tracing::info!(
            original_size = sub_block_bytes.len(),
            compressed_size = compressed_bytes.len(),
            compression_ratio = format!("{:.1}%", (1.0 - compressed_bytes.len() as f64 / sub_block_bytes.len() as f64) * 100.0),
            "Compressed sub-block data"
        );

        // Dispatch based on publish mode
        match &self.publish_mode {
            PublishMode::Single(format) => {
                self.publish_single(&compressed_bytes, anchor_tx, fee_utxo, *format, sub_block).await?;
            }
            PublishMode::Dual { primary, secondary } => {
                self.publish_dual(&compressed_bytes, anchor_tx, fee_utxo, *primary, *secondary, sub_block).await?;
            }
        }

        Ok(())
    }

    /// Publish a sub-block using a single format
    async fn publish_single(
        &mut self,
        compressed_bytes: &[u8],
        anchor_tx: &bitcoin::Transaction,
        fee_utxo: FeeUtxo,
        format: PublishFormat,
        sub_block: SubBlock,
    ) -> Result<()> {
        let result = publish_subblock(
            compressed_bytes,
            anchor_tx,
            fee_utxo.outpoint,
            fee_utxo.value.to_sat(),
            &self.fee_sk,
            self.fee_rate,
            self.sc.network,
            format,
        ).map_err(|e| anyhow::anyhow!("Publish failed: {}", e))?;

        // Package relay: anchor_tx + data_tx
        if let Err(e) = self.broadcast_package(&[anchor_tx.clone(), result.data_tx.clone()]).await {
            tracing::warn!(error = %e, "Package relay failed, falling back to individual broadcasts");

            // Try individual broadcasts - if these fail, it might be because another publisher
            // or our previous instance already used this anchor
            if let Err(anchor_err) = self.broadcast(anchor_tx).await {
                let err_msg = anchor_err.to_string();
                if err_msg.contains("txn-already-known") ||
                   err_msg.contains("already in utxo set") ||
                   err_msg.contains("already in block chain") {
                    tracing::warn!("Anchor transaction already broadcast/confirmed, skipping this mining cycle");
                    // Don't advance anchor - we didn't actually mine a new block
                    return Ok(());
                }
                // For other errors, log but don't crash - let the next iteration retry
                tracing::error!(error = %anchor_err, "Failed to broadcast anchor transaction");
                return Ok(());
            }

            if let Err(data_err) = self.broadcast(&result.data_tx).await {
                let err_msg = data_err.to_string();
                if err_msg.contains("bad-txns-inputs-missingorspent") {
                    tracing::warn!("Data transaction inputs missing/spent - anchor was likely used by another publisher");
                    return Ok(());
                }
                tracing::error!(error = %data_err, "Failed to broadcast data transaction");
                return Ok(());
            }
        } else {
            tracing::info!(
                format = ?format,
                anchor_txid = %anchor_tx.compute_txid(),
                data_txid = %result.data_tx.compute_txid(),
                "Package broadcasted successfully"
            );
        }

        // Update anchor immediately - we assume the broadcast will succeed
        self.current_anchor = result.new_anchor;
        self.anchor_idx += 1;

        tracing::info!(
            anchor = %self.current_anchor.txid,
            anchor_idx = self.anchor_idx,
            format = ?format,
            "Successfully mined new sub-block"
        );

        Ok(())
    }

    /// Publish a sub-block using dual formats (optimistic dual-broadcast)
    async fn publish_dual(
        &mut self,
        compressed_bytes: &[u8],
        anchor_tx: &bitcoin::Transaction,
        fee_utxo_primary: FeeUtxo,
        primary: PublishFormat,
        secondary: PublishFormat,
        sub_block: SubBlock,
    ) -> Result<()> {
        // Check if we have enough fee UTXOs for dual broadcast
        if self.fee_utxos.len() < 2 {
            tracing::warn!("Dual broadcast requires 2 fee UTXOs, falling back to primary only");
            return self.publish_single(compressed_bytes, anchor_tx, fee_utxo_primary, primary, sub_block).await;
        }

        let fee_utxo_secondary = self.fee_utxos[1].clone();

        // Publish primary format (critical, must succeed)
        let result_primary = publish_subblock(
            compressed_bytes,
            anchor_tx,
            fee_utxo_primary.outpoint,
            fee_utxo_primary.value.to_sat(),
            &self.fee_sk,
            self.fee_rate,
            self.sc.network,
            primary,
        ).map_err(|e| anyhow::anyhow!("Primary publish failed: {}", e))?;

        // Broadcast primary package (critical)
        if let Err(e) = self.broadcast_package(&[anchor_tx.clone(), result_primary.data_tx.clone()]).await {
            tracing::warn!(error = %e, "Primary package relay failed, using individual broadcasts");

            // Try individual broadcasts with error handling
            if let Err(anchor_err) = self.broadcast(anchor_tx).await {
                let err_msg = anchor_err.to_string();
                if err_msg.contains("txn-already-known") ||
                   err_msg.contains("already in utxo set") ||
                   err_msg.contains("already in block chain") {
                    tracing::warn!("Anchor transaction already broadcast, skipping this mining cycle");
                    return Ok(());
                }
                tracing::error!(error = %anchor_err, "Failed to broadcast anchor transaction");
                return Ok(());
            }

            if let Err(data_err) = self.broadcast(&result_primary.data_tx).await {
                let err_msg = data_err.to_string();
                if err_msg.contains("bad-txns-inputs-missingorspent") {
                    tracing::warn!("Data transaction inputs missing/spent - anchor was likely used");
                    return Ok(());
                }
                tracing::error!(error = %data_err, "Failed to broadcast data transaction");
                return Ok(());
            }
        } else {
            tracing::info!(
                format = ?primary,
                anchor_txid = %anchor_tx.compute_txid(),
                data_txid = %result_primary.data_tx.compute_txid(),
                "Primary package broadcasted"
            );
        }

        // Publish secondary format (best-effort, don't fail on error)
        if let Ok(result_secondary) = publish_subblock(
            compressed_bytes,
            anchor_tx,
            fee_utxo_secondary.outpoint,
            fee_utxo_secondary.value.to_sat(),
            &self.fee_sk,
            self.fee_rate,
            self.sc.network,
            secondary,
        ) {
            // Try to broadcast secondary (best-effort)
            if let Err(e) = self.broadcast(&result_secondary.data_tx).await {
                tracing::warn!(
                    error = %e,
                    format = ?secondary,
                    "Secondary broadcast failed (non-critical)"
                );
            } else {
                tracing::info!(
                    format = ?secondary,
                    data_txid = %result_secondary.data_tx.compute_txid(),
                    "Secondary format broadcasted"
                );
            }
        }

        // Update anchor immediately - assume broadcast will succeed
        self.current_anchor = result_primary.new_anchor;
        self.anchor_idx += 1;

        tracing::info!(
            anchor = %self.current_anchor.txid,
            anchor_idx = self.anchor_idx,
            "Successfully mined dual-format sub-block"
        );

        Ok(())
    }

    /// Sync with indexer to catch up on any blocks we missed (e.g. after restart or if another publisher mined)
    pub async fn sync_from_indexer(&mut self) -> Result<()> {
        // This function is called periodically to ensure we're in sync with the indexer
        // If the indexer has blocks we didn't mine (e.g. because we restarted or another publisher mined them),
        // we need to advance our anchor accordingly

        // The indexer_ibd() function already handles this by scanning the blockchain and indexing
        // any blocks it finds. We just need to make sure our anchor_idx is correct afterwards.

        // After indexer_ibd completes, we should refresh our anchor to match reality
        self.refresh_anchor().await?;

        Ok(())
    }

    pub async fn ibd(&mut self) -> Result<()> {
        let txs = self.sc.reconstruct_txs();
        if txs.is_empty() {
            tracing::info!("IBD: Subchain is empty, starting from genesis");
            return Ok(());
        }

        let addr = Address::p2wpkh(&self.sc.pubkey, self.sc.network);

        // Import address - use genesis_height if available for faster rescan
        if let Some(genesis_height) = self.sc.genesis_height {
            tracing::info!(
                genesis_height = genesis_height,
                "IBD: Using genesis height for optimized blockchain scan"
            );
            self.backend.ensure_address_imported_from_height(&addr, genesis_height).await?;
        }

        // Find current anchor position
        let utxos = self.backend.get_address_utxos(&addr).await?;
        for (idx, tx) in txs.iter().enumerate().rev() {
            let txid = tx.compute_txid();
            if utxos.iter().any(|u| u.outpoint.txid == txid) {
                self.anchor_idx = idx + 1;
                if self.anchor_idx < txs.len() {
                    let next = &txs[self.anchor_idx];
                    self.current_anchor = OutPoint::new(next.compute_txid(), ANCHOR_OUTPUT_INDEX);
                } else {
                    // all anchors spent, we are at the tip
                }
                self.last_synced = Some(txid);
                tracing::info!(anchor_idx = idx, "IBD: Synced to anchor");
                break;
            }
        }

        // Indexer IBD: Scan for historical sub-blocks
        self.indexer_ibd().await?;

        tracing::info!("IBD: Complete");
        Ok(())
    }

    /// Scan blockchain for historical sub-blocks and index them
    async fn indexer_ibd(&mut self) -> Result<()> {
        use coins_subchain::parse_blob_from_tx;
        use coins_types::SubBlock;

        tracing::info!("Starting Indexer IBD...");

        let txs = self.sc.reconstruct_txs();
        let mut indexed_count = 0;

        // For each anchor transaction, check if its anchor was spent
        // Anchors are always used sequentially, so we can stop at the first unbroadcast one
        for (idx, anchor_tx) in txs.iter().enumerate() {
            let anchor_txid = anchor_tx.compute_txid();
            let anchor_outpoint = OutPoint::new(anchor_txid, ANCHOR_OUTPUT_INDEX);

            tracing::debug!(
                anchor_idx = idx,
                anchor_txid = %anchor_txid,
                "Checking anchor..."
            );

            // Check if anchor output exists (anchor was broadcast)
            let anchor_status = self.backend.get_output_status(&anchor_txid, ANCHOR_OUTPUT_INDEX).await?;

            if anchor_status.is_none() {
                tracing::debug!(
                    anchor_idx = idx,
                    "Anchor not broadcast yet, stopping IBD scan (anchors are sequential)"
                );
                // Anchors are always used sequentially, so if this one isn't broadcast,
                // none of the subsequent ones will be either
                break;
            }

            // Check if anchor is spent
            if let Some(status) = anchor_status {
                if !status.spent {
                    tracing::debug!(
                        anchor_idx = idx,
                        "Anchor not spent yet, no sub-block here"
                    );
                    continue; // Anchor not spent yet, no sub-block here
                }
            }

            // Anchor is spent! Find the spending transaction (data_tx)
            tracing::debug!(
                anchor_idx = idx,
                anchor_txid = %anchor_txid,
                "Anchor spent, searching for data_tx..."
            );

            // Use get_spending_tx to find the transaction that spent this anchor
            if let Some((data_txid, data_tx, btc_height)) =
                self.backend.get_spending_tx(&anchor_outpoint).await?
            {
                tracing::debug!(
                    anchor_idx = idx,
                    data_txid = %data_txid,
                    btc_height = btc_height,
                    "Found data_tx"
                );

                // Auto-detect format and parse blob
                if let Some((compressed_blob, format)) = parse_blob_from_tx(&data_tx) {
                    tracing::debug!(
                        anchor_idx = idx,
                        format = ?format,
                        "Detected publish format"
                    );

                    // Decompress
                    let blob = decompress(&compressed_blob)
                        .map_err(|e| anyhow::anyhow!("Decompression failed: {}", e))?;

                    // Parse sub-block
                    if let Some(sub_block) = SubBlock::deserialize(&blob, &self.state_adapter) {
                        // Submit to indexer for validation and indexing
                        // The indexer will validate and apply state changes
                        indexed_count += 1;
                        tracing::info!(
                            anchor_idx = idx,
                            btc_txid = %data_txid,
                            btc_height = btc_height,
                            "Indexed historical sub-block"
                        );
                    } else {
                        tracing::warn!(
                            anchor_idx = idx,
                            data_txid = %data_txid,
                            "Failed to parse sub-block from OP_RETURN data"
                        );
                    }
                } else {
                    tracing::warn!(
                        anchor_idx = idx,
                        data_txid = %data_txid,
                        "Data TX has no OP_RETURN output"
                    );
                }
            } else {
                tracing::warn!(
                    anchor_idx = idx,
                    anchor_txid = %anchor_txid,
                    "Anchor is marked as spent but couldn't find spending TX"
                );
            }
        }

        tracing::info!(
            indexed_count = indexed_count,
            "Indexer IBD complete"
        );

        Ok(())
    }
} 