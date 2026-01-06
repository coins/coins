//! Finality tracking for 6-block Bitcoin confirmations

use crate::ChainBlock;

/// Number of Bitcoin confirmations required for finality
pub const FINALITY_DEPTH: u32 = 6;

/// Check if a block is finalized given the current Bitcoin height
pub fn is_finalized(btc_height: u32, current_height: u32) -> bool {
    current_height >= btc_height && (current_height - btc_height + 1) >= FINALITY_DEPTH
}

/// Check if a ChainBlock is finalized
pub fn is_block_finalized(block: &ChainBlock) -> bool {
    block.btc_confirmations >= FINALITY_DEPTH
}

/// Get finalized height given current Bitcoin tip
pub fn get_finalized_height(current_height: u32) -> u32 {
    if current_height >= FINALITY_DEPTH {
        current_height - FINALITY_DEPTH + 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_finalized() {
        // Block at height 100, current height 105
        assert!(!is_finalized(100, 104)); // 5 confirmations
        assert!(is_finalized(100, 105));  // 6 confirmations
        assert!(is_finalized(100, 110));  // 11 confirmations
    }

    #[test]
    fn test_get_finalized_height() {
        assert_eq!(get_finalized_height(0), 0);
        assert_eq!(get_finalized_height(5), 0);
        assert_eq!(get_finalized_height(6), 1);
        assert_eq!(get_finalized_height(10), 5);
        assert_eq!(get_finalized_height(100), 95);
    }
}
