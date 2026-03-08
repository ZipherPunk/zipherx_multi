//! ZSTD decompression for boost files and compressed data.

use std::io::{Read, Write};

use crate::types::CryptoError;

/// Maximum decompressed size: 2 GB. Prevents decompression bomb attacks (M-7).
const MAX_DECOMPRESS_SIZE: usize = 2_147_483_648;

/// Chunk size for incremental decompression reads.
const DECOMPRESS_CHUNK_SIZE: usize = 1024 * 1024; // 1 MB

/// Decompress ZSTD-compressed data in memory.
///
/// Reads in chunks and enforces a maximum decompressed size of 2 GB
/// to prevent decompression bomb attacks.
pub fn decompress(compressed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut decoder = zstd::Decoder::new(compressed)
        .map_err(|e| CryptoError::DecompressionFailed(format!("Init: {e}")))?;

    let mut output = Vec::new();
    let mut buf = [0u8; DECOMPRESS_CHUNK_SIZE];
    loop {
        let bytes_read = decoder
            .read(&mut buf)
            .map_err(|e| CryptoError::DecompressionFailed(format!("Read: {e}")))?;
        if bytes_read == 0 {
            break;
        }
        output.extend_from_slice(&buf[..bytes_read]);
        if output.len() > MAX_DECOMPRESS_SIZE {
            return Err(CryptoError::DecompressionFailed(format!(
                "Decompressed size exceeds maximum allowed ({} bytes)",
                MAX_DECOMPRESS_SIZE,
            )));
        }
    }

    Ok(output)
}

/// Decompress a ZSTD file to another file (streaming, low memory).
///
/// Uses ~512KB peak memory vs several GB for in-memory decompression.
/// Enforces a maximum decompressed size of 2 GB to prevent decompression bombs.
pub fn decompress_file(source_path: &str, dest_path: &str) -> Result<u64, CryptoError> {
    let source = std::fs::File::open(source_path)
        .map_err(|e| CryptoError::DecompressionFailed(format!("Open source: {e}")))?;

    let dest = std::fs::File::create(dest_path)
        .map_err(|e| CryptoError::DecompressionFailed(format!("Create dest: {e}")))?;

    let mut decoder = zstd::Decoder::new(source)
        .map_err(|e| CryptoError::DecompressionFailed(format!("Init: {e}")))?;

    let mut writer = std::io::BufWriter::with_capacity(512 * 1024, dest);
    let mut total_written: u64 = 0;
    let mut buf = [0u8; DECOMPRESS_CHUNK_SIZE];
    loop {
        let bytes_read = decoder
            .read(&mut buf)
            .map_err(|e| CryptoError::DecompressionFailed(format!("Read: {e}")))?;
        if bytes_read == 0 {
            break;
        }
        writer
            .write_all(&buf[..bytes_read])
            .map_err(|e| CryptoError::DecompressionFailed(format!("Write: {e}")))?;
        total_written += bytes_read as u64;
        if total_written > MAX_DECOMPRESS_SIZE as u64 {
            return Err(CryptoError::DecompressionFailed(format!(
                "Decompressed size exceeds maximum allowed ({} bytes)",
                MAX_DECOMPRESS_SIZE,
            )));
        }
    }

    writer
        .flush()
        .map_err(|e| CryptoError::DecompressionFailed(format!("Flush: {e}")))?;

    Ok(total_written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompress_roundtrip() {
        let original = b"Hello, ZipherX! This is test data for compression.";

        // Compress
        let compressed = zstd::encode_all(&original[..], 3).unwrap();

        // Decompress
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_decompress_empty() {
        let compressed = zstd::encode_all(&[][..], 3).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert!(decompressed.is_empty());
    }
}
