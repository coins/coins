use std::io::{self, Write};

use anyhow::{anyhow, Result};
use bitcoin::hex::DisplayHex;
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::opcodes::{OP_TRUE, all::OP_PUSHNUM_1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{absolute::LockTime, Amount, Network, OutPoint, PrivateKey, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
use esplora_client::{AsyncClient};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::consensus::encode::serialize;

#[derive(Clone, Debug)]
struct Utxo {
    outpoint: OutPoint,
    value: Amount,
    confirmed: bool,
}

type Secp = Secp256k1<bitcoin::secp256k1::All>;



#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // ----------------- DETERMINISTIC WALLET -----------------
    let network = Network::Signet; // use Signet by default for cheap testing
    let sk_bytes = [1u8; 32]; // 0x01..01 static seed (NOT SECURE!)
    let sk = SecretKey::from_slice(&sk_bytes)?;
    let pk = PrivateKey::new(sk, network);
    let secp = Secp::new();
    let pubkey: PublicKey = pk.public_key(&secp);
    let my_addr = bitcoin::Address::p2pkh(&pubkey, network);

    println!("=== Package-Relay test ===");
    println!("Deterministic test address: {}", my_addr);
    println!("Send some coins to the address above, then press <enter> to continue …");
    use std::io::Write as _;
    io::stdout().flush().unwrap();
    let mut _dummy = String::new();
    io::stdin().read_line(&mut _dummy)?;

    // ----------------- ESPLORA CLIENT -----------------
    let esplora_url = std::env::var("ESPLORA_URL").unwrap_or_else(|_| " https://signet-api.ordinalsbot.com/mempool/api".to_string());
    let client: AsyncClient<esplora_client::r#async::DefaultSleeper> =
        AsyncClient::from_builder(esplora_client::Builder::new(&esplora_url).header("x-api-key", "15524be2-f9b8-430d-a463-6db2f085f075"))
            .expect("failed to create esplora client");
    println!("Fetching UTXOs from {esplora_url} …");

    let block_height = client.get_height().await?;
    println!("Block height: {}", block_height);

    // wait until we see at least two confirmed UTXOs
    let mut utxos = poll_two_utxos(&client, &my_addr).await?;
    // sort by value desc for determinism
    utxos.sort_by_key(|u| std::cmp::Reverse(u.value.to_sat()));
    let funding_utxo = utxos[0].clone();
    let fee_utxo = utxos[1].clone();
    println!("Parent funding UTXO: {}:{} ({} sat)", funding_utxo.outpoint.txid, funding_utxo.outpoint.vout, funding_utxo.value.to_sat());
    println!("Child  fee UTXO:      {}:{} ({} sat)", fee_utxo.outpoint.txid, fee_utxo.outpoint.vout, fee_utxo.value.to_sat());

    // ----------------- BUILD PARENT TX -----------------
    let (parent_tx, anchor_script, anchor_spk) = build_parent(&secp, &pk, &funding_utxo, network)?;
    println!("Anchor scriptPubKey (hex): {:?}", anchor_spk.as_bytes());
    match bitcoin::Address::from_script(&anchor_spk, network) {
        Ok(anchor_addr) => println!("Anchor address: {}", anchor_addr),
        Err(e) => println!("Could not create address from script: {}", e),
    }

    println!("Parent txid: {}", parent_tx.compute_txid());

    // ----------------- BUILD CHILD TX -----------------
    let child_fee_sat = 1000; // pay 1k sat fee for the whole package
    let fee_input = (fee_utxo.outpoint, fee_utxo.value);
    let child_tx = build_child(&secp, &pk, &parent_tx, &fee_input, &anchor_script, child_fee_sat, network)?;
    println!("Child  txid: {}", child_tx.compute_txid());

    // print raw transactions (hex-encoded) for manual inspection / debugging
    let parent_ser = serialize(&parent_tx);
    let parent_hex = parent_ser.as_hex();
    println!("Parent raw hex: {parent_hex}");

    let child_ser = serialize(&child_tx);
    let child_hex = child_ser.as_hex();
    println!("Child  raw hex:  {child_hex}");

    // ----------------- BROADCAST PACKAGE -----------------
    println!("Broadcasting parent+child via /txs/package …");
    client.broadcast_package(&[parent_tx, child_tx]).await?;
    // client.broadcast(&parent_tx).await?;
    // client.broadcast(&child_tx).await?;
    println!("Package sent! You can inspect it on any Signet explorer once mined.");

    Ok(())
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Poll esplora until at least one UTXO for the provided address is returned.
async fn poll_utxo<C: esplora_client::r#async::Sleeper + Send + Sync>(
    client: &AsyncClient<C>,
    addr: &bitcoin::Address,
) -> Result<Utxo> {
    loop {
        let utxos = get_address_utxo(client, addr).await?;
        if let Some(u) = utxos.into_iter().find(|u| u.confirmed) {
            return Ok(u);
        }
        println!("No confirmed UTXO yet – waiting 30s …");
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

async fn poll_two_utxos<C: esplora_client::r#async::Sleeper + Send + Sync>(
    client: &AsyncClient<C>,
    addr: &bitcoin::Address,
) -> Result<Vec<Utxo>> {
    loop {
        let mut utxos: Vec<_> = get_address_utxo(client, addr).await?;
        utxos.retain(|u| u.confirmed);
        if utxos.len() >= 2 {
            return Ok(utxos);
        }
        println!("Need 2 confirmed UTXOs – currently {}. Waiting 30s …", utxos.len());
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

/// Fetch UTXOs for an address using the electrs /address/<addr>/utxo endpoint.
async fn get_address_utxo<C: esplora_client::r#async::Sleeper + Send + Sync>(
    client: &AsyncClient<C>,
    addr: &bitcoin::Address,
) -> Result<Vec<Utxo>> {
    let raw = client.get_address_utxo(addr.clone()).await?;
    Ok(raw
        .into_iter()
        .map(|u| Utxo {
            outpoint: OutPoint::new(u.txid, u.vout as u32),
            value: u.value,
            confirmed: u.status.confirmed,
        })
        .collect())
}

fn build_parent(
    secp: &Secp,
    pk: &PrivateKey,
    funding: &Utxo,
    network: Network,
) -> Result<(Transaction, ScriptBuf, ScriptBuf)> {
    // ---- outputs ----
    let anchor_spk = {
        Builder::new()
            .push_opcode(OP_PUSHNUM_1) // OP_1
            .push_slice(&[0x4e, 0x73]) // "Ns"
            .into_script()
    };
    let anchor_wscript = ScriptBuf::new(); // empty script for spending

    // value calculation
    let parent_fee = Amount::from_sat(0);
    let anchor_value = Amount::from_sat(0);
    let change_value = funding.value - parent_fee - anchor_value; // ensure fee + anchor deducted

    let change_script = bitcoin::Address::p2pkh(&pk.public_key(secp), network).script_pubkey();

    let mut parent = Transaction {
        version: Version(3),
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: funding.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: bitcoin::Witness::default(),
        }],
        output: vec![
            TxOut { value: change_value, script_pubkey: change_script.clone() },
            TxOut { value: anchor_value, script_pubkey: anchor_spk.clone() },
        ],
    };

    // sign the single P2PKH input
    sign_p2pkh(secp, &mut parent, 0, pk, &change_script)?;

    Ok((parent, anchor_wscript, anchor_spk))
}

fn build_child(
    secp: &Secp,
    pk: &PrivateKey,
    parent: &Transaction,
    change_out: &(OutPoint, Amount),
    anchor_wscript: &ScriptBuf,
    fee_sat: u64,
    network: Network,
) -> Result<Transaction> {
    let (change_prev, change_value) = change_out;

    // outputs: send leftovers back to self after fee deduction
    let send_back_sat = change_value.to_sat().saturating_sub(fee_sat);
    if send_back_sat == 0 {
        return Err(anyhow!("Change value too small to pay fee"));
    }

    let change_script = bitcoin::Address::p2pkh(&pk.public_key(secp), network).script_pubkey();

    let mut child = Transaction {
        version: Version(3),
        lock_time: LockTime::ZERO,
        input: vec![
            // anchor input – no signature needed
            TxIn {
                previous_output: OutPoint::new(parent.compute_txid(), 1),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            },
            // fee-paying input (our change)
            TxIn {
                previous_output: *change_prev,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: bitcoin::Witness::default(),
            },
        ],
        output: vec![TxOut { value: Amount::from_sat(send_back_sat), script_pubkey: change_script.clone() }],
    };

    // sign second input (index 1)
    sign_p2pkh(secp, &mut child, 1, pk, &change_script)?;

    Ok(child)
}

fn sign_p2pkh(
    secp: &Secp,
    tx: &mut Transaction,
    idx: usize,
    pk: &PrivateKey,
    script_pubkey: &ScriptBuf,
) -> Result<()> {
    // Create sighash
    let sighash = {
        let mut cache = SighashCache::new(&mut *tx);
        cache.legacy_signature_hash(
            idx,
            script_pubkey,
            EcdsaSighashType::All as u32,
        )?
    };

    let msg = Message::from_digest_slice(&sighash[..])?;
    let sig = secp.sign_ecdsa(&msg, &pk.inner);

    let mut sig_ser = sig.serialize_der().to_vec();
    sig_ser.push(EcdsaSighashType::All as u8);

    let pubkey_bytes = pk.public_key(secp).to_bytes();
    let pb_sig: PushBytesBuf = PushBytesBuf::try_from(sig_ser.clone()).expect("sig bytes");
    let pb_pk: PushBytesBuf = PushBytesBuf::try_from(pubkey_bytes.to_vec()).expect("pk bytes");
    let script_sig = ScriptBuf::builder()
        .push_slice(pb_sig)
        .push_slice(pb_pk)
        .into_script();

    tx.input[idx].script_sig = script_sig;
    Ok(())
} 