//! Block scanner — processes blocks, discovers notes, and tracks nullifiers.
//!
//! CRITICAL INVARIANTS:
//! - `treeAppend` is append-only with NO dedup (FIX #978/#1182)
//! - Size guard: `cmusAlreadyBeyondBoost = max(0, treeSize - boostCMUCount)` (FIX #978)
//! - MUST save DB tree height after appending CMUs (FIX #1182)
//! - Validate BEFORE persisting — snapshot, append, validate, persist (FIX #1194)
//! - `updateAllWitnessesBatch` MUST use same size guard (FIX #1281)
//! - Validate EACH witness root individually (FIX #1280)
//! - Block scanner finds nullifier → confirm → DB write → SETTLEMENT (FIX #1259)

use std::collections::HashMap;
use std::sync::Arc;

use zipherx_crypto::{
    notes::{self, DecryptedNote},
    transparent,
    types::{ENC_CIPHERTEXT_LEN, SPENDING_KEY_LENGTH},
};
use zipherx_network::block_fetcher::CompactBlock;

use crate::CoreError;

// ============================================================================
// Types
// ============================================================================

/// Scan progress callback type.
pub type ScanProgressFn = Arc<dyn Fn(ScanProgress) + Send + Sync>;

/// Progress information during scanning.
#[derive(Debug, Clone)]
pub struct ScanProgress {
    /// Current block height being scanned.
    pub current_height: u64,
    /// Target height to reach.
    pub target_height: u64,
    /// Total notes found so far.
    pub notes_found: u32,
    /// Current phase of scanning.
    pub phase: ScanPhase,
}

/// Phases of the scanning process.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanPhase {
    /// Downloading blocks from P2P network.
    BlockDownload,
    /// Running trial decryption on outputs.
    TrialDecrypt,
    /// Updating witnesses with new CMUs.
    WitnessUpdate,
    /// Complete.
    Complete,
}

/// A note discovered during block scanning.
#[derive(Debug, Clone)]
pub struct DiscoveredNote {
    /// Block height where this note was found.
    pub height: u64,
    /// Transaction ID (internal byte order).
    pub txid: [u8; 32],
    /// Note commitment (32 bytes).
    pub cmu: [u8; 32],
    /// Ephemeral public key (32 bytes).
    pub epk: [u8; 32],
    /// Encrypted ciphertext (580 bytes).
    pub ciphertext: Vec<u8>,
    /// Decrypted note data.
    pub note: DecryptedNote,
    /// Computed nullifier (32 bytes).
    pub nullifier: [u8; 32],
    /// Position in the commitment tree.
    pub tree_position: u64,
}

/// Result of scanning a range of blocks.
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// New notes found for this wallet.
    pub new_notes: Vec<DiscoveredNote>,
    /// Nullifiers found in block spends, paired with their txid and block height.
    pub spent_nullifiers: Vec<([u8; 32], [u8; 32], u64)>,
    /// Last block height successfully scanned.
    pub last_scanned_height: u64,
    /// Sapling roots from scanned blocks (height, root).
    pub sapling_roots: Vec<(u64, [u8; 32])>,
    /// Total CMUs appended to tree.
    pub cmus_appended: u64,
}

/// A transparent UTXO discovered during block scanning.
#[derive(Debug, Clone)]
pub struct DiscoveredUtxo {
    /// Block height where this UTXO was found.
    pub height: u64,
    /// Transaction ID (internal byte order).
    pub txid: [u8; 32],
    /// Output index within the transaction.
    pub output_index: u32,
    /// Raw scriptPubKey bytes.
    pub script_pubkey: Vec<u8>,
    /// Encoded transparent address (t1...).
    pub address: String,
    /// Value in zatoshis.
    pub value: u64,
    /// Whether this is a change address (internal chain).
    pub is_change: bool,
    /// BIP-44 child index.
    pub child_index: u32,
    /// Whether this UTXO belongs to an imported (WIF) key rather than a derived key.
    pub is_imported: bool,
}

/// A transparent spend detected during block scanning.
#[derive(Debug, Clone)]
pub struct DetectedTransparentSpend {
    /// Block height where the spend was found.
    pub height: u64,
    /// Transaction ID of the spending transaction.
    pub spending_txid: [u8; 32],
    /// Previous output txid being spent.
    pub prevout_txid: [u8; 32],
    /// Previous output index being spent.
    pub prevout_index: u32,
}

/// Set of transparent addresses derived from a seed for scanning.
///
/// Pre-derives a gap of addresses on both external and internal chains
/// to match against block outputs. Also holds imported (WIF) addresses.
#[derive(Debug, Clone)]
pub struct TransparentAddressSet {
    /// BIP-44 derived: (address, is_change, child_index)
    addresses: Vec<(String, bool, u32)>,
    /// Imported WIF: (address, db_id)
    imported: Vec<(String, i64)>,
}

impl TransparentAddressSet {
    /// Create an empty address set (for imported-only wallets with no seed).
    pub fn empty() -> Self {
        Self {
            addresses: Vec::new(),
            imported: Vec::new(),
        }
    }

    /// Derive addresses for scanning. Uses a gap limit to cover
    /// addresses that may have been used.
    pub fn from_seed(seed: &[u8], account: u32, gap_limit: u32) -> Self {
        let mut addresses = Vec::new();

        // External chain (receiving addresses)
        for i in 0..gap_limit {
            if let Ok(addr) = transparent::derive_transparent_address(seed, account, i) {
                addresses.push((addr, false, i));
            }
        }

        // Internal chain (change addresses)
        for i in 0..gap_limit {
            if let Ok(addr) = transparent::derive_transparent_change_address(seed, account, i) {
                addresses.push((addr, true, i));
            }
        }

        Self {
            addresses,
            imported: Vec::new(),
        }
    }

    /// Get the derived addresses.
    pub fn addresses(&self) -> &[(String, bool, u32)] {
        &self.addresses
    }

    /// Add an imported (WIF) transparent address for scanning.
    pub fn add_imported(&mut self, address: String, db_id: i64) {
        self.imported.push((address, db_id));
    }

    /// Get the imported addresses.
    pub fn imported_addresses(&self) -> &[(String, i64)] {
        &self.imported
    }

    /// Check if a scriptPubKey matches any of our derived or imported addresses.
    /// Returns (address, is_change, child_index, is_imported) if matched.
    pub fn match_script(&self, script: &[u8]) -> Option<(&str, bool, u32, bool)> {
        let addr = transparent::extract_address_from_script(script)?;
        let encoded = transparent::encode_transparent_address(&addr).ok()?;
        // Check seed-derived first
        for (a, is_change, idx) in &self.addresses {
            if *a == encoded {
                return Some((a.as_str(), *is_change, *idx, false));
            }
        }
        // Check imported
        for (a, _db_id) in &self.imported {
            if *a == encoded {
                return Some((a.as_str(), false, 0, true));
            }
        }
        None
    }
}

// ============================================================================
// Scanner
// ============================================================================

/// Scan a block for transparent UTXOs and spends matching our addresses.
pub fn scan_block_transparent(
    block: &CompactBlock,
    address_set: &TransparentAddressSet,
) -> (Vec<DiscoveredUtxo>, Vec<DetectedTransparentSpend>) {
    let mut utxos = Vec::new();
    let mut spends = Vec::new();

    // Check transparent outputs for matches
    for output in &block.transparent_outputs {
        if let Some((addr, is_change, child_index, is_imported)) =
            address_set.match_script(&output.script_pubkey)
        {
            utxos.push(DiscoveredUtxo {
                height: block.height,
                txid: output.txid,
                output_index: output.output_index,
                script_pubkey: output.script_pubkey.clone(),
                address: addr.to_string(),
                value: output.value,
                is_change,
                child_index,
                is_imported,
            });
        }
    }

    // Record all transparent inputs — the caller checks against known UTXOs
    for input in &block.transparent_inputs {
        spends.push(DetectedTransparentSpend {
            height: block.height,
            spending_txid: input.spending_txid,
            prevout_txid: input.prevout_txid,
            prevout_index: input.prevout_index,
        });
    }

    (utxos, spends)
}

/// Process a single compact block for note discovery.
///
/// Runs trial decryption on all shielded outputs using the spending key.
/// Returns discovered notes with computed nullifiers.
pub fn process_block(
    block: &CompactBlock,
    sk_bytes: &[u8],
    tree_position: u64,
) -> Result<(Vec<DiscoveredNote>, Vec<([u8; 32], [u8; 32], u64)>), CoreError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CoreError::Crypto("Invalid spending key length".into()));
    }

    let mut discovered = Vec::new();
    let mut spent_nullifiers = Vec::new();
    let mut position = tree_position;

    // Collect nullifiers from spends (with block height for confirmation tracking)
    for spend in &block.spends {
        spent_nullifiers.push((spend.nullifier, spend.txid, block.height));
    }

    // Try to decrypt each shielded output
    for output in &block.outputs {
        if output.ciphertext.len() < ENC_CIPHERTEXT_LEN {
            position += 1;
            continue;
        }

        // Try trial decryption with spending key
        match notes::try_decrypt_note_with_sk(
            sk_bytes,
            &output.epk,
            &output.cmu,
            &output.ciphertext,
            block.height,
        ) {
            Some(decrypted) => {
                // Compute nullifier for the discovered note
                let nullifier = notes::compute_nullifier(
                    sk_bytes,
                    &decrypted.diversifier,
                    decrypted.value,
                    &decrypted.rcm,
                    position,
                    decrypted.is_zip212,
                )
                .map_err(|e| CoreError::Crypto(format!("Nullifier computation failed: {e}")))?;

                // FIX #585/#1138: Use computed CMU for consistency
                let computed_cmu = notes::compute_cmu(
                    sk_bytes,
                    &decrypted.diversifier,
                    decrypted.value,
                    &decrypted.rcm,
                    decrypted.is_zip212,
                )
                .unwrap_or(output.cmu);

                discovered.push(DiscoveredNote {
                    height: block.height,
                    txid: output.txid,
                    cmu: computed_cmu,
                    epk: output.epk,
                    ciphertext: output.ciphertext.clone(),
                    note: decrypted,
                    nullifier,
                    tree_position: position,
                });
            }
            None => {}
        }

        position += 1;
    }

    Ok((discovered, spent_nullifiers))
}

/// Scan a range of blocks and aggregate results.
pub fn scan_blocks(
    blocks: &[CompactBlock],
    sk_bytes: &[u8],
    initial_tree_position: u64,
    progress: Option<&ScanProgressFn>,
) -> Result<ScanResult, CoreError> {
    let mut all_notes = Vec::new();
    let mut all_spent_nullifiers = Vec::new();
    let mut sapling_roots = Vec::new();
    let mut position = initial_tree_position;
    let mut last_height = 0u64;
    let mut cmus_appended: u64 = 0;

    for block in blocks.iter() {
        // Report progress
        if let Some(cb) = &progress {
            cb(ScanProgress {
                current_height: block.height,
                target_height: blocks.last().map(|b| b.height).unwrap_or(0),
                // RC-16: Safe conversion — avoids truncation if note count exceeds u32::MAX.
                notes_found: u32::try_from(all_notes.len()).unwrap_or(u32::MAX),
                phase: ScanPhase::TrialDecrypt,
            });
        }

        let (notes, spent) = process_block(block, sk_bytes, position)?;
        all_notes.extend(notes);
        all_spent_nullifiers.extend(spent);

        // Track sapling roots for validation
        if block.final_sapling_root != [0u8; 32] {
            sapling_roots.push((block.height, block.final_sapling_root));
        }

        // Advance tree position by number of outputs in this block
        let output_count = block.outputs.len() as u64;
        position += output_count;
        cmus_appended += output_count;

        last_height = block.height;
    }

    Ok(ScanResult {
        new_notes: all_notes,
        spent_nullifiers: all_spent_nullifiers,
        last_scanned_height: last_height,
        sapling_roots,
        cmus_appended,
    })
}

/// Check if a block contains a confirmation for any pending transactions.
///
/// Looks through the block's spends for nullifiers matching pending TXs.
/// This is the ONLY path for TX confirmation (FIX #1259).
pub fn check_block_for_confirmation(
    block: &CompactBlock,
    pending_nullifiers: &HashMap<[u8; 32], String>,
) -> Vec<(String, u64)> {
    let mut confirmations = Vec::new();

    for spend in &block.spends {
        if let Some(txid) = pending_nullifiers.get(&spend.nullifier) {
            confirmations.push((txid.clone(), block.height));
        }
    }

    confirmations
}

/// Extract all CMUs from a set of blocks (for tree appending).
///
/// Returns CMUs sorted by block height (FIX #1199).
pub fn extract_cmus_from_blocks(blocks: &[CompactBlock]) -> Vec<(u64, Vec<[u8; 32]>)> {
    let mut result: Vec<(u64, Vec<[u8; 32]>)> = blocks
        .iter()
        .map(|block| {
            let cmus: Vec<[u8; 32]> = block.outputs.iter().map(|o| o.cmu).collect();
            (block.height, cmus)
        })
        .collect();

    // Sort by height (FIX #1199)
    result.sort_by_key(|(h, _)| *h);
    result
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use zipherx_network::block_fetcher::{ShieldedOutput, ShieldedSpend};

    fn make_empty_block(height: u64) -> CompactBlock {
        CompactBlock {
            height,
            hash: [0; 32],
            timestamp: 0,
            final_sapling_root: [0xAB; 32],
            outputs: vec![],
            spends: vec![],
            transparent_outputs: vec![],
            transparent_inputs: vec![],
        }
    }

    fn make_block_with_outputs(height: u64, num_outputs: usize) -> CompactBlock {
        let outputs: Vec<ShieldedOutput> = (0..num_outputs)
            .map(|i| ShieldedOutput {
                txid: [i as u8; 32],
                cmu: [i as u8; 32],
                epk: [0; 32],
                ciphertext: vec![0; 580],
                cv: [0; 32],
            })
            .collect();

        CompactBlock {
            height,
            hash: [0; 32],
            timestamp: 0,
            final_sapling_root: [0xAB; 32],
            outputs,
            spends: vec![],
            transparent_outputs: vec![],
            transparent_inputs: vec![],
        }
    }

    fn test_sk() -> Vec<u8> {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let seed = zipherx_crypto::mnemonic::to_seed(phrase).unwrap();
        zipherx_crypto::keys::derive_spending_key(&seed, 0)
            .unwrap()
            .to_vec()
    }

    #[test]
    fn test_scan_empty_blocks() {
        let sk = test_sk();
        let blocks = vec![make_empty_block(100), make_empty_block(101)];
        let result = scan_blocks(&blocks, &sk, 0, None).unwrap();
        assert_eq!(result.new_notes.len(), 0);
        assert_eq!(result.last_scanned_height, 101);
        assert_eq!(result.cmus_appended, 0);
    }

    #[test]
    fn test_scan_blocks_with_unrelated_outputs() {
        let sk = test_sk();
        let blocks = vec![make_block_with_outputs(500, 3)];
        let result = scan_blocks(&blocks, &sk, 0, None).unwrap();
        // Garbage outputs won't decrypt
        assert_eq!(result.new_notes.len(), 0);
        assert_eq!(result.cmus_appended, 3);
    }

    #[test]
    fn test_scan_progress_callback() {
        let sk = test_sk();
        let blocks = vec![make_empty_block(100), make_empty_block(200)];
        let progress_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let pc = progress_count.clone();
        let callback: ScanProgressFn = Arc::new(move |_p| {
            pc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        scan_blocks(&blocks, &sk, 0, Some(&callback)).unwrap();
        assert_eq!(progress_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_check_block_for_confirmation() {
        let mut pending = HashMap::new();
        let nf = [0xAA; 32];
        pending.insert(nf, "txid_abc".to_string());

        let block = CompactBlock {
            height: 1000,
            hash: [0; 32],
            timestamp: 0,
            final_sapling_root: [0; 32],
            outputs: vec![],
            spends: vec![ShieldedSpend {
                txid: [0; 32],
                nullifier: nf,
            }],
            transparent_outputs: vec![],
            transparent_inputs: vec![],
        };

        let confirmations = check_block_for_confirmation(&block, &pending);
        assert_eq!(confirmations.len(), 1);
        assert_eq!(confirmations[0].0, "txid_abc");
        assert_eq!(confirmations[0].1, 1000);
    }

    #[test]
    fn test_check_block_no_confirmation() {
        let pending = HashMap::new();
        let block = make_empty_block(1000);
        let confirmations = check_block_for_confirmation(&block, &pending);
        assert!(confirmations.is_empty());
    }

    #[test]
    fn test_extract_cmus_from_blocks() {
        let blocks = vec![
            make_block_with_outputs(300, 2),
            make_block_with_outputs(100, 1),
            make_block_with_outputs(200, 3),
        ];
        let cmus = extract_cmus_from_blocks(&blocks);
        // Should be sorted by height
        assert_eq!(cmus[0].0, 100);
        assert_eq!(cmus[1].0, 200);
        assert_eq!(cmus[2].0, 300);
        assert_eq!(cmus[0].1.len(), 1);
        assert_eq!(cmus[1].1.len(), 3);
        assert_eq!(cmus[2].1.len(), 2);
    }

    #[test]
    fn test_scan_result_sapling_roots() {
        let sk = test_sk();
        let blocks = vec![make_empty_block(100), make_empty_block(101)];
        let result = scan_blocks(&blocks, &sk, 0, None).unwrap();
        assert_eq!(result.sapling_roots.len(), 2);
        assert_eq!(result.sapling_roots[0].0, 100);
        assert_eq!(result.sapling_roots[1].0, 101);
    }

    #[test]
    fn test_process_block_invalid_sk() {
        let block = make_empty_block(100);
        let result = process_block(&block, &[0u8; 16], 0);
        assert!(result.is_err());
    }
}
