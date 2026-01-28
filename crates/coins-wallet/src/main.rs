//! Coins Wallet Server
//!
//! A web-based wallet service for the Coins protocol.

use anyhow::Result;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Coins Wallet Server starting...");

    // TODO: Implement wallet server in future stories

    Ok(())
}
