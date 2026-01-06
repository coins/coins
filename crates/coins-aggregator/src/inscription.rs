use bitcoin::{Network, OutPoint, Transaction};
use bitcoin::secp256k1::SecretKey;
use coins_subchain::inscribe::{inscribe_blob, parse_blob_from_reveal};
use coins_types::SubBlock;

/// Build commit/reveal transactions that embed the given `subblock` in a Taproot-style
/// inscription. Internally delegates to `coins_subchain::inscribe::inscribe_blob`.
pub fn inscribe_subblock(
    subblock: &SubBlock,
    connector_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
) -> Result<(OutPoint, Transaction, Transaction), &'static str> {
    let blob = subblock.serialize();
    inscribe_blob(&blob, connector_tx, fee_outpoint, fee_value_sat, fee_sk, fee_rate_sat_per_vb, network)
}

/// Try to parse a SubBlock that was embedded in `reveal_tx`.
/// Returns `None` if parsing failed or the revealed data was not a valid SubBlock.
pub fn parse_subblock_from_reveal(reveal_tx: &Transaction) -> Option<SubBlock> {
    let blob = parse_blob_from_reveal(reveal_tx)?;
    SubBlock::deserialize(&blob)
} 