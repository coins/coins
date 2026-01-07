use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use crate::config::CacheConfig;
use crate::models::{BlockDetail, AccountDetail};

pub struct Cache {
    blocks: Arc<RwLock<HashMap<u32, CachedBlock>>>,
    accounts: Arc<RwLock<HashMap<String, CachedAccount>>>,
    config: CacheConfig,
}

struct CachedBlock {
    data: BlockDetail,
    #[allow(dead_code)]
    cached_at: Instant,
}

struct CachedAccount {
    data: AccountDetail,
    cached_at: Instant,
}

impl Cache {
    pub fn new(config: CacheConfig) -> Self {
        Self {
            blocks: Arc::new(RwLock::new(HashMap::new())),
            accounts: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn get_block(&self, height: u32) -> Option<BlockDetail> {
        if !self.config.enabled {
            return None;
        }

        let cache = self.blocks.read().await;
        cache.get(&height).map(|cached| cached.data.clone())
    }

    pub async fn put_block(&self, height: u32, block: BlockDetail) {
        if !self.config.enabled {
            return;
        }

        let mut cache = self.blocks.write().await;

        // Simple size-based eviction (keep only recent blocks)
        if cache.len() >= self.config.block_cache_size {
            // Remove oldest entries
            if let Some(min_key) = cache.keys().min().copied() {
                cache.remove(&min_key);
            }
        }

        cache.insert(height, CachedBlock {
            data: block,
            cached_at: Instant::now(),
        });
    }

    pub async fn get_account(&self, pk: &str) -> Option<AccountDetail> {
        if !self.config.enabled {
            return None;
        }

        let cache = self.accounts.read().await;
        if let Some(cached) = cache.get(pk) {
            // Check TTL
            if cached.cached_at.elapsed() < Duration::from_secs(self.config.account_cache_ttl_seconds) {
                return Some(cached.data.clone());
            }
        }
        None
    }

    pub async fn put_account(&self, pk: String, account: AccountDetail) {
        if !self.config.enabled {
            return;
        }

        let mut cache = self.accounts.write().await;
        cache.insert(pk, CachedAccount {
            data: account,
            cached_at: Instant::now(),
        });
    }

    pub async fn invalidate_account(&self, pk: &str) {
        let mut cache = self.accounts.write().await;
        cache.remove(pk);
    }
}
