use std::path::PathBuf;
use anyhow::Result;
use bitcoin::{Address, Amount, Network, OutPoint, PrivateKey, Txid, CompressedPublicKey, Transaction, FeeRate};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use esplora_client::{AsyncClient, Builder};
use esplora_client::r#async::DefaultSleeper;
use coins_spacechain::Spacechain;
use std::str::FromStr;
use rand::rngs::OsRng;
use crate::api::AppState;
use coins_types::SubBlock;
use coins_crypto::{G1, G2};
use bincode;
use coins_spacechain::inscribe::{inscribe_blob};
use ark_ec::Group;

/// Wrapper for esplora-client UTXO (txid,vout,value)
#[derive(Debug, Clone)]
pub struct FeeUtxo {
    pub outpoint: OutPoint,
    pub value: Amount,
}

pub struct Engine {
    pub client: AsyncClient<DefaultSleeper>,
    pub sc: Spacechain,
    pub fee_sk: SecretKey,
    pub fee_addr: Address,
    pub fee_utxos: Vec<FeeUtxo>,
    pub current_anchor: OutPoint,
    pub connector_idx: usize,
    pub last_synced: Option<Txid>,
    pub app_state: AppState,
    pub base_url: String,
}

impl Engine {
    /// Initialize from cli opts: esplora url, spacechain path, optional fee key path.
    pub async fn new(esplora: &str, spacechain_path: PathBuf, network: Network, key_file: Option<PathBuf>, app_state: AppState) -> Result<Self> {
        // ---------- load spacechain ----------
        let sc_bytes = std::fs::read(&spacechain_path)?;
        let sc = Spacechain::decode(&sc_bytes).ok_or_else(|| anyhow::anyhow!("invalid spacechain file"))?;

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

        // ---------- Esplora client ----------
        let client: AsyncClient<DefaultSleeper> = AsyncClient::from_builder(Builder::new(esplora))?;

        // current anchor (connector.output[1]) for idx 0 initially
        let current_anchor = sc.first_out;

        let mut eng = Self {
            client,
            sc,
            fee_sk,
            fee_addr,
            fee_utxos: Vec::new(),
            current_anchor,
            connector_idx: 0,
            last_synced: None,
            app_state,
            base_url: esplora.trim_end_matches('/').to_string(),
        };
        eng.refresh_fee_utxos().await?;
        Ok(eng)
    }

    /// Query esplora for all UTXOs belonging to `fee_addr`.
    pub async fn refresh_fee_utxos(&mut self) -> Result<()> {
        let utxos = self.client.get_address_utxo(self.fee_addr.clone()).await?;
        self.fee_utxos = utxos.into_iter()
            .filter(|u| u.status.confirmed)
            .map(|u| FeeUtxo { outpoint: OutPoint::new(u.txid, u.vout as u32), value: u.value })
            .collect();
        Ok(())
    }

    /// Check if current anchor is still unspent; if spent, advance to next connector.
    pub async fn refresh_anchor(&mut self) -> Result<()> {
        let txid32 = Txid::from_str(&self.current_anchor.txid.to_string())?;
        let status_opt = self.client.get_output_status(&txid32, self.current_anchor.vout as u64).await?;
        if let Some(status) = status_opt {
            if status.spent {
                self.connector_idx += 1;
                let txs = self.sc.reconstruct_txs();
                if self.connector_idx >= txs.len() {
                    anyhow::bail!("spacechain exhausted");
                }
                let next = &txs[self.connector_idx];
                self.current_anchor = OutPoint::new(next.compute_txid(), 1);
            }
        }
        Ok(())
    }

    /// Broadcast raw tx hex
    pub async fn broadcast(&self, tx: &bitcoin::Transaction) -> Result<()> {
        self.client.broadcast(tx).await?;
        Ok(())
    }

    /// Broadcast a parent/child package via esplora-client's native helper.
    async fn broadcast_package(&self, txs: &[bitcoin::Transaction]) -> Result<()> {
        // The upstream esplora-client crate now supports package relay directly.
        self.client.broadcast_package(txs).await?;
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

        println!("Mining sub-block with {} transactions...", txs.len());

        let (transactions, signatures): (Vec<_>, Vec<_>) = txs.into_iter().unzip();

        // For now, we'll use a dummy aggregated signature
        let aggregated_signature = G2::default();

        let sub_block = SubBlock {
            txs: transactions,
            sigma: aggregated_signature,
            aggregator_pk: G1::default(), // Placeholder
        };

        let sub_block_bytes = sub_block.serialize();

        if self.fee_utxos.is_empty() {
            println!("Cannot mine sub-block: No fee UTXOs available.");
            return Ok(());
        }

        let fee_utxo = self.fee_utxos[0].clone();
        let fee_rate = FeeRate::from_sat_per_vb(1).unwrap();
        
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
            println!("Package relay failed: {}. Falling back to individual broadcasts.", e);
            self.broadcast(&connector_tx).await?;
            self.broadcast(&commit_tx).await?;
        } else {
            println!("Package broadcasted successfully: connector {} commit_tx {}", connector_tx.compute_txid(), commit_tx.compute_txid());
        }

        // broadcast reveal transaction individually
        self.broadcast(&reveal_tx).await?;
        println!("Broadcasted reveal tx: {}", reveal_tx.compute_txid());

        // The new anchor is the first output of the reveal_tx transaction
        self.current_anchor = OutPoint::new(reveal_tx.compute_txid(), 0);

        println!("Successfully mined new sub-block!");

        Ok(())
    }

    pub async fn ibd(&mut self) -> Result<()> {
        let txs = self.sc.reconstruct_txs();
        if txs.is_empty() {
            println!("IBD: Spacechain is empty, starting from genesis.");
            return Ok(());
        }

        let addr = Address::p2wpkh(&self.sc.pubkey, self.sc.network);
        let utxos = self.client.get_address_utxo(addr).await?;

        for (idx, tx) in txs.iter().enumerate().rev() {
            let txid = tx.compute_txid();
            if utxos.iter().any(|u| u.txid == txid) {
                self.connector_idx = idx + 1;
                if self.connector_idx < txs.len() {
                    let next = &txs[self.connector_idx];
                    self.current_anchor = OutPoint::new(next.compute_txid(), 1);
                } else {
                    // all connectors spent, we are at the tip
                }
                self.last_synced = Some(txid);
                println!("IBD: Synced to connector {}", idx);
                return Ok(());
            }
        }

        println!("IBD: No spent connectors found, starting from genesis.");
        Ok(())
    }
} 