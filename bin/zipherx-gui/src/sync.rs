//! Background sync engine.
//!
//! Spawns an OS thread with its own tokio runtime. Communicates with
//! the UI via `Arc<Mutex<SharedState>>`. The UI reads each frame;
//! the sync thread writes on progress callbacks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use zipherx_core::async_wallet::AsyncWallet;
use zipherx_core::wallet::WalletConfig;
use zipherx_platform::SecureStorage;
use rand::RngCore;

use crate::platform::GuiSecureStorage;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Connected peer info for display.
#[derive(Clone)]
pub struct PeerDisplayInfo {
    pub address: String,
    pub protocol_version: u32,
    pub user_agent: String,
    pub start_height: u64,
}

/// State shared between the background sync/wallet thread and the UI.
pub struct SharedState {
    // -- sync progress --
    pub sync_phase: String,
    pub sync_current: u64,
    pub sync_target: u64,
    pub sync_progress: f32,
    pub sync_complete: bool,
    pub sync_height: u64,
    pub sync_error: Option<String>,

    // -- balance --
    pub total_balance: u64,
    pub spendable_balance: u64,
    pub note_count: usize,
    pub spendable_note_count: usize,

    // -- network --
    pub peer_count: u32,
    pub block_height: u64,

    // -- transactions --
    pub transactions: Vec<crate::app::TransactionRecord>,

    // -- send --
    pub send_result: Option<Result<SendResultInfo, String>>,
    pub send_phase: String,
    pub send_current: u32,
    pub send_total: u32,
    pub mempool_accepted: bool,
    pub mempool_peer_status: Option<String>,

    // -- peer info --
    pub peer_infos: Vec<PeerDisplayInfo>,

    // -- maintenance --
    pub maintenance_result: Option<Result<String, String>>,

    // -- mempool --
    pub mempool_tx: Option<zipherx_core::mempool_monitor::MempoolTxInfo>,

    // -- new block notification (from inv MSG_BLOCK) --
    pub new_block_pending: bool,

    // -- boost download failure (user must decide) --
    /// Set when boost download fails after all retries.
    /// Contains (reason, attempts). UI shows a dialog.
    pub boost_failed: Option<(String, u32)>,
    /// User's response to boost failure: true = continue with P2P, false = quit.
    pub boost_failed_continue: Option<bool>,

    // -- commands from UI -> sync thread --
    pub command: Option<SyncCommand>,
}

/// Result of a successful send operation.
#[derive(Clone)]
#[allow(dead_code)]
pub struct SendResultInfo {
    pub txid: String,
    pub amount: u64,
    pub fee: u64,
}

/// Commands the UI can send to the sync thread.
pub enum SyncCommand {
    StartSync { sk_bytes: Vec<u8> },
    Send {
        to_address: String,
        amount: u64,
        fee: u64,
        memo: Option<String>,
        sk_bytes: Vec<u8>,
    },
    SetTorEnabled(bool),
    RepairDatabase,
    FullRescan,
    RefreshPeerInfo,
    Stop,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sync_phase: "Idle".to_string(),
            sync_current: 0,
            sync_target: 0,
            sync_progress: 0.0,
            sync_complete: false,
            sync_height: 0,
            sync_error: None,

            total_balance: 0,
            spendable_balance: 0,
            note_count: 0,
            spendable_note_count: 0,

            peer_count: 0,
            block_height: 0,

            transactions: Vec::new(),

            send_result: None,
            send_phase: String::new(),
            send_current: 0,
            send_total: 0,
            mempool_accepted: false,
            mempool_peer_status: None,

            peer_infos: Vec::new(),

            maintenance_result: None,

            mempool_tx: None,
            new_block_pending: false,

            boost_failed: None,
            boost_failed_continue: None,

            command: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Sync thread
// ---------------------------------------------------------------------------

/// Start the background wallet thread.
///
/// Owns the `AsyncWallet` and its tokio runtime. The UI communicates
/// via the returned `Arc<Mutex<SharedState>>`.
pub fn start_wallet_thread(
    data_dir: PathBuf,
    storage: Arc<GuiSecureStorage>,
) -> Arc<Mutex<SharedState>> {
    let state = Arc::new(Mutex::new(SharedState::default()));
    let state_clone = state.clone();

    std::thread::Builder::new()
        .name("zipherx-wallet".into())
        .spawn(move || {
            wallet_thread_main(data_dir, storage, state_clone);
        })
        .expect("failed to spawn wallet thread");

    state
}

fn wallet_thread_main(
    data_dir: PathBuf,
    storage: Arc<GuiSecureStorage>,
    state: Arc<Mutex<SharedState>>,
) {
    // Build a tokio runtime for this thread
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.sync_error = Some(format!("Failed to create runtime: {}", e));
            }
            return;
        }
    };

    // Load or generate DB encryption key
    let db_encryption_key = match ensure_db_key(&storage) {
        Ok(key) => Some(key),
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.sync_error = Some(format!("DB key error: {}", e));
            }
            return;
        }
    };

    // Ensure subdirectories exist
    let _ = std::fs::create_dir_all(data_dir.join("delta"));

    let config = WalletConfig {
        db_path: data_dir.join("wallet.db").to_string_lossy().into(),
        header_store_path: data_dir.join("headers.db").to_string_lossy().into(),
        delta_store_dir: data_dir.join("delta").to_string_lossy().into(),
        spend_params_path: data_dir.join("sapling-spend.params").to_string_lossy().into(),
        output_params_path: data_dir.join("sapling-output.params").to_string_lossy().into(),
        account_index: 0,
        db_encryption_key,
        boost_cache_dir: None,
    };

    // Initialize wallet
    let wallet = match runtime.block_on(AsyncWallet::initialize(config)) {
        Ok(w) => w,
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.sync_error = Some(format!("Wallet init failed: {}", e));
            }
            return;
        }
    };

    // Load balance + history from DB immediately so UI isn't blank during sync
    refresh_balance_and_history(&runtime, &wallet, &state);

    // Spawn a background thread that polls peer count from the atomic every 500ms.
    // The main loop can't update state.peer_count during blocking operations (sync, send),
    // so this poller ensures the UI always shows the current peer count.
    let peer_poller_state = state.clone();
    let peer_poller_atomic = wallet.connected_peer_count.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(mut s) = peer_poller_state.lock() {
                s.peer_count = peer_poller_atomic.load(std::sync::atomic::Ordering::Relaxed);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    let mut mempool_detector_set = false;
    let mut last_listener_check = std::time::Instant::now();

    // Cached sk_bytes: retained across sync calls so background sync works
    // even when the UI is locked (UI zeroes its own copy for security, but
    // the wallet thread keeps its own for autonomous background sync — same
    // as the FFI path does for Android/iOS).
    let mut cached_sk: Option<Vec<u8>> = None;
    let mut last_bg_sync = std::time::Instant::now();
    let mut initial_sync_done = false;

    // Main loop: process commands from the UI
    loop {
        let cmd = {
            if let Ok(mut s) = state.lock() {
                s.peer_count = wallet.get_connected_peer_count();
                s.command.take()
            } else {
                None
            }
        };

        match cmd {
            Some(SyncCommand::StartSync { mut sk_bytes }) => {
                // Set up event-driven mempool detector before first sync
                // so block listeners started during sync already fire the callback.
                if !mempool_detector_set {
                    setup_mempool_detector(&runtime, &wallet, &sk_bytes, &state);
                    mempool_detector_set = true;
                }

                // Cache sk_bytes for autonomous background sync
                if cached_sk.is_none() {
                    cached_sk = Some(sk_bytes.clone());
                }

                handle_sync(&runtime, &wallet, &sk_bytes, &state);
                // Refresh balance and history after sync
                refresh_balance_and_history(&runtime, &wallet, &state);

                // Check if sync succeeded (no error = success)
                let sync_ok = if let Ok(s) = state.lock() {
                    s.sync_error.is_none()
                } else {
                    false
                };

                if sync_ok {
                    // FIX: Auto-retry when notes lack witnesses (tree root mismatch
                    // triggered FIX #1300 which resets last_scanned for next sync).
                    // Without this, total > spendable until the next periodic sync.
                    let needs_witness_retry = if let Ok(s) = state.lock() {
                        s.total_balance > s.spendable_balance
                    } else {
                        false
                    };
                    if needs_witness_retry {
                        eprintln!("[ZipherX] Auto-retry: total > spendable — notes need witness rebuild");
                        handle_sync(&runtime, &wallet, &sk_bytes, &state);
                        refresh_balance_and_history(&runtime, &wallet, &state);
                    }
                    initial_sync_done = true;
                }

                // GUI-C5: Zeroize the cloned key material after use
                for b in sk_bytes.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                last_bg_sync = std::time::Instant::now();
            }
            Some(SyncCommand::Send { to_address, amount, fee, memo, mut sk_bytes }) => {
                handle_send(&runtime, &wallet, &to_address, amount, fee, memo.as_deref(), &sk_bytes, &state);
                // GUI-C5: Zeroize the cloned key material after use
                for b in sk_bytes.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            }
            Some(SyncCommand::SetTorEnabled(enabled)) => {
                // GUI-L1: Tor toggle not yet implemented
                eprintln!("[ZipherX] Tor toggle requested (enabled={}), not yet implemented", enabled);
            }
            Some(SyncCommand::RepairDatabase) => {
                handle_repair(&runtime, &wallet, &state);
            }
            Some(SyncCommand::FullRescan) => {
                handle_full_rescan(&runtime, &wallet, &state);
            }
            Some(SyncCommand::RefreshPeerInfo) => {
                handle_refresh_peer_info(&runtime, &wallet, &state);
            }
            Some(SyncCommand::Stop) => {
                break;
            }
            None => {
                // Periodic block listener health check (every 30s).
                // Listeners die between syncs (reader consumed by spawned task).
                // Once dead, start_all_block_listeners fails because reader is None.
                // Only fix: full reconnect to get fresh reader+writer pairs.
                if mempool_detector_set && last_listener_check.elapsed().as_secs() >= 30 {
                    let needs_reconnect = runtime.block_on(async {
                        let pm = wallet.peer_manager.lock().await;
                        pm.connected_count() > 0 && !pm.has_active_block_listeners()
                    });
                    if needs_reconnect {
                        eprintln!("[ZipherX] Block listeners dead — reconnecting peers for mempool detection");
                        runtime.block_on(async {
                            let mut pm = wallet.peer_manager.lock().await;
                            pm.disconnect_all().await;
                            if let Err(e) = pm.connect().await {
                                eprintln!("[ZipherX] Peer reconnect failed: {e}");
                            } else {
                                pm.start_all_block_listeners().await;
                                eprintln!(
                                    "[ZipherX] Block listeners restarted: {} peers, listeners={}",
                                    pm.connected_count(),
                                    pm.has_active_block_listeners(),
                                );
                            }
                        });
                    }
                    last_listener_check = std::time::Instant::now();
                }

                // Autonomous background sync: works even when UI is locked.
                // Triggered by:
                //   - inv MSG_BLOCK (instant) when initial sync is done
                //   - periodic timer (90s) when synced
                //   - retry timer (30s) when initial sync failed (network recovery)
                if let Some(ref sk) = cached_sk {
                    let new_block = if let Ok(mut s) = state.lock() {
                        let pending = s.new_block_pending;
                        if pending { s.new_block_pending = false; }
                        pending
                    } else {
                        false
                    };

                    let retry_interval = if initial_sync_done { 90 } else { 30 };
                    let periodic = last_bg_sync.elapsed().as_secs() >= retry_interval;

                    if new_block || periodic {
                        if new_block {
                            eprintln!("[ZipherX] Wallet thread: new block — autonomous sync");
                        } else if !initial_sync_done {
                            eprintln!("[ZipherX] Wallet thread: retrying initial sync (network recovery)");
                        }
                        handle_sync(&runtime, &wallet, sk, &state);
                        refresh_balance_and_history(&runtime, &wallet, &state);
                        last_bg_sync = std::time::Instant::now();

                        // Mark initial sync done if this retry succeeded
                        if !initial_sync_done {
                            if let Ok(s) = state.lock() {
                                if s.sync_error.is_none() {
                                    initial_sync_done = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Zeroize cached sk on thread exit
    if let Some(ref mut sk) = cached_sk {
        for b in sk.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}

fn handle_sync(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    sk_bytes: &[u8],
    state: &Arc<Mutex<SharedState>>,
) {
    use zipherx_core::sync::SyncStatus;

    let state_progress = state.clone();
    let peer_count_atomic = wallet.connected_peer_count.clone();
    let progress_fn = Arc::new(move |status: SyncStatus| {
        if let Ok(mut s) = state_progress.lock() {
            // Update peer count on every progress tick so UI shows live count
            s.peer_count = peer_count_atomic.load(std::sync::atomic::Ordering::Relaxed);
            match &status {
                SyncStatus::BoostDownload { downloaded_bytes, total_bytes } => {
                    s.sync_phase = "boost_download".to_string();
                    s.sync_current = *downloaded_bytes;
                    s.sync_target = *total_bytes;
                }
                SyncStatus::BoostLoad { loaded, total } => {
                    s.sync_phase = "boost_load".to_string();
                    s.sync_current = *loaded;
                    s.sync_target = *total;
                }
                SyncStatus::HeaderSync { current_height, target_height } => {
                    s.sync_phase = "header_sync".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::DeltaSync { current_height, target_height } => {
                    s.sync_phase = "delta_sync".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::BoostScan { outputs_total } => {
                    s.sync_phase = "boost_scan".to_string();
                    s.sync_current = 0;
                    s.sync_target = *outputs_total;
                }
                SyncStatus::BlockScan { current_height, target_height, .. } => {
                    s.sync_phase = "block_scan".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::WitnessUpdate { notes_updated, total_notes } => {
                    s.sync_phase = "witness_update".to_string();
                    s.sync_current = *notes_updated as u64;
                    s.sync_target = *total_notes as u64;
                }
                SyncStatus::GapFill { gaps_remaining } => {
                    s.sync_phase = "gap_fill".to_string();
                    s.sync_current = 0;
                    s.sync_target = *gaps_remaining as u64;
                }
                SyncStatus::Complete { height } => {
                    s.sync_phase = format!("Synced to {}", height);
                    s.sync_current = *height;
                    s.sync_target = *height;
                }
                SyncStatus::BoostFailed { reason, attempts } => {
                    s.sync_phase = "boost_failed".to_string();
                    s.boost_failed = Some((reason.clone(), *attempts));
                }
                SyncStatus::Failed(msg) => {
                    s.sync_error = Some(msg.clone());
                }
                SyncStatus::Idle => {}
            }
            s.sync_progress = if s.sync_target > 0 {
                s.sync_current as f32 / s.sync_target as f32
            } else {
                0.0
            };
        }
    });

    let result = runtime.block_on(wallet.sync(
        sk_bytes,
        Some(progress_fn),
    ));

    match result {
        Ok(height) => {
            // BIP 35: request mempool inventories from all peers after sync.
            // Without this, peers only announce TXs that arrive AFTER the
            // connection — TXs already in their mempool are never announced.
            runtime.block_on(async {
                let pm = wallet.peer_manager.lock().await;
                pm.request_mempool_from_all().await;
            });

            // Auto-repair: if notes are missing witnesses after sync,
            // clear tree state + witnesses then re-sync to force full rebuild.
            if let Ok(balance) = runtime.block_on(wallet.get_balance()) {
                if balance.note_count > 0
                    && balance.note_count > balance.spendable_note_count
                {
                    let missing = balance.note_count - balance.spendable_note_count;
                    eprintln!(
                        "[ZipherX] {}/{} notes spendable — repairing {} witnesses",
                        balance.spendable_note_count, balance.note_count, missing,
                    );
                    if let Ok(mut s) = state.lock() {
                        s.sync_phase = format!("Repairing {} witnesses...", missing);
                        s.sync_progress = 0.0;
                    }
                    if runtime.block_on(wallet.repair_database()).is_ok() {
                        eprintln!("[ZipherX] tree state cleared, re-syncing for witness rebuild");
                        // Progress callback for repair sync
                        let state_repair = state.clone();
                        let repair_progress_fn: std::sync::Arc<dyn Fn(zipherx_core::sync::SyncStatus) + Send + Sync> =
                            std::sync::Arc::new(move |status: zipherx_core::sync::SyncStatus| {
                                if let Ok(mut s) = state_repair.lock() {
                                    use zipherx_core::sync::SyncStatus;
                                    match &status {
                                        SyncStatus::BoostDownload { downloaded_bytes, total_bytes } => {
                                            s.sync_phase = "Repairing: downloading boost".to_string();
                                            s.sync_current = *downloaded_bytes;
                                            s.sync_target = *total_bytes;
                                        }
                                        SyncStatus::HeaderSync { current_height, target_height } => {
                                            s.sync_phase = "Repairing: syncing headers".to_string();
                                            s.sync_current = *current_height;
                                            s.sync_target = *target_height;
                                        }
                                        SyncStatus::DeltaSync { current_height, target_height } => {
                                            s.sync_phase = "Repairing: syncing blocks".to_string();
                                            s.sync_current = *current_height;
                                            s.sync_target = *target_height;
                                        }
                                        SyncStatus::BlockScan { current_height, target_height, .. } => {
                                            s.sync_phase = "Repairing: scanning blocks".to_string();
                                            s.sync_current = *current_height;
                                            s.sync_target = *target_height;
                                        }
                                        SyncStatus::WitnessUpdate { notes_updated, total_notes } => {
                                            s.sync_phase = "Repairing: updating witnesses".to_string();
                                            s.sync_current = *notes_updated as u64;
                                            s.sync_target = *total_notes as u64;
                                        }
                                        SyncStatus::Complete { height } => {
                                            s.sync_phase = format!("Repair complete at {}", height);
                                            s.sync_current = *height;
                                            s.sync_target = *height;
                                        }
                                        _ => {}
                                    }
                                    s.sync_progress = if s.sync_target > 0 {
                                        s.sync_current as f32 / s.sync_target as f32
                                    } else {
                                        0.0
                                    };
                                }
                            });
                        if let Ok(h) = runtime.block_on(wallet.sync(sk_bytes, Some(repair_progress_fn))) {
                            eprintln!("[ZipherX] witness repair complete at height {}", h);
                        }
                    }
                }
            }

            if let Ok(mut s) = state.lock() {
                s.sync_complete = true;
                s.sync_height = height;
                s.sync_phase = format!("Synced to {}", height);
                s.sync_progress = 1.0;
                s.sync_error = None;
                s.block_height = height;
            }
        }
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.sync_error = Some(e.to_string());
            }
        }
    }
}

/// Set up event-driven mempool detection on the peer manager.
///
/// Block listeners handle inv→getdata→tx internally and fire the callback
/// with raw TX bytes. Trial decryption happens synchronously in the callback.
/// No separate task, no channel, no extra TCP connection.
fn setup_mempool_detector(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    sk_bytes: &[u8],
    state: &Arc<Mutex<SharedState>>,
) {
    let state_clone = state.clone();
    let detector = zipherx_core::mempool_monitor::MempoolDetector::new(
        sk_bytes.to_vec(),
        std::sync::Arc::new(move |info: zipherx_core::mempool_monitor::MempoolTxInfo| {
            if let Ok(mut s) = state_clone.lock() {
                s.mempool_tx = Some(info);
            }
        }),
    );
    let mempool_callback = detector.into_callback();
    runtime.block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.set_on_mempool_tx_data(mempool_callback);
    });

    // New-block notification: when any peer sends inv MSG_BLOCK, set flag
    // so the UI triggers an immediate sync instead of waiting for the 90s timer.
    let state_block = state.clone();
    runtime.block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.set_on_new_block(std::sync::Arc::new(move || {
            if let Ok(mut s) = state_block.lock() {
                s.new_block_pending = true;
            }
        }));
    });

    #[cfg(debug_assertions)]
    eprintln!("[ZipherX] Mempool detector + new-block notify set (egui, event-driven)");
}

fn handle_send(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    to_address: &str,
    amount: u64,
    fee: u64,
    memo: Option<&str>,
    sk_bytes: &[u8],
    state: &Arc<Mutex<SharedState>>,
) {
    use zipherx_core::send::SendRequest;

    let request = SendRequest {
        to_address: to_address.to_string(),
        amount_zatoshis: amount,
        fee_zatoshis: fee,
        memo: memo.map(|m| m.to_string()),
    };

    let state_progress = state.clone();
    let progress_fn = Arc::new(move |phase: zipherx_core::async_send::SendPhase| {
        use zipherx_core::async_send::SendPhase;
        if let Ok(mut s) = state_progress.lock() {
            match &phase {
                SendPhase::Validating => {
                    s.send_phase = "Validating...".to_string();
                    s.send_current = 0;
                    s.send_total = 0;
                }
                SendPhase::NoteSelection { count, .. } => {
                    s.send_phase = format!("Selected {} notes", count);
                }
                SendPhase::WitnessValidation { note_index, total } => {
                    s.send_phase = "Validating witnesses...".to_string();
                    s.send_current = *note_index as u32;
                    s.send_total = *total as u32;
                }
                SendPhase::Building { spend_index, total_spends } => {
                    s.send_phase = format!("Building proof {}/{}", spend_index + 1, total_spends);
                    s.send_current = *spend_index;
                    s.send_total = *total_spends;
                }
                SendPhase::Broadcasting => {
                    s.send_phase = "Broadcasting...".to_string();
                }
                SendPhase::PeerResponse { accepted, rejected, total } => {
                    if *rejected > 0 {
                        s.send_phase = format!("REJECTED by {} peer(s)!", rejected);
                    } else {
                        s.send_phase = format!("Peers: {}/{}", accepted, total);
                    }
                    s.send_current = *accepted;
                    s.send_total = *total;
                    if *accepted > 0 && *rejected == 0 {
                        s.mempool_accepted = true;
                        s.mempool_peer_status = Some(format!("{}/{}", accepted, total));
                    }
                }
                SendPhase::Recording => {
                    s.send_phase = "Recording...".to_string();
                }
                SendPhase::Complete { .. } => {
                    s.send_phase = "Complete".to_string();
                }
                SendPhase::Error { message } => {
                    s.send_phase = format!("Error: {}", message);
                }
            }
        }
    });

    // First attempt — send with existing peers
    let result = runtime.block_on(wallet.send(
        request.clone(),
        sk_bytes,
        Some(progress_fn.clone()),
    ));

    // If broadcast failed (broken pipe / stale peers), reconnect and retry once
    let result = match &result {
        Err(e) if e.to_string().contains("Broadcast failed") => {
            eprintln!("[ZipherX] Broadcast failed, reconnecting peers and retrying...");
            if let Ok(mut s) = state.lock() {
                s.send_phase = "Reconnecting peers...".to_string();
            }
            let _ = runtime.block_on(wallet.connect_network());
            let peer_count = wallet.get_connected_peer_count();
            if peer_count > 0 {
                eprintln!("[ZipherX] Reconnected {} peers, retrying broadcast...", peer_count);
                runtime.block_on(wallet.send(
                    request,
                    sk_bytes,
                    Some(progress_fn),
                ))
            } else {
                eprintln!("[ZipherX] Reconnect failed — no peers available");
                result
            }
        }
        _ => result,
    };

    match result {
        Ok(send_result) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Ok(SendResultInfo {
                    txid: send_result.txid.clone(),
                    amount: send_result.amount,
                    fee: send_result.fee,
                }));
            }
        }
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(e.to_string()));
                s.mempool_accepted = false;
                s.mempool_peer_status = None;
            }
        }
    }
}

fn refresh_balance_and_history(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    state: &Arc<Mutex<SharedState>>,
) {
    // Balance
    if let Ok(balance) = runtime.block_on(wallet.get_balance()) {
        if let Ok(mut s) = state.lock() {
            s.total_balance = balance.total;
            s.spendable_balance = balance.spendable;
            s.note_count = balance.note_count;
            s.spendable_note_count = balance.spendable_note_count;
        }
    }

    // History
    if let Ok(records) = runtime.block_on(wallet.get_transaction_history(50, 0)) {
        if let Ok(mut s) = state.lock() {
            // Build display records with self-send detection
            let mut result = Vec::new();
            let mut processed = std::collections::HashSet::new();

            // Group by txid
            let mut grouped: std::collections::HashMap<String, Vec<&zipherx_core::wallet::TransactionDisplay>> =
                std::collections::HashMap::new();
            for r in &records {
                grouped.entry(r.txid.clone()).or_default().push(r);
            }

            for r in &records {
                if processed.contains(&r.txid) {
                    continue;
                }
                let group = &grouped[&r.txid];
                let has_sent = group.iter().any(|t| t.tx_type == "sent");
                let has_received = group.iter().any(|t| t.tx_type == "received");

                if has_sent && has_received {
                    // Self-send: display as "self" with fee as amount
                    let sent = group.iter().find(|t| t.tx_type == "sent").unwrap();
                    result.push(crate::app::TransactionRecord {
                        txid: r.txid.clone(),
                        tx_type: "self".to_string(),
                        amount: sent.fee,
                        fee: sent.fee,
                        address: sent.address.clone(),
                        memo: sent.memo.clone(),
                        confirmations: sent.confirmations,
                        height: sent.height,
                        timestamp: sent.timestamp,
                    });
                } else {
                    result.push(crate::app::TransactionRecord {
                        txid: r.txid.clone(),
                        tx_type: r.tx_type.clone(),
                        amount: r.amount,
                        fee: r.fee,
                        address: r.address.clone(),
                        memo: r.memo.clone(),
                        confirmations: r.confirmations,
                        height: r.height,
                        timestamp: r.timestamp,
                    });
                }
                processed.insert(r.txid.clone());
            }

            s.transactions = result;
        }
    }
}

fn handle_repair(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    state: &Arc<Mutex<SharedState>>,
) {
    eprintln!("[ZipherX] Repair database: clearing tree state...");
    let result = runtime.block_on(wallet.repair_database());
    if let Ok(mut s) = state.lock() {
        match result {
            Ok(()) => {
                s.maintenance_result = Some(Ok("Database repaired. Run sync to rebuild.".to_string()));
                eprintln!("[ZipherX] Repair complete.");
            }
            Err(e) => {
                let msg = format!("Repair failed: {}", e);
                eprintln!("[ZipherX] {}", msg);
                s.maintenance_result = Some(Err(msg));
            }
        }
    }
}

fn handle_full_rescan(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    state: &Arc<Mutex<SharedState>>,
) {
    eprintln!("[ZipherX] Full rescan: deleting all notes + history + delta, resetting sync...");
    // Nuclear reset: delete notes + history + tree + delta, force fresh scan
    let result = runtime.block_on(wallet.full_rescan());
    if let Ok(mut s) = state.lock() {
        match result {
            Ok(()) => {
                s.maintenance_result = Some(Ok("All data cleared. Next sync will re-scan from scratch.".to_string()));
                eprintln!("[ZipherX] Full rescan state cleared.");
            }
            Err(e) => {
                let msg = format!("Full rescan failed: {}", e);
                eprintln!("[ZipherX] {}", msg);
                s.maintenance_result = Some(Err(msg));
            }
        }
    }
}

fn handle_refresh_peer_info(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    state: &Arc<Mutex<SharedState>>,
) {
    let infos = runtime.block_on(async {
        let pm = wallet.peer_manager.lock().await;
        pm.get_connected_peer_infos()
    });
    if let Ok(mut s) = state.lock() {
        s.peer_infos = infos
            .into_iter()
            .map(|p| PeerDisplayInfo {
                address: p.address,
                protocol_version: p.protocol_version,
                user_agent: p.user_agent,
                start_height: p.start_height as u64,
            })
            .collect();
    }
}

/// Load or generate the DB encryption key from secure storage.
fn ensure_db_key(storage: &GuiSecureStorage) -> Result<Vec<u8>, String> {
    let db_key_id = "db_encryption_key";
    if storage.has_key(db_key_id) {
        storage
            .load_key(db_key_id)
            .map_err(|e| format!("Failed to load DB key: {}", e))
    } else {
        let mut key = vec![0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        storage
            .store_key(db_key_id, &key)
            .map_err(|e| format!("Failed to store DB key: {}", e))?;
        Ok(key)
    }
}
