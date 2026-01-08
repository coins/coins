//! Taproot annex publishing for sub-blocks
//!
//! Embeds data in the witness annex (BIP 341) for 75% fee savings due to witness discount.
//!
//! Trade-offs:
//! - ✅ 75% cheaper than OP_RETURN (witness discount)
//! - ✅ Only 2 TXs per block (anchor + data_tx)
//! - ❌ Non-standard transaction (may require custom relay)
//! - ❌ Limited node support initially

use bitcoin::absolute::LockTime;
use bitcoin::script::Builder;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Annex, Prevouts, SighashCache, TapSighashType};
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness, transaction::Version,
};
use bitcoin::key::{Keypair, TapTweak, TweakedKeypair};
use bitcoin::secp256k1::XOnlyPublicKey;

const DUST_LIMIT_SAT: u64 = 546;

/// Build a transaction that publishes `blob` via Taproot annex.
///
/// Transaction structure:
/// - Input 0: Anchor output (anyone-can-spend, 0-sat)
/// - Input 1: Fee-paying UTXO (P2TR key spend with annex)
/// - Output 0 (optional): Change back to fee address
fn compile_taproot_annex_tx(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_sat: u64,
) -> Transaction {
    let secp = Secp256k1::new();

    // Build P2TR address for fee input
    let fee_keypair = Keypair::from_secret_key(&secp, fee_sk);
    let (x_only_pk, _parity) = XOnlyPublicKey::from_keypair(&fee_keypair);
    let fee_addr = Address::p2tr(&secp, x_only_pk, None, network);
    let fee_spk = fee_addr.script_pubkey();

    // Build annex: 0x50 (tag) + 0x00 (version) + blob
    let mut annex_bytes = Vec::with_capacity(2 + blob.len());
    annex_bytes.push(0x50); // Annex tag (BIP 341)
    annex_bytes.push(0x00); // Version byte (for future extensibility)
    annex_bytes.extend_from_slice(blob);

    // Calculate change
    let change_value = fee_value_sat.saturating_sub(fee_sat);

    let mut outputs = Vec::new();

    // Only add change output if above dust limit
    if change_value >= DUST_LIMIT_SAT {
        outputs.push(TxOut {
            value: Amount::from_sat(change_value),
            script_pubkey: fee_spk.clone(),
        });
    }

    // Build anchor scriptPubkey (OP_1 + 0x4e73 "Ns" tag)
    let anchor_spk = Builder::new()
        .push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_1)
        .push_slice([0x4e, 0x73])
        .into_script();

    let mut tx = Transaction {
        version: Version(3),
        lock_time: LockTime::from_height(1).unwrap(), // Set bit 0 = 1 for Taproot annex format
        input: vec![
            // Anchor input (anyone-can-spend)
            TxIn {
                previous_output: anchor,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            },
            // Fee-paying input (will have Schnorr signature + annex)
            TxIn {
                previous_output: fee_outpoint,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            },
        ],
        output: outputs,
    };

    // Sign fee input (index 1) with Taproot key spend including annex
    let prevouts_arr = [
        TxOut {
            value: Amount::ZERO,
            script_pubkey: anchor_spk,
        },
        TxOut {
            value: Amount::from_sat(fee_value_sat),
            script_pubkey: fee_spk,
        },
    ];
    let prevouts = Prevouts::All(&prevouts_arr);

    let mut cache = SighashCache::new(&mut tx);
    let annex = Annex::new(&annex_bytes).expect("valid annex");
    let input_index = 1;
    let sighash_type = TapSighashType::Default;

    let sighash = cache
        .taproot_signature_hash(
            input_index,
            &prevouts,
            Some(annex.clone()),
            None, // key-spend, no script path
            sighash_type,
        )
        .expect("sighash");

    let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
    let tweaked: TweakedKeypair = fee_keypair.tap_tweak(&secp, None);
    let signature = secp.sign_schnorr(&msg, tweaked.as_keypair());
    let signature = bitcoin::taproot::Signature {
        signature,
        sighash_type,
    };

    // Build witness: [signature, annex]
    let mut witness = Witness::p2tr_key_spend(&signature);
    witness.push(annex.as_bytes());
    *cache.witness_mut(input_index).unwrap() = witness;

    cache.into_transaction().clone()
}

/// Helper: compute vbytes from weight.
fn weight_to_vbytes(w: u64) -> u64 {
    w.div_ceil(4)
}

/// High-level function to publish blob via Taproot annex.
///
/// Returns (new_anchor_outpoint, data_tx) where:
/// - new_anchor_outpoint: The change output to use as next anchor (or null if no change)
/// - data_tx: The transaction containing the annex data
pub fn publish_taproot_annex(
    blob: &[u8],
    anchor_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
) -> Result<(OutPoint, Transaction), &'static str> {
    if blob.is_empty() {
        return Err("blob cannot be empty");
    }

    // Taproot annex has no hard size limit, but keep reasonable (~400KB)
    if blob.len() > 400_000 {
        return Err("blob exceeds reasonable annex size");
    }

    if fee_value_sat < 1000 {
        return Err("fee UTXO value too low (< 1000 sats)");
    }

    let anchor_out = OutPoint::new(anchor_tx.compute_txid(), 1);

    // Build with zero fee to estimate weight
    let tx_estimate = compile_taproot_annex_tx(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        0,
    );

    // CPFP: data_tx must pay fees for BOTH anchor and data_tx
    let anchor_vbytes = weight_to_vbytes(anchor_tx.weight().to_wu());
    let data_vbytes = weight_to_vbytes(tx_estimate.weight().to_wu());
    let total_vbytes = anchor_vbytes + data_vbytes;
    let fee_sat = total_vbytes * fee_rate_sat_per_vb;

    if fee_sat > fee_value_sat {
        return Err("fee exceeds UTXO value");
    }

    // Build final transaction with calculated fee
    let data_tx = compile_taproot_annex_tx(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        fee_sat,
    );

    // New anchor is the change output (if exists)
    let new_anchor = if !data_tx.output.is_empty() {
        OutPoint::new(data_tx.compute_txid(), 0)
    } else {
        // No change output - all consumed as fee
        OutPoint::null()
    };

    Ok((new_anchor, data_tx))
}

/// Parse blob from Taproot annex transaction.
/// Looks for annex in fee input (index 1), expecting layout: 0x50 0x00 <data>
pub fn parse_blob_from_taproot_annex(tx: &Transaction) -> Option<Vec<u8>> {
    let witness = &tx.input.get(1)?.witness;

    // Expect [signature, annex]
    if witness.len() < 2 {
        return None;
    }

    let annex_bytes = &witness[1];

    // Check annex tag and version
    if annex_bytes.len() < 2 || annex_bytes[0] != 0x50 || annex_bytes[1] != 0x00 {
        return None;
    }

    Some(annex_bytes[2..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::rand::{rngs::OsRng, RngCore};

    #[test]
    fn test_taproot_annex_roundtrip() {
        let mut rng = OsRng;
        let mut data = vec![0u8; 1000];
        rng.fill_bytes(&mut data);

        let fee_sk = SecretKey::new(&mut rng);
        let network = Network::Regtest;

        // Create dummy anchor tx
        let anchor_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![
                TxOut {
                    value: Amount::ZERO,
                    script_pubkey: ScriptBuf::default(),
                },
                TxOut {
                    value: Amount::ZERO,
                    script_pubkey: ScriptBuf::default(),
                },
            ],
        };

        let fee_outpoint = OutPoint::null();
        let fee_value = 100_000;

        let result = publish_taproot_annex(
            &data,
            &anchor_tx,
            fee_outpoint,
            fee_value,
            &fee_sk,
            1,
            network,
        );

        assert!(result.is_ok());
        let (_anchor, tx) = result.unwrap();

        // Verify nLocktime bit 0 is set
        assert_eq!(tx.lock_time.to_consensus_u32() & 1, 1);

        // Parse back
        let parsed = parse_blob_from_taproot_annex(&tx);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), data);
    }

    #[test]
    fn test_taproot_annex_empty_blob() {
        let mut rng = OsRng;
        let data = vec![];

        let fee_sk = SecretKey::new(&mut rng);
        let network = Network::Regtest;

        let anchor_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
            ],
        };

        let result = publish_taproot_annex(
            &data,
            &anchor_tx,
            OutPoint::null(),
            100_000,
            &fee_sk,
            1,
            network,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "blob cannot be empty");
    }

    #[test]
    fn test_taproot_annex_low_fee_utxo() {
        let mut rng = OsRng;
        let data = vec![0u8; 100];
        let fee_sk = SecretKey::new(&mut rng);

        let anchor_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
            ],
        };

        let result = publish_taproot_annex(
            &data,
            &anchor_tx,
            OutPoint::null(),
            500,  // Too low
            &fee_sk,
            1,
            Network::Regtest,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "fee UTXO value too low (< 1000 sats)");
    }
}
