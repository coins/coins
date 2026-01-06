use bitcoin::transaction::Version;
use bitcoin::key::rand::rngs::OsRng;
use coins_subchain::inscribe::{inscribe_blob, parse_blob_from_reveal};
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
use bitcoin::Network;
use bitcoin::blockdata::script::Builder;
use bitcoin::opcodes::OP_TRUE;
use bitcoin::secp256k1::SecretKey;

fn dummy_connector() -> Transaction {
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
fn roundtrip_inscription() {
    let blob: Vec<u8> = (0..1500).map(|i| (i % 256) as u8).collect();
    let connector_tx = dummy_connector();

    // dummy fee UTXO
    let fee_outpoint = OutPoint::null();
    let fee_value = 10_000u64;
    let fee_sk = SecretKey::new(&mut OsRng);
    let fee_rate = 5u64; // sat/vbyte

    let (_anchor_out, commit_tx, reveal_tx) = inscribe_blob(
        &blob,
        &connector_tx,
        fee_outpoint,
        fee_value,
        &fee_sk,
        fee_rate,
        Network::Regtest,
    ).expect("inscribe ok");

    // ensure reveal spends commit
    assert_eq!(reveal_tx.input[0].previous_output.txid, commit_tx.txid());

    let parsed = parse_blob_from_reveal(&reveal_tx).expect("parse blob");
    assert_eq!(parsed, blob);
} 