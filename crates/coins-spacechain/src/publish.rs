//! Publish helpers – single-tx variant embedding data in a Taproot annex.
//!
//! This is a simplified alternative to `inscribe.rs` that creates just **one**
//! transaction (`publish_tx`) which
//! 1. spends the zero-sat *anchor* output (index 1) of a anchor transaction
//!    – this input requires no signature;
//! 2. uses a second *fee* input (P2TR key spend) to pay miner fees;
//! 3. embeds an arbitrary `blob` in the **Taproot annex** of the *fee* input
//!    (annex layout: `0x50 0x00 <blob>`).
//!


use bitcoin::absolute::LockTime;
use bitcoin::{ScriptBuf};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{SighashCache, Prevouts, TapSighashType, Annex};
use bitcoin::{
    Address, Amount, Network, OutPoint, Sequence, Transaction, TxIn, TxOut, Witness,
    transaction::Version,
};



const DUST_LIMIT_SAT: u64 = 1000;

/// Build a publish_tx that embeds `blob` in the Taproot annex of the anchor input.
/// `fee_sat` is the absolute fee deducted from the fee-UTXO value.
fn compile_publish_with_fee(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_sat: u64,
) -> Transaction {
    let secp = Secp256k1::new();
    // Build a P2TR (Taproot key spend) address for the fee input
    // We create an x-only public key from the secret key and turn it into a P2TR scriptPubKey.
    use bitcoin::key::{Keypair};
    use bitcoin::secp256k1::XOnlyPublicKey;

    let fee_keypair = Keypair::from_secret_key(&secp, fee_sk);
    let (x_only_pk, _parity) = XOnlyPublicKey::from_keypair(&fee_keypair);
    let fee_spk = Address::p2tr(&secp, x_only_pk, None, network).script_pubkey();

    // -------------- build annex -----------------
    let mut annex = Vec::with_capacity(2 + blob.len());
    annex.extend_from_slice(&[0x50, 0x00]); // tag + null version byte
    annex.extend_from_slice(blob);

    // -------------- transaction -----------------
    let mut tx = Transaction {
        version: Version(3),
        lock_time: LockTime::ZERO,
        input: vec![
            // Anchor input (anyone-can-spend)
            TxIn {
                previous_output: anchor,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            },
            // Fee-paying input – placeholder witness set later, carries annex
            TxIn {
                previous_output: fee_outpoint,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                // Put annex placeholder so it's included in sighash (first elem empty sig, second elem annex)
                witness: Witness::default(),
            },
        ],
        output: vec![
            TxOut {
                value: Amount::from_sat(fee_value_sat - fee_sat),
                script_pubkey: fee_spk.clone(),
            }
        ],
    };

    // -------- sign fee input (index 1) using Taproot Schnorr key spend --------

    // Prevouts must match the number of inputs (anchor + fee)
    let prevouts_arr = [
        // Anchor prevout: 0-sat anyone-can-spend (script unknown/irrelevant for sighash)
        TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
        // Fee prevout corresponding to input index 1 (key spend)
        TxOut { value: Amount::from_sat(fee_value_sat), script_pubkey: fee_spk.clone() },
    ];
    let prevouts = Prevouts::All(&prevouts_arr);

    let mut cache = SighashCache::new(&mut tx);
    // Include the annex in the signature hash per BIP-341 (using SIGHASH_DEFAULT)
    let annex_obj = Annex::new(annex.as_slice()).expect("valid annex");
    let input_index = 1;
    let sighash = cache
        .taproot_signature_hash(
            input_index,
            &prevouts,
            Some(annex_obj),
            None, // key-spend, no script path
            TapSighashType::Default,
        )
        .expect("sighash");

    let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
    let signature = secp.sign_schnorr(&msg, &fee_keypair);

    // Manually construct witness with signature and annex
    let sig_bytes: &[u8] = signature.as_ref();
    *cache.witness_mut(input_index).unwrap() = Witness::from_slice(&[
        sig_bytes,        // 64-byte Schnorr signature (SIGHASH_DEFAULT)
        annex.as_slice(), // annex published with fee-paying input
    ]);

    let tx = cache.into_transaction();

    tx.clone()
}

/// Helper: compute vbytes from weight.
fn weight_to_vbytes(w: u64) -> u64 {
    (w + 3) / 4
}

/// High-level convenience: build single publish_tx embedding `blob` and paying
/// fees derived from `fee_rate_sat_per_vb`.
pub fn publish_blob(
    blob: &[u8],
    anchor_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
) -> Result<(OutPoint, Transaction), &'static str> {
    let anchor_out = OutPoint::new(anchor_tx.compute_txid(), 1);

    // Build with zero fee to estimate weight.
    let publish_est = compile_publish_with_fee(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        0,
    );

    let weight = publish_est.weight().to_wu();
    let vbytes = weight_to_vbytes(weight);
    let fee_sat = vbytes * fee_rate_sat_per_vb;

    if fee_sat > fee_value_sat {
        return Err("fee exceeds utxo value");
    }

    // If change after fee would be dust, add to fee.
    let effective_fee_sat = if fee_value_sat - fee_sat < DUST_LIMIT_SAT {
        fee_value_sat // spend full input value as fee
    } else {
        fee_sat
    };

    let publish_tx = compile_publish_with_fee(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        effective_fee_sat,
    );

    Ok((anchor_out, publish_tx))
}

/// Attempt to extract the blob from a publish transaction built by
/// `publish_blob`. Looks at the **fee input** witness (index 1), expecting the
/// witness stack `<sig> <annex>` where the annex has the layout
/// `0x50 0x00 <data>`.
pub fn parse_blob_from_publish(publish_tx: &Transaction) -> Option<Vec<u8>> {
    let wit = &publish_tx.input.get(1)?.witness;
    if wit.len() < 2 { return None; } // expect <sig> <annex>
    let annex_bytes = &wit[1];
    if annex_bytes.len() < 2 || annex_bytes[0] != 0x50 || annex_bytes[1] != 0x00 {
        return None;
    }
    Some(annex_bytes[2..].to_vec())
} 