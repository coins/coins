use coins_types::SubBlockState;
use coins_crypto::G1;
use coins_indexer::IndexerClient;
use std::sync::Arc;

/// Adapter that implements SubBlockState trait using IndexerClient
/// This allows the publisher to validate and serialize sub-blocks
/// without direct database access
pub struct StateAdapter {
    client: Arc<IndexerClient>,
}

impl StateAdapter {
    pub fn new(client: Arc<IndexerClient>) -> Self {
        Self {
            client,
        }
    }
}

impl SubBlockState for StateAdapter {
    fn get_account_id_by_pk(&self, pk: &G1) -> Result<Option<u32>, ()> {
        // Use block_in_place to safely block within an async context
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match self.client.get_account(pk).await {
                    Ok(Some(account)) => Ok(Some(account.id.0)),
                    Ok(None) => Ok(None),
                    Err(_) => Err(()),
                }
            })
        })
    }

    fn get_pk_by_account_id(&self, id: u32) -> Option<G1> {
        // Required for deserializing compact transactions during Initial Block Download (IBD).
        // Not yet implemented: would need a new indexer API endpoint like GET /accounts/by-id/:id
        // that returns the public key for a given account ID.
        tracing::warn!(account_id = id, "get_pk_by_account_id not yet supported in StateAdapter");
        None
    }
}
