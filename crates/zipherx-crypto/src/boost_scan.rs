//! Boost file scanning — parallel note discovery from pre-computed blockchain data.
//!
//! The boost file contains pre-computed shielded outputs and spends,
//! enabling fast initial wallet sync by scanning in parallel with Rayon.
//!
//! Output record (684 bytes): height(4) + index(4) + cmu(32) + epk(32) + ciphertext(580) + txid(32)
//! Spend record (68 bytes): height(4) + nullifier(32) + txid(32)

use std::collections::{HashMap, HashSet};

use ff::PrimeField;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use zcash_note_encryption::{
    EphemeralKeyBytes, ShieldedOutput, COMPACT_NOTE_SIZE, ENC_CIPHERTEXT_SIZE,
};
use zcash_primitives::{
    consensus::BlockHeight,
    sapling::{
        note_encryption::{
            try_sapling_compact_note_decryption, try_sapling_note_decryption,
            PreparedIncomingViewingKey, SaplingDomain,
        },
        Diversifier, Rseed,
    },
    zip32::sapling::ExtendedSpendingKey,
};

use crate::types::{
    CryptoError, ZclassicNetwork, BOOST_OUTPUT_SIZE, BOOST_SPEND_SIZE, SPENDING_KEY_LENGTH,
};

// ============================================================================
// Types
// ============================================================================

/// A note discovered during boost scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostScanNote {
    /// Block height where this note was received.
    pub height: u32,
    /// Position in the commitment tree.
    pub position: u64,
    /// Note value in zatoshis.
    pub value: u64,
    /// Diversifier (11 bytes).
    pub diversifier: [u8; 11],
    /// Randomness commitment (32 bytes) — raw rcm for BeforeZip212, raw rseed for AfterZip212.
    pub rcm: [u8; 32],
    /// Whether this note uses ZIP-212 (AfterZip212) rseed format.
    /// If true, `rcm` contains the rseed and actual rcm is PRF-derived.
    pub is_zip212: bool,
    /// Note commitment (32 bytes) — computed CMU for TX building consistency (FIX #585).
    pub cmu: [u8; 32],
    /// Nullifier (32 bytes).
    pub nullifier: [u8; 32],
    /// Whether this note has been spent.
    pub is_spent: bool,
    /// Height at which this note was spent (0 = unspent).
    pub spent_height: u32,
    /// Transaction ID that spent this note (32 bytes, zeros if unspent).
    pub spent_txid: [u8; 32],
    /// Transaction ID that created this note (32 bytes).
    pub received_txid: [u8; 32],
}

/// Aggregate result of a boost scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostScanResult {
    /// Total ZCL received (zatoshis).
    pub total_received: u64,
    /// Total ZCL spent (zatoshis).
    pub total_spent: u64,
    /// Current unspent balance (zatoshis).
    pub unspent_balance: u64,
    /// Number of notes found (received).
    pub notes_found: u32,
    /// Number of notes spent.
    pub notes_spent: u32,
    /// Number of spend records checked.
    pub spends_checked: u32,
}

/// Internal struct implementing ShieldedOutput for full boost decryption (580 bytes).
struct BoostOutput {
    epk: [u8; 32],
    cmu: [u8; 32],
    enc_ciphertext: [u8; 580],
}

impl ShieldedOutput<SaplingDomain<ZclassicNetwork>, ENC_CIPHERTEXT_SIZE> for BoostOutput {
    fn ephemeral_key(&self) -> EphemeralKeyBytes {
        EphemeralKeyBytes(self.epk)
    }
    fn cmstar_bytes(&self) -> [u8; 32] {
        self.cmu
    }
    fn enc_ciphertext(&self) -> &[u8; ENC_CIPHERTEXT_SIZE] {
        &self.enc_ciphertext
    }
}

/// Internal struct for compact decryption (first 52 bytes only, no MAC check).
/// Used as a fallback to detect notes where full decryption fails due to
/// corrupted memo/tag bytes but the actual note data (first 52 bytes) is intact.
struct CompactBoostOutput {
    epk: [u8; 32],
    cmu: [u8; 32],
    compact_ciphertext: [u8; 52],
}

impl ShieldedOutput<SaplingDomain<ZclassicNetwork>, COMPACT_NOTE_SIZE> for CompactBoostOutput {
    fn ephemeral_key(&self) -> EphemeralKeyBytes {
        EphemeralKeyBytes(self.epk)
    }
    fn cmstar_bytes(&self) -> [u8; 32] {
        self.cmu
    }
    fn enc_ciphertext(&self) -> &[u8; COMPACT_NOTE_SIZE] {
        &self.compact_ciphertext
    }
}

// ============================================================================
// Boost Scanning
// ============================================================================

/// Scan boost file outputs and discover notes belonging to this wallet.
///
/// Performs the complete Phase 1 scanning in Rust:
/// 1. Parse outputs (684 bytes each) and spends (68 bytes each)
/// 2. Parallel note decryption using Rayon
/// 3. Compute nullifiers for discovered notes
/// 4. Match nullifiers against spends to detect spent notes
///
/// # Arguments
/// * `sk_bytes` - ExtendedSpendingKey (169 bytes)
/// * `outputs_data` - Raw outputs section (684 bytes per output)
/// * `spends_data` - Raw spends section (68 bytes per spend)
///
/// # Security (RCR-6)
///
/// This function deserializes the spending key from `sk_bytes`. The `extsk`
/// is dropped at end of scope. Callers SHOULD wrap `sk_bytes` in
/// `zeroize::Zeroizing<Vec<u8>>` to ensure key material is securely zeroed.
pub fn scan_boost_outputs(
    sk_bytes: &[u8],
    outputs_data: &[u8],
    spends_data: &[u8],
) -> Result<(BoostScanResult, Vec<BoostScanNote>), CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let output_count = outputs_data.len() / BOOST_OUTPUT_SIZE;
    let spend_count = spends_data.len() / BOOST_SPEND_SIZE;

    if output_count == 0 {
        return Ok((
            BoostScanResult {
                total_received: 0,
                total_spent: 0,
                unspent_balance: 0,
                notes_found: 0,
                notes_spent: 0,
                spends_checked: u32::try_from(spend_count).unwrap_or(u32::MAX),
            },
            Vec::new(),
        ));
    }

    // Parse spending key and derive keys
    let extsk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let dfvk = extsk.to_diversifiable_full_viewing_key();
    let fvk = dfvk.fvk();
    let ivk = fvk.vk.ivk();
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);
    let nk = fvk.vk.nk;

    // Index spends: nullifier → (spend_height, txid)
    let mut nullifier_map: HashMap<[u8; 32], (u32, [u8; 32])> = HashMap::with_capacity(spend_count);
    for i in 0..spend_count {
        let offset = i * BOOST_SPEND_SIZE;
        let spend_height = u32::from_le_bytes([
            spends_data[offset],
            spends_data[offset + 1],
            spends_data[offset + 2],
            spends_data[offset + 3],
        ]);
        let mut nullifier = [0u8; 32];
        nullifier.copy_from_slice(&spends_data[offset + 4..offset + 36]);
        let mut txid = [0u8; 32];
        txid.copy_from_slice(&spends_data[offset + 36..offset + 68]);
        nullifier_map.insert(nullifier, (spend_height, txid));
    }

    // Parse output at index i directly from raw bytes — avoids ~700MB intermediate allocation.
    // Returns: (array_idx, height, index_field, cmu, epk, ciphertext, received_txid)
    let parse_output = |i: usize| -> (usize, u32, u32, [u8; 32], [u8; 32], [u8; 580], [u8; 32]) {
        let offset = i * BOOST_OUTPUT_SIZE;
        let height = u32::from_le_bytes([
            outputs_data[offset],
            outputs_data[offset + 1],
            outputs_data[offset + 2],
            outputs_data[offset + 3],
        ]);
        let index_field = u32::from_le_bytes([
            outputs_data[offset + 4],
            outputs_data[offset + 5],
            outputs_data[offset + 6],
            outputs_data[offset + 7],
        ]);
        let mut cmu = [0u8; 32];
        cmu.copy_from_slice(&outputs_data[offset + 8..offset + 40]);
        let mut epk = [0u8; 32];
        epk.copy_from_slice(&outputs_data[offset + 40..offset + 72]);
        let mut ciphertext = [0u8; 580];
        ciphertext.copy_from_slice(&outputs_data[offset + 72..offset + 652]);
        let mut received_txid = [0u8; 32];
        received_txid.copy_from_slice(&outputs_data[offset + 652..offset + 684]);
        (i, height, index_field, cmu, epk, ciphertext, received_txid)
    };

    // ====== DIAGNOSTIC: Analyze the index field (streaming, no allocation) ======
    {
        let mut index_eq_arraypos = 0u64;
        let mut index_ne_arraypos = 0u64;
        let mut min_index: u32 = u32::MAX;
        let mut max_index: u32 = 0;
        let mut first_mismatch: Option<(usize, u32, u32)> = None;
        let mut sample_indices: Vec<(usize, u32, u32)> = Vec::new();

        for i in 0..output_count {
            let offset = i * BOOST_OUTPUT_SIZE;
            let height = u32::from_le_bytes([
                outputs_data[offset],
                outputs_data[offset + 1],
                outputs_data[offset + 2],
                outputs_data[offset + 3],
            ]);
            let index_field = u32::from_le_bytes([
                outputs_data[offset + 4],
                outputs_data[offset + 5],
                outputs_data[offset + 6],
                outputs_data[offset + 7],
            ]);
            if index_field == i as u32 {
                index_eq_arraypos += 1;
            } else {
                index_ne_arraypos += 1;
                if first_mismatch.is_none() {
                    first_mismatch = Some((i, index_field, height));
                }
            }
            if index_field < min_index {
                min_index = index_field;
            }
            if index_field > max_index {
                max_index = index_field;
            }
            if sample_indices.len() < 10 {
                sample_indices.push((i, index_field, height));
            }
        }
        if cfg!(debug_assertions) {
            eprintln!(
                "[ZipherX] DIAG index field: range=[{}, {}], matches_arraypos={}, mismatches={}",
                min_index, max_index, index_eq_arraypos, index_ne_arraypos,
            );
            if let Some((pos, idx, h)) = first_mismatch {
                eprintln!(
                    "[ZipherX] DIAG index field: FIRST MISMATCH at array_pos={}, index_field={}, height={}",
                    pos, idx, h,
                );
            }
            eprintln!("[ZipherX] DIAG index field: first 10 samples:");
            for (pos, idx, h) in &sample_indices {
                eprintln!("  array_pos={}, index_field={}, height={}", pos, idx, h);
            }
        }
    }

    // Parallel decryption using Rayon — parses directly from outputs_data bytes,
    // avoiding the ~700MB intermediate Vec that caused OOM on Android.
    // Result tuple: (array_pos, height, index_field, value, diversifier, rcm, is_zip212, cmu, txid)
    let indices: Vec<usize> = (0..output_count).collect();
    let decrypted: Vec<_> = indices
        .par_iter()
        .filter_map(|&i| {
            let (position, height, index_field, cmu, epk, ciphertext, received_txid) =
                parse_output(i);

            let output = BoostOutput {
                epk,
                cmu,
                enc_ciphertext: ciphertext,
            };

            let block_height = BlockHeight::from_u32(height);
            let (note, address, _memo) = try_sapling_note_decryption(
                &ZclassicNetwork,
                block_height,
                &prepared_ivk,
                &output,
            )?;

            let diversifier = address.diversifier().0;
            let is_zip212 = matches!(note.rseed(), Rseed::AfterZip212(_));
            let rcm_repr = match note.rseed() {
                Rseed::BeforeZip212(rcm) => rcm.to_repr(),
                Rseed::AfterZip212(rseed) => *rseed,
            };

            Some((
                position as u64,
                height,
                index_field,
                note.value().inner(),
                diversifier,
                rcm_repr,
                is_zip212,
                cmu,
                received_txid,
            ))
        })
        .collect();

    if cfg!(debug_assertions) {
        eprintln!(
            "[ZipherX] DIAG decryption: {}/{} outputs decrypted successfully",
            decrypted.len(),
            output_count,
        );
    }

    // Collect array indices for compact fallback dedup (before consuming decrypted)
    let decrypted_array_indices: Vec<u64> = decrypted.iter().map(|(pos, ..)| *pos).collect();

    // Compute nullifiers and build results (sequential — fast)
    let mut notes = Vec::with_capacity(decrypted.len());
    let mut total_received: u64 = 0;
    let mut total_spent: u64 = 0;
    let mut notes_spent: u32 = 0;
    let mut cmu_mismatches: u32 = 0;
    let mut zip212_count: u32 = 0;

    for (
        array_pos,
        height,
        _index_field,
        value,
        diversifier,
        rcm_repr,
        is_zip212,
        boost_cmu,
        received_txid,
    ) in decrypted
    {
        // RCR-10: Checked add to prevent total_received overflow
        total_received = total_received
            .checked_add(value)
            .ok_or_else(|| CryptoError::InvalidData("total_received overflow".into()))?;
        if is_zip212 {
            zip212_count += 1;
        }

        // Get payment address for nullifier computation
        let div = Diversifier(diversifier);
        let payment_address = match fvk.vk.to_payment_address(div) {
            Some(addr) => addr,
            None => {
                if cfg!(debug_assertions) {
                    eprintln!(
                        "[ZipherX] WARN: invalid diversifier at array_pos {}, skipping",
                        array_pos
                    );
                }
                continue;
            }
        };

        // Create note with the correct Rseed type for nullifier computation.
        // ZIP-212 (AfterZip212) notes use a PRF-derived rcm from the rseed,
        // while pre-ZIP-212 notes store rcm directly.
        let note = if is_zip212 {
            payment_address.create_note(value, Rseed::AfterZip212(rcm_repr))
        } else {
            let rcm_scalar = match jubjub::Fr::from_repr(rcm_repr).into_option() {
                Some(r) => r,
                None => {
                    if cfg!(debug_assertions) {
                        eprintln!(
                            "[ZipherX] WARN: rcm from_repr failed at array_pos {}, skipping",
                            array_pos
                        );
                    }
                    continue;
                }
            };
            payment_address.create_note(value, Rseed::BeforeZip212(rcm_scalar))
        };

        // Use array_pos as the tree position for nullifier computation.
        // The boost file's index_field is per-block (0, 1, 2...), NOT global tree position.
        // array_pos = global output index = tree position when boost file is complete.
        let position = array_pos;
        let nullifier = note.nf(&nk, position);
        let nf_bytes = nullifier.0;

        // FIX #585: Use computed CMU for TX building consistency
        let computed_cmu = note.cmu();
        let computed_cmu_bytes: [u8; 32] = computed_cmu.to_bytes();
        let cmu_match = computed_cmu_bytes == boost_cmu;
        let cmu_to_store = if cmu_match {
            boost_cmu
        } else {
            cmu_mismatches += 1;
            computed_cmu_bytes
        };

        // Check if spent using array_pos as tree position
        // RCR-NEW-6: Nullifier matching uses HashMap lookup (variable-time hash + eq).
        // This is acceptable because nullifiers are public on-chain data, not secrets.
        // A future improvement could use constant-time comparison via `subtle::ConstantTimeEq`.
        let mut is_spent;
        let mut spent_height;
        let mut spent_txid;
        let mut final_nf = nf_bytes;
        let mut final_position = position;

        match nullifier_map.get(&nf_bytes) {
            Some(&(spend_h, txid)) => {
                is_spent = true;
                spent_height = spend_h;
                spent_txid = txid;
            }
            None => {
                is_spent = false;
                spent_height = 0u32;
                spent_txid = [0u8; 32];
            }
        };

        // If still not spent, probe nearby positions to catch position offsets
        // (boost file may be incomplete, causing array_pos to differ from real tree pos).
        //
        // RISK: This heuristic probes nearby offsets and could theoretically
        // match a wrong note if two notes share the same value at adjacent positions.
        // This is a best-effort optimization for boost scan only; the authoritative
        // nullifier positions are computed via witness-based recompute in async_sync.rs
        // after the full commitment tree is built.
        //
        // Trade-off: Fewer probes reduce false-positive risk at the cost of missing
        // some position mismatches. Extreme offsets (10k-100k) were removed because
        // position drifts that large indicate a fundamentally broken boost file, which
        // the witness-based recompute will correct anyway.
        if !is_spent {
            let mut probe_found = false;
            for delta in &[
                -3i64, -2, -1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, -10, 50, -50, 100, -100, 500, -500,
                1000, -1000, 5000, -5000,
            ] {
                let probe_pos = position as i64 + delta;
                if probe_pos < 0 {
                    continue;
                }
                let probe_nf = note.nf(&nk, probe_pos as u64);
                if let Some(&(spend_h, txid)) = nullifier_map.get(&probe_nf.0) {
                    if cfg!(debug_assertions) {
                        eprintln!(
                            "[ZipherX]   POSITION PROBE FIX: delta={} (pos {} → {}) value={}, marking SPENT",
                            delta, position, probe_pos, value,
                        );
                    }
                    is_spent = true;
                    spent_height = spend_h;
                    spent_txid = txid;
                    final_nf = probe_nf.0;
                    final_position = probe_pos as u64;
                    probe_found = true;
                    break;
                }
            }
            if !probe_found && cfg!(debug_assertions) {
                eprintln!(
                    "[ZipherX]     UNSPENT (confirmed): pos={}, value={} ({:.8} ZCL)",
                    position,
                    value,
                    value as f64 / 1e8,
                );
            }
        }

        if is_spent {
            total_spent = total_spent
                .checked_add(value)
                .ok_or_else(|| CryptoError::InvalidData("total_spent overflow".into()))?;
            notes_spent += 1;
        }

        // Log note summary
        if cfg!(debug_assertions) {
            eprintln!(
                "[ZipherX]   NOTE #{}: value={}, height={}, pos={}{}, spent={}, zip212={}",
                notes.len(),
                value,
                height,
                final_position,
                if final_position != position {
                    format!(" (corrected from {})", position)
                } else {
                    String::new()
                },
                is_spent,
                is_zip212,
            );
            if !cmu_match {
                eprintln!(
                    "[ZipherX]   CMU MISMATCH at pos={}: boost={}, computed={}, spent={}",
                    position,
                    hex::encode(&boost_cmu[..8]),
                    hex::encode(&computed_cmu_bytes[..8]),
                    is_spent,
                );
            }
        }

        notes.push(BoostScanNote {
            height,
            position: final_position,
            value,
            diversifier,
            rcm: rcm_repr,
            is_zip212,
            cmu: cmu_to_store,
            nullifier: final_nf,
            is_spent,
            spent_height,
            spent_txid,
            received_txid,
        });
    }

    // ====== DIAGNOSTIC: Diversifier analysis ======
    if cfg!(debug_assertions) {
        let unique_divs: HashSet<[u8; 11]> = notes.iter().map(|n| n.diversifier).collect();
        eprintln!(
            "[ZipherX] DIAG diversifiers: {} unique across {} notes",
            unique_divs.len(),
            notes.len(),
        );
        for div in &unique_divs {
            let at_div: Vec<_> = notes.iter().filter(|n| &n.diversifier == div).collect();
            let total_val: u64 = at_div.iter().map(|n| n.value).sum();
            let unspent_val: u64 = at_div.iter().filter(|n| !n.is_spent).map(|n| n.value).sum();
            let unspent_cnt = at_div.iter().filter(|n| !n.is_spent).count();
            eprintln!(
                "[ZipherX]   div={}... : {} notes ({} unspent), total_received={}, unspent={}",
                hex::encode(&div[..6]),
                at_div.len(),
                unspent_cnt,
                total_val,
                unspent_val,
            );
        }

        // ====== DIAGNOSTIC: Sorted unspent values for cross-reference ======
        let mut unspent_values: Vec<u64> = notes
            .iter()
            .filter(|n| !n.is_spent)
            .map(|n| n.value)
            .collect();
        unspent_values.sort();
        eprintln!(
            "[ZipherX] DIAG unspent values (sorted, {} notes): {:?}",
            unspent_values.len(),
            unspent_values,
        );
        let unspent_sum: u64 = unspent_values.iter().sum();
        eprintln!(
            "[ZipherX] DIAG unspent sum: {} zatoshis ({:.8} ZCL)",
            unspent_sum,
            unspent_sum as f64 / 1e8,
        );

        // ====== DIAGNOSTIC: All spent note values ======
        let mut spent_values: Vec<(u64, u32, [u8; 32], [u8; 32])> = notes
            .iter()
            .filter(|n| n.is_spent)
            .map(|n| (n.value, n.spent_height, n.spent_txid, n.received_txid))
            .collect();
        spent_values.sort_by_key(|(v, _, _, _)| *v);
        eprintln!(
            "[ZipherX] DIAG spent values (sorted, {} notes):",
            spent_values.len(),
        );
        for (val, sh, stxid, _rtxid) in &spent_values {
            eprintln!(
                "[ZipherX]   SPENT: value={}, spent_height={}, spent_txid={}...",
                val,
                sh,
                hex::encode(&stxid[..8]),
            );
        }
        let spent_sum: u64 = spent_values.iter().map(|(v, _, _, _)| *v).sum();
        eprintln!(
            "[ZipherX] DIAG spent sum: {} zatoshis ({:.8} ZCL)",
            spent_sum,
            spent_sum as f64 / 1e8,
        );
    }

    // ====== DIAGNOSTIC: Change output analysis ======
    // For each spending transaction, check if a change output exists
    // (a received note with the same txid as the spending txid).
    // Missing change outputs = missing balance.
    if cfg!(debug_assertions) {
        // Group spent notes by spending txid
        let mut spends_by_txid: HashMap<[u8; 32], Vec<&BoostScanNote>> = HashMap::new();
        for note in notes.iter().filter(|n| n.is_spent) {
            spends_by_txid
                .entry(note.spent_txid)
                .or_default()
                .push(note);
        }

        // Build received_txid → notes index
        let mut received_by_txid: HashMap<[u8; 32], Vec<&BoostScanNote>> = HashMap::new();
        for note in notes.iter() {
            received_by_txid
                .entry(note.received_txid)
                .or_default()
                .push(note);
        }

        eprintln!(
            "[ZipherX] DIAG change output analysis: {} unique spending transactions",
            spends_by_txid.len(),
        );

        let mut total_input: u64 = 0;
        let mut total_change: u64 = 0;
        let mut txs_with_change: u32 = 0;
        let mut txs_without_change: u32 = 0;

        for (spent_txid, spent_notes) in &spends_by_txid {
            let input_sum: u64 = spent_notes.iter().map(|n| n.value).sum();
            total_input += input_sum;
            let input_count = spent_notes.len();

            // Check if any RECEIVED note has this txid
            let change_notes: Vec<&&BoostScanNote> = received_by_txid
                .get(spent_txid)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            let change_sum: u64 = change_notes.iter().map(|n| n.value).sum();
            let change_count = change_notes.len();

            if change_count > 0 {
                txs_with_change += 1;
                total_change += change_sum;
                eprintln!(
                    "[ZipherX]   TX {}...: {} inputs={}, {} change={}, sent_out={}",
                    hex::encode(&spent_txid[..8]),
                    input_count,
                    input_sum,
                    change_count,
                    change_sum,
                    input_sum.saturating_sub(change_sum),
                );
            } else {
                txs_without_change += 1;
                eprintln!(
                    "[ZipherX]   TX {}...: {} inputs={}, NO CHANGE (all {} sent out)",
                    hex::encode(&spent_txid[..8]),
                    input_count,
                    input_sum,
                    input_sum,
                );
            }
        }

        eprintln!(
            "[ZipherX] DIAG change analysis: {} TXs with change ({} zatoshis), {} TXs without change",
            txs_with_change, total_change, txs_without_change,
        );
        eprintln!(
            "[ZipherX] DIAG change analysis: total_input={}, total_change={}, net_sent_out={}",
            total_input,
            total_change,
            total_input.saturating_sub(total_change),
        );

        eprintln!(
            "[ZipherX] Boost scan: {} ZIP-212 (AfterZip212) notes found out of {} total",
            zip212_count,
            notes.len(),
        );
        eprintln!(
            "[ZipherX] Boost scan nullifier stats: {} CMU mismatches out of {} notes, {} unspent, {} spends in file",
            cmu_mismatches, notes.len(), (notes.len() as u32).saturating_sub(notes_spent), spend_count,
        );
    }

    // ====== DIAGNOSTIC: Compact decryption fallback ======
    // Try compact decryption (52 bytes, no MAC check) on ALL outputs to find
    // notes that full decryption (580 bytes, MAC verified) missed.
    // This catches notes where bytes 52-579 are corrupted but the note data is intact.
    {
        // Build set of array indices already found by full decryption
        // (found_positions holds index_field values, so we also track array indices)
        let found_array_indices: HashSet<u64> = decrypted_array_indices.iter().copied().collect();

        let compact_hits: Vec<_> = indices
            .par_iter()
            .filter_map(|&i| {
                let (_array_pos, height, index_field, cmu, epk, ciphertext, received_txid) =
                    parse_output(i);

                // Skip outputs already found by full decryption
                if found_array_indices.contains(&(i as u64)) {
                    return None;
                }

                let mut compact_ct = [0u8; 52];
                compact_ct.copy_from_slice(&ciphertext[..52]);

                let output = CompactBoostOutput {
                    epk,
                    cmu,
                    compact_ciphertext: compact_ct,
                };

                let block_height = BlockHeight::from_u32(height);
                let (note, address) = try_sapling_compact_note_decryption(
                    &ZclassicNetwork,
                    block_height,
                    &prepared_ivk,
                    &output,
                )?;

                Some((
                    index_field as u64,
                    height,
                    note.value().inner(),
                    address.diversifier().0,
                    cmu,
                    received_txid,
                ))
            })
            .collect();

        if cfg!(debug_assertions) {
            if compact_hits.is_empty() {
                eprintln!(
                    "[ZipherX] DIAG compact fallback: 0 additional notes found — full decryption caught all notes",
                );
            } else {
                eprintln!(
                    "[ZipherX] *** COMPACT FALLBACK: {} notes found by compact that FULL decryption missed! ***",
                    compact_hits.len(),
                );
                eprintln!(
                    "[ZipherX]   This means the boost file has notes with corrupted MAC tag (bytes 52-579)",
                );
                let mut compact_total: u64 = 0;
                for (pos, height, value, div, cmu, txid) in &compact_hits {
                    compact_total += value;
                    eprintln!(
                        "[ZipherX]   COMPACT HIT: pos={}, height={}, value={}, div={}..., cmu={}..., txid={}...",
                        pos, height, value,
                        hex::encode(&div[..4]),
                        hex::encode(&cmu[..8]),
                        hex::encode(&txid[..8]),
                    );
                }
                eprintln!(
                    "[ZipherX]   COMPACT TOTAL: {} zatoshis from {} notes (spend status unknown — need full data for nullifier)",
                    compact_total, compact_hits.len(),
                );
            }
        }
    }

    let result = BoostScanResult {
        total_received,
        total_spent,
        unspent_balance: total_received.saturating_sub(total_spent),
        notes_found: u32::try_from(notes.len()).unwrap_or(u32::MAX),
        notes_spent,
        spends_checked: u32::try_from(spend_count).unwrap_or(u32::MAX),
    };

    Ok((result, notes))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{keys, mnemonic};

    fn test_sk() -> Vec<u8> {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let seed = mnemonic::to_seed(phrase).unwrap();
        keys::derive_spending_key(&seed, 0).unwrap().to_vec()
    }

    #[test]
    fn test_scan_empty_outputs() {
        let sk = test_sk();
        let (result, notes) = scan_boost_outputs(&sk, &[], &[]).unwrap();
        assert_eq!(result.notes_found, 0);
        assert_eq!(result.unspent_balance, 0);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_scan_invalid_sk() {
        let result = scan_boost_outputs(&[0u8; 16], &[], &[]);
        assert!(matches!(result, Err(CryptoError::InvalidSpendingKey)));
    }

    #[test]
    fn test_scan_no_matches() {
        let sk = test_sk();
        // Create fake output data (684 bytes of zeros — will not decrypt)
        let fake_output = vec![0u8; BOOST_OUTPUT_SIZE];
        let (result, notes) = scan_boost_outputs(&sk, &fake_output, &[]).unwrap();
        assert_eq!(result.notes_found, 0);
        assert!(notes.is_empty());
    }

    #[test]
    fn test_boost_scan_result_balance() {
        let result = BoostScanResult {
            total_received: 500_000,
            total_spent: 200_000,
            unspent_balance: 300_000,
            notes_found: 3,
            notes_spent: 1,
            spends_checked: 100,
        };
        assert_eq!(
            result.unspent_balance,
            result.total_received - result.total_spent
        );
    }
}
