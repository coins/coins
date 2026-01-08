mod routes;

pub use routes::create_router;

use std::sync::Arc;
use coins_core::State;
use crate::Indexer;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub state: Arc<State>,
    pub indexer: Arc<Indexer>,
}

impl AppState {
    pub fn new(state: Arc<State>, indexer: Arc<Indexer>) -> Self {
        Self { state, indexer }
    }
}
