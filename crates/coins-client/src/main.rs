use clap::{Parser, Subcommand};
use coins_crypto::{G1, sign, SecretKey, rand_sk};
use rand::rngs::OsRng;
use ark_bn254::{Fr, G1Projective};
use ark_ff::{PrimeField, BigInteger};
use std::fs;
use std::path::PathBuf;
use coins_types::{Account, Transaction};
use bincode::serde::encode_to_vec;
use ark_ec::Group;
use std::ops::Mul;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "coins-client", about = "A simple CLI client for the Coins system")]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config/client.toml")]
    config: PathBuf,

    /// Override keyfile path from config
    #[arg(long)]
    keyfile: Option<PathBuf>,

    /// Override publisher URL from config
    #[arg(long)]
    publisher_url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default = "default_publisher_url")]
    publisher_url: String,
    #[serde(default = "default_keyfile")]
    keyfile: PathBuf,
}

fn default_publisher_url() -> String {
    "http://127.0.0.1:8080".to_string()  // Regtest default; signet uses :8081 (set in config)
}

fn default_keyfile() -> PathBuf {
    PathBuf::from(".data/keys/client_sk.hex")
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new secret key and store it
    Init,
    /// Fetch the account state and display the balance
    Balance,
    /// Send a number of coins to an address
    Send {
        /// The public key of the recipient (hex)
        #[arg(long)]
        recipient_pk: String,
        /// The amount of coins to send
        #[arg(long)]
        amount: u32,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config file or use defaults
    let config: Config = if cli.config.exists() {
        let config_str = fs::read_to_string(&cli.config)?;
        toml::from_str(&config_str)?
    } else {
        Config::default()
    };

    // Apply CLI overrides (CLI flags take priority over config file)
    let keyfile = cli.keyfile.unwrap_or(config.keyfile);
    let publisher_url = cli.publisher_url.unwrap_or(config.publisher_url);

    match cli.command {
        Commands::Init => {
            if keyfile.exists() {
                println!("Secret key file already exists at: {}", keyfile.display());
                println!("Aborting.");
                return Ok(());
            }

            let mut rng = OsRng;
            let sk = rand_sk(&mut rng);
            let pk = G1::from_affine(&G1Projective::generator().mul(sk.0).into());

            let sk_bytes = sk.0.into_bigint().to_bytes_le();

            // Create parent directory if it doesn't exist
            if let Some(parent) = keyfile.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&keyfile, hex::encode(sk_bytes))?;

            println!("New secret key stored in {}", keyfile.display());
            println!("Your public key is: {}", hex::encode(pk.0));
        }
        Commands::Balance => {
            if !keyfile.exists() {
                println!("Secret key file not found at: {}", keyfile.display());
                println!("Please run `init` first.");
                return Ok(());
            }

            let sk_hex = fs::read_to_string(&keyfile)?;
            let sk_bytes = hex::decode(sk_hex.trim())?;
            let fr = Fr::from_le_bytes_mod_order(&sk_bytes);
            let sk = SecretKey(fr);
            let pk = G1::from_affine(&G1Projective::generator().mul(sk.0).into());
            let pk_hex = hex::encode(pk.0);

            let client = reqwest::Client::new();
            let url = format!("{}/account/{}", publisher_url, pk_hex);
            let res = client.get(&url).send().await?;

            if res.status().is_success() {
                let account: Account = res.json().await?;
                println!("Balance: {}", account.balance);
            } else if res.status() == reqwest::StatusCode::NOT_FOUND {
                println!("Account not found. Your public key is: {}", pk_hex);
                println!("Please ensure the account has been funded.");
            } else {
                println!("Error fetching account: {}", res.status());
            }
        }
        Commands::Send { recipient_pk, amount } => {
            if !keyfile.exists() {
                println!("Secret key file not found at: {}", keyfile.display());
                println!("Please run `init` first.");
                return Ok(());
            }

            let sk_hex = fs::read_to_string(&keyfile)?;
            let sk_bytes = hex::decode(sk_hex.trim())?;
            let fr = Fr::from_le_bytes_mod_order(&sk_bytes);
            let sk = SecretKey(fr);

            let recipient_pk_bytes = hex::decode(recipient_pk)?;
            let recipient_pk_arr: [u8; 32] = recipient_pk_bytes.try_into().map_err(|_| anyhow::anyhow!("invalid recipient_pk length"))?;
            let recipient_pk = G1(recipient_pk_arr);

            // Fetch sender's account to get ID and nonce
            let pk = G1::from_affine(&G1Projective::generator().mul(sk.0).into());
            let pk_hex = hex::encode(pk.0);
            let client = reqwest::Client::new();
            let url = format!("{}/account/{}", publisher_url, pk_hex);
            let res = client.get(&url).send().await?;

            let sender_account: Account = if res.status().is_success() {
                res.json().await?
            } else {
                println!("Could not fetch sender account. Please ensure it is funded.");
                return Ok(());
            };

            let tx = Transaction {
                sender_id: sender_account.id.0,
                recipient_pk,
                amount,
                fee: 0, // Assuming no fee for now
            };

            let tx_bytes = encode_to_vec(&tx, bincode::config::standard())?;

            let signature = sign(&sk, &tx_bytes);

            let client = reqwest::Client::new();
            let res = client.post(format!("{}/tx", publisher_url))
                .json(&serde_json::json!({
                    "tx": hex::encode(tx_bytes),
                    "signature": hex::encode(signature.0)
                }))
                .send()
                .await?;

            if res.status().is_success() {
                println!("Transaction sent successfully!");
            } else {
                println!("Failed to send transaction: {}", res.status());
            }
        }
    }

    Ok(())
}
