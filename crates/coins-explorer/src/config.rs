use serde::Deserialize;
use std::path::PathBuf;
use bitcoin::Network;

#[derive(Debug, Deserialize, Clone)]
pub struct ExplorerConfig {
    #[serde(default)]
    pub indexer: IndexerConfig,
    pub database: DatabaseConfig,
    pub bitcoin: BitcoinConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub updates: UpdatesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IndexerConfig {
    #[serde(default = "default_indexer_url")]
    pub url: String,
    #[serde(default)]
    pub publisher_url: Option<String>,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            url: default_indexer_url(),
            publisher_url: None,
        }
    }
}

fn default_indexer_url() -> String {
    "http://localhost:8083".to_string()
}

impl ExplorerConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ExplorerConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub tx_index_db: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BitcoinConfig {
    pub rpc_url: String,
    pub rpc_user: String,
    pub rpc_pass: String,
    #[serde(with = "network_serde")]
    pub network: Network,
}

mod network_serde {
    use bitcoin::Network;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Network, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "bitcoin" | "mainnet" => Ok(Network::Bitcoin),
            "testnet" => Ok(Network::Testnet),
            "signet" => Ok(Network::Signet),
            "regtest" => Ok(Network::Regtest),
            _ => Err(serde::de::Error::custom(format!("Invalid network: {}", s))),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

fn default_api_port() -> u16 {
    3000
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    #[serde(default = "default_block_cache_size")]
    pub block_cache_size: usize,
    #[serde(default = "default_account_cache_ttl")]
    pub account_cache_ttl_seconds: u64,
    #[serde(default = "default_bitcoin_cache_ttl")]
    pub bitcoin_cache_ttl_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_cache_enabled(),
            block_cache_size: default_block_cache_size(),
            account_cache_ttl_seconds: default_account_cache_ttl(),
            bitcoin_cache_ttl_seconds: default_bitcoin_cache_ttl(),
        }
    }
}

fn default_cache_enabled() -> bool {
    true
}

fn default_block_cache_size() -> usize {
    1000
}

fn default_account_cache_ttl() -> u64 {
    60
}

fn default_bitcoin_cache_ttl() -> u64 {
    10
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpdatesConfig {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_seconds: u64,
    #[serde(default = "default_index_batch_size")]
    pub index_batch_size: usize,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            poll_interval_seconds: default_poll_interval(),
            index_batch_size: default_index_batch_size(),
        }
    }
}

fn default_poll_interval() -> u64 {
    5
}

fn default_index_batch_size() -> usize {
    100
}
