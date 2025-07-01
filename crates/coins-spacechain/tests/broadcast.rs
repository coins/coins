use coins_spacechain::{Spacechain, inscribe, broadcast::Broadcaster};
use bitcoin::{Amount, Network, OutPoint, Txid, PrivateKey, CompressedPublicKey, Address};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::env;
use hex;
use coins_spacechain::broadcast::RestBroadcaster;
use std::str::FromStr;
use bitcoin::secp256k1::Secp256k1;

struct MockBroadcaster {
    pub raw: Arc<Mutex<Vec<Txid>>>,
    pub pkg: Arc<Mutex<Vec<Vec<Txid>>>>,
}

impl MockBroadcaster {
    fn new() -> Self { Self { raw: Arc::new(Mutex::new(Vec::new())), pkg: Arc::new(Mutex::new(Vec::new())) } }
}

#[async_trait]
impl Broadcaster for MockBroadcaster {
    async fn broadcast_raw(&self, tx: &bitcoin::Transaction) -> anyhow::Result<()> {
        self.raw.lock().unwrap().push(tx.compute_txid());
        Ok(())
    }

    async fn broadcast_package(&self, txs: &[bitcoin::Transaction]) -> anyhow::Result<()> {
        let ids = txs.iter().map(|t| t.compute_txid()).collect::<Vec<_>>();
        self.pkg.lock().unwrap().push(ids);
        Ok(())
    }
}

#[tokio::test]
async fn build_and_broadcast_package() {
    // build a minimal spacechain with one anchor
    let first_out = OutPoint::null();
    let (sc, sk) = Spacechain::generate(1, first_out, Amount::from_sat(546), Network::Regtest);

    // reconstruct anchor tx
    let anchor_tx = sc.reconstruct_txs().pop().expect("anchor");

    // compile commit/reveal for dummy blob
    let blob = b"hello".to_vec();
    let fee_out = OutPoint::null();
    let (anchor_out, commit_tx, reveal_tx) = inscribe::inscribe_blob(
        &blob,
        &anchor_tx,
        fee_out,
        1000,
        &sk,
        1,
        Network::Regtest,
    ).expect("compile");

    assert_eq!(anchor_out.vout, 1);

    let mock = MockBroadcaster::new();

    // broadcast package anchor+commit
    mock.broadcast_package(&[anchor_tx.clone(), commit_tx.clone()]).await.unwrap();
    // broadcast reveal
    mock.broadcast_raw(&reveal_tx).await.unwrap();

    assert_eq!(mock.pkg.lock().unwrap().len(), 1);
    assert_eq!(mock.raw.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn inscribe_on_real_signet_utxo() -> anyhow::Result<()> {
    // ──────────────────────────────────────────────────────────────
    // 1. read funding information from the environment
    //    export SIGNET_FEE_SK=<hex>  (32-byte secp256k1 secret key)
    //    export SIGNET_FEE_OUT=<txid>:<vout>
    //    export SIGNET_API=https://mempool.space/signet/api
    // ──────────────────────────────────────────────────────────────
    let sk_hex = match env::var("SIGNET_FEE_SK") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Set SIGNET_FEE_SK, SIGNET_FEE_OUT, SIGNET_API to enable this test");
            return Ok(());          // skip
        }
    };
    let out_str = env::var("SIGNET_FEE_OUT")?;
    let api      = env::var("SIGNET_API")?;

    // secret key / address
    let sk_bytes = hex::decode(sk_hex.trim())?;
    let sk       = bitcoin::secp256k1::SecretKey::from_slice(&sk_bytes)?;
    let fee_pk   = PrivateKey::new(sk, Network::Signet);
    let secp = Secp256k1::new();
    let pk_comp = CompressedPublicKey::from_private_key(&secp, &fee_pk).unwrap();
    let fee_addr = Address::p2wpkh(&pk_comp, Network::Signet);

    // funding outpoint
    let mut parts = out_str.split(':');
    let txid = Txid::from_str(parts.next().unwrap())?;
    let vout: u32 = parts.next().unwrap().parse()?;
    let fee_out   = OutPoint::new(txid, vout);

    // ──────────────────────────────────────────────────────────────
    // 2. query the backend to verify the UTXO exists and get its value
    // ──────────────────────────────────────────────────────────────
    let backend = RestBroadcaster::new(&api);
    let utxos   = backend.get_address_utxo(&fee_addr).await?;
    let utxo    = utxos.into_iter().find(|u| u.outpoint == fee_out && u.confirmed)
                  .expect("funding UTXO not confirmed");

    // ──────────────────────────────────────────────────────────────
    // 3. build a 1-anchor space-chain and compile commit/reveal
    // ──────────────────────────────────────────────────────────────
    let (sc, _) = Spacechain::generate(
        1,
        bitcoin::OutPoint::null(),
        Amount::from_sat(546),
        Network::Signet,
    );
    let anchor_tx = sc.reconstruct_txs()[0].clone();

    let blob = b"testing real utxo".to_vec();
    let (_anchor_out, commit, reveal) = inscribe::inscribe_blob(
        &blob,
        &anchor_tx,
        utxo.outpoint,
        utxo.value.to_sat(),
        &sk,
        1,                    // sat/vB
        Network::Signet,
    ).map_err(anyhow::Error::msg)?;

    // ──────────────────────────────────────────────────────────────
    // 4. broadcast anchor+commit as package, then reveal
    // ──────────────────────────────────────────────────────────────
    backend.broadcast_package(&[anchor_tx.clone(), commit.clone()]).await?;
    backend.broadcast_raw(&reveal).await?;

    println!("anchor  {}", anchor_tx.compute_txid());
    println!("commit  {}", commit.compute_txid());
    println!("reveal  {}", reveal.compute_txid());
    Ok(())
} 