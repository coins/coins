use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use bitcoin::{Address, Amount, Network, OutPoint, PrivateKey, Txid, CompressedPublicKey, FeeRate};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use coins_subchain::Subchain;
use std::str::FromStr;
use crate::api::AppState;
use crate::blockchain_backend::BlockchainBackend;
use coins_types::SubBlock;
use coins_crypto::{G1, SecretKey as BLSSecretKey, aggregate};
use coins_validator::validate_subblock;
use coins_subchain::inscribe::inscribe_blob;
use ark_ff::PrimeField;
use ark_bn254::Fr;
use ark_serialize::CanonicalSerialize;

/// Wrapper for esplora-client UTXO (txid,vout,value)
#[derive(Debug, Clone)]
pub struct FeeUtxo {
    pub outpoint: OutPoint,
    pub value: Amount,
}

pub struct Engine {
    pub backend: Arc<dyn BlockchainBackend>,
    pub sc: Subchain,
    pub fee_sk: SecretKey,
    pub fee_addr: Address,
    pub fee_utxos: Vec<FeeUtxo>,
    pub current_anchor: OutPoint,
    pub connector_idx: usize,
    pub last_synced: Option<Txid>,
    pub app_state: AppState,
    pub base_url: String,
    pub aggregator_sk: BLSSecretKey,
    pub aggregator_pk: G1,
}

impl Engine {
    /// Initialize from backend, subchain path, optional fee key path.
    pub async fn new(backend: Arc<dyn BlockchainBackend>, subchain_path: PathBuf, network: Network, key_file: Option<PathBuf>, app_state: AppState) -> Result<Self> {
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
        let fee_addr = Address::p2wpkh(&fee_pk, network);

        // ---------- aggregator BLS secret key ----------
        let aggregator_key_file = PathBuf::from("aggregator_bls_sk.hex");
        let aggregator_sk = if aggregator_key_file.exists() {
            let hex = std::fs::read_to_string(&aggregator_key_file)?;
            let bytes = hex::decode(hex.trim())?;
            let fr = Fr::from_le_bytes_mod_order(&bytes);
            BLSSecretKey(fr)
        } else {
            let sk = BLSSecretKey::random();
            // Save it for next time
            let mut bytes = Vec::new();
            sk.0.serialize_uncompressed(&mut bytes).expect("serialize Fr");
            std::fs::write(&aggregator_key_file, hex::encode(&bytes))?;
            sk
        };
        let aggregator_pk = G1(aggregator_sk.public_key());

        // current anchor (connector.output[1]) for idx 0 initially
        let current_anchor = sc.first_out;

        let mut eng = Self {
            backend,
            sc,
            fee_sk,
            fee_addr,
            fee_utxos: Vec::new(),
            current_anchor,
            connector_idx: 0,
            last_synced: None,
            app_state,
            base_url: String::new(), // Not needed anymore, but kept for compatibility
            aggregator_sk,
            aggregator_pk,
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

    /// Check if current anchor is still unspent; if spent, advance to next connector.
    pub async fn refresh_anchor(&mut self) -> Result<()> {
        let txid32 = Txid::from_str(&self.current_anchor.txid.to_string())?;
        let status_opt = self.backend.get_output_status(&txid32, self.current_anchor.vout).await?;
        if let Some(status) = status_opt {
            if status.spent {
                self.connector_idx += 1;
                let txs = self.sc.reconstruct_txs();
                if self.connector_idx >= txs.len() {
                    anyhow::bail!("subchain exhausted");
                }
                let next = &txs[self.connector_idx];
                self.current_anchor = OutPoint::new(next.compute_txid(), 1);
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
            let mut mempool = self.app_state.mempool.lock().unwrap();
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
            aggregator_pk: self.aggregator_pk,
        };

        // Validate sub-block before broadcasting
        if let Err(e) = validate_subblock(&sub_block, &self.app_state.state) {
            tracing::error!(error = ?e, "Sub-block validation failed");
            return Err(anyhow::anyhow!("Sub-block validation failed: {:?}", e));
        }
        tracing::debug!("Sub-block validated successfully");

        let sub_block_bytes = sub_block.serialize();

        if self.fee_utxos.is_empty() {
            tracing::warn!("Cannot mine sub-block: No fee UTXOs available");
            return Ok(());
        }

        let fee_utxo = self.fee_utxos[0].clone();
        let fee_rate = FeeRate::from_sat_per_vb(4).unwrap();
        
        let txs = self.sc.reconstruct_txs();

        // Ensure all connector transactions up to the current index are on-chain.
        // We broadcast them best-effort; if they are already in the chain/mempool
        // the node will just return an error which we can safely ignore.
        for idx in 0..=self.connector_idx {
            let _ = self.broadcast(&txs[idx]).await; // still broadcast raw connector individually in case anchor already standard
        }

        let connector_tx = &txs[self.connector_idx];

        let (_anchor, commit_tx, reveal_tx) = inscribe_blob(
            &sub_block_bytes,
            connector_tx,
            fee_utxo.outpoint,
            fee_utxo.value.to_sat(),
            &self.fee_sk,
            fee_rate.to_sat_per_vb_ceil(),
            self.sc.network,
        ).map_err(|e| anyhow::anyhow!(e))?;

        // Package relay: parent = connector_tx, child = commit_tx
        if let Err(e) = self.broadcast_package(&[connector_tx.clone(), commit_tx.clone()]).await {
            tracing::warn!(error = %e, "Package relay failed, falling back to individual broadcasts");
            self.broadcast(&connector_tx).await?;
            self.broadcast(&commit_tx).await?;
        } else {
            tracing::info!(
                anchor_txid = %connector_tx.compute_txid(),
                commit_txid = %commit_tx.compute_txid(),
                "Package broadcasted successfully"
            );
        }

        // broadcast reveal transaction – retry in case parents not yet visible
        let reveal_txid = reveal_tx.compute_txid();
        const MAX_RETRIES: usize = 10;
        const RETRY_DELAY_MS: u64 = 10000;
        let mut attempt = 0;
        loop {
            match self.broadcast(&reveal_tx).await {
                Ok(_) => {
                    tracing::info!(txid = %reveal_txid, attempt = attempt + 1, "Broadcasted reveal transaction");
                    break;
                }
                Err(e) if attempt < MAX_RETRIES => {
                    tracing::warn!(
                        attempt = attempt + 1,
                        error = %e,
                        retry_delay_ms = RETRY_DELAY_MS,
                        "Reveal broadcast failed, retrying"
                    );
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
                Err(e) => {
                    tracing::error!(attempt = attempt + 1, error = %e, "Reveal broadcast failed after retries");
                    return Err(e);
                }
            }
        }

        // The new anchor is the first output of the reveal_tx transaction
        self.current_anchor = OutPoint::new(reveal_tx.compute_txid(), 0);

        // Index the sub-block
        // TODO: Get actual Bitcoin height from esplora
        let btc_height = 0u32; // Placeholder - would query from Bitcoin node
        let reveal_txid = reveal_tx.compute_txid();

        if let Err(e) = self.app_state.indexer.index_block(reveal_txid, btc_height, sub_block) {
            tracing::warn!(error = ?e, "Failed to index sub-block");
        }

        tracing::info!(
            anchor = %self.current_anchor.txid,
            "Successfully mined new sub-block"
        );

        Ok(())
    }

    pub async fn ibd(&mut self) -> Result<()> {
        let txs = self.sc.reconstruct_txs();
        if txs.is_empty() {
            tracing::info!("IBD: Subchain is empty, starting from genesis");
            return Ok(());
        }

        let addr = Address::p2wpkh(&self.sc.pubkey, self.sc.network);
        let utxos = self.backend.get_address_utxos(&addr).await?;

        for (idx, tx) in txs.iter().enumerate().rev() {
            let txid = tx.compute_txid();
            if utxos.iter().any(|u| u.outpoint.txid == txid) {
                self.connector_idx = idx + 1;
                if self.connector_idx < txs.len() {
                    let next = &txs[self.connector_idx];
                    self.current_anchor = OutPoint::new(next.compute_txid(), 1);
                } else {
                    // all connectors spent, we are at the tip
                }
                self.last_synced = Some(txid);
                tracing::info!(connector_idx = idx, "IBD: Synced to connector");
                return Ok(());
            }
        }

        tracing::info!("IBD: No spent connectors found, starting from genesis");
        Ok(())
    }
} 