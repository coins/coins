//! Unified publishing interface supporting multiple formats
//!
//! This module provides a common interface for publishing sub-blocks using
//! different methods (OP_RETURN, Taproot annex, etc.) and auto-detecting
//! the format during IBD.

use bitcoin::{Transaction, OutPoint, Network, absolute::LockTime};
use bitcoin::secp256k1::SecretKey;
use serde::{Serialize, Deserialize};

/// Publishing format identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublishFormat {
    OpReturn,
    TaprootAnnex,
}

impl PublishFormat {
    /// Convert format to nLocktime value for encoding in transaction
    pub fn to_locktime(&self) -> LockTime {
        match self {
            Self::OpReturn => LockTime::ZERO,                              // 0 (bit 0 = 0)
            Self::TaprootAnnex => LockTime::from_height(1).unwrap(),      // 1 (bit 0 = 1)
        }
    }

    /// Detect format from transaction's nLocktime field
    pub fn from_locktime(locktime: LockTime) -> Self {
        match locktime.to_consensus_u32() & 1 {
            0 => Self::OpReturn,
            1 => Self::TaprootAnnex,
            _ => unreachable!()
        }
    }

    /// Get the alternate format
    pub fn other(&self) -> Self {
        match self {
            Self::OpReturn => Self::TaprootAnnex,
            Self::TaprootAnnex => Self::OpReturn,
        }
    }
}

/// Publishing mode configuration
#[derive(Debug, Clone)]
pub enum PublishMode {
    /// Publish using a single format
    Single(PublishFormat),
    /// Publish using both formats simultaneously (optimistic dual-broadcast)
    Dual {
        /// Primary format (used for anchor tracking)
        primary: PublishFormat,
        /// Secondary format (best-effort)
        secondary: PublishFormat,
    },
}

/// Result of a publish operation
pub struct PublishResult {
    /// New anchor outpoint for next publication
    pub new_anchor: OutPoint,
    /// Data transaction containing the published sub-block
    pub data_tx: Transaction,
    /// Format used for this publication
    pub format: PublishFormat,
}

/// Unified publishing interface
///
/// Dispatches to the appropriate publishing method based on format.
pub fn publish_subblock(
    blob: &[u8],
    anchor_tx: &Transaction,
    fee_outpoint: OutPoint,
    fee_value_sat: u64,
    fee_sk: &SecretKey,
    fee_rate_sat_per_vb: u64,
    network: Network,
    format: PublishFormat,
) -> Result<PublishResult, String> {
    match format {
        PublishFormat::OpReturn => {
            let (new_anchor, data_tx) = crate::op_return::publish_op_return(
                blob,
                anchor_tx,
                fee_outpoint,
                fee_value_sat,
                fee_sk,
                fee_rate_sat_per_vb,
                network,
            )?;
            Ok(PublishResult {
                new_anchor,
                data_tx,
                format,
            })
        }
        PublishFormat::TaprootAnnex => {
            let (new_anchor, data_tx) = crate::taproot_annex::publish_taproot_annex(
                blob,
                anchor_tx,
                fee_outpoint,
                fee_value_sat,
                fee_sk,
                fee_rate_sat_per_vb,
                network,
            )?;
            Ok(PublishResult {
                new_anchor,
                data_tx,
                format,
            })
        }
    }
}

/// Parse blob from transaction, auto-detecting format from nLocktime
///
/// Returns the blob data and the detected format, or None if parsing fails.
pub fn parse_blob_from_tx(tx: &Transaction) -> Option<(Vec<u8>, PublishFormat)> {
    let format = PublishFormat::from_locktime(tx.lock_time);

    let blob = match format {
        PublishFormat::OpReturn => {
            crate::op_return::parse_blob_from_op_return(tx)?
        }
        PublishFormat::TaprootAnnex => {
            crate::taproot_annex::parse_blob_from_taproot_annex(tx)?
        }
    };

    Some((blob, format))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_encoding() {
        // OP_RETURN should encode to locktime 0
        let op_return_locktime = PublishFormat::OpReturn.to_locktime();
        assert_eq!(op_return_locktime.to_consensus_u32(), 0);
        assert_eq!(op_return_locktime.to_consensus_u32() & 1, 0);

        // Taproot annex should encode to locktime 1
        let taproot_locktime = PublishFormat::TaprootAnnex.to_locktime();
        assert_eq!(taproot_locktime.to_consensus_u32(), 1);
        assert_eq!(taproot_locktime.to_consensus_u32() & 1, 1);
    }

    #[test]
    fn test_format_detection() {
        // Detect OP_RETURN from locktime 0
        let format = PublishFormat::from_locktime(LockTime::ZERO);
        assert_eq!(format, PublishFormat::OpReturn);

        // Detect Taproot from locktime 1
        let format = PublishFormat::from_locktime(LockTime::from_height(1).unwrap());
        assert_eq!(format, PublishFormat::TaprootAnnex);

        // Even values should be OP_RETURN
        let format = PublishFormat::from_locktime(LockTime::from_height(100).unwrap());
        assert_eq!(format, PublishFormat::OpReturn);

        // Odd values should be Taproot
        let format = PublishFormat::from_locktime(LockTime::from_height(101).unwrap());
        assert_eq!(format, PublishFormat::TaprootAnnex);
    }

    #[test]
    fn test_format_other() {
        assert_eq!(PublishFormat::OpReturn.other(), PublishFormat::TaprootAnnex);
        assert_eq!(PublishFormat::TaprootAnnex.other(), PublishFormat::OpReturn);
    }
}
