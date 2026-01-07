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
    ///
    /// If `rescan` is true, imports with timestamp 0 to rescan the entire blockchain.
    /// This is needed for IBD to find historical transactions.
    async fn ensure_address_imported_internal(&self, address: &Address, rescan: bool) -> Result<()> {
        let addr_str = address.to_string();

        // Check if already imported
        {
            let addrs = self.initialized_addresses.read().await;
            if addrs.contains(&addr_str) && !rescan {
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

        let timestamp = if rescan { json!(0) } else { json!("now") };

        let import_request = json!([{
            "desc": descriptor_with_checksum,
            "timestamp": timestamp,
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

    /// Ensure an address has been imported to the wallet (no rescan)
    async fn ensure_address_imported(&self, address: &Address) -> Result<()> {
        self.ensure_address_imported_internal(address, false).await
    }

    /// Ensure an address has been imported with full blockchain rescan
    /// This is needed for IBD to find historical transactions
    async fn ensure_address_imported_with_rescan(&self, address: &Address) -> Result<()> {
        self.ensure_address_imported_internal(address, true).await
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

        // First, test if the package would be accepted
        let test_result: serde_json::Value = self
            .rpc
            .call("testmempoolaccept", &[json!(hex_txs)])
            .context("testmempoolaccept RPC call failed")?;

        tracing::debug!(
            test_result = ?test_result,
            "Package mempool acceptance test"
        );

        // Check test results for each transaction
        if let Some(test_array) = test_result.as_array() {
            for (idx, test_tx) in test_array.iter().enumerate() {
                if let Some(allowed) = test_tx.get("allowed").and_then(|v| v.as_bool()) {
                    if !allowed {
                        let reject_reason = test_tx
                            .get("reject-reason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        tracing::error!(
                            tx_index = idx,
                            txid = test_tx.get("txid").and_then(|v| v.as_str()),
                            reject_reason = reject_reason,
                            "Transaction would be rejected by mempool"
                        );

                        return Err(anyhow!(
                            "Transaction {} rejected: {}",
                            idx,
                            reject_reason
                        ));
                    }
                } else {
                    tracing::warn!(
                        tx_index = idx,
                        "Missing 'allowed' field in testmempoolaccept response"
                    );
                }
            }
        }

        // If tests pass, submit the package
        let result: serde_json::Value = self
            .rpc
            .call("submitpackage", &[json!(hex_txs)])
            .context("submitpackage RPC call failed")?;

        tracing::debug!(
            result = ?result,
            "Package submission result"
        );

        // Parse tx-results to verify all transactions were accepted
        if let Some(tx_results) = result.get("tx-results").and_then(|v| v.as_object()) {
            let mut accepted_count = 0;
            let mut ignored_count = 0;

            for (wtxid, tx_result) in tx_results {
                // Check if transaction was ignored (already in mempool with different witness)
                if tx_result.get("other-wtxid").is_some() {
                    ignored_count += 1;
                    tracing::warn!(
                        wtxid = wtxid,
                        "Transaction ignored - already in mempool with different witness"
                    );
                } else {
                    accepted_count += 1;
                    let txid = tx_result.get("txid").and_then(|v| v.as_str());
                    tracing::debug!(
                        wtxid = wtxid,
                        txid = txid,
                        "Transaction accepted to mempool"
                    );
                }
            }

            if accepted_count == 0 && ignored_count == 0 {
                return Err(anyhow!(
                    "Package submission returned empty tx-results: {:?}",
                    result
                ));
            }

            tracing::info!(
                accepted = accepted_count,
                ignored = ignored_count,
                total = txs.len(),
                "Package relay result"
            );
        } else {
            tracing::warn!(
                result = ?result,
                "Unexpected submitpackage response format"
            );
        }

        Ok(())
    }

    async fn get_address_transactions(&self, address: &Address) -> Result<Vec<(Txid, u32)>> {
        // Ensure address is imported with full blockchain rescan
        // This is needed for IBD to find historical transactions
        self.ensure_address_imported_with_rescan(address).await?;

        let wallet_rpc = self.wallet_rpc()?;

        // Get all transactions for this address using listtransactions
        // Label is "aggregator" from our import
        let result: serde_json::Value = wallet_rpc
            .call("listtransactions", &[json!("*"), json!(100000), json!(0), json!(true)])
            .context("Failed to list transactions")?;

        let mut txs = Vec::new();

        if let Some(tx_array) = result.as_array() {
            for tx_entry in tx_array {
                // Filter for our address
                if let Some(tx_addr) = tx_entry.get("address").and_then(|v| v.as_str()) {
                    if tx_addr == address.to_string() {
                        if let Some(txid_str) = tx_entry.get("txid").and_then(|v| v.as_str()) {
                            if let Ok(txid) = txid_str.parse::<Txid>() {
                                // Get block height (0 if unconfirmed)
                                let height = tx_entry.get("blockheight")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32;

                                txs.push((txid, height));
                            }
                        }
                    }
                }
            }
        }

        // Sort by height for chronological order
        txs.sort_by_key(|(_, height)| *height);

        // Remove duplicates (same txid can appear multiple times)
        txs.dedup_by_key(|(txid, _)| *txid);

        Ok(txs)
    }

    async fn get_transaction(&self, txid: &Txid) -> Result<Option<Transaction>> {
        // Use base RPC (doesn't need wallet)
        let result: Result<String, _> = self.rpc.call("getrawtransaction", &[json!(txid.to_string())]);

        match result {
            Ok(hex_str) => {
                // Decode hex to Transaction
                let bytes = hex::decode(&hex_str)
                    .context("Failed to decode transaction hex")?;
                let tx = bitcoin::consensus::deserialize::<Transaction>(&bytes)
                    .context("Failed to deserialize transaction")?;
                Ok(Some(tx))
            }
            Err(_) => Ok(None), // TX not found
        }
    }

    async fn get_spending_tx(&self, outpoint: &OutPoint) -> Result<Option<(Txid, Transaction, u32)>> {
        // Use gettxout with verbose to check if spent
        let result: Result<serde_json::Value, _> = self.rpc.call(
            "gettxout",
            &[
                json!(outpoint.txid.to_string()),
                json!(outpoint.vout),
                json!(true), // include_mempool
            ],
        );

        // If gettxout returns null, the output is spent
        if result.is_ok() && !result.as_ref().unwrap().is_null() {
            return Ok(None); // Not spent yet
        }

        // Output is spent! Now we need to find the spending TX
        // Strategy: Get the block containing the spent output, then scan forward
        // because the spending TX must be in the same block or a later block

        let tx_result: Result<serde_json::Value, _> = self.rpc.call(
            "getrawtransaction",
            &[json!(outpoint.txid.to_string()), json!(true)], // verbose=true
        );

        if let Ok(tx_data) = tx_result {
            // Get the block containing the output being spent
            if let Some(start_blockhash) = tx_data.get("blockhash").and_then(|v| v.as_str()) {
                let start_block_result: Result<serde_json::Value, _> =
                    self.rpc.call("getblock", &[json!(start_blockhash)]);

                if let Ok(start_block_data) = start_block_result {
                    let start_height = start_block_data
                        .get("height")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;

                    // Get current chain height
                    let chain_info: serde_json::Value = self.rpc.call("getblockchaininfo", &[])?;
                    let chain_height = chain_info
                        .get("blocks")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;

                    // Scan from start_height to chain tip (or up to 100 blocks ahead max)
                    let max_scan_height = std::cmp::min(chain_height, start_height + 100);

                    for scan_height in start_height..=max_scan_height {
                        // Get block hash at this height
                        let blockhash_result: Result<String, _> =
                            self.rpc.call("getblockhash", &[json!(scan_height)]);

                        if let Ok(blockhash) = blockhash_result {
                            // Get block with all transactions
                            let block_result: Result<serde_json::Value, _> =
                                self.rpc.call("getblock", &[json!(blockhash), json!(2)]); // verbosity=2

                            if let Ok(block_data) = block_result {
                                // Scan all transactions in this block
                                if let Some(txs) = block_data.get("tx").and_then(|v| v.as_array()) {
                                    for tx_obj in txs {
                                        if let Some(inputs) = tx_obj.get("vin").and_then(|v| v.as_array()) {
                                            for input in inputs {
                                                let input_txid = input.get("txid").and_then(|v| v.as_str());
                                                let input_vout = input.get("vout").and_then(|v| v.as_u64());

                                                if let (Some(txid_str), Some(vout)) = (input_txid, input_vout) {
                                                    if txid_str == outpoint.txid.to_string()
                                                        && vout == outpoint.vout as u64
                                                    {
                                                        // Found the spending TX!
                                                        if let Some(spending_txid_str) =
                                                            tx_obj.get("txid").and_then(|v| v.as_str())
                                                        {
                                                            let spending_txid = spending_txid_str
                                                                .parse::<Txid>()
                                                                .context("Failed to parse spending txid")?;

                                                            // Parse transaction from hex in block response (more efficient than separate RPC call)
                                                            if let Some(hex_str) = tx_obj.get("hex").and_then(|v| v.as_str()) {
                                                                let bytes = hex::decode(hex_str)
                                                                    .context("Failed to decode transaction hex")?;
                                                                let spending_tx = bitcoin::consensus::deserialize::<Transaction>(&bytes)
                                                                    .context("Failed to deserialize transaction")?;

                                                                return Ok(Some((
                                                                    spending_txid,
                                                                    spending_tx,
                                                                    scan_height,
                                                                )));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}
