//! coins-subchain – trusted setup generator for successor/anchor chain.
//!
//! This crate is **offline-only**: it produces a JSON file that contains a
//! sequence of pre-signed *anchor* transactions.  Each anchor has
//!  - input:   the previous successor UTXO (taproot key-path spend)
//!  - output0: the next successor UTXO (pays back to the same address)
//!  - output1: a 0-sat OP_TRUE anchor which aggregators can spend to attach a
//!              sub-block (reveal tx).
//!
//! For now we provide a *minimal* generator that builds dummy transactions so
//! that the crate compiles and the JSON layout is fixed.  Integrating the full
//! taproot logic from `publisher.rs` will follow in a later phase.

use bitcoin::{
    script::Builder, secp256k1::{rand::rngs::OsRng, Message, Secp256k1, SecretKey}, transaction::Version, Address, Amount, CompressedPublicKey, Network, OutPoint, PrivateKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness
};
use bitcoin::sighash::{SighashCache, EcdsaSighashType};
use serde::{Serialize, Deserialize};
use bincode::serde::{encode_to_vec as bincode_serialize, decode_from_slice as bincode_deserialize};
use bincode::config::{standard, Config};

pub mod op_return;

#[derive(Debug, Serialize, Deserialize)]
pub struct Subchain {
    /// First UTXO to spend.
    pub first_out: OutPoint,
    /// Amount forwarded in each successor output.
    pub value_sat: Amount,
    /// Compressed secp256k1 public key used for P2WPKH outputs.
    pub pubkey: bitcoin::CompressedPublicKey,
    /// Bitcoin network the address belongs to (only used for (de)serializing).
    pub network: Network,
    /// DER-encoded ECDSA signatures (including sighash byte), one per anchor tx.
    pub sigs: Vec<Vec<u8>>,
}

impl Subchain {
    /// Build an unsigned anchor transaction spending `prev_out` and forwarding `value_sat` sat to the
    /// space-chain successor output. Creates a zero-sat anchor output (P2WSH OP_TRUE).
    fn build_anchor_tx(prev_out: OutPoint, value_sat: Amount, pk_script: ScriptBuf) -> Transaction {
        // Pay-to-Anchor (BIP-431): OP_1 OP_PUSHBYTES_2 0x4e 0x73  – zero-value, empty witness
        let anchor_spk = Builder::new()
            .push_opcode(bitcoin::opcodes::all::OP_PUSHNUM_1) // OP_1 (witness-version 1)
            .push_slice(&[0x4e, 0x73])                        // "Ns" tag
            .into_script();

        Transaction {
            version: Version(3),
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: prev_out,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::from_height(1),
                witness: Witness::default(),
            }],
            output: vec![
                TxOut { value: value_sat, script_pubkey: pk_script },
                TxOut { value: Amount::ZERO, script_pubkey: anchor_spk },
            ],
        }
    }

    /// Generate using a provided one-time secret key (deterministic).
    pub fn generate_with_private_key(
        n: usize,
        first_out: OutPoint,
        value_sat: Amount,
        network: Network,
        sk: &PrivateKey,
    ) -> Self {
        let secp = Secp256k1::new();
        let pk_compressed = bitcoin::CompressedPublicKey::from_private_key(&secp, sk).expect("compressed");

        let addr = Address::p2wpkh(&pk_compressed, network);
        let script_pubkey = addr.script_pubkey();

        let mut sigs = Vec::with_capacity(n);
        let mut prev_out = first_out;

        for _ in 0..n {
            let tx = Self::build_anchor_tx(prev_out, value_sat, script_pubkey.clone());

            // ---- sign input 0 ----
            let mut tx_clone = tx.clone();
            let mut cache = SighashCache::new(&mut tx_clone);
            // Witness spend: use BIP-143 style sighash for P2WPKH
            let sighash = cache
                .p2wpkh_signature_hash(
                    0,
                    &script_pubkey,
                    value_sat,
                    EcdsaSighashType::All,
                )
                .expect("sighash");

            let msg = Message::from_digest_slice(&sighash[..]).expect("32 bytes");
            let sig = secp.sign_ecdsa(&msg, &sk.inner);
            let mut sig_der = sig.serialize_der().to_vec();
            sig_der.push(EcdsaSighashType::All as u8);

            // store signature
            sigs.push(sig_der.clone());

            // attach witness for chaining (compute txid)
            let mut tx_final = tx;
            let pk_bytes = pk_compressed.to_bytes();
            tx_final.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), pk_bytes.as_slice()]);

            let txid = tx_final.compute_txid();
            prev_out = OutPoint::new(txid, 0);
        }

        Self { first_out, value_sat, pubkey: pk_compressed, network, sigs }
    }

    /// Convenience helper: derive a new random key and run `generate_with_private_key` in one go.
    pub fn generate(
        n: usize,
        first_out: OutPoint,
        value_sat: Amount,
        network: Network,
    ) -> (Self, SecretKey) {
        let mut rng = OsRng;
        let sk = SecretKey::new(&mut rng);
        let pk = PrivateKey::new(sk, network);
        let sc = Self::generate_with_private_key(n, first_out, value_sat, network, &pk);
        (sc, sk)
    }

    /// Create a new one-time key and return (secret_key, pubkey, address)
    pub fn new_key(network: Network) -> (SecretKey, CompressedPublicKey, Address) {
        let mut rng = OsRng;
        let sk = SecretKey::new(&mut rng);
        let secp = Secp256k1::new();
        let pk = PrivateKey::new(sk, network);
        let pk_compressed = CompressedPublicKey::from_private_key(&secp, &pk).expect("private key");
        let addr = Address::p2wpkh(&pk_compressed, network);
        (sk, pk_compressed, addr)
    }

    /// Serialize into compact binary using bincode with fixed-int encoding.
    pub fn encode(&self) -> Vec<u8> {
        bincode_serialize(self, bin_config()).expect("encode")
    }

    /// Decode binary format produced by `encode`.
    pub fn decode(data: &[u8]) -> Option<Self> {
        bincode_deserialize(data, bin_config()).ok().map(|(sc, _)| sc)
    }

    /// Reconstruct all anchor transactions from stored signatures.
    pub fn reconstruct_txs(&self) -> Vec<Transaction> {
        let script_pubkey = Address::p2wpkh(&self.pubkey, self.network).script_pubkey();
        let mut prev_out = self.first_out;
        let mut txs = Vec::with_capacity(self.sigs.len());
        for sig in &self.sigs {
            let mut tx = Self::build_anchor_tx(prev_out, self.value_sat, script_pubkey.clone());
            let pkb = self.pubkey.to_bytes();
            tx.input[0].witness = Witness::from_slice(&[sig.as_slice(), pkb.as_slice()]);
            let txid = tx.compute_txid();
            prev_out = OutPoint::new(txid, 0);
            txs.push(tx);
        }
        txs
    }
}



// -----------------------------------------------------------------------------
// Helper configuration
// -----------------------------------------------------------------------------

/// Fixed bincode configuration: little-endian + fixed-int encoding.
fn bin_config() -> impl Config {
    standard()
        .with_fixed_int_encoding()
        .with_little_endian()
} 