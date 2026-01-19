use anyhow::Result;
use bitcoin::Txid;
use coins_crypto::G1;
use coins_types::{Account, SubBlock, Transaction};
use serde::{Deserialize, Serialize};

/// HTTP client for querying the indexer service
#[derive(Clone)]
pub struct IndexerClient {
    base_url: String,
    client: reqwest::Client,
}

impl IndexerClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    /// Health check
    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/health", self.base_url);
        self.client.get(&url).send().await?.error_for_status()?;
        Ok(())
    }

    /// Get account by public key
    pub async fn get_account(&self, pk: &G1) -> Result<Option<Account>> {
        let pk_hex = hex::encode(pk.0);
        let url = format!("{}/accounts/{}", self.base_url, pk_hex);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let account = response.error_for_status()?.json().await?;
        Ok(Some(account))
    }

    /// Get account by ID (needed for IBD sub-block deserialization)
    pub async fn get_account_by_id(&self, id: u32) -> Result<Option<Account>> {
        let url = format!("{}/accounts/by-id/{}", self.base_url, id);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let account = response.error_for_status()?.json().await?;
        Ok(Some(account))
    }

    /// Get latest block
    pub async fn get_latest_block(&self) -> Result<Option<ChainBlockResponse>> {
        let url = format!("{}/blocks/latest", self.base_url);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let block = response.error_for_status()?.json().await?;
        Ok(Some(block))
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: u32) -> Result<Option<ChainBlockResponse>> {
        let url = format!("{}/blocks/{}", self.base_url, height);

        let response = self.client.get(&url).send().await?;

        if response.status() == 404 {
            return Ok(None);
        }

        let block = response.error_for_status()?.json().await?;
        Ok(Some(block))
    }

    /// Get range of blocks
    pub async fn get_blocks_range(&self, from: Option<u32>, to: Option<u32>) -> Result<Vec<ChainBlockResponse>> {
        let mut url = format!("{}/blocks", self.base_url);

        let mut params = Vec::new();
        if let Some(from) = from {
            params.push(format!("from={}", from));
        }
        if let Some(to) = to {
            params.push(format!("to={}", to));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let blocks = self.client.get(&url).send().await?.error_for_status()?.json().await?;
        Ok(blocks)
    }

    /// Submit a new block for indexing (called by publisher)
    pub async fn submit_block(&self, btc_txid: Txid, btc_height: u32, sub_block: SubBlock) -> Result<()> {
        let url = format!("{}/blocks", self.base_url);

        let request = SubmitBlockRequest {
            btc_txid: btc_txid.to_string(),
            btc_height,
            sub_block,
        };

        self.client
            .post(&url)
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    /// Get network statistics
    pub async fn get_stats(&self) -> Result<NetworkStats> {
        let url = format!("{}/stats", self.base_url);
        let stats = self.client.get(&url).send().await?.error_for_status()?.json().await?;
        Ok(stats)
    }

    /// Get account transaction history
    pub async fn get_account_transactions(&self, pk: &G1) -> Result<Vec<TransactionWithStatus>> {
        let pk_hex = hex::encode(pk.0);
        let url = format!("{}/accounts/{}/transactions", self.base_url, pk_hex);
        let txs = self.client.get(&url).send().await?.error_for_status()?.json().await?;
        Ok(txs)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionWithStatus {
    #[serde(flatten)]
    pub tx: Transaction,
    pub btc_height: u32,
    pub btc_txid: String,
    pub confirmations: u32,
    pub finalized: bool,
    pub confirmations_remaining: u32,
    pub direction: TransactionDirection,
    pub sender_pk: G1,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChainBlockResponse {
    pub height: u32,
    pub btc_height: u32,
    pub btc_txid: String,
    pub sub_block: SubBlock,
}

#[derive(Serialize)]
struct SubmitBlockRequest {
    pub btc_txid: String,
    pub btc_height: u32,
    pub sub_block: SubBlock,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkStats {
    pub total_blocks: u32,
    pub total_accounts: u64,
    pub total_supply: u64,
    pub network: String,
}
