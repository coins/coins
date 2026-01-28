//! Block validator (skeleton)
//!
//! Implements the state transition function as described in §6 of the spec.

use coins_crypto as crypto;
use coins_types::{SubBlock, Account, AccountId, NATIVE_TOKEN_ID};
use crate::state::State;
use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum ValidationError {
    #[error("account not found for sender id {0}")]
    UnknownSender(u32),
    #[error("insufficient balance")]
    Balance,
    #[error("signature mismatch")]
    BadSignature,
    #[error("nonce mismatch")]
    Nonce,
    #[error("database error")]
    Db,
    #[error("balance overflow")]
    Overflow,
}

/// Validate a SubBlock and mutate the provided state atomically.
/// Returns the nonces used by each transaction (parallel array to sb.txs).
pub fn validate_subblock(sb: &SubBlock, state: &State) -> Result<Vec<u32>, ValidationError> {
    // staging area: updated accounts
    let mut updates: HashMap<u32, Account> = HashMap::new();
    let mut fee_total: u32 = 0;

    // collect pk/msg pairs for BLS check
    let mut pairs = Vec::with_capacity(sb.txs.len());
    // collect nonces for each transaction (the nonce used for signing, before increment)
    let mut nonces = Vec::with_capacity(sb.txs.len());

    for tx in &sb.txs {
        // fetch sender account - check updates first to handle multiple txs from same sender
        let mut acct = match updates.get(&tx.sender_id) {
            Some(a) => a.clone(),
            None => state
                .get_account(AccountId(tx.sender_id))
                .map_err(|_| ValidationError::UnknownSender(tx.sender_id))?
                .ok_or(ValidationError::UnknownSender(tx.sender_id))?,
        };

        let spend = tx.amount as u64 + tx.fee as u64;
        if acct.balance(tx.token_id) < spend {
            return Err(ValidationError::Balance);
        }

        // Record the nonce used for signing (before increment)
        let signing_nonce = acct.nonce;
        nonces.push(signing_nonce);

        // build message and collect for signature check
        let msg = tx.message_to_sign(signing_nonce);
        pairs.push((acct.pk, msg));

        // stage account updates
        let old_balance = acct.balance(tx.token_id);
        acct.balances.insert(tx.token_id, old_balance - spend);
        acct.nonce += 1;
        updates.insert(acct.id.0, acct);

        // credit recipient
        let recv_from_state = match state.get_by_pk(&tx.recipient_pk).map_err(|_| ValidationError::Db)? {
            Some(a) => a,
            None => state.create_account(tx.recipient_pk).map_err(|_| ValidationError::Db)?
        };

        // Check if recipient was already updated in this batch
        let mut recv = updates.get(&recv_from_state.id.0).cloned().unwrap_or(recv_from_state);

        let recv_old_balance = recv.balance(tx.token_id);
        recv.balances.insert(tx.token_id, recv_old_balance.checked_add(tx.amount as u64).ok_or(ValidationError::Overflow)?);
        updates.insert(recv.id.0, recv);

        fee_total = fee_total.checked_add(tx.fee as u32).ok_or(ValidationError::Overflow)?;
    }

    // verify aggregate signature
    let pair_refs = pairs
        .iter()
        .map(|(pk, m)| (pk, m.as_slice()));
    if !crypto::verify_aggregate(pair_refs, &sb.sigma) {
        return Err(ValidationError::BadSignature);
    }

    // credit publisher with total fees
    let mut pub_acct = match state.get_by_pk(&sb.publisher_pk).map_err(|_| ValidationError::UnknownSender(0))? {
        Some(a) => a,
        None => {
            // create new account for publisher
            state.create_account(sb.publisher_pk).map_err(|_| ValidationError::Db)?
        }
    };
    let pub_old_balance = pub_acct.balance(NATIVE_TOKEN_ID);
    pub_acct.balances.insert(NATIVE_TOKEN_ID, pub_old_balance.checked_add(fee_total as u64).ok_or(ValidationError::Overflow)?);
    updates.insert(pub_acct.id.0, pub_acct);

    // write batch
    let updated: Vec<Account> = updates.into_values().collect();
    state.apply_batch(&updated).map_err(|_| ValidationError::Db)?;

    Ok(nonces)
} 