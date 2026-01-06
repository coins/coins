use bitcoin::transaction::Version;
use bitcoin::{Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
use bitcoin::blockdata::script::Builder;
use bitcoin::opcodes::OP_TRUE;
use coins_types::{SubBlock, Transaction as CoinsTx};
use coins_crypto::{SecretKey as CoinsSecret, G1, G2};
use coins_aggregator::inscription::{inscribe_subblock, parse_subblock_from_reveal};
use rand::rngs::OsRng;

fn dummy_connector(value_sat: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence(0xffffffff),
            witness: bitcoin::Witness::default(),
        }],
        output: vec![
            TxOut { value: Amount::from_sat(546), script_pubkey: ScriptBuf::new() },
            TxOut { value: Amount::ZERO, script_pubkey: Builder::new().push_opcode(OP_TRUE).into_script() },
        ],
    }
}

#[test]
fn subblock_roundtrip() {
    // Construct dummy SubBlock with one tx
    let sigma = G2([0u8; 64]);
    let agg_pk = G1(CoinsSecret::random().public_key());
    let tx = CoinsTx { sender_id: 1, recipient_pk: G1(CoinsSecret::random().public_key()), amount: 42, fee: 1 };
    let sb = SubBlock { sigma, aggregator_pk: agg_pk, txs: vec![tx] };

    // connector tx and fee utxo
    let connector_tx = dummy_connector(0);
    let fee_outpoint = OutPoint::null();
    let fee_value_sat = 10_000;
    let fee_sk = bitcoin::secp256k1::SecretKey::new(&mut OsRng);
    let fee_rate = 5;

    let res = inscribe_subblock(&sb, &connector_tx, fee_outpoint, fee_value_sat, &fee_sk, fee_rate, Network::Regtest);
    assert!(res.is_ok());
    let (_anchor, _commit, reveal) = res.unwrap();

    let parsed = parse_subblock_from_reveal(&reveal).expect("parse");
    assert_eq!(parsed.txs.len(), 1);
} 