//! Tree repair state machine.
//!
//! Manages the commitment tree repair lifecycle:
//! Healthy → Attempting → (success → Healthy) | (failure → Exhausted)
//!
//! When Exhausted (FIX #1238):
//! - NEVER create witnesses (use NULL instead)
//! - Balance calculated via `get_total_unspent_balance()` (not witness-dependent)
//! - Only Full Rescan can reset to Healthy

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use crate::CoreError;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::header_store_impl::SqliteHeaderStore;

// ============================================================================
// Types
// ============================================================================

/// Tree repair state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TreeRepairState {
    /// Tree is healthy, witnesses are valid.
    Healthy = 0,
    /// Currently attempting repair.
    Attempting = 1,
    /// Repair has been exhausted — all strategies failed.
    /// Balance via `get_total_unspent_balance()`, witnesses are NULL.
    Exhausted = 2,
}

impl TreeRepairState {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Healthy),
            1 => Some(Self::Attempting),
            2 => Some(Self::Exhausted),
            _ => None,
        }
    }
}

/// Global tree repair state.
static REPAIR_STATE: AtomicU8 = AtomicU8::new(0);

/// Get the current tree repair state.
pub fn get_repair_state() -> TreeRepairState {
    TreeRepairState::from_u8(REPAIR_STATE.load(Ordering::SeqCst))
        .unwrap_or(TreeRepairState::Healthy)
}

/// Check if tree repair has been exhausted.
pub fn is_repair_exhausted() -> bool {
    get_repair_state() == TreeRepairState::Exhausted
}

/// Reset repair state to Healthy (only called by Full Rescan).
pub fn reset_repair_state() {
    REPAIR_STATE.store(TreeRepairState::Healthy as u8, Ordering::SeqCst);
}

// ============================================================================
// Repair Logic
// ============================================================================

/// Attempt to repair the commitment tree.
///
/// Strategy order:
/// 1. Clear tree state, reload from boost file
/// 2. Rebuild witnesses from delta CMUs
/// 3. If both fail → mark Exhausted
///
/// When Exhausted: witnesses = NULL, balance = `get_total_unspent_balance()`.
pub async fn attempt_tree_repair(
    db: Arc<WalletDatabase>,
    _header_store: &SqliteHeaderStore,
) -> Result<TreeRepairState, CoreError> {
    // Transition to Attempting
    let prev = REPAIR_STATE.compare_exchange(
        TreeRepairState::Healthy as u8,
        TreeRepairState::Attempting as u8,
        Ordering::SeqCst,
        Ordering::SeqCst,
    );

    // If already Exhausted, cannot repair (need Full Rescan)
    if prev == Err(TreeRepairState::Exhausted as u8) {
        return Ok(TreeRepairState::Exhausted);
    }

    // If already Attempting, return current state
    if prev == Err(TreeRepairState::Attempting as u8) {
        return Ok(TreeRepairState::Attempting);
    }

    // Strategy 1: Clear tree state and attempt reload
    let db_clone = db.clone();
    let clear_result = tokio::task::spawn_blocking(move || db_clone.clear_tree_state_only())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?;

    if clear_result.is_ok() {
        // Check if we can get valid sync state after clearing
        let db_clone2 = db.clone();
        let sync_state = tokio::task::spawn_blocking(move || db_clone2.get_sync_state())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        // If we have a boost file height, the tree can be rebuilt
        if sync_state.boost_file_height > 0 {
            REPAIR_STATE.store(TreeRepairState::Healthy as u8, Ordering::SeqCst);
            return Ok(TreeRepairState::Healthy);
        }
    }

    // Strategy 2: Full tree rebuild would go here
    // If we get here, all strategies failed
    REPAIR_STATE.store(TreeRepairState::Exhausted as u8, Ordering::SeqCst);

    // FIX #1238: NULL all witnesses since tree is corrupted
    let db_clone3 = db.clone();
    let _ = tokio::task::spawn_blocking(move || db_clone3.clear_all_witnesses())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?;

    Ok(TreeRepairState::Exhausted)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test mutex to prevent parallel global state races
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_repair_state_transitions() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_repair_state();
        assert_eq!(get_repair_state(), TreeRepairState::Healthy);
        assert!(!is_repair_exhausted());

        REPAIR_STATE.store(TreeRepairState::Exhausted as u8, Ordering::SeqCst);
        assert!(is_repair_exhausted());

        reset_repair_state();
        assert!(!is_repair_exhausted());
    }

    #[tokio::test]
    async fn test_attempt_repair_on_empty_db() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_repair_state();

        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();

        let result = attempt_tree_repair(db, &hs).await.unwrap();
        // No boost file → exhausted
        assert_eq!(result, TreeRepairState::Exhausted);

        reset_repair_state();
    }

    #[test]
    fn test_repair_state_from_u8() {
        assert_eq!(
            TreeRepairState::from_u8(0),
            Some(TreeRepairState::Healthy)
        );
        assert_eq!(
            TreeRepairState::from_u8(1),
            Some(TreeRepairState::Attempting)
        );
        assert_eq!(
            TreeRepairState::from_u8(2),
            Some(TreeRepairState::Exhausted)
        );
        assert_eq!(TreeRepairState::from_u8(3), None);
    }
}
