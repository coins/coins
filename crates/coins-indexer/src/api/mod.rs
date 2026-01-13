mod routes;

pub use routes::create_router;

use std::sync::Arc;
use coins_core::State;
use crate::Indexer;
use coins_bitcoin_rpc::RpcBackend;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub state: Arc<State>,
    pub indexer: Arc<Indexer>,
    pub rpc_backend: Arc<RpcBackend>,
    pub network: String,
}

impl AppState {
    pub fn new(state: Arc<State>, indexer: Arc<Indexer>, rpc_backend: Arc<RpcBackend>, network: String) -> Self {
        Self { state, indexer, rpc_backend, network }
    }
}
