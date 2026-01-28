use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

use coins_types::{Account, AccountId, Transaction, NATIVE_TOKEN_ID};
use coins_core::State;

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

        // Check balance based on token type
        // For native tokens: check native_balance >= amount + fee
        // For non-native tokens: check token_balance >= amount AND native_balance >= fee
        if tx.token_id == NATIVE_TOKEN_ID {
            if sender_acc.native_balance() < (tx.amount as u64 + tx.fee as u64) {
                return Err(MempoolError::InsufficientBalance);
            }
        } else {
            if sender_acc.balance(tx.token_id) < tx.amount as u64 {
                return Err(MempoolError::InsufficientBalance);
            }
            if sender_acc.native_balance() < tx.fee as u64 {
                return Err(MempoolError::InsufficientBalance);
            }
        }
        // apply deductions
        if tx.token_id == NATIVE_TOKEN_ID {
            let new_balance = sender_acc.native_balance() - tx.amount as u64 - tx.fee as u64;
            sender_acc.balances.insert(NATIVE_TOKEN_ID, new_balance);
        } else {
            // Deduct token amount
            let new_token_balance = sender_acc.balance(tx.token_id) - tx.amount as u64;
            sender_acc.balances.insert(tx.token_id, new_token_balance);
            // Deduct fee from native balance
            let new_native_balance = sender_acc.native_balance() - tx.fee as u64;
            sender_acc.balances.insert(NATIVE_TOKEN_ID, new_native_balance);
        }
        sender_acc.nonce += 1;
        // credit recipient overlay lazily (create if needed)
        // Map recipient pk to deterministic but demo-only account id via first 4 bytes.
        let mut tmp=[0u8;4]; tmp.copy_from_slice(&tx.recipient_pk.0[..4]);
        let recipient_id = AccountId(u32::from_le_bytes(tmp));
        let rec_acc = ov.entry(recipient_id).or_insert(Account { id: recipient_id, pk: tx.recipient_pk, balances: BTreeMap::new(), nonce:0});
        let new_rec_balance = rec_acc.balance(tx.token_id) + tx.amount as u64;
        rec_acc.balances.insert(tx.token_id, new_rec_balance);

        drop(ov);
        self.queue.write().await.push(tx);
        Ok(())
    }

    pub async fn nonce_of(&self, id: AccountId) -> Option<u32> {
        let ov = self.overlay.read().await;
        ov.get(&id).map(|acc| acc.nonce)
    }

    /// Returns the number of transactions in the queue
    pub async fn len(&self) -> usize {
        self.queue.read().await.len()
    }

    /// Returns true if the queue is empty
    pub async fn is_empty(&self) -> bool {
        self.queue.read().await.is_empty()
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