//! Blockchain backend abstraction
//!
//! Provides a unified interface for querying blockchain state and broadcasting transactions.
//! Supports multiple backends: Bitcoin RPC (for regtest) and Esplora (for public networks).

use anyhow::Result;
use async_trait::async_trait;
use bitcoin::{Address, Amount, OutPoint, Transaction, Txid};

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

/// Trait for blockchain backends
///
/// Implementations must support:
/// - Querying UTXOs for addresses
/// - Checking if outputs are spent
/// - Broadcasting transactions
#[async_trait]
pub trait BlockchainBackend: Send + Sync {
    /// Get all UTXOs for an address
    async fn get_address_utxos(&self, address: &Address) -> Result<Vec<Utxo>>;

    /// Check if a specific output is spent
    ///
    /// Returns None if the output doesn't exist or is already spent,
    /// Some(OutputStatus) if it's still unspent
    async fn get_output_status(&self, txid: &Txid, vout: u32) -> Result<Option<OutputStatus>>;

    /// Broadcast a single transaction
    async fn broadcast(&self, tx: &Transaction) -> Result<()>;

    /// Broadcast multiple transactions as a package
    ///
    /// Default implementation broadcasts individually.
    /// Backends may override to use package relay if available.
    async fn broadcast_package(&self, txs: &[Transaction]) -> Result<()> {
        // Default: broadcast individually
        for tx in txs {
            self.broadcast(tx).await?;
        }
        Ok(())
    }
}
