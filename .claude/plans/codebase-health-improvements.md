# Codebase Health Improvements Plan

## Priority 1: Code Quality Issues
- [ ] Review and remove 3 `#[allow(dead_code)]` annotations
- [ ] Refactor `get_spending_tx` in `coins-bitcoin-rpc/backend.rs` to reduce complexity

## Priority 2: Documentation Alignment
- [ ] Update README to reflect hybrid transaction format (41-byte standard + 13-byte compact)

## Priority 3: Silent Failure Points
- [ ] Audit 108 unwrap/expect calls (52 non-test)
- [ ] Add proper error handling to indexer deserialization paths
- [ ] Add proper error handling to publisher API server startup

## Priority 4: Code Duplication
- [ ] Extract shared state transition logic (duplicated in 2 places)
- [ ] Create common PK parsing utility (duplicated in 3 places)
- [ ] Consolidate bincode config (duplicated in 3 places)

## Priority 5: Security Concerns
- [ ] Replace `std::sync::Mutex` with `tokio::sync::Mutex` in async contexts
- [ ] Review and address README security warnings

## Priority 6: Test Coverage
- [ ] Add unit tests for `coins-core/validator.rs`
- [ ] Add unit tests for `coins-core/state.rs`
- [ ] Add unit tests for `coins-indexer` crate
- [ ] Add unit tests for `coins-bitcoin-rpc` crate
- [ ] Add unit tests for `coins-publisher/engine.rs`
- [ ] Add unit tests for `coins-client` crate
