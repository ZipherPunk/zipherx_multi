//! P2P block fetching — download and parse raw blocks for scanning.
//!
//! CRITICAL INVARIANTS:
//! - ALL fetches through dispatcher (FIX #1184) — no direct reads
//! - Multi-round: peers × 128 blocks per round (FIX #1189)
//! - NEVER advance cursor past unfetched blocks (FIX #1218)
//! - 50% threshold: <50% received = `BlockFetchFailed` (FIX #1218)
//! - Adaptive TCP pacing: 0/100/200ms per-peer ramp (FIX #1197)
//! - Delta CMUs MUST be sorted by block height (FIX #1199)

use crate::constants::MAX_BLOCKS_PER_REQUEST;
use crate::messages::{self, BlockHeader};
use crate::types::NetworkError;

// ============================================================================
// Types
// ============================================================================

/// A shielded output extracted from a raw block.
#[derive(Debug, Clone)]
pub struct ShieldedOutput {
    /// Transaction ID (32 bytes, internal byte order).
    pub txid: [u8; 32],
    /// Note commitment (32 bytes).
    pub cmu: [u8; 32],
    /// Ephemeral public key (32 bytes).
    pub epk: [u8; 32],
    /// Encrypted ciphertext (580 bytes).
    pub ciphertext: Vec<u8>,
    /// Value commitment (32 bytes, for OVK recovery).
    pub cv: [u8; 32],
}

/// A shielded spend extracted from a raw block.
#[derive(Debug, Clone)]
pub struct ShieldedSpend {
    /// Transaction ID (32 bytes, internal byte order).
    pub txid: [u8; 32],
    /// Nullifier (32 bytes).
    pub nullifier: [u8; 32],
}

/// A parsed block with extracted Sapling data.
#[derive(Debug, Clone)]
pub struct CompactBlock {
    /// Block height.
    pub height: u64,
    /// Block hash (32 bytes).
    pub hash: [u8; 32],
    /// Block timestamp.
    pub timestamp: u32,
    /// Final Sapling root from block header.
    pub final_sapling_root: [u8; 32],
    /// Shielded outputs in this block.
    pub outputs: Vec<ShieldedOutput>,
    /// Shielded spends in this block.
    pub spends: Vec<ShieldedSpend>,
}

/// Configuration for adaptive TCP pacing (FIX #1197).
#[derive(Debug, Clone)]
pub struct PacingConfig {
    /// Per-peer delay ramp (milliseconds): [0, 100, 200, ...].
    pub per_peer_delays_ms: Vec<u64>,
    /// Delay between fetch rounds (milliseconds).
    pub inter_round_delay_ms: u64,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            per_peer_delays_ms: vec![0, 100, 200],
            inter_round_delay_ms: 300,
        }
    }
}

/// Result of a batch block fetch.
#[derive(Debug)]
pub struct FetchResult {
    /// Successfully fetched blocks (sorted by height).
    pub blocks: Vec<CompactBlock>,
    /// Heights that were requested but not received.
    pub missing_heights: Vec<u64>,
    /// Number of fetch rounds performed.
    pub rounds: usize,
}

// ============================================================================
// Block Parsing
// ============================================================================

/// Parse a raw block into a CompactBlock, extracting Sapling outputs and spends.
///
/// The raw block format is:
/// - Block header (variable size, ~543 bytes)
/// - Transaction count (compact size)
/// - Transactions (variable)
///
/// Each Sapling transaction contains:
/// - vShieldedSpend: array of SpendDescription
/// - vShieldedOutput: array of OutputDescription
pub fn parse_raw_block(
    data: &[u8],
    height: u64,
    hash: [u8; 32],
) -> Result<CompactBlock, NetworkError> {
    if data.len() < BlockHeader::BASE_SIZE + 3 {
        return Err(NetworkError::Protocol(
            crate::types::ProtocolError::InsufficientData {
                needed: BlockHeader::BASE_SIZE + 3,
                available: data.len(),
            },
        ));
    }

    // Parse block header
    let (header, header_size) = BlockHeader::deserialize(data).ok_or_else(|| {
        NetworkError::Protocol(crate::types::ProtocolError::Malformed(
            "Failed to parse block header".into(),
        ))
    })?;

    let mut offset = header_size;

    // Parse transaction count
    let (tx_count, varint_size) = messages::read_compact_size(data, offset).ok_or_else(|| {
        NetworkError::Protocol(crate::types::ProtocolError::Malformed(
            "Failed to read tx count".into(),
        ))
    })?;
    offset += varint_size;
    // RN-4: Cap tx_count to prevent memory exhaustion from malformed blocks.
    // No valid Zclassic block has more than 100,000 transactions.
    if tx_count > 100_000 {
        return Err(NetworkError::Protocol(
            crate::types::ProtocolError::Malformed(format!(
                "Block tx_count {} exceeds maximum 100,000",
                tx_count
            )),
        ));
    }
    let tx_count = tx_count as usize;

    let mut outputs = Vec::new();
    let mut spends = Vec::new();

    // Parse each transaction
    for _tx_idx in 0..tx_count {
        if offset >= data.len() {
            break;
        }

        // Parse transaction and extract Sapling data
        match parse_transaction(&data[offset..]) {
            Some((tx_size, _txid, tx_outputs, tx_spends)) => {
                offset += tx_size;
                outputs.extend(tx_outputs);
                spends.extend(tx_spends);
            }
            None => {
                // Skip rest of block if a TX fails to parse
                break;
            }
        }
    }

    Ok(CompactBlock {
        height,
        hash,
        timestamp: header.timestamp,
        final_sapling_root: header.final_sapling_root,
        outputs,
        spends,
    })
}

/// Parse a single transaction, extracting Sapling outputs and spends.
///
/// Returns (bytes_consumed, txid, outputs, spends).
///
/// Sapling transaction format (v4):
/// - header (4) + version_group_id (4) = 8 bytes
/// - vin (transparent inputs)
/// - vout (transparent outputs)
/// - lock_time (4) + expiry_height (4) = 8 bytes
/// - value_balance (8) — only for Sapling v4
/// - vShieldedSpend
/// - vShieldedOutput
/// - binding_sig (64) — only if shielded data present
pub fn parse_transaction(
    data: &[u8],
) -> Option<(usize, [u8; 32], Vec<ShieldedOutput>, Vec<ShieldedSpend>)> {
    if data.len() < 12 {
        return None;
    }

    let start = 0;
    let mut offset = 0;

    // TX header: version (4 bytes) with overwintered flag in bit 31
    let version_raw = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
    let _is_overwintered = (version_raw >> 31) != 0;
    let version = (version_raw & 0x7FFFFFFF) as i32;
    offset += 4;

    // Version group ID (4 bytes)
    let _version_group = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
    offset += 4;

    // Transparent inputs (vin)
    let (vin_count, sz) = messages::read_compact_size(data, offset)?;
    // NET-002: Cap vin/vout counts to prevent memory exhaustion
    if vin_count > 10_000 {
        return None;
    }
    offset += sz;
    for _ in 0..vin_count {
        if offset + 36 > data.len() {
            return None;
        }
        offset += 36; // prevout hash (32) + index (4)
        let (script_len, sz) = messages::read_compact_size(data, offset)?;
        offset += sz;
        // NET-002: Checked addition to prevent overflow
        offset = offset
            .checked_add(script_len as usize)
            .filter(|&o| o <= data.len())?; // scriptSig
        if offset + 4 > data.len() {
            return None;
        }
        offset += 4; // sequence
    }

    // Transparent outputs (vout)
    let (vout_count, sz) = messages::read_compact_size(data, offset)?;
    // NET-002: Cap vin/vout counts to prevent memory exhaustion
    if vout_count > 10_000 {
        return None;
    }
    offset += sz;
    for _ in 0..vout_count {
        if offset + 8 > data.len() {
            return None;
        }
        offset += 8; // value
        let (script_len, sz) = messages::read_compact_size(data, offset)?;
        offset += sz;
        // NET-002: Checked addition to prevent overflow
        offset = offset
            .checked_add(script_len as usize)
            .filter(|&o| o <= data.len())?; // scriptPubKey
    }

    // lock_time (4) + expiry_height (4)
    if offset + 8 > data.len() {
        return None;
    }
    offset += 8;

    // txid is computed after we know the full TX size (see below)
    let mut outputs = Vec::new();
    let mut spends = Vec::new();

    // Sapling v4+ specific fields
    if version >= 4 {
        // value_balance (8 bytes)
        if offset + 8 > data.len() {
            return None;
        }
        offset += 8;

        // vShieldedSpend
        let (spend_count, sz) = messages::read_compact_size(data, offset)?;
        // NET-002: Cap spend/output counts to prevent memory exhaustion
        if spend_count > 10_000 {
            return None;
        }
        offset += sz;
        for _ in 0..spend_count {
            // SpendDescription: cv(32) + anchor(32) + nullifier(32) + rk(32) + zkproof(192) + sig(64) = 384
            if offset + 384 > data.len() {
                return None;
            }
            let mut nullifier = [0u8; 32];
            nullifier.copy_from_slice(&data[offset + 64..offset + 96]);
            spends.push(ShieldedSpend {
                txid: [0u8; 32],
                nullifier,
            });
            offset += 384;
        }

        // vShieldedOutput
        let (output_count, sz) = messages::read_compact_size(data, offset)?;
        // NET-002: Cap spend/output counts to prevent memory exhaustion
        if output_count > 10_000 {
            return None;
        }
        offset += sz;
        for _ in 0..output_count {
            // OutputDescription: cv(32) + cmu(32) + epk(32) + enc(580) + out(80) + zkproof(192) = 948
            if offset + 948 > data.len() {
                return None;
            }
            let mut cv = [0u8; 32];
            cv.copy_from_slice(&data[offset..offset + 32]);
            let mut cmu = [0u8; 32];
            cmu.copy_from_slice(&data[offset + 32..offset + 64]);
            let mut epk = [0u8; 32];
            epk.copy_from_slice(&data[offset + 64..offset + 96]);
            let ciphertext = data[offset + 96..offset + 676].to_vec(); // 580 bytes

            outputs.push(ShieldedOutput {
                txid: [0u8; 32],
                cmu,
                epk,
                ciphertext,
                cv,
            });
            offset += 948;
        }

        // vJoinSplit — MUST come BEFORE bindingSig per Zcash Sapling spec
        let (js_count, sz) = messages::read_compact_size(data, offset)?;
        offset += sz;
        if js_count > 0 {
            // Each JoinSplit is 1698 bytes (Groth16) or 1802 bytes (PHGR)
            // Skip them — we don't use Sprout
            for _ in 0..js_count {
                // Groth16 JoinSplit: 1698 bytes
                if offset + 1698 > data.len() {
                    return None;
                }
                offset += 1698;
            }
            // joinsplitPubKey (32) + joinsplitSig (64) if js_count > 0
            if offset + 96 > data.len() {
                return None;
            }
            offset += 96;
        }

        // Binding signature (64 bytes) — only if shielded spends or outputs present
        // Per Zcash spec, bindingSig is the LAST field in v4 transactions
        if !spends.is_empty() || !outputs.is_empty() {
            if offset + 64 > data.len() {
                return None;
            }
            offset += 64;
        }
    }

    // Compute txid: double-SHA256 of the full serialized transaction, reversed.
    // Must hash exactly data[start..offset] — the complete TX bytes.
    let txid = compute_txid(&data[start..offset]);

    // Patch txid into outputs and spends (they were built with zeroed txid)
    for o in &mut outputs {
        o.txid = txid;
    }
    for s in &mut spends {
        s.txid = txid;
    }

    Some((offset, txid, outputs, spends))
}

/// Compute txid: double-SHA256 of the full serialized transaction bytes.
/// Returns internal byte order (NOT reversed — reversal happens at display time).
fn compute_txid(tx_data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let first_pass = Sha256::digest(tx_data);
    let second_pass = Sha256::digest(&first_pass);
    let mut txid = [0u8; 32];
    txid.copy_from_slice(&second_pass);
    txid
}

/// Calculate the block hash from a block header (double SHA-256 of base header).
pub fn compute_block_hash(header: &BlockHeader) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let base = header.serialize();
    let first = Sha256::digest(&base);
    let second = Sha256::digest(&first);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&second);
    hash
}

/// Sort blocks by height (FIX #1199).
pub fn sort_blocks_by_height(blocks: &mut Vec<CompactBlock>) {
    blocks.sort_by_key(|b| b.height);
}

/// Calculate the number of rounds needed for a height range.
pub fn calculate_rounds(start: u64, end: u64, peer_count: usize) -> usize {
    if peer_count == 0 || end < start {
        return 0;
    }
    let total_blocks = (end - start + 1) as usize;
    let blocks_per_round = peer_count * MAX_BLOCKS_PER_REQUEST;
    (total_blocks + blocks_per_round - 1) / blocks_per_round
}

/// Check if a fetch result meets the 50% threshold (FIX #1218).
pub fn meets_threshold(received: usize, expected: usize) -> bool {
    if expected == 0 {
        return true;
    }
    received * 2 >= expected // >= 50%
}

/// Parse a raw transaction from P2P `tx` message payload.
/// Returns (txid, shielded_outputs, shielded_spends) or None.
pub fn parse_raw_tx(data: &[u8]) -> Option<([u8; 32], Vec<ShieldedOutput>, Vec<ShieldedSpend>)> {
    let (_size, txid, outputs, spends) = parse_transaction(data)?;
    Some((txid, outputs, spends))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_block_creation() {
        let block = CompactBlock {
            height: 500_000,
            hash: [0xAB; 32],
            timestamp: 1700000000,
            final_sapling_root: [0xCD; 32],
            outputs: vec![],
            spends: vec![],
        };
        assert_eq!(block.height, 500_000);
        assert!(block.outputs.is_empty());
        assert!(block.spends.is_empty());
    }

    #[test]
    fn test_calculate_rounds() {
        // 4 peers × 128 = 512 blocks per round
        assert_eq!(calculate_rounds(100, 611, 4), 1);
        assert_eq!(calculate_rounds(100, 612, 4), 2);
        assert_eq!(calculate_rounds(100, 100, 4), 1);
        assert_eq!(calculate_rounds(100, 100, 0), 0);
        assert_eq!(calculate_rounds(200, 100, 4), 0); // end < start
    }

    #[test]
    fn test_meets_threshold() {
        assert!(meets_threshold(50, 100)); // 50% exactly
        assert!(meets_threshold(51, 100)); // > 50%
        assert!(!meets_threshold(49, 100)); // < 50%
        assert!(meets_threshold(0, 0)); // empty is OK
        assert!(meets_threshold(1, 1)); // 100%
    }

    #[test]
    fn test_sort_blocks_by_height() {
        let mut blocks = vec![
            CompactBlock {
                height: 300,
                hash: [0; 32],
                timestamp: 0,
                final_sapling_root: [0; 32],
                outputs: vec![],
                spends: vec![],
            },
            CompactBlock {
                height: 100,
                hash: [0; 32],
                timestamp: 0,
                final_sapling_root: [0; 32],
                outputs: vec![],
                spends: vec![],
            },
            CompactBlock {
                height: 200,
                hash: [0; 32],
                timestamp: 0,
                final_sapling_root: [0; 32],
                outputs: vec![],
                spends: vec![],
            },
        ];
        sort_blocks_by_height(&mut blocks);
        assert_eq!(blocks[0].height, 100);
        assert_eq!(blocks[1].height, 200);
        assert_eq!(blocks[2].height, 300);
    }

    #[test]
    fn test_pacing_config_default() {
        let config = PacingConfig::default();
        assert_eq!(config.per_peer_delays_ms, vec![0, 100, 200]);
        assert_eq!(config.inter_round_delay_ms, 300);
    }

    #[test]
    fn test_parse_raw_block_too_short() {
        let result = parse_raw_block(&[0u8; 10], 100, [0; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_block_hash() {
        let header = BlockHeader {
            version: 4,
            prev_hash: [0; 32],
            merkle_root: [0; 32],
            final_sapling_root: [0; 32],
            timestamp: 1700000000,
            bits: 0x2007ffff,
            nonce: [0; 32],
            solution: vec![0; 400],
        };
        let hash = compute_block_hash(&header);
        assert_ne!(hash, [0; 32]);
    }

    #[test]
    fn test_shielded_output_fields() {
        let output = ShieldedOutput {
            txid: [1; 32],
            cmu: [2; 32],
            epk: [3; 32],
            ciphertext: vec![4; 580],
            cv: [5; 32],
        };
        assert_eq!(output.ciphertext.len(), 580);
        assert_eq!(output.txid, [1; 32]);
    }
}
