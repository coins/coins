use bitcoincore_rpc::{Auth, Client as RpcClient, RpcApi};
use bitcoin::Network;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct BitcoinClient {
    rpc: Arc<RpcClient>,
    network: Network,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinLinks {
    pub mempool_space: String,
    pub blockstream: String,
}

impl BitcoinClient {
    pub fn new(url: String, user: String, pass: String, network: Network) -> Result<Self> {
        let auth = Auth::UserPass(user, pass);
        let rpc = Arc::new(RpcClient::new(&url, auth)?);
        Ok(Self { rpc, network })
    }

    /// Get current Bitcoin block height
    pub fn get_block_height(&self) -> Result<u32> {
        let info = self.rpc.get_blockchain_info()?;
        Ok(info.blocks as u32)
    }

    /// Get block timestamp
    pub fn get_block_timestamp(&self, height: u32) -> Result<Option<u64>> {
        match self.rpc.get_block_hash(height as u64) {
            Ok(hash) => {
                let header = self.rpc.get_block_header_info(&hash)?;
                Ok(Some(header.time as u64))
            }
            Err(_) => Ok(None),
        }
    }

    /// Calculate confirmations
    pub fn get_confirmations(&self, btc_height: u32) -> Result<u32> {
        let current = self.get_block_height()?;
        Ok(current.saturating_sub(btc_height) + 1)
    }

    /// Generate Bitcoin explorer links
    pub fn get_explorer_links(&self, txid: &str) -> BitcoinLinks {
        let base_mempool = match self.network {
            Network::Bitcoin => "https://mempool.space",
            Network::Testnet => "https://mempool.space/testnet",
            Network::Signet => "https://mempool.space/signet",
            Network::Regtest => "http://localhost:8080",
            _ => "https://mempool.space",
        };

        let base_blockstream = match self.network {
            Network::Bitcoin => "https://blockstream.info",
            Network::Testnet => "https://blockstream.info/testnet",
            _ => "https://blockstream.info",
        };

        BitcoinLinks {
            mempool_space: format!("{}/tx/{}", base_mempool, txid),
            blockstream: format!("{}/tx/{}", base_blockstream, txid),
        }
    }
}
