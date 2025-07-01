//! Block validator (skeleton)
//!
//! Implements the state transition function as described in §6 of the spec.

use coins_crypto as crypto;
use coins_state::{State, StateError};
use coins_types::{SubBlock, Account, AccountId};
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
pub fn validate_subblock(sb: &SubBlock, state: &State) -> Result<(), ValidationError> {
    // staging area: updated accounts
    let mut updates: HashMap<u32, Account> = HashMap::new();
    let mut fee_total: u32 = 0;

    // collect pk/msg pairs for BLS check
    let mut pairs = Vec::with_capacity(sb.txs.len());

    for tx in &sb.txs {
        // fetch sender account
        let mut acct = state
            .get_account(AccountId(tx.sender_id))
            .map_err(|_| ValidationError::UnknownSender(tx.sender_id))?
            .ok_or(ValidationError::UnknownSender(tx.sender_id))?;

        let spend = tx.amount as u64 + tx.fee as u64;
        if acct.balance < spend {
            return Err(ValidationError::Balance);
        }

        // build message and collect for signature check
        let msg = tx.message_to_sign(acct.nonce);
        pairs.push((acct.pk, msg));

        // stage account updates
        acct.balance -= spend;
        acct.nonce += 1;
        updates.insert(acct.id.0, acct);

        // credit recipient
        let mut recv = match state.get_by_pk(&tx.recipient_pk).map_err(|_| ValidationError::Db)? {
            Some(a) => a,
            None => state.create_account(tx.recipient_pk).map_err(|_| ValidationError::Db)?
        };
        recv.balance = recv.balance.checked_add(tx.amount as u64).ok_or(ValidationError::Overflow)?;
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

    // credit aggregator with total fees
    let mut agg_acct = match state.get_by_pk(&sb.aggregator_pk).map_err(|_| ValidationError::UnknownSender(0))? {
        Some(a) => a,
        None => {
            // create new account for aggregator
            state.create_account(sb.aggregator_pk).map_err(|_| ValidationError::Db)?
        }
    };
    agg_acct.balance = agg_acct.balance.checked_add(fee_total as u64).ok_or(ValidationError::Overflow)?;
    updates.insert(agg_acct.id.0, agg_acct);

    // write batch
    let updated: Vec<Account> = updates.into_values().collect();
    state.apply_batch(&updated).map_err(|_| ValidationError::Db)?;

    Ok(())
} 