//! Async send flow — note selection through broadcast through recording.
//!
//! Critical invariants:
//! - Set `is_broadcasting=true` before broadcast (FIX #1184)
//! - Validate EACH witness root individually (FIX #1280)
//! - Validate witness internal consistency (FIX #827 — same as official ZipherX)
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
use zipherx_crypto::transaction::{SpendInfo, TransparentSpendInfo};
use zipherx_crypto::util::double_sha256;
use zipherx_network::peer_manager::PeerManager;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::delta_cmu::DeltaCMUStore;
use zipherx_storage::header_store_impl::SqliteHeaderStore;
use zipherx_storage::types::TxType;

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
    /// Peer response (with reject-based verification)
    PeerResponse {
        accepted: u32,
        rejected: u32,
        total: u32,
    },
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
    _header_store: &SqliteHeaderStore,
    _delta_store: &DeltaCMUStore,
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

    // Step 4: Load unspent notes, select, verify witness consistency (FIX #827)
    let (selected, total_value) =
        select_notes_with_witness_check(db.clone(), request, &progress).await?;

    // Step 5: FIX #1300 — Validate anchors against header store BEFORE building TX.
    // HARD ERROR: Zclassic nodes accept TXs into mempool WITHOUT checking Sapling
    // anchors (HaveShieldedRequirements runs during ConnectBlock, not AcceptToMemoryPool).
    // Invalid-anchor TXs get "accepted" by peers but NEVER mined. Peers won't reject.
    // This local check is the ONLY defense. If it fails, the commitment tree is
    // corrupted and must be rebuilt via Full Rescan.
    for (i, note) in selected.iter().enumerate() {
        let anchor = zipherx_crypto::tree::verify_witness_and_get_root(&note.witness)
            .map_err(|e| CoreError::Crypto(format!("Witness {} root extraction failed: {e}", i)))?;

        let found = _header_store
            .contains_sapling_root(&anchor)
            .map_err(CoreError::Network)?;

        if found {
            eprintln!("[ZipherX] Anchor OK for note {} (id={})", i, note.id);
        } else {
            let anchor_hex = hex::encode(&anchor);
            eprintln!(
                "[ZipherX] ANCHOR INVALID for note {} (id={}): {} — not in any block header",
                i, note.id, anchor_hex
            );
            return Err(CoreError::BroadcastFailed(format!(
                "Invalid anchor for note {} — commitment tree is corrupted. Run Full Rescan to rebuild witnesses.",
                i
            )));
        }
    }
    eprintln!("[ZipherX] All {} anchors validated OK", selected.len());

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

    // Build the actual transaction (Groth16 proofs)
    // RC-25: Wrap sk_bytes in Zeroizing so it is securely zeroed on drop,
    // even if the task is cancelled between deserialization and completion.
    let sk_owned = Zeroizing::new(sk_bytes.to_vec());

    // Detect if destination is a transparent address (t1...)
    let is_transparent_dest = request.to_address.starts_with("t1")
        || request.to_address.starts_with("t3");

    let tx_result = if is_transparent_dest {
        // Deshielding: z → t
        async_prover::build_deshield_transaction_async(
            sk_owned.to_vec(),
            request.to_address.clone(),
            request.amount_zatoshis,
            spend_infos,
            chain_height,
        )
        .await?
    } else {
        // Shielded: z → z
        let to_address_bytes = zipherx_crypto::address::decode_address(&request.to_address)
            .map_err(|e| CoreError::Crypto(e.to_string()))?;
        let to_address: [u8; 43] = to_address_bytes
            .try_into()
            .map_err(|_| CoreError::Crypto("Invalid address length".into()))?;

        let memo_bytes = request.memo.as_ref().map(|m| m.as_bytes().to_vec());

        async_prover::build_transaction_async(
            sk_owned.to_vec(),
            to_address,
            request.amount_zatoshis,
            memo_bytes,
            spend_infos,
            chain_height,
            None,
        )
        .await?
    };
    drop(sk_owned); // RC-25: Explicit drop triggers zeroization

    // Compute txid: double-SHA256 of serialized TX bytes, reversed
    let hash = double_sha256(&tx_result.tx_bytes);
    let mut txid_bytes = hash;
    txid_bytes.reverse();
    let txid = hex::encode(txid_bytes);

    // Step 8: Broadcast with reject detection (FIX #1184, FIX #1300)
    report_progress(&progress, SendPhase::Broadcasting);

    eprintln!(
        "[ZipherX] Broadcasting TX {}...",
        &txid[..16.min(txid.len())]
    );

    let broadcast_result = peer_manager
        .broadcast_transaction(&tx_result.tx_bytes, &txid)
        .await
        .map_err(|e| {
            eprintln!("[ZipherX] Broadcast FAILED: {e}");
            CoreError::BroadcastFailed(e.to_string())
        })?;

    let accepted = broadcast_result.total_accepted() as u32;
    let rejected = broadcast_result.rejected_by.len() as u32;
    let total = broadcast_result.total_attempted() as u32;

    report_progress(
        &progress,
        SendPhase::PeerResponse {
            accepted,
            rejected,
            total,
        },
    );

    // FIX #1300: If ANY peer explicitly rejected, treat as failure —
    // even if some peers "accepted" (silence), a reject means the TX is invalid.
    if !broadcast_result.rejected_by.is_empty() {
        let reasons: Vec<String> = broadcast_result
            .rejected_by
            .iter()
            .map(|(peer, reason)| format!("{}: {}", peer, reason))
            .collect();
        let msg = format!(
            "TX rejected by {} peer(s): {}",
            rejected,
            reasons.join(", ")
        );
        eprintln!("[ZipherX] {}", msg);
        return Err(CoreError::BroadcastFailed(msg));
    }

    if accepted == 0 {
        return Err(CoreError::BroadcastFailed(format!(
            "0/{total} peers accepted the transaction"
        )));
    }

    eprintln!("[ZipherX] TX accepted by {}/{} peers", accepted, total);

    // Step 10: FIX #1259 — mempool verify is monitoring only, NOT confirmation
    // TX confirmation happens ONLY via block scanner

    // Step 11: Record in DB atomically
    // NOTE: Only reached if broadcast was accepted (no reject messages received).
    report_progress(&progress, SendPhase::Recording);

    eprintln!(
        "[ZipherX] Recording TX {} — marking {} notes as spent",
        &txid[..16.min(txid.len())],
        selected.len()
    );

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

    eprintln!(
        "[ZipherX] TX {} recorded successfully",
        &txid[..16.min(txid.len())]
    );

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
// Transparent Send Transaction
// ============================================================================

/// Execute a full transparent send flow: UTXO selection → key derivation → build → broadcast → record.
///
/// Spends transparent UTXOs from the wallet. Supports sending to either
/// shielded (t→z, "shielding") or transparent (t→t) destinations.
///
/// Requires both the seed (for transparent key derivation) and the spending key
/// (for Sapling change output / OVK).
pub async fn send_transparent_transaction(
    db: Arc<WalletDatabase>,
    peer_manager: &PeerManager,
    sk_bytes: &[u8],
    seed: &[u8],
    request: &SendRequest,
    guards: &SyncGuards,
    progress: Option<SendProgressFn>,
    chain_height: u64,
    decrypt_fn: impl Fn(&[u8]) -> Result<Zeroizing<Vec<u8>>, String> + Send + 'static,
) -> Result<SendResult, CoreError> {
    // Step 1: Check guards
    if guards.is_syncing.load(Ordering::SeqCst) {
        return Err(CoreError::SyncInProgress);
    }
    if guards.is_gap_filling.load(Ordering::SeqCst) {
        return Err(CoreError::GapFillInProgress);
    }
    if guards.is_repairing.load(Ordering::SeqCst) {
        return Err(CoreError::RepairInProgress);
    }
    if guards.is_broadcasting.load(Ordering::SeqCst) {
        return Err(CoreError::BroadcastFailed(
            "Another broadcast is already in progress".into(),
        ));
    }

    // Step 2: Validate request
    report_progress(&progress, SendPhase::Validating);
    send::validate_send_request(request)?;

    // Step 3: Broadcasting guard
    let _broadcast_guard = BroadcastGuard::new(guards)?;

    // Step 4: Load unspent transparent UTXOs
    report_progress(
        &progress,
        SendPhase::NoteSelection {
            count: 0,
            total_value: 0,
        },
    );

    let db_c = db.clone();
    let utxos = tokio::task::spawn_blocking(move || db_c.get_unspent_transparent_utxos())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    if utxos.is_empty() {
        return Err(CoreError::InsufficientBalance { have: 0, need: request.total_needed() });
    }

    // Step 5: Select UTXOs (largest first to minimize inputs)
    let mut sorted = utxos.clone();
    sorted.sort_by(|a, b| b.value.cmp(&a.value));

    // C3: Use checked arithmetic to prevent silent overflow
    let total_needed = request.amount_zatoshis
        .checked_add(request.fee_zatoshis)
        .ok_or(CoreError::Crypto("amount + fee overflow".into()))?;
    let mut selected = Vec::new();
    let mut selected_total: u64 = 0;
    for utxo in &sorted {
        selected.push(utxo.clone());
        selected_total = selected_total
            .checked_add(utxo.value)
            .ok_or(CoreError::Crypto("UTXO accumulation overflow".into()))?;
        if selected_total >= total_needed {
            break;
        }
    }

    if selected_total < total_needed {
        return Err(CoreError::InsufficientBalance {
            have: selected_total,
            need: total_needed,
        });
    }

    report_progress(
        &progress,
        SendPhase::NoteSelection {
            count: selected.len(),
            total_value: selected_total,
        },
    );

    eprintln!(
        "[ZipherX] Transparent send: {} UTXOs selected, total={}, need={}",
        selected.len(), selected_total, total_needed,
    );

    // Step 6: Derive secret keys and build TransparentSpendInfo
    let seed_owned = Zeroizing::new(seed.to_vec());
    let mut spend_infos: Vec<TransparentSpendInfo> = Vec::with_capacity(selected.len());

    // Pre-load any imported keys we need (DB access before spawn_blocking loop)
    let has_imported = selected.iter().any(|u| u.is_imported);
    let imported_keys: std::collections::HashMap<String, Vec<u8>> = if has_imported {
        let db_for_keys = db.clone();
        let imported_addrs: Vec<String> = selected
            .iter()
            .filter(|u| u.is_imported)
            .map(|u| u.address.clone())
            .collect();
        tokio::task::spawn_blocking(move || {
            let mut map = std::collections::HashMap::new();
            for addr in &imported_addrs {
                if let Ok(Some(encrypted)) = db_for_keys.get_imported_transparent_secret(addr) {
                    map.insert(addr.clone(), encrypted);
                }
            }
            map
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    } else {
        std::collections::HashMap::new()
    };

    for (i, utxo) in selected.iter().enumerate() {
        let sk = if utxo.is_imported {
            // Imported key: decrypt from DB
            let encrypted_sk = imported_keys.get(&utxo.address).ok_or_else(|| {
                CoreError::Crypto(format!(
                    "Imported key not found for address {}",
                    utxo.address
                ))
            })?;
            decrypt_fn(encrypted_sk).map_err(|e| {
                CoreError::Crypto(format!("Failed to decrypt imported key: {e}"))
            })?
        } else {
            // Seed-derived key
            let derived = zipherx_crypto::transparent::derive_transparent_secret_key(
                &seed_owned,
                0,
                utxo.child_index,
                utxo.is_change,
            )
            .map_err(|e| {
                CoreError::Crypto(format!(
                    "Failed to derive key for UTXO {i} (child={}, change={}): {e}",
                    utxo.child_index, utxo.is_change
                ))
            })?;

            // Verify derived address matches UTXO address
            let derived_addr = if utxo.is_change {
                zipherx_crypto::transparent::derive_transparent_change_address(
                    &seed_owned,
                    0,
                    utxo.child_index,
                )
            } else {
                zipherx_crypto::transparent::derive_transparent_address(
                    &seed_owned,
                    0,
                    utxo.child_index,
                )
            }
            .map_err(|e| {
                CoreError::Crypto(format!("Address derivation failed for UTXO {i}: {e}"))
            })?;

            if derived_addr != utxo.address {
                return Err(CoreError::Crypto(format!(
                    "UTXO {i} address mismatch: expected {}, derived {}",
                    utxo.address, derived_addr
                )));
            }

            derived
        };

        // Convert txid from hex display format to internal byte order
        let txid_hex = &utxo.txid;
        let txid_display_bytes = hex::decode(txid_hex)
            .map_err(|e| CoreError::Crypto(format!("Invalid UTXO txid hex: {e}")))?;
        if txid_display_bytes.len() != 32 {
            return Err(CoreError::Crypto(format!(
                "Invalid UTXO txid length: {}",
                txid_display_bytes.len()
            )));
        }
        // Wire format is reversed display format
        let mut txid_bytes = [0u8; 32];
        for (j, b) in txid_display_bytes.iter().rev().enumerate() {
            txid_bytes[j] = *b;
        }

        spend_infos.push(TransparentSpendInfo {
            secret_key: sk.to_vec(),
            prevout_txid: txid_bytes,
            prevout_index: utxo.output_index,
            script_pubkey: utxo.script_pubkey.clone(),
            value: utxo.value,
        });
    }

    // Step 7: Determine change address
    // Change always goes back to a transparent change address.
    // For t→t: change stays in transparent pool.
    // For t→z: only the exact user amount goes to shielded, change stays transparent.
    // This ensures the user controls exactly how much is shielded.
    let t_change_addr = {
        // I2: Rotate change address — use next available child_index to avoid reuse.
        let db_for_idx = db.clone();
        let next_change_idx = tokio::task::spawn_blocking(move || {
            db_for_idx.next_transparent_change_index()
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Some(
            zipherx_crypto::transparent::derive_transparent_change_address(&seed_owned, 0, next_change_idx)
                .map_err(|e| CoreError::Crypto(format!("Change address derivation failed: {e}")))?,
        )
    };

    // Step 8: Build transaction (prover must already be initialized by caller)
    report_progress(
        &progress,
        SendPhase::Building {
            spend_index: 0,
            total_spends: selected.len() as u32,
        },
    );

    let sk_owned = Zeroizing::new(sk_bytes.to_vec());
    let to_address = request.to_address.clone();
    let amount = request.amount_zatoshis;
    let memo_bytes = request.memo.as_ref().map(|m| m.as_bytes().to_vec());
    let t_change = t_change_addr.clone();

    let tx_result = tokio::task::spawn_blocking(move || {
        let result = zipherx_crypto::transaction::build_transparent_spend_transaction(
            &sk_owned,
            &to_address,
            amount,
            memo_bytes.as_deref(),
            &spend_infos,
            chain_height,
            t_change.as_deref(),
        );
        // C2: Explicit zeroization of spend_infos (contains secret keys).
        // ZeroizeOnDrop handles this automatically, but drop(spend_infos) here
        // ensures keys are cleared before the closure returns its result.
        drop(spend_infos);
        result
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Crypto(e.to_string()))?;

    // Compute txid
    let hash = double_sha256(&tx_result.tx_bytes);
    let mut txid_bytes = hash;
    txid_bytes.reverse();
    let txid = hex::encode(txid_bytes);

    // Step 9: Broadcast
    report_progress(&progress, SendPhase::Broadcasting);
    eprintln!(
        "[ZipherX] Broadcasting transparent TX {}...",
        &txid[..16.min(txid.len())]
    );

    let broadcast_result = peer_manager
        .broadcast_transaction(&tx_result.tx_bytes, &txid)
        .await
        .map_err(|e| {
            eprintln!("[ZipherX] Broadcast FAILED: {e}");
            CoreError::BroadcastFailed(e.to_string())
        })?;

    let accepted = broadcast_result.total_accepted() as u32;
    let rejected = broadcast_result.rejected_by.len() as u32;
    let total = broadcast_result.total_attempted() as u32;

    report_progress(
        &progress,
        SendPhase::PeerResponse {
            accepted,
            rejected,
            total,
        },
    );

    if !broadcast_result.rejected_by.is_empty() {
        let reasons: Vec<String> = broadcast_result
            .rejected_by
            .iter()
            .map(|(peer, reason)| format!("{}: {}", peer, reason))
            .collect();
        let msg = format!(
            "TX rejected by {} peer(s): {}",
            rejected,
            reasons.join(", ")
        );
        eprintln!("[ZipherX] {}", msg);
        return Err(CoreError::BroadcastFailed(msg));
    }

    if accepted == 0 {
        return Err(CoreError::BroadcastFailed(format!(
            "0/{total} peers accepted the transaction"
        )));
    }

    eprintln!("[ZipherX] TX accepted by {}/{} peers", accepted, total);

    // Step 10: Record in DB
    report_progress(&progress, SendPhase::Recording);

    let txid_clone = txid.clone();
    let db_clone = db.clone();
    let selected_clone = selected.clone();
    let address = request.to_address.clone();
    let memo = request.memo.clone();
    let fee = request.fee_zatoshis;
    let change = selected_total.saturating_sub(total_needed);

    tokio::task::spawn_blocking(move || {
        // Mark selected UTXOs as spent
        for utxo in &selected_clone {
            let _ = db_clone.mark_transparent_spent_by_prevout(
                &utxo.txid,
                utxo.output_index,
                &txid_clone,
                0, // unconfirmed
            );
        }

        // Record transaction in history
        let tx_type = if address.starts_with("zs") {
            TxType::SelfT2Z
        } else {
            TxType::Sent
        };

        db_clone.insert_transaction(
            &txid_clone,
            0, // height: unconfirmed
            None,
            tx_type,
            amount,
            fee,
            Some(&address),
            memo.as_deref(),
            zipherx_storage::types::TxStatus::Pending,
        )?;

        Ok::<(), zipherx_storage::types::StorageError>(())
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Storage(e.to_string()))?;

    eprintln!(
        "[ZipherX] Transparent TX {} recorded successfully",
        &txid[..16.min(txid.len())]
    );

    let result = SendResult {
        txid: txid.clone(),
        amount: request.amount_zatoshis,
        fee: request.fee_zatoshis,
        change_value: change,
        notes_used: selected.len(),
        spent_nullifiers: vec![], // no shielded spends
    };

    report_progress(&progress, SendPhase::Complete { txid });

    Ok(result)
}

// ============================================================================
// Note selection + witness consistency check (FIX #827)
// ============================================================================

/// Load notes, select for spending, validate witness internal consistency.
///
/// Matches the official ZipherX approach (FIX #827): validates that the witness
/// is deserializable and has a valid merkle path. Does NOT check against
/// HeaderStore — the network validates the anchor when the TX is broadcast.
async fn select_notes_with_witness_check(
    db: Arc<WalletDatabase>,
    request: &SendRequest,
    progress: &Option<SendProgressFn>,
) -> Result<(Vec<SpendableNote>, u64), CoreError> {
    report_progress(
        progress,
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
        progress,
        SendPhase::NoteSelection {
            count: selected.len(),
            total_value,
        },
    );

    // FIX #827: Validate witness internal consistency (same as official ZipherX)
    for (i, note) in selected.iter().enumerate() {
        report_progress(
            progress,
            SendPhase::WitnessValidation {
                note_index: i,
                total: selected.len(),
            },
        );

        if let Err(e) = zipherx_crypto::tree::verify_witness_consistency(&note.witness) {
            eprintln!(
                "[ZipherX] Send: witness corrupted for note {} (id={}): {} — go to Settings → FULL RESCAN",
                i, note.id, e
            );
            return Err(CoreError::InvalidAnchor);
        }
    }

    Ok((selected, total_value))
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

    fn make_test_delta_store() -> DeltaCMUStore {
        let dir = std::env::temp_dir().join(format!("zipherx_test_delta_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        DeltaCMUStore::new(&dir).unwrap()
    }

    #[tokio::test]
    async fn test_send_validates_request() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let ds = make_test_delta_store();
        let guards = SyncGuards::new();
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let bad_request = SendRequest {
            to_address: "invalid".into(),
            amount_zatoshis: 0, // invalid
            fee_zatoshis: 10_000,
            memo: None,
        };

        let result =
            send_transaction(db, &pm, &hs, &ds, &[], &bad_request, &guards, None, 100).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_blocked_during_sync() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let ds = make_test_delta_store();
        let guards = SyncGuards::new();
        guards.is_syncing.store(true, Ordering::SeqCst);
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let result = send_transaction(
            db,
            &pm,
            &hs,
            &ds,
            &[],
            &make_test_request(),
            &guards,
            None,
            100,
        )
        .await;

        assert!(matches!(result, Err(CoreError::SyncInProgress)));
    }

    #[tokio::test]
    async fn test_send_blocked_during_gap_fill() {
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let ds = make_test_delta_store();
        let guards = SyncGuards::new();
        guards.is_gap_filling.store(true, Ordering::SeqCst);
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        let result = send_transaction(
            db,
            &pm,
            &hs,
            &ds,
            &[],
            &make_test_request(),
            &guards,
            None,
            100,
        )
        .await;

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
        let ds = make_test_delta_store();
        let guards = SyncGuards::new();
        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let pm = PeerManager::new(pm_config);

        // No notes in DB → insufficient balance
        let result = send_transaction(
            db,
            &pm,
            &hs,
            &ds,
            &[],
            &make_test_request(),
            &guards,
            None,
            100,
        )
        .await;

        assert!(matches!(result, Err(CoreError::InsufficientBalance { .. })));
    }
}
