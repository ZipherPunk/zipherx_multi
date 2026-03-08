//! Sync orchestration — manages startup, delta sync, gap-fill, and background sync.
//!
//! CRITICAL INVARIANTS:
//! - Delta bundle is IMMUTABLE after `DeltaBundleVerified=true` (FIX #1252)
//!   Only append new blocks from tip. Never re-validate or clear.
//! - `clearDeltaBundle()` BLOCKED when verified (FIX #1254)
//!   Only 3 callers use force: Full Rescan, wallet wipe, boost file update.
//! - NEVER advance endHeight when 0 blocks fetched (FIX #1262)
//! - `syncDeltaBundleIfNeeded` NEVER fills internal gaps (FIX #1220)
//!   Use `gapFillDeltaBundle()` for gaps.
//! - Gap-fill MUST wait for header sync (FIX #1220)
//! - Use HeaderStore for root validation, not P2P (FIX #1220)
//! - NEVER clear failedPeers on restart — ADD (FIX #1246)
//! - Truncate headers to chainTip (FIX #1250)
//! - Validate BEFORE persisting — snapshot, append, validate, persist (FIX #1194)
//! - Size guard for witness updates (FIX #1281)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::CoreError;

/// Sapling activation height for Zclassic mainnet.
pub const SAPLING_ACTIVATION_HEIGHT: u64 = 476_969;

// ============================================================================
// Types
// ============================================================================

/// Startup mode determination based on wallet state.
#[derive(Debug, Clone, PartialEq)]
pub enum StartupMode {
    /// < 3s — checkpoint exists, witnesses valid, tree loaded.
    /// Skip P2P sync, show cached balance, background catch-up.
    Instant,
    /// 10-30s — load tree from boost + delta, sync headers, validate witnesses.
    Fast,
    /// 2-5 min — fresh import or corrupted state.
    /// Download boost file, load 1M+ CMUs, full blockchain scan.
    Full,
}

/// Current status of the sync engine.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    /// No sync in progress.
    Idle,
    /// Downloading boost file from GitHub.
    BoostDownload {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    /// Loading headers from boost file into HeaderStore.
    BoostLoad {
        loaded: u64,
        total: u64,
    },
    /// Syncing block headers.
    HeaderSync {
        current_height: u64,
        target_height: u64,
    },
    /// Syncing delta CMU bundle.
    DeltaSync {
        current_height: u64,
        target_height: u64,
    },
    /// Scanning blocks for notes.
    BlockScan {
        current_height: u64,
        target_height: u64,
        notes_found: u32,
    },
    /// Filling gaps in delta bundle.
    GapFill {
        gaps_remaining: usize,
    },
    /// Updating witnesses with new CMUs.
    WitnessUpdate {
        notes_updated: usize,
        total_notes: usize,
    },
    /// Sync complete.
    Complete {
        height: u64,
    },
    /// Sync failed.
    Failed(String),
}

/// Progress callback for sync operations.
pub type SyncProgressFn = Arc<dyn Fn(SyncStatus) + Send + Sync>;

/// Delta sync configuration.
#[derive(Debug, Clone)]
pub struct DeltaSyncConfig {
    /// Maximum blocks to fetch per round.
    pub max_blocks_per_round: u64,
    /// Minimum coverage threshold (0.0 - 1.0).
    pub coverage_threshold: f64,
    /// Whether to perform gap-fill on startup.
    pub gap_fill_on_startup: bool,
}

impl Default for DeltaSyncConfig {
    fn default() -> Self {
        Self {
            max_blocks_per_round: 512,
            coverage_threshold: 0.5, // 50% threshold (FIX #1218)
            gap_fill_on_startup: true,
        }
    }
}

/// Result of a delta sync operation.
#[derive(Debug, Clone)]
pub struct DeltaSyncResult {
    /// New end height after sync.
    pub end_height: u64,
    /// Number of new CMUs appended.
    pub cmus_appended: u64,
    /// Number of new sapling roots stored.
    pub roots_stored: u64,
    /// Whether the sync was a no-op (already at tip).
    pub was_noop: bool,
}

/// Result of a gap-fill operation.
#[derive(Debug, Clone)]
pub struct GapFillResult {
    /// Number of gaps found.
    pub gaps_found: usize,
    /// Number of gaps successfully filled.
    pub gaps_filled: usize,
    /// Total CMUs recovered.
    pub cmus_recovered: u64,
}

/// Guard flags to prevent concurrent operations.
#[derive(Debug)]
pub struct SyncGuards {
    /// Whether sync is in progress.
    pub is_syncing: AtomicBool,
    /// Whether database repair is in progress.
    pub is_repairing: AtomicBool,
    /// Whether a broadcast is in progress (FIX #1184).
    pub is_broadcasting: AtomicBool,
    /// Whether gap-fill is in progress (FIX #1220).
    pub is_gap_filling: AtomicBool,
    /// Whether block scanning is in progress.
    pub is_scanning: AtomicBool,
    /// Whether witness rebuild is in progress (FIX #1239).
    pub is_rebuilding_witnesses: AtomicBool,
}

impl SyncGuards {
    pub fn new() -> Self {
        Self {
            is_syncing: AtomicBool::new(false),
            is_repairing: AtomicBool::new(false),
            is_broadcasting: AtomicBool::new(false),
            is_gap_filling: AtomicBool::new(false),
            is_scanning: AtomicBool::new(false),
            is_rebuilding_witnesses: AtomicBool::new(false),
        }
    }

    /// Check if background sync is allowed.
    ///
    /// Background sync MUST NOT run when any of these are active (FIX #1239):
    /// - Syncing, repairing, broadcasting, gap-filling, scanning, rebuilding witnesses.
    pub fn can_background_sync(&self) -> bool {
        !self.is_syncing.load(Ordering::SeqCst)
            && !self.is_repairing.load(Ordering::SeqCst)
            && !self.is_broadcasting.load(Ordering::SeqCst)
            && !self.is_gap_filling.load(Ordering::SeqCst)
            && !self.is_scanning.load(Ordering::SeqCst)
            && !self.is_rebuilding_witnesses.load(Ordering::SeqCst)
    }

    /// Try to acquire sync lock. Returns false if already syncing.
    pub fn try_acquire_sync(&self) -> bool {
        self.is_syncing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Release sync lock.
    pub fn release_sync(&self) {
        self.is_syncing.store(false, Ordering::SeqCst);
    }

    /// Try to acquire gap-fill lock.
    pub fn try_acquire_gap_fill(&self) -> bool {
        self.is_gap_filling
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Release gap-fill lock.
    pub fn release_gap_fill(&self) {
        self.is_gap_filling.store(false, Ordering::SeqCst);
    }
}

impl Default for SyncGuards {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Startup Mode Determination
// ============================================================================

/// Input state for determining startup mode.
#[derive(Debug, Clone)]
pub struct WalletState {
    /// Whether the tree state blob exists in DB.
    pub has_tree_state: bool,
    /// Current tree height (CMU count).
    pub tree_height: u64,
    /// Last scanned block height.
    pub last_scanned_height: u64,
    /// Whether delta bundle has been verified.
    pub delta_bundle_verified: bool,
    /// Delta bundle end height (0 if no bundle).
    pub delta_end_height: u64,
    /// Boost file height used for initial load.
    pub boost_file_height: u64,
    /// Boost CMU count loaded.
    pub boost_cmu_count: u64,
    /// Whether any unspent notes exist with valid witnesses.
    pub has_valid_witnesses: bool,
    /// Current chain tip from peers.
    pub chain_tip: u64,
}

/// Determine the appropriate startup mode based on wallet state.
///
/// - **Instant**: Tree loaded + verified delta + witnesses valid → < 3s.
/// - **Fast**: Tree exists but needs delta catch-up → 10-30s.
/// - **Full**: No tree state or corrupted → 2-5 min.
pub fn determine_startup_mode(state: &WalletState) -> StartupMode {
    // No tree state at all → Full rescan
    if !state.has_tree_state || state.tree_height == 0 {
        return StartupMode::Full;
    }

    // Tree exists but hasn't been through boost scan
    if state.boost_file_height == 0 && state.tree_height == 0 {
        return StartupMode::Full;
    }

    // Delta verified + witnesses valid + close to tip → Instant
    if state.delta_bundle_verified && state.has_valid_witnesses {
        // Within 100 blocks of tip = instant (background catch-up)
        if state.chain_tip > 0 && state.chain_tip.saturating_sub(state.last_scanned_height) <= 100 {
            return StartupMode::Instant;
        }
        // Delta verified but further behind → Fast (catch up from delta tip)
        return StartupMode::Fast;
    }

    // Tree loaded but delta not verified → Fast (validate + catch up)
    if state.has_tree_state && state.tree_height > 0 {
        return StartupMode::Fast;
    }

    StartupMode::Full
}

// ============================================================================
// Delta Sync Logic
// ============================================================================

/// Calculate the height range for a delta sync operation.
///
/// Returns (start_height, end_height) for the next sync range.
/// Cap end_height to header store height (FIX #1250).
///
/// Returns None if no sync is needed.
pub fn calculate_delta_sync_range(
    delta_end_height: u64,
    chain_tip: u64,
    header_store_height: u64,
) -> Option<(u64, u64)> {
    if chain_tip == 0 || header_store_height == 0 {
        return None;
    }

    let start = delta_end_height + 1;
    // Cap to header store height — peer may report beyond consensus (FIX #1250)
    let end = chain_tip.min(header_store_height);

    if start > end {
        return None; // Already caught up
    }

    Some((start, end))
}

/// Validate a delta sync result — NEVER advance when 0 blocks fetched (FIX #1262).
///
/// Returns the new end height to store, or None if the result should be discarded.
pub fn validate_delta_sync_result(
    blocks_fetched: u64,
    _cmus_appended: u64,
    previous_end_height: u64,
    reported_end_height: u64,
) -> Option<u64> {
    // FIX #1262: NEVER advance endHeight when 0 blocks fetched
    if blocks_fetched == 0 {
        return None;
    }

    // Must have appended at least some CMUs (unless all blocks were empty)
    // The new end height should be at least the previous
    if reported_end_height < previous_end_height {
        return None;
    }

    Some(reported_end_height)
}

/// Calculate the size guard for tree operations (FIX #978/#1182/#1281).
///
/// Returns the number of CMUs already in the tree beyond the boost count.
/// These must be skipped when applying delta CMUs to avoid double-append.
pub fn calculate_size_guard(tree_size: u64, boost_cmu_count: u64) -> u64 {
    tree_size.saturating_sub(boost_cmu_count)
}

/// Calculate how many delta CMUs to skip based on the size guard.
///
/// When witnesses are loaded from DB, they already have N delta CMUs in their
/// merkle paths. Skip those to avoid double-apply (FIX #1281).
pub fn calculate_witness_skip_count(tree_size: u64, boost_cmu_count: u64) -> usize {
    calculate_size_guard(tree_size, boost_cmu_count) as usize
}

// ============================================================================
// Gap Detection
// ============================================================================

/// A gap in the delta CMU bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct DeltaGap {
    /// First missing height.
    pub start: u64,
    /// Last missing height (inclusive).
    pub end: u64,
}

impl DeltaGap {
    /// Number of blocks in this gap.
    pub fn block_count(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Detect gaps in a sorted list of heights.
///
/// Heights must be sorted ascending. Returns gaps between consecutive heights.
pub fn detect_gaps(heights: &[u64], expected_start: u64, expected_end: u64) -> Vec<DeltaGap> {
    let mut gaps = Vec::new();

    if heights.is_empty() {
        if expected_end >= expected_start {
            gaps.push(DeltaGap {
                start: expected_start,
                end: expected_end,
            });
        }
        return gaps;
    }

    // Gap before first height
    if heights[0] > expected_start {
        gaps.push(DeltaGap {
            start: expected_start,
            end: heights[0] - 1,
        });
    }

    // Gaps between consecutive heights
    for window in heights.windows(2) {
        if window[1] > window[0] + 1 {
            gaps.push(DeltaGap {
                start: window[0] + 1,
                end: window[1] - 1,
            });
        }
    }

    // Gap after last height
    if let Some(&last) = heights.last() {
        if last < expected_end {
            gaps.push(DeltaGap {
                start: last + 1,
                end: expected_end,
            });
        }
    }

    gaps
}

/// Check if a delta bundle is incomplete (FIX #1219).
///
/// Incomplete != corrupt — too few CMUs means P2P failure, not corruption.
/// Preserve what we have and gap-fill.
///
/// RC-17: Uses integer arithmetic instead of floating-point to avoid precision
/// issues. The threshold is expressed as a fraction (numerator/denominator=1000).
/// A threshold of 0.5 becomes: actual * 1000 < expected * 500.
pub fn is_delta_incomplete(
    actual_cmu_count: u64,
    expected_output_count: u64,
    threshold: f64,
) -> bool {
    if expected_output_count == 0 {
        return false;
    }
    // Convert threshold to per-mille integer comparison to avoid float imprecision.
    // threshold=0.5 → threshold_permille=500, so we check:
    //   actual * 1000 < expected * 500
    let threshold_permille = (threshold.clamp(0.0, 1.0) * 1000.0) as u64;
    let lhs = actual_cmu_count.saturating_mul(1000);
    let rhs = expected_output_count.saturating_mul(threshold_permille);
    lhs < rhs
}

// ============================================================================
// Root Validation
// ============================================================================

/// Validate a tree root against a stored sapling root.
///
/// Checks BOTH byte orders — wire format vs FFI canonical are reversed (FIX #1230).
pub fn roots_match(computed_root: &[u8; 32], stored_root: &[u8; 32]) -> bool {
    if computed_root == stored_root {
        return true;
    }
    // Check reversed byte order (FIX #1230)
    let mut reversed = [0u8; 32];
    for i in 0..32 {
        reversed[i] = stored_root[31 - i];
    }
    computed_root == &reversed
}

/// Determine if a tree root validation passed.
///
/// The root must match either the header store root or the reversed form.
/// If no root is available in the header store, the validation cannot be performed.
pub fn validate_tree_root(
    computed_root: &[u8; 32],
    blockchain_root: Option<&[u8; 32]>,
) -> Result<bool, CoreError> {
    match blockchain_root {
        Some(root) => Ok(roots_match(computed_root, root)),
        None => {
            // No root available — cannot validate (FIX #1221)
            // NEVER bypass for unverifiable anchors
            Err(CoreError::InvalidAnchor)
        }
    }
}

// ============================================================================
// Header Sync Helpers
// ============================================================================

/// Calculate the number of headers needed for sync.
///
/// Range calculation MUST be INCLUSIVE (FIX #1249).
pub fn calculate_headers_needed(current_height: u64, chain_tip: u64) -> u64 {
    if chain_tip <= current_height {
        return 0; // Early-return when nothing to sync (FIX #1251)
    }
    chain_tip - current_height // This is the count (inclusive range)
}

/// Validate that a header store height is safe to use.
///
/// Truncate to chain tip — peer may send beyond consensus (FIX #1250).
pub fn safe_header_height(header_store_height: u64, chain_tip: u64) -> u64 {
    header_store_height.min(chain_tip)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Startup Mode Tests ----

    #[test]
    fn test_startup_mode_full_no_tree() {
        let state = WalletState {
            has_tree_state: false,
            tree_height: 0,
            last_scanned_height: 0,
            delta_bundle_verified: false,
            delta_end_height: 0,
            boost_file_height: 0,
            boost_cmu_count: 0,
            has_valid_witnesses: false,
            chain_tip: 1_000_000,
        };
        assert_eq!(determine_startup_mode(&state), StartupMode::Full);
    }

    #[test]
    fn test_startup_mode_instant() {
        let state = WalletState {
            has_tree_state: true,
            tree_height: 1_043_472,
            last_scanned_height: 2_951_900,
            delta_bundle_verified: true,
            delta_end_height: 2_951_900,
            boost_file_height: 2_951_853,
            boost_cmu_count: 1_043_472,
            has_valid_witnesses: true,
            chain_tip: 2_951_950, // 50 blocks behind
        };
        assert_eq!(determine_startup_mode(&state), StartupMode::Instant);
    }

    #[test]
    fn test_startup_mode_fast_verified_but_behind() {
        let state = WalletState {
            has_tree_state: true,
            tree_height: 1_043_472,
            last_scanned_height: 2_951_853,
            delta_bundle_verified: true,
            delta_end_height: 2_951_853,
            boost_file_height: 2_951_853,
            boost_cmu_count: 1_043_472,
            has_valid_witnesses: true,
            chain_tip: 2_952_500, // 647 blocks behind
        };
        assert_eq!(determine_startup_mode(&state), StartupMode::Fast);
    }

    #[test]
    fn test_startup_mode_fast_not_verified() {
        let state = WalletState {
            has_tree_state: true,
            tree_height: 500_000,
            last_scanned_height: 2_000_000,
            delta_bundle_verified: false,
            delta_end_height: 2_000_000,
            boost_file_height: 2_951_853,
            boost_cmu_count: 1_043_472,
            has_valid_witnesses: false,
            chain_tip: 2_951_900,
        };
        assert_eq!(determine_startup_mode(&state), StartupMode::Fast);
    }

    // ---- Delta Sync Range Tests ----

    #[test]
    fn test_delta_sync_range_normal() {
        let range = calculate_delta_sync_range(100_000, 100_500, 100_500);
        assert_eq!(range, Some((100_001, 100_500)));
    }

    #[test]
    fn test_delta_sync_range_already_caught_up() {
        let range = calculate_delta_sync_range(100_500, 100_500, 100_500);
        assert!(range.is_none());
    }

    #[test]
    fn test_delta_sync_range_capped_to_header_store() {
        // Chain tip 100_500 but header store only at 100_400 (FIX #1250)
        let range = calculate_delta_sync_range(100_000, 100_500, 100_400);
        assert_eq!(range, Some((100_001, 100_400)));
    }

    #[test]
    fn test_delta_sync_range_zero_chain() {
        assert!(calculate_delta_sync_range(0, 0, 0).is_none());
    }

    // ---- Validation Tests ----

    #[test]
    fn test_validate_delta_sync_zero_blocks() {
        // FIX #1262: NEVER advance when 0 blocks fetched
        let result = validate_delta_sync_result(0, 0, 100_000, 100_500);
        assert!(result.is_none());
    }

    #[test]
    fn test_validate_delta_sync_normal() {
        let result = validate_delta_sync_result(500, 1200, 100_000, 100_500);
        assert_eq!(result, Some(100_500));
    }

    #[test]
    fn test_validate_delta_sync_regression() {
        // End height should never go backwards
        let result = validate_delta_sync_result(10, 5, 100_000, 99_000);
        assert!(result.is_none());
    }

    // ---- Size Guard Tests ----

    #[test]
    fn test_size_guard_normal() {
        // Tree has 1_050_000 CMUs, boost loaded 1_043_472
        // So 6,528 delta CMUs are already in the tree
        assert_eq!(calculate_size_guard(1_050_000, 1_043_472), 6_528);
    }

    #[test]
    fn test_size_guard_exact_boost() {
        // Tree exactly at boost count — no delta CMUs yet
        assert_eq!(calculate_size_guard(1_043_472, 1_043_472), 0);
    }

    #[test]
    fn test_size_guard_underflow() {
        // Tree smaller than boost (shouldn't happen, but safe)
        assert_eq!(calculate_size_guard(500_000, 1_043_472), 0);
    }

    #[test]
    fn test_witness_skip_count() {
        assert_eq!(calculate_witness_skip_count(1_050_000, 1_043_472), 6_528);
    }

    // ---- Gap Detection Tests ----

    #[test]
    fn test_detect_gaps_no_gaps() {
        let heights = vec![100, 101, 102, 103, 104];
        let gaps = detect_gaps(&heights, 100, 104);
        assert!(gaps.is_empty());
    }

    #[test]
    fn test_detect_gaps_single_gap() {
        let heights = vec![100, 101, 105, 106];
        let gaps = detect_gaps(&heights, 100, 106);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].start, 102);
        assert_eq!(gaps[0].end, 104);
        assert_eq!(gaps[0].block_count(), 3);
    }

    #[test]
    fn test_detect_gaps_multiple() {
        let heights = vec![100, 105, 110];
        let gaps = detect_gaps(&heights, 100, 110);
        assert_eq!(gaps.len(), 2);
        assert_eq!(gaps[0], DeltaGap { start: 101, end: 104 });
        assert_eq!(gaps[1], DeltaGap { start: 106, end: 109 });
    }

    #[test]
    fn test_detect_gaps_empty_input() {
        let gaps = detect_gaps(&[], 100, 200);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], DeltaGap { start: 100, end: 200 });
    }

    #[test]
    fn test_detect_gaps_leading_gap() {
        let heights = vec![105, 106, 107];
        let gaps = detect_gaps(&heights, 100, 107);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], DeltaGap { start: 100, end: 104 });
    }

    #[test]
    fn test_detect_gaps_trailing_gap() {
        let heights = vec![100, 101, 102];
        let gaps = detect_gaps(&heights, 100, 110);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], DeltaGap { start: 103, end: 110 });
    }

    // ---- Incomplete Delta Tests ----

    #[test]
    fn test_delta_complete() {
        assert!(!is_delta_incomplete(1000, 1000, 0.5));
        assert!(!is_delta_incomplete(600, 1000, 0.5));
    }

    #[test]
    fn test_delta_incomplete() {
        // Less than 50% → incomplete (FIX #1218)
        assert!(is_delta_incomplete(400, 1000, 0.5));
        assert!(is_delta_incomplete(0, 1000, 0.5));
    }

    #[test]
    fn test_delta_incomplete_zero_expected() {
        assert!(!is_delta_incomplete(0, 0, 0.5));
    }

    // ---- Root Validation Tests ----

    #[test]
    fn test_roots_match_direct() {
        let root = [0xAB; 32];
        assert!(roots_match(&root, &root));
    }

    #[test]
    fn test_roots_match_reversed() {
        // FIX #1230: Wire format vs FFI canonical are reversed
        let _root_a = [0xAB; 32]; // Same byte repeated — reversed is same
        let mut root_b = [0u8; 32];
        for i in 0..32 {
            root_b[i] = (i as u8) + 1;
        }
        let mut root_b_reversed = [0u8; 32];
        for i in 0..32 {
            root_b_reversed[i] = root_b[31 - i];
        }
        assert!(roots_match(&root_b, &root_b_reversed));
    }

    #[test]
    fn test_roots_no_match() {
        let root_a = [0xAB; 32];
        let root_b = [0xCD; 32];
        assert!(!roots_match(&root_a, &root_b));
    }

    #[test]
    fn test_validate_tree_root_matches() {
        let root = [0xAB; 32];
        assert!(validate_tree_root(&root, Some(&root)).unwrap());
    }

    #[test]
    fn test_validate_tree_root_no_blockchain_root() {
        // FIX #1221: NEVER bypass for unverifiable anchors
        let root = [0xAB; 32];
        assert!(validate_tree_root(&root, None).is_err());
    }

    // ---- Header Sync Tests ----

    #[test]
    fn test_headers_needed_normal() {
        assert_eq!(calculate_headers_needed(100_000, 100_500), 500);
    }

    #[test]
    fn test_headers_needed_caught_up() {
        // FIX #1251: Early-return when nothing to sync
        assert_eq!(calculate_headers_needed(100_500, 100_500), 0);
        assert_eq!(calculate_headers_needed(100_600, 100_500), 0);
    }

    #[test]
    fn test_safe_header_height() {
        // FIX #1250: Truncate to chain tip
        assert_eq!(safe_header_height(100_500, 100_400), 100_400);
        assert_eq!(safe_header_height(100_400, 100_500), 100_400);
    }

    // ---- Sync Guards Tests ----

    #[test]
    fn test_sync_guards_initial_state() {
        let guards = SyncGuards::new();
        assert!(guards.can_background_sync());
    }

    #[test]
    fn test_sync_guards_blocks_background() {
        let guards = SyncGuards::new();
        guards.is_syncing.store(true, Ordering::SeqCst);
        assert!(!guards.can_background_sync());
    }

    #[test]
    fn test_sync_guards_try_acquire() {
        let guards = SyncGuards::new();
        assert!(guards.try_acquire_sync());
        assert!(!guards.try_acquire_sync()); // Already acquired
        guards.release_sync();
        assert!(guards.try_acquire_sync()); // Available again
    }

    #[test]
    fn test_sync_guards_broadcasting_blocks() {
        let guards = SyncGuards::new();
        guards.is_broadcasting.store(true, Ordering::SeqCst);
        assert!(!guards.can_background_sync());
    }

    #[test]
    fn test_sync_guards_rebuilding_witnesses_blocks() {
        // FIX #1239: MUST check !isRebuildingWitnesses
        let guards = SyncGuards::new();
        guards.is_rebuilding_witnesses.store(true, Ordering::SeqCst);
        assert!(!guards.can_background_sync());
    }
}
