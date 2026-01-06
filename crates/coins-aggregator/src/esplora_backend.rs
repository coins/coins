//! Esplora backend implementation
//!
//! Thin wrapper around esplora-client that implements the BlockchainBackend trait.
//! Used for public networks (signet, mainnet) where running a full node is inconvenient.

use crate::blockchain_backend::{BlockchainBackend, OutputStatus, Utxo};
use anyhow::{Context, Result};
use async_trait::async_trait;
use bitcoin::{Address, OutPoint, Transaction, Txid};
use esplora_client::r#async::DefaultSleeper;
use esplora_client::{AsyncClient, Builder};

/// Esplora backend for public networks
pub struct EsploraBackend {
    client: AsyncClient<DefaultSleeper>,
}

impl EsploraBackend {
    /// Create a new Esplora backend
    pub fn new(esplora_url: &str) -> Result<Self> {
        let client = AsyncClient::from_builder(Builder::new(esplora_url))
            .with_context(|| format!("Failed to create Esplora client for {}", esplora_url))?;

        Ok(Self { client })
    }
}

#[async_trait]
impl BlockchainBackend for EsploraBackend {
    async fn get_address_utxos(&self, address: &Address) -> Result<Vec<Utxo>> {
        let utxos = self
            .client
            .get_address_utxo(address.clone())
            .await
            .with_context(|| format!("Failed to get UTXOs for {}", address))?;

        Ok(utxos
            .into_iter()
            .map(|u| Utxo {
                outpoint: OutPoint::new(u.txid, u.vout as u32),
                value: u.value,  // u.value is already Amount
                confirmed: u.status.confirmed,
            })
            .collect())
    }

    async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<Option<OutputStatus>> {
        let status = self
            .client
            .get_output_status(txid, vout as u64)
            .await
            .with_context(|| format!("Failed to get output status for {}:{}", txid, vout))?;

        Ok(status.map(|s| OutputStatus {
            spent: s.spent,
            confirmations: s
                .status
                .and_then(|st| st.block_height.map(|_| 1))
                .unwrap_or(0),
        }))
    }

    async fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.client
            .broadcast(tx)
            .await
            .context("Failed to broadcast transaction via Esplora")?;
        Ok(())
    }

    // Use default broadcast_package (broadcasts individually)
    // Esplora doesn't support package relay
}
