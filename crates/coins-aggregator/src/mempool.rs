use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use coins_types::{Account, AccountId, Transaction};
use coins_state::State;

#[derive(thiserror::Error, Debug)]
pub enum MempoolError {
    #[error("sender not found")]
    UnknownSender,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("nonce mismatch")]
    NonceMismatch,
    #[error("duplicate nonce in mempool")]
    Duplicate,
}

#[derive(Clone)]
pub struct Mempool {
    state: Arc<State>,
    overlay: Arc<RwLock<HashMap<AccountId, Account>>>,
    queue: Arc<RwLock<Vec<Transaction>>>,
}

impl Mempool {
    pub fn new(state: Arc<State>) -> Self {
        Self { state, overlay: Arc::new(RwLock::new(HashMap::new())), queue: Arc::new(RwLock::new(Vec::new())) }
    }

    pub async fn validate_and_enqueue(&self, tx: Transaction) -> Result<(), MempoolError> {
        // resolve sender account
        let sender_id = AccountId(tx.sender_id);
        // get mutable overlay snapshot
        let mut ov = self.overlay.write().await;
        let sender_acc = if let Some(acc) = ov.get_mut(&sender_id) {
            acc
        } else {
            // fetch from persistent state
            if let Some(acc) = self.state.get_account(sender_id).ok().flatten() {
                ov.insert(sender_id, acc.clone());
                ov.get_mut(&sender_id).unwrap()
            } else {
                return Err(MempoolError::UnknownSender);
            }
        };

        // Prevent duplicate transaction already in queue (exact same fields)
        {
            let q = self.queue.read().await;
            if q.iter().any(|p| p.sender_id == tx.sender_id
                && p.recipient_pk == tx.recipient_pk
                && p.amount == tx.amount
                && p.fee == tx.fee)
            {
                return Err(MempoolError::Duplicate);
            }
        }

        if sender_acc.balance < (tx.amount as u64 + tx.fee as u64) {
            return Err(MempoolError::InsufficientBalance);
        }
        // apply
        sender_acc.balance -= tx.amount as u64 + tx.fee as u64;
        sender_acc.nonce += 1;
        // credit recipient overlay lazily (create if needed)
        // Map recipient pk to deterministic but demo-only account id via first 4 bytes.
        let mut tmp=[0u8;4]; tmp.copy_from_slice(&tx.recipient_pk.0[..4]);
        let recipient_id = AccountId(u32::from_le_bytes(tmp));
        let rec_acc = ov.entry(recipient_id).or_insert(Account { id: recipient_id, pk: tx.recipient_pk, balance:0, nonce:0});
        rec_acc.balance += tx.amount as u64;

        drop(ov);
        self.queue.write().await.push(tx);
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn pop_batch(&self, max: usize) -> Vec<Transaction> {
        let mut q = self.queue.write().await;
        let n = max.min(q.len());
        q.drain(0..n).collect()
    }

    pub async fn nonce_of(&self, id: AccountId) -> Option<u32> {
        let ov = self.overlay.read().await;
        ov.get(&id).map(|acc| acc.nonce)
    }

    /// Re-validate all pending transactions against *current* persistent state.
    /// This should be called after a sub-block has been applied so that
    /// executed or now-invalid transactions are removed from the mempool.
    pub async fn refresh(&self) {
        // Drain the old queue
        let old_queue = {
            let mut q = self.queue.write().await;
            let txs = q.clone();
            q.clear();
            txs
        };
        // Clear overlay so we rebuild fresh
        {
            let mut ov = self.overlay.write().await;
            ov.clear();
        }
        // Re-insert transactions in original order, ignoring those that are now invalid
        for tx in old_queue {
            let _ = self.validate_and_enqueue(tx).await; // ignore Err – simply drop invalid ones
        }
    }

    /// Remove all executed transactions (or those that now conflict) and re-validate the rest.
    /// `executed` should list all transactions included in the recently applied sub-block.
    pub async fn apply_subblock(&self, executed: &[Transaction]) {
        // 1. Remove any txs that match `executed` by comparing all fields
        {
            let mut q = self.queue.write().await;
            q.retain(|t| !executed.iter().any(|e| {
                e.sender_id == t.sender_id &&
                e.recipient_pk == t.recipient_pk &&
                e.amount == t.amount &&
                e.fee == t.fee
            }));
        }

        // 2. Re-validate the remaining queue against updated persistent state
        self.refresh().await;
    }
} 