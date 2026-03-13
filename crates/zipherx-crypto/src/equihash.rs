//! Equihash proof-of-work verification for block headers.
//!
//! Zclassic uses:
//! - Equihash(200,9) pre-Bubbles (solution = 1344 bytes)
//! - Equihash(192,7) post-Bubbles (solution = 400 bytes)
//!
//! Solution size auto-detection determines which parameters to use.

use crate::types::{
    CryptoError, BLOCK_HEADER_BASE_SIZE, EQUIHASH_SOLUTION_SIZE_192_7, EQUIHASH_SOLUTION_SIZE_200_9,
};
use sha2::{Digest, Sha256};

/// Verify an Equihash solution for a block header.
///
/// # Arguments
/// * `header_bytes` - 140-byte block header (input(108) + nonce(32))
/// * `solution` - Equihash solution (400 or 1344 bytes)
///
/// Auto-detects Equihash parameters from solution length.
pub fn verify(
    header_bytes: &[u8; BLOCK_HEADER_BASE_SIZE],
    solution: &[u8],
) -> Result<bool, CryptoError> {
    let (n, k) = match solution.len() {
        EQUIHASH_SOLUTION_SIZE_192_7 => (192u32, 7u32),
        EQUIHASH_SOLUTION_SIZE_200_9 => (200u32, 9u32),
        _ => return Err(CryptoError::InvalidBlockHeader),
    };

    // Use the equihash crate for verification
    let input = &header_bytes[..108]; // First 108 bytes (without nonce) as equihash input
    let nonce = &header_bytes[108..140]; // 32-byte nonce

    match equihash::is_valid_solution(n, k, input, nonce, solution) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Compute the double-SHA256 block hash from header + solution.
///
/// Zcash/Zclassic block hash = double_sha256(header_base(140) + compact_size(solution_len) + solution).
/// The compact_size varint prefix is part of the serialized header — omitting it produces wrong hashes.
pub fn compute_block_hash(
    header_bytes: &[u8; BLOCK_HEADER_BASE_SIZE],
    solution: &[u8],
) -> [u8; 32] {
    let mut data = Vec::with_capacity(BLOCK_HEADER_BASE_SIZE + 3 + solution.len());
    data.extend_from_slice(header_bytes);
    // CompactSize varint for solution length (same encoding as Bitcoin protocol)
    let sol_len = solution.len() as u64;
    if sol_len < 0xFD {
        data.push(sol_len as u8);
    } else if sol_len <= 0xFFFF {
        data.push(0xFD);
        data.extend_from_slice(&(sol_len as u16).to_le_bytes());
    } else {
        data.push(0xFE);
        data.extend_from_slice(&(sol_len as u32).to_le_bytes());
    }
    data.extend_from_slice(solution);

    let hash1 = Sha256::digest(&data);
    let hash2 = Sha256::digest(hash1);

    let mut result = [0u8; 32];
    result.copy_from_slice(&hash2);
    result
}

/// Verify a chain of block headers (each header's prev_hash matches the previous hash).
///
/// # Arguments
/// * `headers` - Concatenated header data (header_bytes + solution for each)
/// * `header_count` - Number of headers
/// * `expected_prev_hash` - Expected prev_hash of the first header
/// * `header_offsets` - Byte offset of each header in the data
/// * `header_sizes` - Size of each header in bytes (including solution)
pub fn verify_header_chain(
    headers_data: &[u8],
    header_count: usize,
    expected_prev_hash: &[u8; 32],
    header_offsets: &[usize],
    header_sizes: &[usize],
) -> Result<bool, CryptoError> {
    if header_count == 0 {
        return Ok(true);
    }

    if header_offsets.len() != header_count || header_sizes.len() != header_count {
        return Err(CryptoError::InvalidData(
            "Offset/size array mismatch".into(),
        ));
    }

    let mut prev_hash = *expected_prev_hash;

    for i in 0..header_count {
        let offset = header_offsets[i];
        let size = header_sizes[i];
        let end = offset
            .checked_add(size)
            .ok_or(CryptoError::InvalidBlockHeader)?;

        if end > headers_data.len() || size < BLOCK_HEADER_BASE_SIZE {
            return Err(CryptoError::InvalidBlockHeader);
        }

        let header_data = &headers_data[offset..end];

        // Check prev_hash matches (bytes 4..36 in header)
        if header_data[4..36] != prev_hash {
            return Ok(false);
        }

        // Extract header base and solution
        let header_base: [u8; BLOCK_HEADER_BASE_SIZE] = header_data[..BLOCK_HEADER_BASE_SIZE]
            .try_into()
            .map_err(|_| CryptoError::InvalidBlockHeader)?;
        let solution = &header_data[BLOCK_HEADER_BASE_SIZE..];

        // Verify Equihash
        if !verify(&header_base, solution)? {
            return Ok(false);
        }

        // Compute this block's hash for next iteration
        prev_hash = compute_block_hash(&header_base, solution);
    }

    Ok(true)
}
