use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    pub database: DatabaseConfig,
    pub bitcoin: BitcoinConfig,
    pub server: ServerConfig,
    pub genesis: GenesisConfig,
    pub subchain: PathBuf,
    pub wallet_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub state_db: PathBuf,
    pub indexer_db: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinConfig {
    pub rpc_url: String,
    pub rpc_user: String,
    pub rpc_pass: String,
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub api_port: u16,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub genesis_pk: String,
    pub genesis_balance: u64,
}

impl IndexerConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: IndexerConfig = toml::from_str(&contents)?;
        Ok(config)
    }
}
