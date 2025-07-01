use coins_spacechain::Spacechain;
use bitcoin::{Amount, OutPoint};

#[test]
fn generate_chain_len() {
    let first_out = OutPoint::null();
    let (sc, _sk) = Spacechain::generate(5, first_out, Amount::from_sat(546), bitcoin::Network::Regtest);
    assert_eq!(sc.sigs.len(), 5);

    let blob = sc.encode();
    let sc2 = Spacechain::decode(&blob).expect("decode");
    assert_eq!(sc2.sigs.len(), 5);

    let txs = sc2.reconstruct_txs();
    assert_eq!(txs.len(), 5);
} 