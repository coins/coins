//! OP_RETURN publishing for sub-blocks
//!
//! Uses Bitcoin Core v30's increased OP_RETURN limit (100KB) to publish
//! sub-block data directly on-chain. This is standard and propagates immediately
//! across 72% of the network (Core v30 nodes), but costs 4x more than witness data
//! due to the lack of segwit discount.
//!
//! Trade-offs:
//! - ✅ Standard transaction (fast propagation)
//! - ✅ No custom mining pools needed
//! - ✅ Only 2 TXs per block (connector + data_tx)
//! - ❌ 4x higher fees than witness-based methods
//! - ⚠️  28% of nodes (Bitcoin Knots) may reject large OP_RETURN

use bitcoin::absolute::LockTime;
use bitcoin::opcodes::all::OP_RETURN;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    Address, Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness, transaction::Version, PrivateKey, key::CompressedPublicKey,
};
use std::io::{Read, Write};
use core::convert::TryFrom;

const DUST_LIMIT_SAT: u64 = 546;

/// Compress data using zstd (level 3 is a good balance of speed/compression)
pub fn compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = zstd::Encoder::new(Vec::new(), 3)?;
    encoder.write_all(data)?;
    encoder.finish()
}

/// Decompress zstd-compressed data
pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = zstd::Decoder::new(compressed)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

/// Build a transaction that publishes `blob` via OP_RETURN output.
///
/// Transaction structure:
/// - Input 0: Anchor output (anyone-can-spend, 0-sat)
/// - Input 1: Fee-paying UTXO (P2WPKH)
/// - Output 0: OP_RETURN with blob data
/// - Output 1 (optional): Change back to fee address
fn compile_op_return_tx(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_sat: u64,
) -> Transaction {
    let secp = Secp256k1::new();
    let fee_priv_key = PrivateKey::new(*fee_sk, network);
    let fee_pk = CompressedPublicKey::from_private_key(&secp, &fee_priv_key)
        .expect("Invalid secret key");
    let fee_addr = Address::p2wpkh(&fee_pk, network);
    let fee_spk = fee_addr.script_pubkey();

    // Build OP_RETURN output with blob
    let push_bytes = PushBytesBuf::try_from(blob.to_vec())
        .expect("blob too large for push");
    let op_return_script = Builder::new()
        .push_opcode(OP_RETURN)
        .push_slice(push_bytes)
        .into_script();

    // Calculate change
    let change_value = fee_value_sat.saturating_sub(fee_sat);

    let mut outputs = vec![
        TxOut {
            value: Amount::ZERO,
            script_pubkey: op_return_script,
        },
    ];

    // Only add change output if above dust limit
    if change_value >= DUST_LIMIT_SAT {
        outputs.push(TxOut {
            value: Amount::from_sat(change_value),
            script_pubkey: fee_spk.clone(),
        });
    }

    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![
            // Anchor input (anyone-can-spend)
            TxIn {
                previous_output: anchor,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            },
            // Fee-paying input
            TxIn {
                previous_output: fee_outpoint,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(), // Will be set after signing
            },
        ],
        output: outputs,
    };

    // Sign fee input (index 1)
    let mut cache = SighashCache::new(&mut tx);
    let sighash = cache
        .p2wpkh_signature_hash(
            1,
            &fee_spk,
            Amount::from_sat(fee_value_sat),
            EcdsaSighashType::All,
        )
        .expect("sighash");

    let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
    let sig = secp.sign_ecdsa(&msg, fee_sk);
    let mut sig_der = sig.serialize_der().to_vec();
    sig_der.push(EcdsaSighashType::All as u8);

    *cache.witness_mut(1).unwrap() = Witness::from_slice(&[
        sig_der.as_slice(),
        fee_pk.to_bytes().as_slice(),
    ]);

    cache.into_transaction().clone()
}

/// Helper: compute vbytes from weight.
fn weight_to_vbytes(w: u64) -> u64 {
    (w + 3) / 4
}

/// High-level function to publish blob via OP_RETURN.
///
/// Returns (new_anchor_outpoint, data_tx) where:
/// - new_anchor_outpoint: The change output to use as next anchor
/// - data_tx: The transaction containing the OP_RETURN data
pub fn publish_op_return(
    blob: &[u8],
    connector_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
) -> Result<(OutPoint, Transaction), &'static str> {
    if blob.is_empty() {
        return Err("blob cannot be empty");
    }

    // Bitcoin Core v30 allows up to 100KB in OP_RETURN
    if blob.len() > 100_000 {
        return Err("blob exceeds 100KB OP_RETURN limit");
    }

    let anchor_out = OutPoint::new(connector_tx.compute_txid(), 1);

    // Build with zero fee to estimate weight
    let tx_estimate = compile_op_return_tx(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        0,
    );

    let weight = tx_estimate.weight().to_wu();
    let vbytes = weight_to_vbytes(weight);
    let fee_sat = vbytes * fee_rate_sat_per_vb;

    if fee_sat > fee_value_sat {
        return Err("fee exceeds UTXO value");
    }

    // Build final transaction with calculated fee
    let data_tx = compile_op_return_tx(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        fee_sat,
    );

    // New anchor is the change output (if exists)
    let new_anchor = if data_tx.output.len() > 1 {
        OutPoint::new(data_tx.compute_txid(), 1)
    } else {
        // No change output - all consumed as fee
        // This means we need a new fee UTXO for the next block
        OutPoint::null()
    };

    Ok((new_anchor, data_tx))
}

/// Parse blob from OP_RETURN output.
/// Looks for the first OP_RETURN output and extracts the data.
pub fn parse_blob_from_op_return(tx: &Transaction) -> Option<Vec<u8>> {
    for output in &tx.output {
        let script = &output.script_pubkey;
        let bytes = script.as_bytes();

        // Check if it's OP_RETURN (0x6a)
        if bytes.is_empty() || bytes[0] != 0x6a {
            continue;
        }

        // Skip OP_RETURN opcode, parse the push
        if bytes.len() < 2 {
            continue;
        }

        // Handle different push opcodes
        let data_start = if bytes[1] <= 75 {
            // Direct push (OP_PUSHBYTES_1 to OP_PUSHBYTES_75)
            2
        } else if bytes[1] == 0x4c {
            // OP_PUSHDATA1
            if bytes.len() < 3 {
                continue;
            }
            3
        } else if bytes[1] == 0x4d {
            // OP_PUSHDATA2
            if bytes.len() < 4 {
                continue;
            }
            4
        } else if bytes[1] == 0x4e {
            // OP_PUSHDATA4
            if bytes.len() < 6 {
                continue;
            }
            6
        } else {
            continue;
        };

        return Some(bytes[data_start..].to_vec());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::rand::{rngs::OsRng, RngCore};

    #[test]
    fn test_op_return_roundtrip() {
        let mut rng = OsRng;
        let mut data = vec![0u8; 1000];
        rng.fill_bytes(&mut data);

        let fee_sk = SecretKey::new(&mut rng);
        let network = Network::Regtest;

        // Create dummy connector tx
        let connector_tx = Transaction {
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

        let result = publish_op_return(
            &data,
            &connector_tx,
            fee_outpoint,
            fee_value,
            &fee_sk,
            1,
            network,
        );

        assert!(result.is_ok());
        let (_anchor, tx) = result.unwrap();

        // Parse back
        let parsed = parse_blob_from_op_return(&tx);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), data);
    }

    #[test]
    fn test_op_return_size_limit() {
        let mut rng = OsRng;
        let data = vec![0u8; 100_001]; // 1 byte over limit
        let fee_sk = SecretKey::new(&mut rng);

        let connector_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
                TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::default() },
            ],
        };

        let result = publish_op_return(
            &data,
            &connector_tx,
            OutPoint::null(),
            100_000,
            &fee_sk,
            1,
            Network::Regtest,
        );

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "blob exceeds 100KB OP_RETURN limit");
    }
}
