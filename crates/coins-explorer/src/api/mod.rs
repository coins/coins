pub mod blocks;
pub mod accounts;
pub mod transactions;
pub mod stats;
pub mod websocket;

use axum::{
    Router,
    routing::get,
};
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use crate::indexer::ExplorerIndexer;

#[derive(Clone)]
pub struct AppState {
    pub indexer: Arc<ExplorerIndexer>,
    pub ws_state: Arc<websocket::WebSocketState>,
}

pub fn create_router(indexer: Arc<ExplorerIndexer>) -> Router {
    let ws_state = Arc::new(websocket::WebSocketState::new());

    let app_state = AppState {
        indexer,
        ws_state,
    };

    // API routes
    let api_routes = Router::new()
        .route("/blocks", get(blocks::list_blocks))
        .route("/blocks/latest", get(blocks::latest_block))
        .route("/blocks/:height", get(blocks::get_block))
        .route("/accounts/:pk", get(accounts::get_account))
        .route("/accounts/:pk/transactions", get(accounts::get_account_transactions))
        .route("/transactions", get(transactions::search_transactions))
        .route("/stats", get(stats::get_stats))
        .with_state(app_state.clone());

    // WebSocket route
    let ws_route = Router::new()
        .route("/ws", get(websocket::websocket_handler))
        .with_state(app_state);

    // Static file serving
    let static_files = ServeDir::new("crates/coins-explorer/src/web/static");

    // Combine everything
    Router::new()
        .nest("/api/v1", api_routes)
        .merge(ws_route)
        .fallback_service(static_files)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
}
