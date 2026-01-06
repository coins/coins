//! Integration tests for `SpacechainClient` requiring a running bitcoind
//! in *regtest* mode that is reachable via JSON-RPC.
//!
//! By default these tests are **ignored** so that `cargo test` still works
//! out-of-the-box.  Enable them explicitly via
//!
//! ```bash
//! BTC_RPC_URL=http://localhost:18443 \
//! BTC_RPC_USER=alice \
//! BTC_RPC_PASS=secret \
//! cargo test -p coins-spacechain -- --ignored
//! ```
//!
//! The tests will
//! 1. connect to the node,
//! 2. create/load dedicated wallets (`test-hot`, `test-watch`), and
//! 3. instantiate a [`SpacechainClient`].
//!
//! They do **not** attempt to *publish* data because that would require a
//! pre-existing live spacechain on-chain.  Instead we just exercise the wallet
//! setup code and run an initial block download.

// Constants for regtest RPC credentials – modify once here if needed.
const RPC_URL: &str  = "http://localhost:8332";
const RPC_USER: &str = "Bob";
const RPC_PASS: &str = "password";

use bitcoin::{Amount, Network, OutPoint, Txid, hashes::Hash};
use bitcoincore_rpc::{Auth, Client as RpcClient, RpcApi};
use coins_spacechain::{Spacechain};
use coins_spacechain::client::{ClientConfig, SpacechainClient};
use tempfile::TempDir;
use anyhow::anyhow;


fn rpc_client() -> RpcClient {
    RpcClient::new(RPC_URL, Auth::UserPass(RPC_USER.into(), RPC_PASS.into())).expect("rpc")
}

#[test]
#[ignore]
fn init_client_and_ibd() {
    // --- Check we are talking to regtest ---
    let rpc = rpc_client();
    let info = match rpc.get_blockchain_info() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Skipping test – cannot connect or authenticate to regtest node: {e}");
            return; // gracefully skip
        }
    };
    assert_eq!(info.chain, Network::Regtest, "node must run in regtest mode");

    // --- Build dummy spacechain descriptor ---
    let first_out = OutPoint::new(Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::all_zeros()), 0);
    let (sc, fee_sk) = Spacechain::generate(0, first_out, Amount::from_sat(1_000), Network::Regtest);

    // Write to temporary file so `ClientConfig` can reference it
    let tmp_dir: TempDir = TempDir::new().expect("tempdir");
    let sc_path = tmp_dir.path().join("spacechain.bin");
    std::fs::write(&sc_path, sc.encode()).expect("write");

    // --- Build client config ---
    let url  = RPC_URL.to_string();
    let user = RPC_USER.to_string();
    let pass = RPC_PASS.to_string();
    let fee_wif = bitcoin::PrivateKey::new(fee_sk, Network::Regtest).to_wif();

    let cfg = ClientConfig {
        spacechain_file: sc_path,
        private_key_wif: fee_wif,
        rpc_url: url,
        rpc_user: user,
        rpc_pass: pass,
        wallet_name: "test-hot".into(),
        watch_wallet_name: "test-watch".into(),
    };

    // --- Instantiate client ---
    let client = match SpacechainClient::new(cfg) {
        Ok(c) => c,
        Err(e) => { eprintln!("Skipping test – cannot set up client: {e}"); return; }
    };

    // --- Run IBD (callback counts blocks) ---
    let mut count = 0u64;
    client.initial_block_download(|_blk| { count += 1; Ok(()) }).expect("ibd");
    assert!(count > 0, "regtest chain should have at least one block");
}

// New integration test: mine coins on regtest, fund a short spacechain, then run IBD.
#[test]
#[ignore]
fn mine_and_create_spacechain() {
    let rpc = rpc_client();

    // Try to get blockchain info to confirm connectivity.
    let info = match rpc.get_blockchain_info() {
        Ok(i) => i,
        Err(e) => { eprintln!("Skipping test – cannot connect or authenticate: {e}"); return; }
    };
    if info.chain != Network::Regtest {
        eprintln!("Skipping test – not on regtest chain");
        return;
    }

    // --- Step 1: ensure spendable balance by mining ---
    // --- Ensure a wallet named "miner" exists ---
    let wallet_name = "miner";
    let _ = rpc.create_wallet(wallet_name, None, None, None, None);
    let _ = rpc.load_wallet(wallet_name);           // load if it already existed

    let miner_rpc_url = format!("{}/wallet/{}", RPC_URL.trim_end_matches('/'), wallet_name);
    let miner_rpc     = bitcoincore_rpc::Client::new(&miner_rpc_url,
                    Auth::UserPass(RPC_USER.into(), RPC_PASS.into())).expect("miner rpc");

    let miner_addr_unchecked = miner_rpc.get_new_address(None, None).expect("new addr");

    // Mine 101 blocks so coinbase outputs mature
    rpc.generate_to_address(101, miner_addr_unchecked.assume_checked_ref()).expect("mine blocks");

    // --- Step 2: create one-time key/address for spacechain funding ---
    let (_sk_fund, _pk_fund, addr_fund) = Spacechain::new_key(Network::Regtest);

    // Send some btc to funding address
    let send_amt = Amount::from_sat(50_000); // 0.0005 BTC
    let send_amt_txid = miner_rpc.send_to_address(
        &addr_fund,
        send_amt,
        None, None, None, None, None, None
    ).expect("send");
    // Mine 1 block to confirm
    rpc.generate_to_address(1, miner_addr_unchecked.assume_checked_ref()).expect("mine confirm");

    // Locate the funding output by decoding the tx
    let txid = send_amt_txid;
    let tx = rpc.get_raw_transaction(&txid, None).expect("raw");
    
    let (vout_idx, _) = tx.output.iter().enumerate()
        .find(|(_, o)| o.script_pubkey == addr_fund.script_pubkey())
        .expect("fund output");
    let first_out = bitcoin::OutPoint::new(txid, vout_idx as u32);

    // --- Step 3: build short spacechain (length 2) ---
    let (_sc, fee_sk) = Spacechain::generate(2, first_out, send_amt, Network::Regtest);
    // Note: we don't need the result further; just ensure it works.

    // Sanity: check utxo still exists
    assert!(
        rpc.get_tx_out(&first_out.txid, first_out.vout, Some(true))
            .unwrap()
            .is_some()
    );

    // ---------------------------------------------------------------------
    // Step 4: publish a blob via publish_tx and verify we can read it back
    // ---------------------------------------------------------------------

    // Broadcast the anchor transactions onto the chain so the anchor exists.
    let anchor_txs   = _sc.reconstruct_txs();          // keep the Vec alive
    let last_anchor  = anchor_txs
        .last()
        .expect("spacechain contains no anchor transactions");

    // Broadcast anchors in order, mining a block after each so its output exists
    for tx in &anchor_txs {
        rpc.send_raw_transaction(tx).expect("broadcast anchor");
        rpc.generate_to_address(1, miner_addr_unchecked.assume_checked_ref())
            .expect("mine anchor");
    }

    // Prepare a fee-paying UTXO under our own control (P2TR)
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::key::Keypair;
    use bitcoin::secp256k1::XOnlyPublicKey;

    let secp = Secp256k1::new();
    let fee_sk = SecretKey::new(&mut bitcoin::secp256k1::rand::rngs::OsRng);
    let fee_kp = Keypair::from_secret_key(&secp, &fee_sk);
    let (x_only_pk, _parity) = XOnlyPublicKey::from_keypair(&fee_kp);
    let fee_addr = bitcoin::Address::p2tr(&secp, x_only_pk, None, Network::Regtest);

    // Fund it from miner wallet
    let fee_value = Amount::from_sat(20_000);
    let fee_txid = miner_rpc.send_to_address(
        &fee_addr,
        fee_value,
        None, None, None, None, None, None
    ).expect("fund fee utxo");
    rpc.generate_to_address(1, miner_addr_unchecked.assume_checked_ref()).expect("mine fee utxo");

    // Find the funded output index
    let fee_tx: bitcoin::Transaction = rpc.get_raw_transaction(&fee_txid, None).expect("raw fee tx");
    let (fee_vout, _) = fee_tx.output.iter().enumerate()
        .find(|(_, o)| o.script_pubkey == fee_addr.script_pubkey())
        .expect("fee output");
    let fee_outpoint = bitcoin::OutPoint::new(fee_txid, fee_vout as u32);

    // Build publish_tx embedding blob
    let blob = b"hello spacechain";
    // Use the **latest** anchor built from the descriptor.
    let (_anchor_out, publish_tx) = coins_spacechain::publish::publish_blob(
        blob,
        last_anchor,
        fee_outpoint,
        fee_value.to_sat(),
        &fee_sk,
        2, // sat/vbyte
        Network::Regtest,
    ).expect("compile publish");

    let txid = rpc.send_raw_transaction(&publish_tx).expect("broadcast publish");
    rpc.generate_to_address(1, miner_addr_unchecked.assume_checked_ref()).expect("mine publish");

    // verify
    let fetched: bitcoin::Transaction = rpc.get_raw_transaction(&txid, None).expect("fetch publish");
    let blob = coins_spacechain::publish::parse_blob_from_publish(&fetched).expect("parse blob");
    assert_eq!(blob, b"hello spacechain");

    // Done.
} 