//! Core protocol implementation for Coins
//!
//! This crate contains state management and validation logic.
//!
//! ## Modules
//! - `state`: Persistent state management (account database)
//! - `validator`: Sub-block validation and state transition logic

pub mod state;
pub mod validator;

// Re-export commonly used items
pub use state::{State, StateError};
pub use validator::{validate_subblock, ValidationError};
