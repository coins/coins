use coins_subchain::{Subchain, inscribe, broadcast::{RestBroadcaster, Broadcaster}};
use bitcoin::{Amount, Network, OutPoint, Address, PrivateKey, CompressedPublicKey};
use bitcoin::secp256k1::Secp256k1;

#[tokio::test]
async fn real_utxo_flow() -> anyhow::Result<()> {
    // read env vars or skip
    let sk_hex = match std::env::var("SIGNET_FEE_SK") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SIGNET_FEE_SK not set – skipping real_utxo test");
            return Ok(());
        }
    };
    let api  = std::env::var("SIGNET_API").unwrap_or_else(|_| "https://mempool.space/signet/api".into());

    // secret key & fee address
    let sk_bytes = hex::decode(sk_hex.trim())?;
    let sk       = bitcoin::secp256k1::SecretKey::from_slice(&sk_bytes)?;
    let fee_pk   = PrivateKey::new(sk, Network::Signet);
    let secp = Secp256k1::new();
    let pk_comp = CompressedPublicKey::from_private_key(&secp, &fee_pk).unwrap();
    let fee_addr = Address::p2wpkh(&pk_comp, Network::Signet);

    let backend = RestBroadcaster::new(&api);
    let utxos = backend.get_address_utxo(&fee_addr).await?;
    let utxo = utxos.into_iter().find(|u| u.confirmed)
        .expect("no confirmed UTXO for fee address; fund it on Signet first");

    // build minimal chain
    let (sc, _) = Subchain::generate(1, OutPoint::null(), Amount::from_sat(546), Network::Signet);
    let anchor_tx = sc.reconstruct_txs()[0].clone();

    // compile commit/reveal
    let blob = b"test blob".to_vec();
    let (anchor_out, commit, reveal) = inscribe::inscribe_blob(
        &blob,
        &anchor_tx,
        utxo.outpoint,
        utxo.value.to_sat(),
        &sk,
        2, // sat/vb fee rate
        Network::Signet,
    ).map_err(|e| anyhow::Error::msg(e.to_string()))?;

    // broadcast
    backend.broadcast_package(&[anchor_tx.clone(), commit.clone()]).await?;
    backend.broadcast_raw(&reveal).await?;

    println!("anchor  {}", anchor_tx.compute_txid());
    println!("commit  {}", commit.compute_txid());
    println!("reveal  {}", reveal.compute_txid());
    Ok(())
} 