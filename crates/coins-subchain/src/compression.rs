//! Compression utilities for sub-block data
//!
//! Uses zstd for efficient compression of sub-block payloads.
//! Level 3 provides good balance between compression ratio and speed.

use std::io::{Read, Write};

/// Compress data using zstd (level 3 is a good balance of speed/compression)
pub fn compress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut encoder = zstd::Encoder::new(Vec::new(), 3)?;
    encoder.write_all(data)?;
    encoder.finish()
}

/// Decompress zstd-compressed data
pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = zstd::Decoder::new(compressed)?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_roundtrip() {
        let data = b"Hello, world! This is test data for compression.".repeat(100);
        let compressed = compress(&data).unwrap();
        let decompressed = decompress(&compressed).unwrap();

        assert_eq!(data.to_vec(), decompressed);
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_empty_data() {
        let data = b"";
        let compressed = compress(data).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(data.to_vec(), decompressed);
    }

    #[test]
    fn test_small_data() {
        let data = b"small";
        let compressed = compress(data).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(data.to_vec(), decompressed);
    }
}
