#![cfg(test)]

use bitcoin::{absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness, Address, Network, key::CompressedPublicKey, PrivateKey};
use bitcoin::secp256k1::{rand::{rngs::OsRng, RngCore}, Secp256k1};

#[test]
fn taproot_annex_300b() {
    // Generate 300 bytes of random payload
    let mut rng = OsRng;
    let mut data = [0u8; 300];
    rng.fill_bytes(&mut data);

    // Build annex: first byte must be 0x50 according to BIP-341
    let mut annex = Vec::with_capacity(2 + data.len());
    annex.extend_from_slice(&[0x50, 0x00]); // BIP-341 annex tag + null version byte per PTodd suggestion
    annex.extend_from_slice(&data);

    // Create a dummy 65-byte signature (64-byte random + sighash byte)
    let mut sig = [0u8; 64];
    rng.fill_bytes(&mut sig);
    let mut sig_vec = sig.to_vec();
    sig_vec.push(0x01); // SIGHASH_ALL

    // Build a random P2WPKH address for the output
    let secp = Secp256k1::new();
    let rand_sk = bitcoin::secp256k1::SecretKey::new(&mut rng);
    let rand_pk = CompressedPublicKey::from_private_key(&secp, &PrivateKey::new(rand_sk, Network::Regtest)).unwrap();
    let rand_addr = Address::p2wpkh(&rand_pk, Network::Regtest);

    // Construct a dummy transaction with annex + signature and one pay-to-random-address output
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            // Witness stack: <sig> <annex>
            witness: Witness::from_slice(&[sig_vec.as_slice(), annex.as_slice()]),
        }],
        output: vec![
            TxOut {
                value: Amount::from_sat(4242),
                script_pubkey: rand_addr.script_pubkey(),
            },
        ],
    };

    // Debug print (visible with `cargo test -- --nocapture`)
    let raw_hex = hex::encode(bitcoin::consensus::encode::serialize(&tx));
    println!("tx hex: {} (annex {} bytes)", raw_hex, annex.len());

    // Assertions – ensure annex is present and has expected size
    assert_eq!(tx.input[0].witness.len(), 2);
    let annex_wit = &tx.input[0].witness[1];
    assert_eq!(annex_wit[0], 0x50);
    assert_eq!(annex_wit.len(), 302); // 1 tag + 1 version + 300 random bytes
}
