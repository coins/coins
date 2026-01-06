use coins_state::State;
use coins_crypto::{SecretKey, G1};
use tempfile::tempdir;

#[test]
fn persistent_roundtrip() {
    let dir = tempdir().unwrap();
    let state = State::open(dir.path()).unwrap();

    // create account
    let sk = SecretKey::random();
    let acct = state.create_account(G1(sk.public_key())).unwrap();
    assert_eq!(acct.balance, 0);

    // reopen DB
    drop(state);
    let state2 = State::open(dir.path()).unwrap();
    let loaded = state2.get_account(acct.id).unwrap().unwrap();
    assert_eq!(loaded.pk.0, acct.pk.0);
} 