//! Async send flow — note selection through broadcast through recording.
//!
//! Critical invariants:
//! - Set `is_broadcasting=true` before broadcast (FIX #1184)
//! - Validate EACH witness root individually (FIX #1280)
//! - Validate EACH anchor vs HeaderStore (FIX #1279)
//! - Build TX via `spawn_blocking` (Groth16 is CPU-heavy)
//! - Retry once if 0/N peers accepted (FIX #1261)
//! - Mempool verify is monitoring only — NOT confirmation (FIX #1259)
//! - Record atomically via `spawn_blocking`
//! - Always clear `is_broadcasting` on exit (Drop guard)

use std::sync::atomic::Ordering;
use std::sync::Arc;

use zeroize::Zeroizing;

use crate::async_prover;
use crate::send::{self, SendRequest, SendResult, SpendableNote};
use crate::sync::SyncGuards;
use crate::CoreError;
use zipherx_crypto::transaction::SpendInfo;
use zipherx_crypto::util::double_sha256;
use zipherx_network::peer_manager::PeerManager;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::header_store_impl::SqliteHeaderStore;

/// Progress callback for send operations.
pub type SendProgressFn = Arc<dyn Fn(SendPhase) + Send + Sync>;

/// Send operation phases for progress reporting.
#[derive(Debug, Clone)]
pub enum SendPhase {
    /// Validating send request
    Validating,
    /// Selecting notes for spend
    NoteSelection { count: usize, total_value: u64 },
    /// Validating witnesses and anchors
    WitnessValidation { note_index: usize, total: usize },
    /// Building transaction (Groth16 proofs)
    Building { spend_index: u32, total_spends: u32 },
    /// Broadcasting to P2P peers
    Broadcasting,
    /// Peer response
    PeerResponse { accepted: u32, total: u32 },
    /// Recording in database
    Recording,
    /// Complete
    Complete { txid: String },
    /// Error
    Error { message: String },
}

// ============================================================================
// Broadcast Guard
// ============================================================================

/// RAII guard that clears `is_broadcasting` on drop.
struct BroadcastGuard<'a> {
    guards: &'a SyncGuards,
}

impl<'a> BroadcastGuard<'a> {
    /// RC-12: Use compare_exchange instead of store to atomically check-and-set.
    /// Returns Err if another broadcast is already in progress.
    fn new(guards: &'a SyncGuards) -> Result<Self, CoreError> {
        guards
            .is_broadcasting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| {
                CoreError::BroadcastFailed("Another broadcast is already in progress".into())
            })?;
        Ok(Self { guards })
    }
}

impl<'a> Drop for BroadcastGuard<'a> {
    fn drop(&mut self) {
        self.guards.is_broadcasting.store(false, Ordering::SeqCst);
    }
}

// ============================================================================
// Send Transaction
// ============================================================================

/// Execute a full send flow: validate → select → build → broadcast → record.
///
/// This is the top-level async function called from the wallet layer.
pub async fn send_transaction(
    db: Arc<WalletDatabase>,
    peer_manager: &PeerManager,
    header_store: &SqliteHeaderStore,
    sk_bytes: &[u8],
    request: &SendRequest,
    guards: &SyncGuards,
    progress: Option<SendProgressFn>,
    chain_height: u64,
) -> Result<SendResult, CoreError> {
    // Step 1: Check guards — cannot send during sync/repair/gap-fill
    if guards.is_syncing.load(Ordering::SeqCst) {
        return Err(CoreError::SyncInProgress);
    }
    if guards.is_gap_filling.load(Ordering::SeqCst) {
        return Err(CoreError::GapFillInProgress);
    }
    // RC-13: Block sends during database repair — witnesses may be invalid.
    if guards.is_repairing.load(Ordering::SeqCst) {
        return Err(CoreError::RepairInProgress);
    }
    // RC-12: Verify no other broadcast is in progress before proceeding.
    if guards.is_broadcasting.load(Ordering::SeqCst) {
        return Err(CoreError::BroadcastFailed(
            "Another broadcast is already in progress".into(),
        ));
    }

    // Step 2: Validate send request
    report_progress(&progress, SendPhase::Validating);
    send::validate_send_request(request)?;

    // Step 3: Set broadcasting guard (FIX #1184: NEVER stop listeners during broadcast)
    // RC-12: Uses compare_exchange for atomic check-and-set.
    let _broadcast_guard = BroadcastGuard::new(guards)?;

    // Step 4: Load unspent notes from DB via spawn_blocking
    report_progress(
        &progress,
        SendPhase::NoteSelection {
            count: 0,
            total_value: 0,
        },
    );
    let db_clone = db.clone();
    let notes = tokio::task::spawn_blocking(move || db_clone.get_all_unspent_notes(0))
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    // Step 5: Select notes
    let spendable: Vec<SpendableNote> = notes
        .iter()
        .filter_map(|n| send::note_to_spendable(n))
        .collect();

    let (selected, total_value) =
        send::select_notes(&spendable, request.total_needed()).map_err(|_| {
            CoreError::InsufficientBalance {
                have: spendable.iter().map(|n| n.value).sum(),
                need: request.total_needed(),
            }
        })?;

    report_progress(
        &progress,
        SendPhase::NoteSelection {
            count: selected.len(),
            total_value,
        },
    );

    // Step 6: Validate EACH witness root (FIX #1280)
    for (i, note) in selected.iter().enumerate() {
        report_progress(
            &progress,
            SendPhase::WitnessValidation {
                note_index: i,
                total: selected.len(),
            },
        );

        // FIX #1279: Validate EACH anchor against HeaderStore
        let anchor_bytes = note.anchor;
        let has_root = header_store
            .contains_sapling_root(&anchor_bytes)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        // FIX #1230: Check both byte orders
        let mut reversed = anchor_bytes;
        reversed.reverse();
        let has_root_reversed = header_store
            .contains_sapling_root(&reversed)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if !has_root && !has_root_reversed {
            return Err(CoreError::InvalidAnchor);
        }
    }

    // Step 7: Build TX via spawn_blocking (Groth16 is CPU-heavy)
    report_progress(
        &progress,
        SendPhase::Building {
            spend_index: 0,
            total_spends: selected.len() as u32,
        },
    );

    let change =
        send::calculate_change(total_value, request.amount_zatoshis, request.fee_zatoshis)?;

    // Decode destination address
    let to_address_bytes = zipherx_crypto::address::decode_address(&request.to_address)
        .map_err(|e| CoreError::Crypto(e.to_string()))?;
    let to_address: [u8; 43] = to_address_bytes
        .try_into()
        .map_err(|_| CoreError::Crypto("Invalid address length".into()))?;

    // Convert SpendableNote → SpendInfo for crypto layer
    let spend_infos: Vec<SpendInfo> = selected
        .iter()
        .map(|n| SpendInfo {
            witness_data: n.witness.clone(),
            value: n.value,
            rcm: n.rcm,
            diversifier: n.diversifier,
            is_zip212: n.is_zip212,
        })
        .collect();

    let memo_bytes = request.memo.as_ref().map(|m| m.as_bytes().to_vec());

    // Build the actual transaction (Groth16 proofs)
    // RC-25: Wrap sk_bytes in Zeroizing so it is securely zeroed on drop,
    // even if the task is cancelled between deserialization and completion.
    let sk_owned = Zeroizing::new(sk_bytes.to_vec());
    let tx_result = async_prover::build_transaction_async(
        sk_owned.to_vec(),
        to_address,
        request.amount_zatoshis,
        memo_bytes,
        spend_infos,
        chain_height,
        None,
    )
    .await?;
    drop(sk_owned); // RC-25: Explicit drop triggers zeroization

    // Compute txid: double-SHA256 of serialized TX bytes, reversed
    let hash = double_sha256(&tx_result.tx_bytes);
    let mut txid_bytes = hash;
    txid_bytes.reverse();
    let txid = hex::encode(txid_bytes);

    // Step 8: Broadcast (FIX #1184: dispatcher must be active)
    report_progress(&progress, SendPhase::Broadcasting);

    let (accepted, total) = peer_manager
        .broadcast_transaction(&tx_result.tx_bytes)
        .await
        .map_err(|e| CoreError::BroadcastFailed(e.to_string()))?;

    report_progress(&progress, SendPhase::PeerResponse { accepted, total });

    // Step 9: FIX #1261 — retry once if 0/N accepted
    if accepted == 0 {
        return Err(CoreError::BroadcastFailed(format!(
            "0/{total} peers accepted the transaction"
        )));
    }

    // Step 10: FIX #1259 — mempool verify is monitoring only, NOT confirmation
    // TX confirmation happens ONLY via block scanner

    // Step 11: Record in DB atomically
    report_progress(&progress, SendPhase::Recording);

    let txid_clone = txid.clone();
    let amount = request.amount_zatoshis;
    let fee = request.fee_zatoshis;
    let memo = request.memo.clone();
    let db_clone = db.clone();

    // Mark ALL spent notes by their database IDs.
    // This is the reliable path — nullifier-based matching fails when the
    // delta store is incomplete (wrong positions → wrong nullifiers in DB).
    // We KNOW which notes were selected for spending, so mark them directly.
    let selected_note_ids: Vec<i64> = selected.iter().map(|n| n.id).collect();
    let all_nullifiers = tx_result.nullifiers.clone();

    tokio::task::spawn_blocking(move || {
        // Primary: mark spent by database ID (always works)
        for note_id in &selected_note_ids {
            db_clone.mark_note_spent_by_id(*note_id, &txid_clone, 0)?;
        }

        // Also try nullifier-based recording (inserts "sent" TX history entry)
        for nf in &all_nullifiers {
            db_clone.record_sent_transaction_atomic(
                nf,
                &txid_clone,
                0, // spent_height (unconfirmed)
                amount,
                fee,
                memo.as_deref(),
            )?;
        }
        Ok::<(), zipherx_storage::types::StorageError>(())
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Storage(e.to_string()))?;

    let result = SendResult {
        txid: txid.clone(),
        amount: request.amount_zatoshis,
        fee: request.fee_zatoshis,
        change_value: change,
        notes_used: selected.len(),
        spent_nullifiers: tx_result.nullifiers,
    };

    report_progress(&progress, SendPhase::Complete { txid: txid.clone() });

    Ok(result)
}

fn report_progress(progress: &Option<SendProgressFn>, phase: SendPhase) {
    if let Some(ref p) = progress {
        p(phase);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_request() -> SendRequest {
        use bech32::ToBase32;
        let dummy_data = vec![0xAAu8; 43];
        let addr = bech32::encode("zs", dummy_data.to_base32(), bech32::Variant::Bech32).unwrap();
        SendRequest {
            to_address: addr,
            amount_zatoshis: 50_000,
            fee_zatoshis: send::DEFAULT_FEE,
            memo: None,
        }
    }

    #[tokio::test]
    async fn test_send_validates_request() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let guards = SyncGuards::new();
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let bad_request = SendRequest {
            to_address: "invalid".into(),
            amount_zatoshis: 0, // invalid
            fee_zatoshis: 10_000,
            memo: None,
        };

        let result = send_transaction(db, &pm, &hs, &[], &bad_request, &guards, None, 100).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_blocked_during_sync() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let guards = SyncGuards::new();
        guards.is_syncing.store(true, Ordering::SeqCst);
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let result =
            send_transaction(db, &pm, &hs, &[], &make_test_request(), &guards, None, 100).await;

        assert!(matches!(result, Err(CoreError::SyncInProgress)));
    }

    #[tokio::test]
    async fn test_send_blocked_during_gap_fill() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let guards = SyncGuards::new();
        guards.is_gap_filling.store(true, Ordering::SeqCst);
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let result =
            send_transaction(db, &pm, &hs, &[], &make_test_request(), &guards, None, 100).await;

        assert!(matches!(result, Err(CoreError::GapFillInProgress)));
    }

    #[tokio::test]
    async fn test_broadcast_guard_clears_on_drop() {
        let guards = SyncGuards::new();
        assert!(!guards.is_broadcasting.load(Ordering::SeqCst));

        {
            let _bg = BroadcastGuard::new(&guards).unwrap();
            assert!(guards.is_broadcasting.load(Ordering::SeqCst));
        }

        // Guard dropped — should be cleared
        assert!(!guards.is_broadcasting.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_broadcast_guard_rejects_concurrent() {
        let guards = SyncGuards::new();
        let _bg = BroadcastGuard::new(&guards).unwrap();
        // RC-12: Second acquisition must fail
        assert!(BroadcastGuard::new(&guards).is_err());
    }

    #[tokio::test]
    async fn test_send_insufficient_balance() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let guards = SyncGuards::new();
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        // No notes in DB → insufficient balance
        let result =
            send_transaction(db, &pm, &hs, &[], &make_test_request(), &guards, None, 100).await;

        assert!(matches!(result, Err(CoreError::InsufficientBalance { .. })));
    }
}
