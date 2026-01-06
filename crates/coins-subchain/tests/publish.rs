// Integration tests for publish.rs helpers

use coins_subchain::publish::{publish_blob, parse_blob_from_publish};
use bitcoin::{Amount, Network, OutPoint, ScriptBuf, Transaction, TxOut};
use bitcoin::secp256k1::{Secp256k1, SecretKey, schnorr::Signature, Message, XOnlyPublicKey};
use bitcoin::key::Keypair;
use bitcoin::sighash::{SighashCache, Prevouts, TapSighashType, Annex};
use bitcoin::Address;

/// Helper: build a dummy connector transaction with at least two outputs so
/// that output index 1 can serve as the anchor in `publish_blob`.
fn dummy_connector_tx() -> Transaction {
    Transaction {
        version: bitcoin::transaction::Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![],
        output: vec![
            // arbitrary output 0
            TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
            // zero-sat anchor output at index 1
            TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
        ],
    }
}

#[test]
fn publish_roundtrip_parse() {
    let blob = b"hello taproot annex".to_vec();
    let connector_tx = dummy_connector_tx();

    // Fixed fee key so test is deterministic.
    let fee_sk = SecretKey::from_slice(&[1u8; 32]).unwrap();
    let fee_outpoint = OutPoint::null(); // dummy, not validated by logic
    let fee_value_sat = 10_000;
    let fee_rate_sat_per_vb = 2;

    let (_anchor_out, publish_tx) = publish_blob(
        &blob,
        &connector_tx,
        fee_outpoint,
        fee_value_sat,
        &fee_sk,
        fee_rate_sat_per_vb,
        Network::Regtest,
    ).expect("publish_blob");

    // parse back and compare
    let parsed = parse_blob_from_publish(&publish_tx).expect("parse_blob_from_publish");
    assert_eq!(parsed, blob);
}

#[test]
fn fee_signature_verifies() {
    let blob = b"verification test".to_vec();
    let connector_tx = dummy_connector_tx();

    let fee_sk = SecretKey::from_slice(&[2u8; 32]).unwrap();
    let fee_outpoint = OutPoint::null();
    let fee_value_sat = 20_000;

    let (_anchor_out, publish_tx) = publish_blob(
        &blob,
        &connector_tx,
        fee_outpoint,
        fee_value_sat,
        &fee_sk,
        1, // sat/vB
        Network::Regtest,
    ).expect("publish_blob");

    // Extract witness data
    let wit = &publish_tx.input[1].witness;
    assert_eq!(wit.len(), 2, "witness stack should be <sig> <annex>");
    let sig_bytes = &wit[0];
    assert_eq!(sig_bytes.len(), 64, "schnorr signature length");
    let annex_bytes = &wit[1];

    // Derive fee scriptPubKey again
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, &fee_sk);
    let (xonly_pk, _parity) = XOnlyPublicKey::from_keypair(&keypair);
    let fee_spk = Address::p2tr(&secp, xonly_pk, None, Network::Regtest).script_pubkey();

    // Prepare prevouts for sighash: anchor + fee
    let anchor_prevout = connector_tx.output[1].clone(); // zero-sat anchor
    let prevouts_arr = [
        anchor_prevout,
        TxOut { value: Amount::from_sat(fee_value_sat), script_pubkey: fee_spk.clone() },
    ];
    let prevouts = Prevouts::All(&prevouts_arr);

    // Compute sighash including annex
    let mut tx_clone = publish_tx.clone();
    let mut cache = SighashCache::new(&mut tx_clone);
    let annex = Annex::new(annex_bytes).expect("valid annex");
    let sighash = cache.taproot_signature_hash(1, &prevouts, Some(annex), None, TapSighashType::Default)
        .expect("sighash");
    let msg = Message::from_digest_slice(&sighash[..]).unwrap();

    let sig = Signature::from_slice(sig_bytes).expect("schnorr signature");
    secp.verify_schnorr(&sig, &msg, &xonly_pk).expect("signature verification");
} 