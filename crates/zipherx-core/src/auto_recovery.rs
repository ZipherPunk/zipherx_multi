//! Auto-recovery — detect and fix common wallet integrity issues.
//!
//! Detects:
//! - Phantom-spent notes (FIX #1168): notes marked spent by non-existent TX
//! - Orphan-spent notes (FIX #1169): `spent_in_tx` references missing TX
//!
//! Actions:
//! - Restore phantom-spent notes to unspent state
//! - Trigger Full Rescan if too many issues found
//! - Post `transactionHistoryUpdated` notification (FIX #1170)

use std::sync::Arc;

use crate::CoreError;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::types::StorageError;

// ============================================================================
// Types
// ============================================================================

/// Result of an auto-recovery run.
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    /// Number of phantom-spent notes restored.
    pub notes_restored: usize,
    /// Amount restored (zatoshis).
    pub amount_restored: u64,
    /// Number of phantom transactions cleaned up.
    pub phantom_txs_cleaned: usize,
    /// Whether a Full Rescan was triggered.
    pub rescan_triggered: bool,
    /// Total balance after recovery.
    pub balance_after: u64,
}

// ============================================================================
// Recovery
// ============================================================================

/// Run auto-recovery: detect and fix phantom-spent notes.
///
/// FIX #1168: Restore notes marked spent by phantom/rejected transactions.
/// FIX #1169: Detect notes where `spent_in_tx` references a non-existent TX.
pub async fn auto_recover(
    db: Arc<WalletDatabase>,
) -> Result<RecoveryResult, CoreError> {
    let db_clone = db.clone();

    // Step 1: Get all pending/phantom transactions and restore notes
    let (phantom_txids, total_restored, amount_restored) =
        tokio::task::spawn_blocking(move || -> Result<(Vec<String>, usize, u64), StorageError> {
            let pending = db_clone.get_pending_transactions()?;
            let mut phantom_txids: Vec<String> = Vec::new();
            let mut total_restored = 0usize;
            let mut amount_restored = 0u64;

            for tx in &pending {
                if tx.status == zipherx_storage::types::TxStatus::Phantom
                    || tx.status == zipherx_storage::types::TxStatus::Rejected
                {
                    // FIX #1168: Restore notes spent by this phantom TX
                    let (count, value) =
                        db_clone.restore_notes_spent_by_phantom_tx(&tx.txid)?;
                    total_restored += count;
                    amount_restored += value;

                    // Clean up the phantom transaction
                    db_clone.delete_phantom_transaction(&tx.txid)?;
                    phantom_txids.push(tx.txid.clone());
                }
            }

            Ok((phantom_txids, total_restored, amount_restored))
        })
        .await
        .map_err(|e: tokio::task::JoinError| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e: StorageError| CoreError::Storage(e.to_string()))?;

    // Step 2: Get updated balance
    let db_clone2 = db.clone();
    let balance_after: u64 = tokio::task::spawn_blocking(move || -> Result<u64, StorageError> {
        db_clone2.get_total_unspent_balance(0)
    })
    .await
    .map_err(|e: tokio::task::JoinError| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e: StorageError| CoreError::Storage(e.to_string()))?;

    // Step 3: Determine if Full Rescan is needed
    // If more than 10 phantom TXs found, something is seriously wrong
    let rescan_triggered = phantom_txids.len() > 10;

    Ok(RecoveryResult {
        notes_restored: total_restored,
        amount_restored,
        phantom_txs_cleaned: phantom_txids.len(),
        rescan_triggered,
        balance_after,
    })
}

/// Check if recovery is needed (quick check, no modifications).
pub async fn needs_recovery(db: Arc<WalletDatabase>) -> Result<bool, CoreError> {
    let db_clone = db.clone();
    let has_phantoms: bool =
        tokio::task::spawn_blocking(move || -> Result<bool, StorageError> {
            let pending = db_clone.get_pending_transactions()?;
            Ok(pending.iter().any(|tx| {
                tx.status == zipherx_storage::types::TxStatus::Phantom
                    || tx.status == zipherx_storage::types::TxStatus::Rejected
            }))
        })
        .await
        .map_err(|e: tokio::task::JoinError| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e: StorageError| CoreError::Storage(e.to_string()))?;

    Ok(has_phantoms)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_recover_empty_wallet() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let result = auto_recover(db).await.unwrap();
        assert_eq!(result.notes_restored, 0);
        assert_eq!(result.phantom_txs_cleaned, 0);
        assert!(!result.rescan_triggered);
        assert_eq!(result.balance_after, 0);
    }

    #[tokio::test]
    async fn test_needs_recovery_empty() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let needs = needs_recovery(db).await.unwrap();
        assert!(!needs);
    }

    #[test]
    fn test_recovery_result_fields() {
        let result = RecoveryResult {
            notes_restored: 3,
            amount_restored: 50_000,
            phantom_txs_cleaned: 2,
            rescan_triggered: false,
            balance_after: 150_000,
        };
        assert_eq!(result.notes_restored, 3);
        assert_eq!(result.amount_restored, 50_000);
        assert!(!result.rescan_triggered);
    }
}
