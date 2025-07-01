use async_trait::async_trait;
use anyhow::Result;
use bitcoin::Transaction;


use esplora_client::{AsyncClient, Builder};
use esplora_client::r#async::DefaultSleeper;

#[async_trait]
pub trait Broadcaster: Send + Sync {
    async fn broadcast_raw(&self, tx: &Transaction) -> Result<()>;

    async fn broadcast_package(&self, txs: &[Transaction]) -> Result<()> {
        // default: individual
        for tx in txs {
            self.broadcast_raw(tx).await?;
        }
        Ok(())
    }
}

/// Simple REST broadcaster that talks to an electrs REST or Esplora-like backend
/// supporting `POST /tx` and (optionally) `POST /txs/package`.
pub struct RestBroadcaster {
    pub base_url: String, // e.g. "https://mempool.space/signet/api" or self-hosted electrs
    client: AsyncClient<DefaultSleeper>,
}

impl RestBroadcaster {
    pub fn new(base_url: &str) -> Self {
        let client: AsyncClient<DefaultSleeper> = AsyncClient::from_builder(Builder::new(base_url))
            .expect("failed to create esplora client");
        Self { base_url: base_url.trim_end_matches('/').to_string(), client }
    }

    /// Fetch UTXOs for a bech32 address using electrs `/address/<addr>/utxo`.
    pub async fn get_address_utxo(&self, addr: &bitcoin::Address) -> Result<Vec<Utxo>> {
        let utxos = self.client.get_address_utxo(addr.clone()).await?;
        Ok(utxos
            .into_iter()
            .map(|u| Utxo {
                outpoint: bitcoin::OutPoint::new(u.txid, u.vout as u32),
                value: u.value,
                confirmed: u.status.confirmed,
            })
            .collect())
    }
}

#[async_trait]
impl Broadcaster for RestBroadcaster {
    async fn broadcast_raw(&self, tx: &Transaction) -> Result<()> {
        self.client.broadcast(tx).await?;
        Ok(())
    }

    async fn broadcast_package(&self, txs: &[Transaction]) -> Result<()> {
        self.client.broadcast_package(txs).await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: bitcoin::OutPoint,
    pub value: bitcoin::Amount,
    pub confirmed: bool,
}

