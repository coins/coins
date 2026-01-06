//! Bitcoin RPC backend implementation
//!
//! Uses Bitcoin Core's wallet API for efficient UTXO queries:
//! - One-time address import with `importaddress` (rescan=false)
//! - Fast queries with `listunspent` (uses address index)
//! - Direct transaction broadcasting with `sendrawtransaction`
//! - Package relay support with `submitpackage`

use crate::blockchain_backend::{BlockchainBackend, OutputStatus, Utxo};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use bitcoin::{Address, OutPoint, Transaction, Txid};
use bitcoincore_rpc::{Auth, Client as RpcClient, RpcApi};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Bitcoin RPC backend using wallet API
pub struct RpcBackend {
    /// RPC URL for creating wallet clients
    rpc_url: String,
    /// RPC auth
    rpc_user: String,
    rpc_pass: String,
    /// Base RPC client (no wallet loaded)
    rpc: Arc<RpcClient>,
    /// Wallet name for this backend
    wallet_name: String,
    /// Track which addresses have been imported
    initialized_addresses: Arc<RwLock<HashSet<String>>>,
}

impl RpcBackend {
    /// Create a new RPC backend
    ///
    /// Ensures the specified wallet exists and is watch-only (no private keys).
    pub fn new(
        rpc_url: String,
        rpc_user: String,
        rpc_pass: String,
        wallet_name: String,
    ) -> Result<Self> {
        let auth = Auth::UserPass(rpc_user.clone(), rpc_pass.clone());
        let rpc = RpcClient::new(&rpc_url, auth)
            .with_context(|| format!("Failed to connect to Bitcoin RPC at {}", rpc_url))?;

        // Ensure watch-only wallet exists
        Self::ensure_wallet(&rpc, &wallet_name)?;

        Ok(Self {
            rpc_url: rpc_url.clone(),
            rpc_user,
            rpc_pass,
            rpc: Arc::new(rpc),
            wallet_name,
            initialized_addresses: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Ensure wallet exists, create if needed
    fn ensure_wallet(rpc: &RpcClient, wallet_name: &str) -> Result<()> {
        // List existing wallets
        let wallets: Vec<String> = match rpc.list_wallets() {
            Ok(w) => w,
            Err(_) => Vec::new(),
        };

        // If wallet is already loaded, we're done
        if wallets.contains(&wallet_name.to_string()) {
            return Ok(());
        }

        // Try to load wallet
        if rpc.load_wallet(wallet_name).is_ok() {
            return Ok(());
        }

        // Create new watch-only wallet
        rpc.create_wallet(wallet_name, Some(true), None, None, None)
            .with_context(|| format!("Failed to create wallet '{}'", wallet_name))?;

        Ok(())
    }

    /// Get RPC client with wallet loaded
    fn wallet_rpc(&self) -> Result<RpcClient> {
        let wallet_url = format!(
            "{}/wallet/{}",
            self.rpc_url.trim_end_matches('/'),
            self.wallet_name
        );

        let auth = Auth::UserPass(self.rpc_user.clone(), self.rpc_pass.clone());
        RpcClient::new(&wallet_url, auth).context("Failed to create wallet RPC client")
    }

    /// Ensure an address has been imported to the wallet
    async fn ensure_address_imported(&self, address: &Address) -> Result<()> {
        let addr_str = address.to_string();

        // Check if already imported
        {
            let addrs = self.initialized_addresses.read().await;
            if addrs.contains(&addr_str) {
                return Ok(());
            }
        }

        // Import address using importdescriptors for descriptor wallets
        // Create an addr() descriptor for the address and get checksum
        let descriptor_no_checksum = format!("addr({})", addr_str);
        let wallet_rpc = self.wallet_rpc()?;

        // Get descriptor info with checksum
        let desc_info: serde_json::Value = wallet_rpc
            .call("getdescriptorinfo", &[json!(descriptor_no_checksum)])
            .with_context(|| format!("Failed to get descriptor info for {}", addr_str))?;

        let descriptor_with_checksum = desc_info["descriptor"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing descriptor in getdescriptorinfo response"))?;

        let import_request = json!([{
            "desc": descriptor_with_checksum,
            "timestamp": "now",
            "watchonly": true,
            "label": "aggregator"
        }]);

        let result: serde_json::Value = wallet_rpc
            .call("importdescriptors", &[import_request])
            .with_context(|| format!("Failed to import address {}", addr_str))?;

        // Check if import was successful
        if let Some(first) = result.as_array().and_then(|arr| arr.first()) {
            if let Some(success) = first.get("success").and_then(|v| v.as_bool()) {
                if !success {
                    let error = first.get("error").map(|e| e.to_string()).unwrap_or_else(|| "unknown error".to_string());
                    return Err(anyhow!("Import descriptor failed: {}", error));
                }
            }
        }

        // Mark as imported
        let mut addrs = self.initialized_addresses.write().await;
        addrs.insert(addr_str);

        Ok(())
    }
}

#[async_trait]
impl BlockchainBackend for RpcBackend {
    async fn get_address_utxos(&self, address: &Address) -> Result<Vec<Utxo>> {
        // Ensure address is imported
        self.ensure_address_imported(address).await?;

        // Query UTXOs using listunspent - fast O(1) lookup
        let wallet_rpc = self.wallet_rpc()?;
        let addresses_vec = vec![address];
        let utxos = wallet_rpc
            .list_unspent(
                Some(1),                      // min_conf = 1 (confirmed only)
                None,                         // max_conf (unlimited)
                Some(&addresses_vec),         // filter by address
                None,                         // include_unsafe
                None,                         // query_options
            )
            .with_context(|| format!("Failed to list unspent for {}", address))?;

        Ok(utxos
            .into_iter()
            .map(|u| Utxo {
                outpoint: OutPoint::new(u.txid, u.vout),
                value: u.amount,
                confirmed: u.confirmations >= 1,
            })
            .collect())
    }

    async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<Option<OutputStatus>> {
        // gettxout returns None if spent, UTXO data if unspent
        match self.rpc.get_tx_out(txid, vout, Some(true))? {
            None => Ok(Some(OutputStatus {
                spent: true,
                confirmations: 0,
            })),
            Some(out) => Ok(Some(OutputStatus {
                spent: false,
                confirmations: out.confirmations,
            })),
        }
    }

    async fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.rpc
            .send_raw_transaction(tx)
            .with_context(|| "Failed to broadcast transaction")?;
        Ok(())
    }

    async fn broadcast_package(&self, txs: &[Transaction]) -> Result<()> {
        // Use submitpackage RPC for package relay (Bitcoin Core 24+)
        let hex_txs: Vec<String> = txs
            .iter()
            .map(|tx| bitcoin::consensus::encode::serialize_hex(tx))
            .collect();

        let result: serde_json::Value = self
            .rpc
            .call("submitpackage", &[json!(hex_txs)])
            .context("submitpackage RPC call failed")?;

        // Check for errors in package result
        if let Some(pkg_msg) = result.get("package_msg").and_then(|v| v.as_str()) {
            if pkg_msg.to_lowercase().contains("error") {
                return Err(anyhow!("Package relay failed: {}", pkg_msg));
            }
        }

        Ok(())
    }
}
