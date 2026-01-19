//! Bitcoin RPC backend implementation
//!
//! Uses Bitcoin Core's wallet API for efficient UTXO queries:
//! - One-time address import with `importaddress` (rescan=false)
//! - Fast queries with `listunspent` (uses address index)
//! - Direct transaction broadcasting with `sendrawtransaction`
//! - Package relay support with `submitpackage`

use anyhow::{anyhow, Context, Result};
use bitcoin::{Address, Amount, OutPoint, Transaction, Txid};
use bitcoincore_rpc::{Auth, Client as RpcClient, RpcApi};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

// RPC backend configuration constants
/// Poll interval during wallet rescan (milliseconds)
const RESCAN_POLL_INTERVAL_MS: u64 = 100;
/// Minimum confirmations for UTXO queries
const MIN_UTXO_CONFIRMATIONS: usize = 1;
/// Maximum transactions to query in listtransactions
const MAX_TRANSACTION_HISTORY: i64 = 100_000;

/// A UTXO with confirmation status
#[derive(Debug, Clone)]
pub struct Utxo {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub confirmed: bool,
}

/// Output status indicating if spent and confirmation count
#[derive(Debug, Clone)]
pub struct OutputStatus {
    pub spent: bool,
    pub confirmations: u32,
}

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
        let wallets: Vec<String> = rpc.list_wallets().unwrap_or_default();

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

    /// Get a safe rescan timestamp for pruned mode
    ///
    /// Returns a timestamp ~50 blocks back to avoid pruned blocks
    async fn get_safe_rescan_timestamp(&self) -> Result<u64> {
        let rpc = self.wallet_rpc()?;

        // Get blockchain info to check if pruned
        let chain_info: serde_json::Value = rpc
            .call("getblockchaininfo", &[])
            .context("Failed to get blockchain info")?;

        let current_height = chain_info["blocks"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing blocks in getblockchaininfo"))?;

        // Go back 50 blocks for safety
        let rescan_height = current_height.saturating_sub(50);

        // Get block hash at that height
        let block_hash: String = rpc
            .call("getblockhash", &[json!(rescan_height)])
            .context("Failed to get block hash")?;

        // Get block info to get timestamp
        let block_info: serde_json::Value = rpc
            .call("getblock", &[json!(block_hash)])
            .context("Failed to get block")?;

        let timestamp = block_info["time"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing time in block info"))?;

        Ok(timestamp)
    }

    /// Ensure an address has been imported to the wallet
    ///
    /// If `rescan` is true, imports with recent timestamp to rescan from ~50 blocks back.
    /// This is compatible with pruned mode while still catching recent transactions.
    async fn ensure_address_imported_internal(&self, address: &Address, rescan: bool) -> Result<()> {
        let addr_str = address.to_string();

        // Check if already imported
        let already_imported = {
            let addrs = self.initialized_addresses.read().await;
            addrs.contains(&addr_str)
        };

        if already_imported && !rescan {
            return Ok(());
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

        // Use recent timestamp (50 blocks back) for pruned mode compatibility
        // or "now" if already imported and not rescanning
        let timestamp = if rescan || !already_imported {
            json!(self.get_safe_rescan_timestamp().await?)
        } else {
            json!("now")
        };

        let import_request = json!([{
            "desc": descriptor_with_checksum,
            "timestamp": timestamp,
            "watchonly": true,
            "label": "publisher"
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

        // If we're rescanning, wait for the rescan to complete
        if rescan {
            tracing::info!(address = %addr_str, "Waiting for blockchain rescan to complete...");

            // Poll getwalletinfo until scanning is complete
            loop {
                let wallet_info: serde_json::Value = wallet_rpc
                    .call("getwalletinfo", &[])
                    .context("Failed to get wallet info")?;

                // Check if scanning field exists and is not false
                if let Some(scanning) = wallet_info.get("scanning") {
                    if scanning.is_object() {
                        // Scanning in progress, get progress if available
                        if let Some(progress) = scanning.get("progress").and_then(|v| v.as_f64()) {
                            tracing::debug!(progress = format!("{:.2}%", progress * 100.0), "Rescan progress");
                        }
                        // Wait a bit before checking again
                        tokio::time::sleep(tokio::time::Duration::from_millis(RESCAN_POLL_INTERVAL_MS)).await;
                        continue;
                    }
                }

                // Scanning complete
                tracing::info!(address = %addr_str, "Rescan complete");
                break;
            }
        }

        // Mark as imported
        let mut addrs = self.initialized_addresses.write().await;
        addrs.insert(addr_str);

        Ok(())
    }

    /// Ensure an address has been imported to the wallet (no rescan)
    pub async fn ensure_address_imported(&self, address: &Address) -> Result<()> {
        self.ensure_address_imported_internal(address, false).await
    }

    /// Ensure an address has been imported with full blockchain rescan
    /// This is needed for IBD to find historical transactions
    async fn ensure_address_imported_with_rescan(&self, address: &Address) -> Result<()> {
        self.ensure_address_imported_internal(address, true).await
    }

    /// Ensure an address has been imported starting from a specific block height
    /// Converts block height to timestamp for wallet import
    pub async fn ensure_address_imported_from_height(&self, address: &Address, genesis_height: u32) -> Result<()> {
        let addr_str = address.to_string();

        // Check if already imported
        let already_imported = {
            let addrs = self.initialized_addresses.read().await;
            addrs.contains(&addr_str)
        };

        if already_imported {
            return Ok(());
        }

        // Get block hash at genesis height
        let blockhash: String = self.rpc
            .call("getblockhash", &[json!(genesis_height)])
            .with_context(|| format!("Failed to get block hash at height {}", genesis_height))?;

        // Get block header to extract timestamp
        let block_header: serde_json::Value = self.rpc
            .call("getblockheader", &[json!(blockhash)])
            .with_context(|| format!("Failed to get block header for {}", blockhash))?;

        let timestamp = block_header.get("time")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Missing timestamp in block header"))?;

        tracing::info!(
            address = %addr_str,
            genesis_height = genesis_height,
            timestamp = timestamp,
            "Importing address from genesis height"
        );

        // Import with specific timestamp
        let descriptor_no_checksum = format!("addr({})", addr_str);
        let wallet_rpc = self.wallet_rpc()?;

        let desc_info: serde_json::Value = wallet_rpc
            .call("getdescriptorinfo", &[json!(descriptor_no_checksum)])
            .with_context(|| format!("Failed to get descriptor info for {}", addr_str))?;

        let descriptor_with_checksum = desc_info["descriptor"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing descriptor in getdescriptorinfo response"))?;

        let import_request = json!([{
            "desc": descriptor_with_checksum,
            "timestamp": timestamp,
            "watchonly": true,
            "label": "publisher"
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

        // Wait for rescan to complete
        tracing::info!(address = %addr_str, "Waiting for blockchain rescan to complete...");
        loop {
            let wallet_info: serde_json::Value = wallet_rpc
                .call("getwalletinfo", &[])
                .context("Failed to get wallet info")?;

            if let Some(scanning) = wallet_info.get("scanning") {
                if scanning.is_object() {
                    if let Some(progress) = scanning.get("progress").and_then(|v| v.as_f64()) {
                        tracing::debug!(progress = format!("{:.2}%", progress * 100.0), "Rescan progress");
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(RESCAN_POLL_INTERVAL_MS)).await;
                    continue;
                }
            }

            tracing::info!(address = %addr_str, "Rescan complete");
            break;
        }

        // Mark as imported
        let mut addrs = self.initialized_addresses.write().await;
        addrs.insert(addr_str);

        Ok(())
    }
}

impl RpcBackend {
    pub async fn get_address_utxos(&self, address: &Address) -> Result<Vec<Utxo>> {
        // Ensure address is imported
        self.ensure_address_imported(address).await?;

        // Query UTXOs using listunspent - fast O(1) lookup
        let wallet_rpc = self.wallet_rpc()?;
        let addresses_vec = vec![address];
        let utxos = wallet_rpc
            .list_unspent(
                Some(MIN_UTXO_CONFIRMATIONS),  // min_conf (confirmed only)
                None,                          // max_conf (unlimited)
                Some(&addresses_vec),          // filter by address
                None,                          // include_unsafe
                None,                          // query_options
            )
            .with_context(|| format!("Failed to list unspent for {}", address))?;

        Ok(utxos
            .into_iter()
            .map(|u| Utxo {
                outpoint: OutPoint::new(u.txid, u.vout),
                value: u.amount,
                confirmed: u.confirmations >= MIN_UTXO_CONFIRMATIONS as u32,
            })
            .collect())
    }

    pub async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<Option<OutputStatus>> {
        // First check if transaction exists at all
        // Try base RPC first (works with txindex or for mempool txs)
        let tx_exists_base = matches!(
            self.rpc.call::<Option<String>>("getrawtransaction", &[json!(txid.to_string())]),
            Ok(Some(_))
        );

        // If that fails, try wallet RPC (works for wallet transactions without txindex)
        let tx_exists = if !tx_exists_base {
            let wallet_rpc = self.wallet_rpc()?;
            matches!(
                wallet_rpc.call::<Option<serde_json::Value>>("gettransaction", &[json!(txid.to_string())]),
                Ok(Some(_))
            )
        } else {
            true
        };

        if !tx_exists {
            // Transaction not found (never broadcast or pruned)
            return Ok(None);
        }

        // Transaction exists - check if output is spent or unspent
        // gettxout returns None if spent, Some if unspent
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

    pub async fn broadcast(&self, tx: &Transaction) -> Result<()> {
        self.rpc
            .send_raw_transaction(tx)
            .with_context(|| "Failed to broadcast transaction")?;
        Ok(())
    }

    pub async fn broadcast_package(&self, txs: &[Transaction]) -> Result<()> {
        // Use submitpackage RPC for package relay (Bitcoin Core 24+)
        // submitpackage validates the package as a whole (CPFP), so individual
        // transactions may have 0 fees as long as the package fee rate is sufficient
        let hex_txs: Vec<String> = txs
            .iter()
            .map(bitcoin::consensus::encode::serialize_hex)
            .collect();

        tracing::debug!(
            tx_count = txs.len(),
            "Submitting package"
        );

        // Submit the package directly (no testmempoolaccept - it tests individual TXs)
        // maxfeerate=0 means accept any fee rate
        // maxburnamount=21000000 (21M BTC) allows any OP_RETURN outputs
        let result: serde_json::Value = self
            .rpc
            .call("submitpackage", &[json!(hex_txs), json!(0), json!(21000000)])
            .context("submitpackage RPC call failed")?;

        tracing::info!(
            result = ?result,
            "Package submission full result"
        );

        // Parse tx-results to verify all transactions were accepted
        if let Some(tx_results) = result.get("tx-results").and_then(|v| v.as_object()) {
            let mut accepted_count = 0;
            let mut ignored_count = 0;

            for (wtxid, tx_result) in tx_results {
                // Check for errors
                if let Some(error) = tx_result.get("error") {
                    tracing::error!(
                        wtxid = wtxid,
                        error = ?error,
                        "Transaction rejected in package"
                    );
                    return Err(anyhow!("Transaction {} rejected: {:?}", wtxid, error));
                }

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
                    tracing::info!(
                        wtxid = wtxid,
                        txid = txid,
                        tx_result = ?tx_result,
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

    pub async fn get_address_transactions(&self, address: &Address) -> Result<Vec<(Txid, u32)>> {
        // Ensure address is imported (without rescan - it should already be imported from IBD)
        // Use the no-rescan version since we import during discover_blocks_loop
        self.ensure_address_imported(address).await?;

        let wallet_rpc = self.wallet_rpc()?;

        // Get all transactions for this address using listtransactions
        // Label is "publisher" from our import
        let result: serde_json::Value = wallet_rpc
            .call("listtransactions", &[json!("*"), json!(MAX_TRANSACTION_HISTORY), json!(0), json!(true)])
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

    pub async fn get_transaction(&self, txid: &Txid) -> Result<serde_json::Value> {
        // Use base RPC (doesn't need wallet)
        // Returns verbose JSON with blockhash if confirmed
        let result: serde_json::Value = self.rpc.call(
            "getrawtransaction",
            &[json!(txid.to_string()), json!(true)] // verbose=true
        )?;

        Ok(result)
    }

    pub async fn get_block_by_hash(&self, blockhash: &str) -> Result<serde_json::Value> {
        // Get block info by hash
        let result: serde_json::Value = self.rpc.call(
            "getblock",
            &[json!(blockhash)]
        )?;

        Ok(result)
    }

    pub async fn get_current_height(&self) -> Result<u32> {
        // Get current blockchain height
        let info: serde_json::Value = self.rpc.call("getblockchaininfo", &[])?;
        let height = info["blocks"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing blocks in getblockchaininfo"))?;
        Ok(height as u32)
    }

    /// Find the transaction that spent a given outpoint (anchor output[1]).
    ///
    /// # Data Transaction Validity Rule
    ///
    /// A data_tx is only considered valid if it is included in the **same Bitcoin block**
    /// as its corresponding anchor_tx. This rule ensures:
    ///
    /// - **Determinism**: All nodes agree on which data_txs are valid
    /// - **No withheld data attacks**: Publishers cannot hold back a data_tx and publish
    ///   it later to rewrite history
    /// - **Incentive alignment**: Publishers must use package relay (TRUC) to ensure
    ///   anchor_tx and data_tx are mined together, or risk losing fees
    ///
    /// If a data_tx ends up in a different block than its anchor_tx (due to miner behavior,
    /// network issues, etc.), it is ignored. This is the publisher's risk to manage.
    ///
    /// # Strategy
    ///
    /// 1. Check mempool first (for unconfirmed anchor+data pairs)
    /// 2. Get the anchor_tx's block and scan only that block for the data_tx
    /// 3. Return None if data_tx is not in the same block
    ///
    /// Returns (txid, transaction, block_height) where block_height is 0 for mempool txs.
    pub async fn get_spending_tx(&self, outpoint: &OutPoint) -> Result<Option<(Txid, Transaction, u32)>> {
        // Check if output is still unspent
        let gettxout_result: Result<serde_json::Value, _> = self.rpc.call(
            "gettxout",
            &[json!(outpoint.txid.to_string()), json!(outpoint.vout), json!(true)],
        );
        if gettxout_result.is_ok() && !gettxout_result.as_ref().unwrap().is_null() {
            return Ok(None); // Not spent yet
        }

        // The outpoint is anchor_tx:1 (P2A anchor output).
        // The anchor_tx itself is in the wallet (it sends to subchain address).
        // Get the anchor_tx to find which block it's in.
        let anchor_txid = outpoint.txid.to_string();

        let anchor_tx_data: serde_json::Value = match self.rpc.call(
            "getrawtransaction",
            &[json!(&anchor_txid), json!(true)],
        ) {
            Ok(data) => data,
            Err(_) => return Ok(None),
        };

        // Check mempool first - if anchor_tx is unconfirmed, scan mempool for data_tx
        if anchor_tx_data.get("blockhash").is_none() {
            return self.find_spending_tx_in_mempool(outpoint).await;
        }

        // Anchor is confirmed - get its block and scan for the data_tx
        let blockhash = anchor_tx_data.get("blockhash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing blockhash"))?;

        let block_data: serde_json::Value = self.rpc.call(
            "getblock",
            &[json!(blockhash), json!(2)], // verbosity 2 = full tx data
        ).context("Failed to get block")?;

        let height = block_data.get("height")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // Scan this block for the tx that spends our outpoint
        if let Some(txs) = block_data.get("tx").and_then(|v| v.as_array()) {
            for tx_obj in txs {
                if self.tx_spends_outpoint(tx_obj, outpoint) {
                    let spending_txid = tx_obj.get("txid")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| anyhow!("Missing txid"))?
                        .parse::<Txid>()
                        .context("Failed to parse txid")?;

                    let spending_tx = self.parse_tx_from_json(tx_obj)
                        .context("Failed to parse transaction")?;

                    return Ok(Some((spending_txid, spending_tx, height)));
                }
            }
        }

        Ok(None)
    }

    /// Search mempool for a transaction spending the given outpoint
    async fn find_spending_tx_in_mempool(&self, outpoint: &OutPoint) -> Result<Option<(Txid, Transaction, u32)>> {
        let mempool: serde_json::Value = match self.rpc.call("getrawmempool", &[json!(false)]) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };

        let Some(txids) = mempool.as_array() else {
            return Ok(None);
        };

        for txid_val in txids {
            let Some(txid_str) = txid_val.as_str() else {
                continue;
            };

            let tx_data: serde_json::Value = match self.rpc.call(
                "getrawtransaction",
                &[json!(txid_str), json!(true)],
            ) {
                Ok(data) => data,
                Err(_) => continue,
            };

            if self.tx_spends_outpoint(&tx_data, outpoint) {
                let spending_txid = txid_str.parse::<Txid>()
                    .context("Failed to parse txid")?;

                let spending_tx = self.parse_tx_from_json(&tx_data)
                    .context("Failed to parse transaction")?;

                return Ok(Some((spending_txid, spending_tx, 0))); // height 0 = mempool
            }
        }

        Ok(None)
    }

    /// Check if a transaction spends a specific outpoint
    fn tx_spends_outpoint(&self, tx_data: &serde_json::Value, outpoint: &OutPoint) -> bool {
        let Some(inputs) = tx_data.get("vin").and_then(|v| v.as_array()) else {
            return false;
        };

        let outpoint_txid_str = outpoint.txid.to_string();

        for input in inputs {
            let input_txid = input.get("txid").and_then(|v| v.as_str());
            let input_vout = input.get("vout").and_then(|v| v.as_u64());

            if let (Some(txid_str), Some(vout)) = (input_txid, input_vout) {
                if txid_str == outpoint_txid_str && vout == outpoint.vout as u64 {
                    return true;
                }
            }
        }

        false
    }

    /// Parse a Bitcoin transaction from JSON RPC response
    fn parse_tx_from_json(&self, tx_data: &serde_json::Value) -> Result<Transaction> {
        let hex_str = tx_data.get("hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing hex field in transaction"))?;

        let bytes = hex::decode(hex_str)
            .context("Failed to decode transaction hex")?;

        bitcoin::consensus::deserialize(&bytes)
            .context("Failed to deserialize transaction")
    }
}
