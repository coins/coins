//! Persistent key-value store abstraction on top of sled.

use coins_types::{Account, AccountId};
use coins_crypto::G1;
use sled::{Db, Tree};
use thiserror::Error;
use bincode::serde::{encode_to_vec as bincode_serialize, decode_from_slice as bincode_deserialize};
use bincode::config::{standard, Config};
use bincode::error::{EncodeError, DecodeError};

const TREE_ACCOUNTS: &str = "accounts";     // id  -> Account (bincode)
const TREE_PK_INDEX: &str = "pk_index";     // pk_bytes -> id (u32 LE)
const KEY_LAST_ID: &[u8] = b"last_id";      // stored in accounts tree

/// Size of AccountId in bytes (u32)
const ACCOUNT_ID_SIZE: usize = std::mem::size_of::<u32>();

#[derive(Debug, Error)]
pub enum StateError {
    #[error("database error: {0}")]
    Db(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Codec(Box<dyn std::error::Error + Send + Sync>),
    #[error("account not found")]
    NotFound,
}

impl From<EncodeError> for StateError {
    fn from(err: EncodeError) -> Self {
        StateError::Codec(Box::new(err))
    }
}

impl From<DecodeError> for StateError {
    fn from(err: DecodeError) -> Self {
        StateError::Codec(Box::new(err))
    }
}

pub struct State {
    /// Database handle (kept alive to maintain connection)
    #[allow(dead_code)]
    db: Db,
    accounts: Tree,
    pk_index: Tree,
}

impl State {
    /// Open (or create) a persistent state database at `path`.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, StateError> {
        let db = sled::open(path)?;
        let accounts = db.open_tree(TREE_ACCOUNTS)?;
        let pk_index = db.open_tree(TREE_PK_INDEX)?;
        Ok(Self { db, accounts, pk_index })
    }

    /// Generate the next AccountId atomically.
    fn next_id(&self) -> Result<AccountId, StateError> {
        let id = match self.accounts.get(KEY_LAST_ID)? {
            Some(bytes) => {
                let mut arr = [0u8; ACCOUNT_ID_SIZE];
                arr.copy_from_slice(&bytes);
                u32::from_le_bytes(arr) + 1
            },
            None => 0,
        };
        self.accounts.insert(KEY_LAST_ID, &id.to_le_bytes())?;
        Ok(AccountId(id))
    }

    /// Fetch account by id.
    pub fn get_account(&self, id: AccountId) -> Result<Option<Account>, StateError> {
        if let Some(bytes) = self.accounts.get(id.0.to_le_bytes())? {
            let (acct, _): (Account, usize) = bincode_deserialize(&bytes, bin_config())?;
            Ok(Some(acct))
        } else { Ok(None) }
    }

    /// Fetch account by public key.
    pub fn get_by_pk(&self, pk: &G1) -> Result<Option<Account>, StateError> {
        if let Some(id_bytes) = self.pk_index.get(pk.0)? {
            let mut arr = [0u8; ACCOUNT_ID_SIZE];
            arr.copy_from_slice(&id_bytes);
            self.get_account(AccountId(u32::from_le_bytes(arr)))
        } else { Ok(None) }
    }

    /// Create new account with given pk, zero balance.
    pub fn create_account(&self, pk: G1) -> Result<Account, StateError> {
        if self.pk_index.contains_key(pk.0)? {
            return Err(StateError::Db(sled::Error::Unsupported("pk already exists".into())));
        }
        let id = self.next_id()?;
        let acct = Account { id, pk, balance: 0, nonce: 0 };
        self.insert_account(&acct)?;
        Ok(acct)
    }

    /// Insert or overwrite an account (internal helper).
    pub fn insert_account(&self, acct: &Account) -> Result<(), StateError> {
        let id_key = acct.id.0.to_le_bytes();
        let bytes: Vec<u8> = bincode_serialize(acct, bin_config())?;
        self.accounts.insert(id_key.to_vec(), bytes)?;
        self.pk_index.insert(acct.pk.0, id_key.to_vec())?;
        Ok(())
    }

    /// Atomically apply a list of updated accounts.
    pub fn apply_batch(&self, updated: &[Account]) -> Result<(), StateError> {
        let mut batch = sled::Batch::default();
        for acct in updated {
            let bytes: Vec<u8> = bincode_serialize(acct, bin_config())?;
            batch.insert(acct.id.0.to_le_bytes().to_vec(), bytes);
        }
        self.accounts.apply_batch(batch)?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// SubBlockState trait implementation
// -----------------------------------------------------------------------------

impl coins_types::SubBlockState for State {
    fn get_account_id_by_pk(&self, pk: &G1) -> Result<Option<u32>, ()> {
        match self.get_by_pk(pk) {
            Ok(Some(account)) => Ok(Some(account.id.0)),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    fn get_pk_by_account_id(&self, id: u32) -> Option<G1> {
        self.get_account(AccountId(id))
            .ok()?
            .map(|account| account.pk)
    }
}

// -----------------------------------------------------------------------------
// Helper configuration
// -----------------------------------------------------------------------------

fn bin_config() -> impl Config {
    standard().with_fixed_int_encoding().with_little_endian()
} 