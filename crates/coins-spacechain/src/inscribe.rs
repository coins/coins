//! Inscription utility helpers.
//!
//! This is **demo-quality** code that builds placeholder commit / reveal
//! transactions similar to the workflow used by ordinal inscriptions.  The
//! commit transaction spends the current *anchor* output (index 1) of the
//! connector transaction, forwarding its value into a new **P2WSH** output
//! whose witness‐script encodes the user blob.  The corresponding reveal
//! transaction spends this P2WSH output, revealing the blob in the script and
//! completing the inscription.
//!
//! Limitations / assumptions
//! -------------------------
//! * Anchor output (`connector_tx.output[1]`) is assumed to be spendable
//!   without signature (e.g. `OP_TRUE`).
//! * We operate on regtest / testnet – zero-satoshi outputs are accepted.
//! * Fee handling: a P2WPKH fee-UTXO funds the package; the user specifies a
//!   fee rate in sat/vbyte and the helper auto-calculates the absolute fee.
//! * If the remaining change would be below the dust limit (1000 sat), it is
//!   absorbed into the fee and no change output is created.
//! * Script taps into the *520-byte* push limit by chunking the blob into
//!   `≤520`-byte pushes inserted between `OP_FALSE OP_IF … OP_ENDIF` followed
//!   by `<pk> OP_CHECKSIG`; spending therefore requires a valid signature for
//!   the embedded pubkey.
//!
//! The public API exposes three helpers:
//!
//! * `compile_commit_and_reveal(blob, anchor_out, network, value_sat, fee_sk)`
//! * `parse_blob_from_reveal(reveal_tx)`
//! * `inscribe_blob(blob, connector_tx)` – convenience wrapper for typical flow.
//!
//! This module purposefully avoids persisting any keys; all spends rely on the
//! assumption that the anchor output is anyone-can-spend.

use bitcoin::blockdata::script::Instruction;
use bitcoin::key::{Keypair, Secp256k1};
use bitcoin::{
    Address, Network, OutPoint, Sequence, Transaction, TxIn, TxOut, Witness,
    script::Builder, opcodes::{OP_FALSE, OP_TRUE}, Amount,
};
use bitcoin::opcodes::all::{OP_IF, OP_ENDIF};
use bitcoin::script::PushBytesBuf;
use bitcoin::secp256k1::{SecretKey, Message, PublicKey as SecpPublicKey};
use bitcoin::sighash::{SighashCache, EcdsaSighashType};
use bitcoin::{transaction::Version, key::CompressedPublicKey, PublicKey};
use core::convert::TryFrom;

/// Maximum bytes that can be pushed onto the stack in a single opcode.
const MAX_SCRIPT_ELEMENT_SIZE: usize = 520;

/// Split `blob` into `≤520`-byte slices.
fn chunk_blob(blob: &[u8]) -> Vec<&[u8]> {
    if blob.is_empty() {
        return vec![&[][..]];
    }
    blob.chunks(MAX_SCRIPT_ELEMENT_SIZE).collect()
}

/// Build the witness **script** (to be hashed for P2WSH) containing the blob.
fn build_witness_script(chunks: &[&[u8]], pk: &CompressedPublicKey) -> bitcoin::ScriptBuf {
    let mut b = Builder::new();
    b = b.push_opcode(OP_FALSE);      // 0               – disables the IF branch at spend time
    b = b.push_opcode(OP_IF);         // IF              – everything until ENDIF is unreachable
    for chunk in chunks {
        let pb: PushBytesBuf = PushBytesBuf::try_from(chunk.to_vec()).expect("chunk <= 520");
        b = b.push_slice(pb);      // push ≤520-byte chunk
    }
    b = b.push_opcode(OP_ENDIF);      // ENDIF

    let pb: PushBytesBuf = PushBytesBuf::try_from(pk.to_bytes().to_vec()).expect("pk bytes");
    b = b.push_slice(pb);
    b = b.push_opcode(bitcoin::opcodes::all::OP_CHECKSIG);

    b.into_script()
}

/// Compile a **bare-bones** commit/reveal pair without fee handling.
///
/// Parameters
/// * `blob`      – opaque binary data to embed.
/// * `anchor`    – the anchor outpoint spent by the commit transaction.
/// * `network`   – Bitcoin network (for address generation).
/// * `value_sat` – amount (in sat) assigned to the single P2WSH output; for
///                 inscriptions this is commonly zero.
/// * `fee_sk`    – secret key whose corresponding pubkey is embedded in the
///                 witness script; reveal_tx must supply a valid signature.
///
/// Result: `(commit_tx, reveal_tx)` where commit_tx has one P2WSH output and
/// reveal_tx spends it with a signature produced from `fee_sk`.
pub fn compile_commit_and_reveal(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    value_sat: Amount,
    fee_sk: &SecretKey,
) -> (Transaction, Transaction) {
    // ----- pubkey derived from sk -----
    let secp = Secp256k1::new();
    let raw_pk: SecpPublicKey = SecpPublicKey::from_secret_key(&secp, fee_sk);
    let pk_inner = PublicKey { compressed: true, inner: raw_pk };
    let pk = CompressedPublicKey::try_from(pk_inner).expect("compressed pk");

    // ----- build witness script & destination output -----
    let chunks = chunk_blob(blob);
    let wscript = build_witness_script(&chunks, &pk);

    let addr = Address::p2wsh(&wscript, network);
    let script_pubkey = addr.script_pubkey();

    // ----- commit transaction -----
    let mut commit_tx = Transaction {
        version: bitcoin::transaction::Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: anchor,
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::default(), // anchor assumed anyone-can-spend
        }],
        output: vec![TxOut { value: value_sat, script_pubkey }],
    };

    let commit_out = OutPoint::new(commit_tx.compute_txid(), 0);

    // set witness for anchor spend: P2WSH(OP_TRUE)
    {
        let wscript = Builder::new().push_opcode(OP_TRUE).into_script();
        commit_tx.input[0].witness = Witness::from_slice(&[wscript.as_bytes()]);
    }

    // ----- reveal transaction -----
    let mut reveal_tx = Transaction {
        version: bitcoin::transaction::Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: commit_out,
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::from_slice(&[&[][..], wscript.as_bytes()]),
        }],
        // reveals are zero-value anyone-can-spend to keep chain moving
        output: vec![TxOut { value: Amount::from_sat(0), script_pubkey: Builder::new().push_opcode(OP_TRUE).into_script() }],
    };

    // sign reveal input
    {
        let mut cache = SighashCache::new(&mut reveal_tx);
        let sighash = cache.p2wpkh_signature_hash(0, &wscript, value_sat, EcdsaSighashType::All).expect("sighash");
        let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
        let sig = secp.sign_ecdsa(&msg, fee_sk);
        let mut sig_der = sig.serialize_der().to_vec();
        sig_der.push(EcdsaSighashType::All as u8);
        reveal_tx.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), wscript.as_bytes()]);
    }

    (commit_tx, reveal_tx)
}

/// Build commit/reveal that consumes a *P2WPKH* funding UTXO so the reveal
/// transaction can pay miner fees.
///
/// Behaviour:
/// 1. commit_tx has two inputs (anchor + fee_utxo) and **one** P2WSH output
///    locking the *full* value of the fee_utxo with
///    `OP_FALSE OP_IF <blob> OP_ENDIF <pk> OP_CHECKSIG`.
/// 2. reveal_tx spends that output, proves ownership by signing with
///    `fee_sk`, pays `fee_sat` in miner fees, and (optionally) returns change
///    back to the same P2WPKH address.
///
/// This function does *not* compute `fee_sat` – callers must supply it.
///
/// Parameters
/// * `blob`          – opaque binary data to embed.
/// * `anchor`        – anchor outpoint spent by the commit transaction.
/// * `network`       – Bitcoin network (for address/address-encoding).
/// * `fee_outpoint` – outpoint of the fee UTXO.
/// * `fee_value_sat` – value of the fee UTXO.
/// * `fee_sk` – secret key for signing the fee input.
/// * `fee_sat`   – absolute fee to pay (deducted in commit_tx).
pub fn compile_commit_and_reveal_with_fee(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_sat: u64,
) -> (Transaction, Transaction) {
    let secp = Secp256k1::new();
    let fee_raw_pk = SecpPublicKey::from_secret_key(&secp, fee_sk);
    let fee_pk_inner = PublicKey { compressed: true, inner: fee_raw_pk };
    let fee_pk = CompressedPublicKey::try_from(fee_pk_inner).expect("compressed");
    let fee_spk = Address::p2wpkh(&fee_pk, network).script_pubkey();
    let fee_prev_txout = TxOut { value: Amount::from_sat(fee_value_sat), script_pubkey: fee_spk.clone() };

    // build witness script and destination P2WSH output
    let chunks = chunk_blob(blob);
    let wscript = build_witness_script(&chunks, &fee_pk);
    let inscription_spk = Address::p2wsh(&wscript, network).script_pubkey();

    // -------- commit_tx (0-fee) --------
    let mut commit_tx = Transaction {
        version: Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![
            TxIn { // anchor
                previous_output: anchor,
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(),
            },
            TxIn { // fee-funding input
                previous_output: fee_outpoint,
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::default(), // placeholder, will sign below
            },
        ],
        output: vec![
            // inscription output locking remaining value after paying fee in commit
            TxOut { value: Amount::from_sat(fee_value_sat - fee_sat), script_pubkey: inscription_spk.clone() },
        ],
    };

    // set witness for anchor spend: P2WSH(OP_TRUE)
    {
        let wscript = Builder::new().push_opcode(OP_TRUE).into_script();
        let empty: &[u8] = &[];
        commit_tx.input[0].witness = Witness::from_slice(&[empty, wscript.as_bytes()]);
    }

    let inscription_outpoint = OutPoint::new(commit_tx.compute_txid(), 0);

    // -------- reveal_tx (no additional fee; commit already paid) --------
    let value_sat = fee_value_sat - fee_sat; // value locked in inscription
    let change_after_fee_sat = value_sat.saturating_sub(ANCHOR_VALUE_SAT);

    let mut reveal_outputs = {
        let wscript_anchor = Builder::new().push_opcode(OP_TRUE).into_script();
        let spk_anchor = Address::p2wsh(&wscript_anchor, network).script_pubkey();
        vec![TxOut { value: Amount::from_sat(ANCHOR_VALUE_SAT), script_pubkey: spk_anchor }]
    };
    if change_after_fee_sat > 0 {
        reveal_outputs.push(TxOut { value: Amount::from_sat(change_after_fee_sat), script_pubkey: fee_spk.clone() });
    }

    let mut reveal_tx = Transaction {
        version: Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: inscription_outpoint,
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::default(), // to be set
        }],
        output: reveal_outputs,
    };

    // sign input 0 of reveal_tx against wscript
    {
        let mut cache = SighashCache::new(&mut reveal_tx);
        let sighash = cache.p2wsh_signature_hash(0, &wscript, Amount::from_sat(value_sat), EcdsaSighashType::All).expect("sighash");
        let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
        let sig = secp.sign_ecdsa(&msg, fee_sk);
        let mut sig_der = sig.serialize_der().to_vec();
        sig_der.push(EcdsaSighashType::All as u8);
        reveal_tx.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), wscript.as_bytes()]);
    }

    (commit_tx, reveal_tx)
}

/// Same as compile_commit_and_reveal_with_fee but allows separate fee values for commit and reveal.
fn compile_commit_and_reveal_with_fee_split(
    blob: &[u8],
    anchor: OutPoint,
    network: Network,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_commit_sat: u64,
    fee_reveal_sat: u64,
) -> (Transaction, Transaction) {
    // we first lock amount minus commit fee into inscription
    let inscription_value = fee_value_sat - fee_commit_sat;

    // Build witness script etc.
    let chunks = chunk_blob(blob);
    let secp = Secp256k1::new();
    let fee_pk = {
        let pk = bitcoin::PrivateKey::new(*fee_sk, network);
        CompressedPublicKey::from_private_key(&secp, &pk).expect("pk")
    };
    let wscript = build_witness_script(&chunks, &fee_pk);
    let inscription_spk = Address::p2wsh(&wscript, network).script_pubkey();

    // commit tx
    let mut commit_tx = Transaction {
        version: Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![
            TxIn { previous_output: anchor, script_sig: bitcoin::ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::default() },
            TxIn { previous_output: fee_outpoint, script_sig: bitcoin::ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::default() },
        ],
        output: vec![TxOut { value: Amount::from_sat(inscription_value), script_pubkey: inscription_spk.clone() }],
    };
    // anchor witness empty+script
    commit_tx.input[0].witness = Witness::from_slice(&[wscript.as_bytes()]);

    // sign fee input
    let fee_addr = Address::p2wpkh(&fee_pk, network);
    let fee_spk = fee_addr.script_pubkey();
    let fee_prev_txout = TxOut { value: Amount::from_sat(fee_value_sat), script_pubkey: fee_spk.clone() };
    {
        let mut cache = SighashCache::new(&mut commit_tx);
        let sighash = cache.p2wpkh_signature_hash(1, &fee_spk, Amount::from_sat(fee_value_sat), EcdsaSighashType::All).expect("sighash");
        let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
        let sig = secp.sign_ecdsa(&msg, fee_sk);
        let mut sig_der = sig.serialize_der().to_vec();
        sig_der.push(EcdsaSighashType::All as u8);
        commit_tx.input[1].witness = Witness::from_slice(&[sig_der.as_slice(), fee_pk.to_bytes().as_slice()]);
    }

    let inscription_outpoint = OutPoint::new(commit_tx.compute_txid(), 0);

    // reveal tx
    let change_after_fee_sat = inscription_value.saturating_sub(ANCHOR_VALUE_SAT + fee_reveal_sat);
    let anchor_spk = {
        let ws = Builder::new().push_opcode(OP_TRUE).into_script();
        Address::p2wsh(&ws, network).script_pubkey()
    };
    let mut reveal_outputs = vec![TxOut { value: Amount::from_sat(ANCHOR_VALUE_SAT), script_pubkey: anchor_spk }];
    if change_after_fee_sat > 0 { reveal_outputs.push(TxOut { value: Amount::from_sat(change_after_fee_sat), script_pubkey: fee_spk.clone() }); }

    let mut reveal_tx = Transaction {
        version: Version(3),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn { previous_output: inscription_outpoint, script_sig: bitcoin::ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::default() }],
        output: reveal_outputs,
    };

    // sign reveal input with wscript
    {
        let mut cache = SighashCache::new(&mut reveal_tx);
        let sighash = cache.p2wsh_signature_hash(0, &wscript, Amount::from_sat(inscription_value), EcdsaSighashType::All).expect("sighash");
        let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
        let sig = secp.sign_ecdsa(&msg, fee_sk);
        let mut sig_der = sig.serialize_der().to_vec();
        sig_der.push(EcdsaSighashType::All as u8);
        reveal_tx.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), wscript.as_bytes()]);
    }

    (commit_tx, reveal_tx)
}

/// Convenience function wrapping `compile_commit_and_reveal_with_fee` when an anchor
/// outpoint can be derived from the connector tx.
///
/// Parameters
/// * `blob`            – arbitrary data to inscribe.
/// * `connector_tx`    – latest connector transaction whose vout=1 is the anchor.
/// * `fee_outpoint`    – outpoint of a P2WPKH UTXO owned by `fee_sk`.
/// * `fee_value_sat`   – value (sats) of the fee UTXO.
/// * `fee_sk`          – secret key corresponding to the P2WPKH address.
/// * `fee_sat`         – absolute fee to pay (deducted in commit_tx).
/// * `network`         – Bitcoin network.
///
/// Returns `(anchor_out, commit_tx, reveal_tx)`.
pub fn inscribe_blob_with_fee(
    blob: &[u8],
    connector_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_sat: u64,
    network: Network,
) -> (OutPoint, Transaction, Transaction) {
    let anchor_out = OutPoint::new(connector_tx.compute_txid(), 1);
    let (commit_tx, reveal_tx) = compile_commit_and_reveal_with_fee(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        fee_sat,
    );
    (anchor_out, commit_tx, reveal_tx)
}

/// Extract the original blob from a reveal transaction. Returns `None` if the
/// witness script does not follow the expected layout.
///
/// Layout expected: `OP_0 OP_IF <chunked-data> OP_ENDIF <pk> OP_CHECKSIG` in
/// the witness script (last item of the witness stack).
///
/// The function concatenates all push bytes between `OP_IF` and `OP_ENDIF` and
/// returns them.
pub fn parse_blob_from_reveal(reveal_tx: &Transaction) -> Option<Vec<u8>> {
    let wit = reveal_tx.input.get(0)?.witness.clone();
    // witness form: [ <empty> , <wscript>]
    let wscript_bytes = wit.last()?; // last element is script
    let script = bitcoin::ScriptBuf::from(wscript_bytes.to_vec());

    let mut inside = false;
    let mut data = Vec::new();
    for instr in script.instructions() {
        let instr = instr.ok()?;
        match instr {
            Instruction::Op(op) if op == OP_IF => inside = true,
            Instruction::Op(op) if op == OP_ENDIF => {
                inside = false;
                break; // blob fully parsed
            },
            Instruction::PushBytes(bytes) if inside => data.extend_from_slice(bytes.as_bytes()),
            _ => {}
        }
    }
    if inside { None } else { Some(data) }
}

const DUST_LIMIT_SAT: u64 = 1000;
const ANCHOR_VALUE_SAT: u64 = 546;

/// Compute the weight (in wu) of the connector + commit + reveal package.
fn package_weight(conn: &Transaction, commit: &Transaction, reveal: &Transaction) -> u64 {
    conn.weight().to_wu() + commit.weight().to_wu() + reveal.weight().to_wu()
}

/// Helper: compute vbytes from weight.
fn weight_to_vbytes(w: u64) -> u64 { (w + 3) / 4 }

/// High-level convenience: build commit/reveal package and pay a fee derived
/// from `fee_rate_sat_per_vb` (sat/vbyte).
/// Errors if fee exceeds available value.
pub fn inscribe_blob(
    blob: &[u8],
    connector_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
) -> Result<(OutPoint, Transaction, Transaction), &'static str> {
    let anchor_out = OutPoint::new(connector_tx.compute_txid(), 1);

    // First build with zero fee to estimate weight.
    let (commit_tx_est, reveal_tx_est) = compile_commit_and_reveal_with_fee(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        0,
    );

    let weight_total = package_weight(connector_tx, &commit_tx_est, &reveal_tx_est);
    let weight_commit = commit_tx_est.weight().to_wu();

    let vbytes_total = weight_to_vbytes(weight_total);
    let fee_total = vbytes_total * fee_rate_sat_per_vb;

    if fee_total > fee_value_sat { return Err("fee exceeds utxo value"); }

    // split fee proportional to weight but ensure both txs pay at least 1 sat/vB
    let vbytes_commit = weight_to_vbytes(weight_commit);
    let mut fee_commit = vbytes_commit * fee_rate_sat_per_vb;
    if fee_commit == 0 { fee_commit = 1; }
    if fee_commit > fee_total { fee_commit = fee_total; }
    let mut fee_reveal = fee_total - fee_commit;

    // If change after fees would be dust adjust: prefer moving sats to reveal fee
    if fee_value_sat - fee_total < DUST_LIMIT_SAT {
        fee_reveal += fee_value_sat - fee_total; // absorb dust into reveal fee
    }

    // Rebuild package with split fees.
    let (commit_tx_f, reveal_tx_f) = compile_commit_and_reveal_with_fee_split(
        blob,
        anchor_out,
        network,
        fee_outpoint,
        fee_value_sat,
        fee_sk,
        fee_commit,
        fee_reveal,
    );

    Ok((anchor_out, commit_tx_f, reveal_tx_f))
}
