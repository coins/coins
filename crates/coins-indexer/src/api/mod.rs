mod routes;

pub use routes::create_router;

use std::sync::Arc;
use coins_core::State;
use crate::Indexer;
use bitcoincore_rpc::Auth;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub state: Arc<State>,
    pub indexer: Arc<Indexer>,
    pub rpc_url: String,
    pub rpc_auth: Auth,
}

impl AppState {
    pub fn new(state: Arc<State>, indexer: Arc<Indexer>, rpc_url: String, rpc_auth: Auth) -> Self {
        Self { state, indexer, rpc_url, rpc_auth }
    }
}
