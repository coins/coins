//! coins-subchain – trusted setup generator for successor/anchor chain.
//!
//! This crate is **offline-only**: it produces a JSON file that contains a
//! sequence of pre-signed *anchor* transactions.  Each anchor has
//!  - input:   the previous successor UTXO (taproot key-path spend)
//!  - output0: the next successor UTXO (pays back to the same address)
//!  - output1: a 0-sat OP_TRUE anchor which publishers can spend to attach a
//!              sub-block (publish tx).
//!


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
    /// Compact 64-byte ECDSA signatures (r || s), one per anchor tx.
    /// DER encoding is reconstructed on-the-fly during transaction reconstruction.
    /// Each signature must be exactly 64 bytes.
    pub sigs: Vec<Vec<u8>>,
    /// Genesis block height where the first anchor was broadcast.
    /// Used as starting point for IBD scanning. None means scan from block 0.
    #[serde(default)]
    pub genesis_height: Option<u32>,
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

            // Store in compact 64-byte format (r || s)
            let sig_compact = sig.serialize_compact();
            sigs.push(sig_compact.to_vec());

            // Convert to DER for witness (needed for txid computation)
            let mut sig_der = sig.serialize_der().to_vec();
            sig_der.push(EcdsaSighashType::All as u8);

            // attach witness for chaining (compute txid)
            let mut tx_final = tx;
            let pk_bytes = pk_compressed.to_bytes();
            tx_final.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), pk_bytes.as_slice()]);

            let txid = tx_final.compute_txid();
            prev_out = OutPoint::new(txid, 0);
        }

        Self { first_out, value_sat, pubkey: pk_compressed, network, sigs, genesis_height: None }
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
        let (sc, _): (Subchain, usize) = bincode_deserialize(data, bin_config()).ok()?;

        // Validate that all signatures are exactly 64 bytes (compact format)
        if sc.sigs.iter().any(|sig| sig.len() != 64) {
            return None;
        }

        Some(sc)
    }

    /// Reconstruct all anchor transactions from stored signatures.
    pub fn reconstruct_txs(&self) -> Vec<Transaction> {
        use bitcoin::secp256k1::ecdsa::Signature;

        let script_pubkey = Address::p2wpkh(&self.pubkey, self.network).script_pubkey();
        let mut prev_out = self.first_out;
        let mut txs = Vec::with_capacity(self.sigs.len());

        for sig_bytes in &self.sigs {
            let mut tx = Self::build_anchor_tx(prev_out, self.value_sat, script_pubkey.clone());

            // Convert compact signature (64 bytes) back to DER format for witness
            if sig_bytes.len() != 64 {
                panic!("Invalid signature length: expected 64 bytes, got {}", sig_bytes.len());
            }
            let sig_compact: [u8; 64] = sig_bytes.as_slice().try_into()
                .expect("64-byte signature");
            let sig = Signature::from_compact(&sig_compact)
                .expect("valid compact signature");
            let mut sig_der = sig.serialize_der().to_vec();
            sig_der.push(EcdsaSighashType::All as u8);

            let pkb = self.pubkey.to_bytes();
            tx.input[0].witness = Witness::from_slice(&[sig_der.as_slice(), pkb.as_slice()]);
            let txid = tx.compute_txid();
            prev_out = OutPoint::new(txid, 0);
            txs.push(tx);
        }
        txs
    }

    /// Set the genesis block height (block where the first anchor was broadcast).
    /// This is used to optimize IBD scanning by starting from this height instead of block 0.
    pub fn set_genesis_height(&mut self, height: u32) {
        self.genesis_height = Some(height);
    }

    /// Get the P2WPKH address for this subchain.
    /// This is the address that receives the successor outputs.
    pub fn address(&self) -> Address {
        Address::p2wpkh(&self.pubkey, self.network)
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