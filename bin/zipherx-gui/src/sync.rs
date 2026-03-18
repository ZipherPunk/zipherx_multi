//! Background sync engine.
//!
//! Spawns an OS thread with its own tokio runtime. Communicates with
//! the UI via `Arc<Mutex<SharedState>>`. The UI reads each frame;
//! the sync thread writes on progress callbacks.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rand::RngCore;
use zeroize::Zeroizing;
use zipherx_core::async_wallet::AsyncWallet;
use zipherx_core::wallet::WalletConfig;
use zipherx_network::header_sync::HeaderStore;
use zipherx_platform::SecureStorage;

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

    // -- pending confirmation: UI sets true when awaiting TX confirmation --
    // Wallet thread uses shorter sync interval (15s vs 90s) while true.
    pub pending_confirmation: bool,

    // -- transparent --
    pub transparent_balance: u64,
    pub transparent_address: Option<String>,

    // -- boost download failure (user must decide) --
    /// Set when boost download fails after all retries.
    /// Contains (reason, attempts). UI shows a dialog.
    pub boost_failed: Option<(String, u32)>,
    /// User's response to boost failure: true = continue with P2P, false = quit.
    pub boost_failed_continue: Option<bool>,

    // -- WIF import: raw decoded keys queued by UI for DB storage --
    pub pending_wif_imports: Vec<(Zeroizing<Vec<u8>>, String)>, // (raw_secret_key, address)

    // -- imported key count + funded transparent addresses for export --
    pub imported_key_count: u32,
    /// (address, balance, is_change, child_index, is_imported)
    pub funded_transparent_keys: Vec<(String, u64, bool, u32, bool)>,

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
    StartSync {
        sk_bytes: Zeroizing<Vec<u8>>,
    },
    Send {
        to_address: String,
        amount: u64,
        fee: u64,
        memo: Option<String>,
        sk_bytes: Zeroizing<Vec<u8>>,
    },
    TransparentSend {
        to_address: String,
        amount: u64,
        fee: u64,
        memo: Option<String>,
        /// C7: Seed wrapped in Zeroizing for automatic secure zeroing on drop.
        seed: Zeroizing<Vec<u8>>,
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

            transparent_balance: 0,
            transparent_address: None,

            mempool_tx: None,
            new_block_pending: false,
            pending_confirmation: false,

            boost_failed: None,
            boost_failed_continue: None,

            pending_wif_imports: Vec::new(),
            imported_key_count: 0,
            funded_transparent_keys: Vec::new(),
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
        spend_params_path: data_dir
            .join("sapling-spend.params")
            .to_string_lossy()
            .into(),
        output_params_path: data_dir
            .join("sapling-output.params")
            .to_string_lossy()
            .into(),
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

    // Check if there are pending unconfirmed TXs from a previous session.
    // If so, enable faster sync polling (15s) until they confirm.
    {
        let db = wallet.db.clone();
        if let Ok(has_pending) =
            runtime.block_on(async { tokio::task::spawn_blocking(move || db.has_pending_sent_transactions()).await })
        {
            if has_pending {
                if let Ok(mut s) = state.lock() {
                    s.pending_confirmation = true;
                }
                eprintln!("[ZipherX] Found pending unconfirmed TXs — using faster sync interval (15s)");
            }
        }
    }

    // Spawn a background thread that polls peer count from the atomic every 500ms.
    // The main loop can't update state.peer_count during blocking operations (sync, send),
    // so this poller ensures the UI always shows the current peer count.
    let peer_poller_state = state.clone();
    let peer_poller_atomic = wallet.connected_peer_count.clone();
    std::thread::spawn(move || loop {
        if let Ok(mut s) = peer_poller_state.lock() {
            s.peer_count = peer_poller_atomic.load(std::sync::atomic::Ordering::Relaxed);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    });

    let mut mempool_detector_set = false;
    let mut last_listener_check = std::time::Instant::now();

    // Cached sk_bytes: retained across sync calls so background sync works
    // even when the UI is locked (UI zeroes its own copy for security, but
    // the wallet thread keeps its own for autonomous background sync — same
    // as the FFI path does for Android/iOS).
    // I3: Wrap cached keys in Zeroizing for automatic secure zeroing on drop
    let mut cached_sk: Option<Zeroizing<Vec<u8>>> = None;
    let mut cached_seed: Option<Zeroizing<Vec<u8>>> = None;
    let mut last_bg_sync = std::time::Instant::now();
    let mut initial_sync_done = false;

    // Main loop: process commands from the UI
    loop {
        // Process pending WIF imports (queued by UI)
        {
            let pending = if let Ok(mut s) = state.lock() {
                std::mem::take(&mut s.pending_wif_imports)
            } else {
                Vec::new()
            };
            if !pending.is_empty() {
                let db = wallet.db.clone();
                for (raw_sk, address) in &pending {
                    // Encrypt the raw secret key using the storage, then store in DB
                    match storage.store_key(&format!("imported_wif_{}", address), raw_sk) {
                        Ok(()) => {
                            // Also store in the imported_transparent_keys table.
                            // SECURITY NOTE: The raw 32-byte secret key is stored in the DB,
                            // protected by SQLCipher (AES-256-CBC with Argon2id-derived key).
                            // If running without DB encryption (should not happen in production),
                            // the secret key would be exposed. The file-based copy via
                            // storage.store_key() above provides defense-in-depth encryption.
                            // Both layers require the user password.
                            let _ = runtime.block_on(async {
                                let db_c = db.clone();
                                let addr = address.clone();
                                let sk = raw_sk.clone();
                                tokio::task::spawn_blocking(move || {
                                    db_c.store_imported_transparent_key(&addr, &sk)
                                }).await
                            });
                            eprintln!("[ZipherX] Imported WIF key for {}", address);
                            // Set the transparent address in shared state so the UI can display it
                            if let Ok(mut s) = state.lock() {
                                s.transparent_address = Some(address.clone());
                            }
                        }
                        Err(e) => {
                            eprintln!("[ZipherX] Failed to import WIF for {}: {}", address, e);
                        }
                    }
                }
                // Zeroize raw keys using write_volatile to prevent compiler elision
                for (mut sk, _) in pending {
                    for b in sk.iter_mut() {
                        unsafe { std::ptr::write_volatile(b, 0); }
                    }
                }

                // Trigger full rescan so the scanner picks up UTXOs for the imported addresses.
                // This clears notes/history/UTXOs and resets scan state — the next sync cycle
                // will redo boost scan + full block scan with the imported address in the address set.
                eprintln!("[ZipherX] WIF import: triggering full rescan for imported addresses...");
                let _ = runtime.block_on(async {
                    let db_c = db.clone();
                    tokio::task::spawn_blocking(move || db_c.full_rescan_reset()).await
                });
                initial_sync_done = false;

                // Notify UI that a rescan is starting
                if let Ok(mut s) = state.lock() {
                    s.sync_phase = "Rescanning for imported addresses...".into();
                    s.sync_progress = 0.0;
                }
            }
        }

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
                // Cache sk_bytes for autonomous background sync
                if cached_sk.is_none() {
                    cached_sk = Some(sk_bytes.clone());
                }
                // Cache seed for transparent address scanning (I3: Zeroizing wrapper)
                if cached_seed.is_none() {
                    if let Ok(seed) = storage.load_key("wallet_seed") {
                        cached_seed = Some(Zeroizing::new(seed));
                    }
                }

                // Set up event-driven mempool detector before first sync
                // so block listeners started during sync already fire the callback.
                if !mempool_detector_set {
                    setup_mempool_detector(
                        &runtime,
                        &wallet,
                        &sk_bytes,
                        cached_seed.as_ref().map(|s| s.as_slice()),
                        &state,
                    );
                    mempool_detector_set = true;
                }

                handle_sync(&runtime, &wallet, &sk_bytes, cached_seed.as_ref().map(|s| s.as_slice()), &state);
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
                        eprintln!(
                            "[ZipherX] Auto-retry: total > spendable — notes need witness rebuild"
                        );
                        handle_sync(&runtime, &wallet, &sk_bytes, cached_seed.as_ref().map(|s| s.as_slice()), &state);
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
            Some(SyncCommand::Send {
                to_address,
                amount,
                fee,
                memo,
                mut sk_bytes,
            }) => {
                handle_send(
                    &runtime,
                    &wallet,
                    &to_address,
                    amount,
                    fee,
                    memo.as_deref(),
                    &sk_bytes,
                    &state,
                );
                // GUI-C5: Zeroize the cloned key material after use
                for b in sk_bytes.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            }
            Some(SyncCommand::TransparentSend {
                to_address,
                amount,
                fee,
                memo,
                seed,
            }) => {
                // C7: seed is Zeroizing<Vec<u8>> — auto-zeroed on drop
                handle_transparent_send(
                    &runtime,
                    &wallet,
                    &to_address,
                    amount,
                    fee,
                    memo.as_deref(),
                    &seed,
                    &state,
                    &storage,
                );
                // seed is dropped here — Zeroizing handles secure zeroing
            }
            Some(SyncCommand::SetTorEnabled(enabled)) => {
                // GUI-L1: Tor toggle not yet implemented
                eprintln!(
                    "[ZipherX] Tor toggle requested (enabled={}), not yet implemented",
                    enabled
                );
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
                //   - pending TX timer (15s) when awaiting confirmation
                //   - periodic timer (90s) when synced, no pending TX
                //   - retry timer (30s) when initial sync failed (network recovery)
                if let Some(ref sk) = cached_sk {
                    let (new_block, has_pending_tx) = if let Ok(mut s) = state.lock() {
                        let pending = s.new_block_pending;
                        if pending {
                            s.new_block_pending = false;
                        }
                        (pending, s.pending_confirmation)
                    } else {
                        (false, false)
                    };

                    // Check if we're significantly behind chain tip (>10 blocks).
                    // If so, use aggressive polling to catch up quickly.
                    let is_behind = if let Ok(_s) = state.lock() {
                        let peer_tip = wallet.connected_peer_count.load(std::sync::atomic::Ordering::Relaxed);
                        // If last sync was >5 min ago, we're likely behind
                        last_bg_sync.elapsed().as_secs() > 300 && peer_tip > 0
                    } else {
                        false
                    };

                    // Faster polling (15s) when awaiting TX confirmation or catching up,
                    // normal (90s) otherwise, retry (30s) on initial fail.
                    let retry_interval = if !initial_sync_done {
                        30
                    } else if has_pending_tx || is_behind {
                        15
                    } else {
                        90
                    };
                    let periodic = last_bg_sync.elapsed().as_secs() >= retry_interval;

                    if new_block || periodic {
                        if new_block {
                            // Wait for block to propagate before syncing.
                            eprintln!("[ZipherX] Wallet thread: new block — waiting 5s for propagation...");
                            std::thread::sleep(std::time::Duration::from_secs(5));
                        } else if !initial_sync_done {
                            eprintln!(
                                "[ZipherX] Wallet thread: retrying initial sync (network recovery)"
                            );
                        }
                        let height_before = if let Ok(s) = state.lock() { s.block_height } else { 0 };
                        handle_sync(&runtime, &wallet, sk, cached_seed.as_ref().map(|s| s.as_slice()), &state);
                        refresh_balance_and_history(&runtime, &wallet, &state);

                        // If inv MSG_BLOCK triggered this but no new block was found,
                        // peers haven't propagated headers yet. Retry once after 10s.
                        if new_block {
                            let height_after = if let Ok(s) = state.lock() { s.block_height } else { 0 };
                            if height_after <= height_before {
                                eprintln!("[ZipherX] Wallet thread: inv block but no new header — retrying in 10s");
                                std::thread::sleep(std::time::Duration::from_secs(10));
                                handle_sync(&runtime, &wallet, sk, cached_seed.as_ref().map(|s| s.as_slice()), &state);
                                refresh_balance_and_history(&runtime, &wallet, &state);

                                // If retry also failed, force reconnect to get fresh peers
                                let height_after_retry = if let Ok(s) = state.lock() { s.block_height } else { 0 };
                                if height_after_retry <= height_before {
                                    eprintln!("[ZipherX] Wallet thread: peers stale after inv — reconnecting");
                                    runtime.block_on(async {
                                        let mut pm = wallet.peer_manager.lock().await;
                                        pm.disconnect_all().await;
                                        let _ = pm.connect().await;
                                    });
                                    // One more sync attempt with fresh peers
                                    handle_sync(&runtime, &wallet, sk, cached_seed.as_ref().map(|s| s.as_slice()), &state);
                                    refresh_balance_and_history(&runtime, &wallet, &state);
                                }
                            }
                        }

                        last_bg_sync = std::time::Instant::now();

                        // Restart block listeners immediately after sync — sync consumes
                        // the readers (kills listeners). Without this, inv MSG_BLOCK
                        // messages are missed until the 30s health check restarts them.
                        if mempool_detector_set {
                            runtime.block_on(async {
                                let mut pm = wallet.peer_manager.lock().await;
                                if pm.connected_count() > 0 && !pm.has_active_block_listeners() {
                                    pm.start_all_block_listeners().await;
                                }
                            });
                        }

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

    // I3: cached_sk and cached_seed are Zeroizing<Vec<u8>> — auto-zeroed on drop
    drop(cached_sk);
    drop(cached_seed);
}

fn handle_sync(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    sk_bytes: &[u8],
    seed: Option<&[u8]>,
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
                SyncStatus::BoostDownload {
                    downloaded_bytes,
                    total_bytes,
                } => {
                    s.sync_phase = "boost_download".to_string();
                    s.sync_current = *downloaded_bytes;
                    s.sync_target = *total_bytes;
                }
                SyncStatus::BoostLoad { loaded, total } => {
                    s.sync_phase = "boost_load".to_string();
                    s.sync_current = *loaded;
                    s.sync_target = *total;
                }
                SyncStatus::HeaderSync {
                    current_height,
                    target_height,
                } => {
                    s.sync_phase = "header_sync".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::DeltaSync {
                    current_height,
                    target_height,
                } => {
                    s.sync_phase = "delta_sync".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::BoostScan { outputs_total } => {
                    s.sync_phase = "boost_scan".to_string();
                    s.sync_current = 0;
                    s.sync_target = *outputs_total;
                }
                SyncStatus::BlockScan {
                    current_height,
                    target_height,
                    ..
                } => {
                    s.sync_phase = "block_scan".to_string();
                    s.sync_current = *current_height;
                    s.sync_target = *target_height;
                }
                SyncStatus::WitnessUpdate {
                    notes_updated,
                    total_notes,
                } => {
                    s.sync_phase = "witness_update".to_string();
                    s.sync_current = *notes_updated as u64;
                    s.sync_target = *total_notes as u64;
                }
                SyncStatus::GapFill { gaps_remaining } => {
                    s.sync_phase = "gap_fill".to_string();
                    s.sync_current = 0;
                    s.sync_target = *gaps_remaining as u64;
                }
                SyncStatus::ConfirmationsUpdated { height } => {
                    s.sync_phase = format!("Finalizing {}", height);
                    s.sync_height = *height;
                    s.block_height = *height;
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

    let result = if let Some(s) = seed {
        runtime.block_on(wallet.sync_with_transparent(sk_bytes, s, Some(progress_fn)))
    } else {
        runtime.block_on(wallet.sync(sk_bytes, Some(progress_fn)))
    };

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
                if balance.note_count > 0 && balance.note_count > balance.spendable_note_count {
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
                        let repair_progress_fn: std::sync::Arc<
                            dyn Fn(zipherx_core::sync::SyncStatus) + Send + Sync,
                        > = std::sync::Arc::new(move |status: zipherx_core::sync::SyncStatus| {
                            if let Ok(mut s) = state_repair.lock() {
                                use zipherx_core::sync::SyncStatus;
                                match &status {
                                    SyncStatus::BoostDownload {
                                        downloaded_bytes,
                                        total_bytes,
                                    } => {
                                        s.sync_phase = "Repairing: downloading boost".to_string();
                                        s.sync_current = *downloaded_bytes;
                                        s.sync_target = *total_bytes;
                                    }
                                    SyncStatus::HeaderSync {
                                        current_height,
                                        target_height,
                                    } => {
                                        s.sync_phase = "Repairing: syncing headers".to_string();
                                        s.sync_current = *current_height;
                                        s.sync_target = *target_height;
                                    }
                                    SyncStatus::DeltaSync {
                                        current_height,
                                        target_height,
                                    } => {
                                        s.sync_phase = "Repairing: syncing blocks".to_string();
                                        s.sync_current = *current_height;
                                        s.sync_target = *target_height;
                                    }
                                    SyncStatus::BlockScan {
                                        current_height,
                                        target_height,
                                        ..
                                    } => {
                                        s.sync_phase = "Repairing: scanning blocks".to_string();
                                        s.sync_current = *current_height;
                                        s.sync_target = *target_height;
                                    }
                                    SyncStatus::WitnessUpdate {
                                        notes_updated,
                                        total_notes,
                                    } => {
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
                        if let Ok(h) =
                            runtime.block_on(wallet.sync(sk_bytes, Some(repair_progress_fn)))
                        {
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
            let err_msg = e.to_string();
            eprintln!("[ZipherX] Sync error: {}", err_msg);

            // If sync failed due to no peers, try to reconnect
            if err_msg.contains("No peers") || err_msg.contains("peer") {
                eprintln!("[ZipherX] Attempting peer reconnection after sync failure");
                runtime.block_on(async {
                    let mut pm = wallet.peer_manager.lock().await;
                    pm.disconnect_all().await;
                    let _ = pm.connect().await;
                });
            }

            if let Ok(mut s) = state.lock() {
                s.sync_error = Some(err_msg);
            }
        }
    }
}

/// Set up event-driven mempool detection on the peer manager.
///
/// Block listeners handle inv→getdata→tx internally and fire the callback
/// with raw TX bytes. Trial decryption happens synchronously in the callback.
/// When seed is available, also matches transparent outputs against derived addresses.
/// No separate task, no channel, no extra TCP connection.
fn setup_mempool_detector(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    sk_bytes: &[u8],
    seed: Option<&[u8]>,
    state: &Arc<Mutex<SharedState>>,
) {
    let state_clone = state.clone();
    let detector = if let Some(seed) = seed {
        zipherx_core::mempool_monitor::MempoolDetector::new_with_transparent(
            sk_bytes.to_vec(),
            seed,
            std::sync::Arc::new(move |info: zipherx_core::mempool_monitor::MempoolTxInfo| {
                if let Ok(mut s) = state_clone.lock() {
                    s.mempool_tx = Some(info);
                }
            }),
        )
    } else {
        zipherx_core::mempool_monitor::MempoolDetector::new(
            sk_bytes.to_vec(),
            std::sync::Arc::new(move |info: zipherx_core::mempool_monitor::MempoolTxInfo| {
                if let Ok(mut s) = state_clone.lock() {
                    s.mempool_tx = Some(info);
                }
            }),
        )
    };
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
                SendPhase::Building {
                    spend_index,
                    total_spends,
                } => {
                    s.send_phase = format!("Building proof {}/{}", spend_index + 1, total_spends);
                    s.send_current = *spend_index;
                    s.send_total = *total_spends;
                }
                SendPhase::Broadcasting => {
                    s.send_phase = "Broadcasting...".to_string();
                }
                SendPhase::PeerResponse {
                    accepted,
                    rejected,
                    total,
                } => {
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
    let result =
        runtime.block_on(wallet.send(request.clone(), sk_bytes, Some(progress_fn.clone())));

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
                eprintln!(
                    "[ZipherX] Reconnected {} peers, retrying broadcast...",
                    peer_count
                );
                runtime.block_on(wallet.send(request, sk_bytes, Some(progress_fn)))
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

fn handle_transparent_send(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    to_address: &str,
    amount: u64,
    fee: u64,
    memo: Option<&str>,
    seed: &[u8],
    state: &Arc<Mutex<SharedState>>,
    storage: &Arc<crate::platform::GuiSecureStorage>,
) {
    eprintln!(
        "[ZipherX] Transparent send: {} zatoshis to {}",
        amount, to_address,
    );

    // Update progress
    if let Ok(mut s) = state.lock() {
        s.send_phase = "Selecting UTXOs...".to_string();
        s.send_current = 0;
        s.send_total = 4;
    }

    // Get unspent UTXOs
    let utxos = match runtime.block_on(wallet.get_unspent_transparent_utxos()) {
        Ok(u) => u,
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(format!("Failed to get UTXOs: {}", e)));
            }
            return;
        }
    };

    if utxos.is_empty() {
        if let Ok(mut s) = state.lock() {
            s.send_result = Some(Err("No transparent UTXOs available".to_string()));
        }
        return;
    }

    // Select UTXOs to cover amount + fee.
    // Sort by value DESCENDING so we pick the fewest UTXOs possible
    // and avoid consuming the entire transparent balance.
    let total_needed = match amount.checked_add(fee) {
        Some(v) => v,
        None => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err("amount + fee overflow".to_string()));
            }
            return;
        }
    };
    let mut sorted_utxos = utxos.clone();
    sorted_utxos.sort_by(|a, b| b.value.cmp(&a.value));

    eprintln!(
        "[ZipherX] UTXO selection: amount={}, fee={}, total_needed={}, {} UTXOs available",
        amount, fee, total_needed, sorted_utxos.len(),
    );
    for (i, u) in sorted_utxos.iter().enumerate() {
        eprintln!(
            "[ZipherX]   UTXO[{}]: txid={}.. value={} is_change={}",
            i, &u.txid[..16], u.value, u.is_change,
        );
    }

    let mut selected = Vec::new();
    let mut selected_total: u64 = 0;
    for utxo in &sorted_utxos {
        selected.push(utxo.clone());
        selected_total = selected_total.checked_add(utxo.value).unwrap_or(u64::MAX);
        if selected_total >= total_needed {
            break;
        }
    }

    let change_amount = selected_total.saturating_sub(total_needed);
    eprintln!(
        "[ZipherX] UTXO selection: picked {} UTXOs, total_input={}, change={}",
        selected.len(), selected_total, change_amount,
    );

    if selected_total < total_needed {
        if let Ok(mut s) = state.lock() {
            s.send_result = Some(Err(format!(
                "Insufficient transparent funds: have {}, need {}",
                selected_total, total_needed,
            )));
        }
        return;
    }

    if let Ok(mut s) = state.lock() {
        s.send_phase = "Building transaction...".to_string();
        s.send_current = 1;
    }

    // Build transparent spend infos
    let mut spend_infos = Vec::new();
    for utxo in &selected {
        #[cfg(debug_assertions)]
        eprintln!(
            "[ZipherX] UTXO spend: txid={}.. output_index={} value={} is_change={} child_index={} addr={}",
            &utxo.txid[..16], utxo.output_index, utxo.value, utxo.is_change, utxo.child_index, utxo.address,
        );
        #[cfg(debug_assertions)]
        eprintln!(
            "[ZipherX] UTXO script_pubkey ({} bytes): {}",
            utxo.script_pubkey.len(), hex::encode(&utxo.script_pubkey),
        );

        // Derive the secret key for this UTXO
        let sk = if utxo.is_imported {
            // Imported key: load from secure file storage
            let key_id = format!("imported_wif_{}", utxo.address);
            match storage.load_key(&key_id) {
                Ok(raw_sk) => zeroize::Zeroizing::new(raw_sk),
                Err(e) => {
                    if let Ok(mut s) = state.lock() {
                        s.send_result = Some(Err(format!("Imported key load failed: {}", e)));
                    }
                    return;
                }
            }
        } else {
            // Seed-derived key
            match zipherx_crypto::transparent::derive_transparent_secret_key(
                seed, 0, utxo.child_index, utxo.is_change,
            ) {
                Ok(s) => s,
                Err(e) => {
                    if let Ok(mut s) = state.lock() {
                        s.send_result = Some(Err(format!("Key derivation failed: {}", e)));
                    }
                    return;
                }
            }
        };

        // Verify address match (skip for imported — already validated at import time)
        if !utxo.is_imported {
        let derived_addr = if utxo.is_change {
            zipherx_crypto::transparent::derive_transparent_change_address(seed, 0, utxo.child_index)
        } else {
            zipherx_crypto::transparent::derive_transparent_address(seed, 0, utxo.child_index)
        };
        match &derived_addr {
            Ok(addr) => {
                let matches = addr == &utxo.address;
                #[cfg(debug_assertions)]
                eprintln!(
                    "[ZipherX] Key verification: derived={} utxo={} match={}",
                    addr, utxo.address, matches,
                );
                if !matches {
                    if let Ok(mut s) = state.lock() {
                        s.send_result = Some(Err(format!(
                            "UTXO address mismatch: derived {} but UTXO has {}",
                            addr, utxo.address,
                        )));
                    }
                    return;
                }
            }
            Err(e) => {
                eprintln!("[ZipherX] WARNING: address derivation for verification failed: {}", e);
            }
        }
        } // end if !utxo.is_imported

        // Parse txid hex to bytes — UTXO stores display format (reversed),
        // but OutPoint needs internal byte order, so reverse after decode.
        let mut txid_bytes = [0u8; 32];
        if let Ok(decoded) = hex::decode(&utxo.txid) {
            if decoded.len() == 32 {
                txid_bytes.copy_from_slice(&decoded);
                txid_bytes.reverse(); // display → internal byte order
            }
        }

        spend_infos.push(zipherx_crypto::transaction::TransparentSpendInfo {
            secret_key: sk.to_vec(),
            prevout_txid: txid_bytes,
            prevout_index: utxo.output_index,
            script_pubkey: utxo.script_pubkey.clone(),
            value: utxo.value,
        });
    }

    // We need a Sapling SK for change address (shielded change for privacy).
    // Load spending key from the seed.
    let sk_bytes = match zipherx_crypto::keys::derive_spending_key(seed, 0) {
        Ok(sk) => sk, // Keep Zeroizing<Vec<u8>> wrapper for automatic zeroing on drop
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(format!("SK derivation failed: {}", e)));
            }
            return;
        }
    };

    // Get chain height from header store
    let chain_height = wallet
        .header_store
        .get_latest_height()
        .ok()
        .flatten()
        .unwrap_or(0);

    if let Ok(mut s) = state.lock() {
        s.send_phase = "Building proof...".to_string();
        s.send_current = 2;
    }

    // Build the transaction (memo supported for shielded destinations)
    let memo_bytes = memo.map(|m| m.as_bytes().to_vec());

    // I2: Rotate change address — use next available child_index to avoid address reuse.
    // Propagate DB errors instead of silently falling back to index 0 (which would reuse addresses).
    let next_change_idx = match wallet.db.next_transparent_change_index() {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("[ZipherX] Failed to get next change index: {}", e);
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(format!("DB error getting change index: {}", e)));
            }
            return;
        }
    };
    let t_change_addr = match zipherx_crypto::transparent::derive_transparent_change_address(seed, 0, next_change_idx) {
        Ok(addr) => {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Transparent change address (child_index={}): {}", next_change_idx, &addr);
            Some(addr)
        }
        Err(_) => {
            // No seed (PK + WIF import) — use an imported transparent address for change.
            // Read from shared state, or fall back to the source UTXO address.
            let fallback_addr = if let Ok(s) = state.lock() {
                s.transparent_address.clone()
            } else {
                None
            };
            if let Some(addr) = fallback_addr {
                eprintln!("[ZipherX] No seed — using imported address for change: {}", addr);
                Some(addr)
            } else {
                eprintln!("[ZipherX] WARNING: no seed and no imported address — change will go to shielded");
                None
            }
        }
    };

    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] Building transparent TX: amount={} to={} chain_height={} change_addr={:?}",
        amount, to_address, chain_height, t_change_addr.as_deref(),
    );

    let tx_result = zipherx_crypto::transaction::build_transparent_spend_transaction(
        &sk_bytes,
        to_address,
        amount,
        memo_bytes.as_deref(),
        &spend_infos,
        chain_height,
        t_change_addr.as_deref(),
    );

    let tx_result = match tx_result {
        Ok(r) => {
            eprintln!(
                "[ZipherX] TX built: {} bytes",
                r.tx_bytes.len(),
            );
            // Log first 8 bytes (version + version_group_id) and last 4 (expiry)
            if r.tx_bytes.len() >= 16 {
                eprintln!(
                    "[ZipherX] TX header: {}",
                    hex::encode(&r.tx_bytes[..16]),
                );
            }
            r
        }
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(format!("TX build failed: {}", e)));
            }
            return;
        }
    };

    if let Ok(mut s) = state.lock() {
        s.send_phase = "Broadcasting...".to_string();
        s.send_current = 3;
    }

    // Compute txid before broadcast
    let txid_bytes = zipherx_crypto::util::double_sha256(&tx_result.tx_bytes);
    let mut txid_display = txid_bytes;
    txid_display.reverse();
    let txid_hex = hex::encode(txid_display);

    // Broadcast
    let broadcast_result = runtime.block_on(async {
        let pm = wallet.peer_manager.lock().await;
        pm.broadcast_transaction(&tx_result.tx_bytes, &txid_hex).await
    });

    match broadcast_result {
        Ok(br) => {
            let accepted = br.accepted_by.len();
            let rejected = br.rejected_by.len();

            if !br.success {
                if let Ok(mut s) = state.lock() {
                    let reasons: Vec<String> = br.rejected_by.iter().map(|(_, r)| r.clone()).collect();
                    s.send_result = Some(Err(format!(
                        "TX rejected: {}",
                        reasons.join(", "),
                    )));
                }
                return;
            }

            eprintln!(
                "[ZipherX] Transparent TX broadcast: {} ({} accepted, {} rejected)",
                &txid_hex[..16], accepted, rejected,
            );

            // Mark spent UTXOs in DB
            let db = wallet.db.clone();
            for utxo in &selected {
                let _ = db.mark_transparent_spent_by_prevout(
                    &utxo.txid,
                    utxo.output_index,
                    &txid_hex,
                    0, // unconfirmed — height will be set when TX is mined
                );
            }

            // Record TX in transaction_history (so it shows in history view)
            let tx_type_val = if to_address.starts_with("zs") {
                zipherx_storage::types::TxType::SelfT2Z
            } else {
                zipherx_storage::types::TxType::Sent
            };
            let now_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = db.insert_transaction(
                &txid_hex,
                0, // height (unconfirmed)
                Some(now_ts),
                tx_type_val,
                amount,
                fee,
                Some(to_address),
                None, // memo
                zipherx_storage::types::TxStatus::Pending,
            );
            eprintln!(
                "[ZipherX] Transparent TX {} recorded as '{}'",
                &txid_hex[..16], tx_type_val.as_str()
            );

            if let Ok(mut s) = state.lock() {
                s.send_phase = "Complete".to_string();
                s.send_current = 4;
                s.send_result = Some(Ok(SendResultInfo {
                    txid: txid_hex,
                    amount,
                    fee,
                }));
                s.mempool_accepted = true;
            }
        }
        Err(e) => {
            if let Ok(mut s) = state.lock() {
                s.send_result = Some(Err(format!("Broadcast failed: {}", e)));
            }
        }
    }
}

fn refresh_balance_and_history(
    runtime: &tokio::runtime::Runtime,
    wallet: &AsyncWallet,
    state: &Arc<Mutex<SharedState>>,
) {
    // Balance (shielded)
    if let Ok(balance) = runtime.block_on(wallet.get_balance()) {
        if let Ok(mut s) = state.lock() {
            s.total_balance = balance.total;
            s.spendable_balance = balance.spendable;
            s.note_count = balance.note_count;
            s.spendable_note_count = balance.spendable_note_count;
        }
    }
    // Balance (transparent)
    if let Ok(t_balance) = runtime.block_on(wallet.get_transparent_balance()) {
        if let Ok(mut s) = state.lock() {
            s.transparent_balance = t_balance;
        }
    }

    // Imported key count + funded transparent addresses (for export UI)
    {
        let db = wallet.db.clone();
        if let Ok(count) = db.get_imported_key_count() {
            if let Ok(mut s) = state.lock() {
                s.imported_key_count = count;
            }
        }
        if let Ok(funded) = db.get_funded_transparent_addresses() {
            if let Ok(mut s) = state.lock() {
                // Set transparent_address from the first funded address (for display)
                if s.transparent_address.is_none() {
                    if let Some(first) = funded.first() {
                        s.transparent_address = Some(first.address.clone());
                    }
                }
                s.funded_transparent_keys = funded
                    .iter()
                    .map(|f| {
                        (
                            f.address.clone(),
                            f.balance,
                            f.is_change,
                            f.child_index,
                            f.is_imported,
                        )
                    })
                    .collect();
            }
        }
        // Also set transparent_address from imported keys (even if no funded UTXOs yet)
        if let Ok(imported) = db.get_imported_transparent_addresses() {
            if !imported.is_empty() {
                if let Ok(mut s) = state.lock() {
                    if s.transparent_address.is_none() {
                        s.transparent_address = Some(imported[0].1.clone());
                    }
                }
            }
        }
    }

    // History
    if let Ok(records) = runtime.block_on(wallet.get_transaction_history(50, 0)) {
        if let Ok(mut s) = state.lock() {
            // Build display records with self-send detection
            let mut result = Vec::new();
            let mut processed = std::collections::HashSet::new();

            // Group by txid
            let mut grouped: std::collections::HashMap<
                String,
                Vec<&zipherx_core::wallet::TransactionDisplay>,
            > = std::collections::HashMap::new();
            for r in &records {
                grouped.entry(r.txid.clone()).or_default().push(r);
            }

            for r in &records {
                if processed.contains(&r.txid) {
                    continue;
                }
                let group = &grouped[&r.txid];
                let sent_types = ["sent", "self_z2t", "self_t2z"];
                let has_sent = group.iter().any(|t| sent_types.contains(&t.tx_type.as_str()));
                let has_received = group.iter().any(|t| t.tx_type == "received");

                if has_sent && has_received {
                    let sent = group.iter().find(|t| sent_types.contains(&t.tx_type.as_str())).unwrap();

                    // Check ALL received entries for transparent vs shielded addresses.
                    // A z→t tx has multiple received entries: shielded change + transparent UTXO.
                    let has_t_received = group.iter().any(|t| {
                        t.tx_type == "received"
                            && t.address.as_ref().map_or(false, |a| a.starts_with("t1") || a.starts_with("t3"))
                    });
                    let has_z_received = group.iter().any(|t| {
                        t.tx_type == "received"
                            && t.address.as_ref().map_or(true, |a| !a.starts_with("t1") && !a.starts_with("t3"))
                    });

                    let sent_dest_is_transparent = sent.address.as_ref()
                        .map_or(false, |a| a.starts_with("t1") || a.starts_with("t3"));

                    // z→t: sent destination is t-addr AND a transparent UTXO was received
                    // t→z: sent destination is z-addr AND a shielded note was received (no t-received)
                    let is_z2t = sent_dest_is_transparent && has_t_received;
                    let is_t2z = !sent_dest_is_transparent && has_z_received && !has_t_received;

                    if is_z2t {
                        // Cross-pool: z→t shield-to-transparent transfer
                        // Use the transparent received entry for amount/address
                        let t_received = group.iter().find(|t| {
                            t.tx_type == "received"
                                && t.address.as_ref().map_or(false, |a| a.starts_with("t1") || a.starts_with("t3"))
                        }).unwrap();
                        result.push(crate::app::TransactionRecord {
                            txid: r.txid.clone(),
                            tx_type: "self_z2t".to_string(),
                            amount: t_received.amount,
                            fee: sent.fee,
                            address: t_received.address.clone(),
                            memo: sent.memo.clone(),
                            confirmations: sent.confirmations.max(t_received.confirmations),
                            height: sent.height.max(t_received.height),
                            timestamp: sent.timestamp.max(t_received.timestamp),
                        });
                    } else if is_t2z {
                        // Cross-pool: t→z transparent-to-shield transfer
                        // Use the shielded received entry for amount
                        let z_received = group.iter().find(|t| {
                            t.tx_type == "received"
                                && t.address.as_ref().map_or(true, |a| !a.starts_with("t1") && !a.starts_with("t3"))
                        }).unwrap();
                        result.push(crate::app::TransactionRecord {
                            txid: r.txid.clone(),
                            tx_type: "self_t2z".to_string(),
                            amount: z_received.amount,
                            fee: sent.fee,
                            address: sent.address.clone(),
                            memo: z_received.memo.clone(),
                            confirmations: sent.confirmations.max(z_received.confirmations),
                            height: sent.height.max(z_received.height),
                            timestamp: sent.timestamp.max(z_received.timestamp),
                        });
                    } else {
                        // Same-pool self-send (z→z or t→t): display as "self" with fee
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
                    }
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
                s.maintenance_result =
                    Some(Ok("Database repaired. Run sync to rebuild.".to_string()));
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
                s.maintenance_result = Some(Ok(
                    "All data cleared. Next sync will re-scan from scratch.".to_string(),
                ));
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
