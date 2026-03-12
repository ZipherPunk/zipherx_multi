//! Async sync orchestration — wires pure-logic sync functions to real P2P and storage.
//!
//! Critical invariants:
//! - DB ops via `spawn_blocking` (rusqlite is sync)
//! - Guard acquisition with automatic release (Drop guard)
//! - Never advance endHeight when 0 blocks fetched (FIX #1262)
//! - Delta immutable when verified (FIX #1252)
//! - Gap-fill waits for header sync (FIX #1220)
//! - Root validation via HeaderStore, not P2P (FIX #1220)
//! - Both byte orders for root comparison (FIX #1230)

use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use zeroize::Zeroize;

use crate::async_block_fetch;
use crate::boost_download::{
    self, BOOST_SECTION_OUTPUTS, BOOST_SECTION_SPENDS, BOOST_SECTION_TREE,
};
use crate::scanner;
use crate::sync::{
    self, DeltaSyncConfig, DeltaSyncResult, GapFillResult, SyncGuards, SyncStatus,
    SAPLING_ACTIVATION_HEIGHT,
};
use crate::CoreError;
use zipherx_crypto::tree as commitment_tree;
use zipherx_network::block_fetcher::{CompactBlock, PacingConfig, ShieldedOutput, ShieldedSpend};
use zipherx_network::header_sync::{HeaderStore, HeaderSync};
use zipherx_network::peer_manager::PeerManager;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::delta_cmu::{DeltaCMUStore, DeltaNullifier, DeltaOutput};
use zipherx_storage::header_store_impl::SqliteHeaderStore;
use zipherx_storage::types::{TxStatus, TxType};

/// Progress callback type for sync operations.
pub type SyncProgressFn = Arc<dyn Fn(SyncStatus) + Send + Sync>;

// ============================================================================
// Drop Guard for SyncGuards
// ============================================================================

/// RAII guard that releases the sync flag on drop.
/// Ensures guards are always released, even on early return or error.
struct SyncDropGuard<'a> {
    guards: &'a SyncGuards,
    flag: SyncFlag,
}

enum SyncFlag {
    Syncing,
    GapFilling,
}

impl<'a> Drop for SyncDropGuard<'a> {
    fn drop(&mut self) {
        match self.flag {
            SyncFlag::Syncing => {
                self.guards
                    .is_syncing
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
            SyncFlag::GapFilling => {
                self.guards
                    .is_gap_filling
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}

// ============================================================================
// Sync to Tip
// ============================================================================

/// Full sync flow: connect → headers → report complete.
///
/// Acquires the sync guard and releases on completion or error.
///
/// RC-23: Ideally this would use `tokio::task::Builder::new().name("zipherx-sync")`
/// for the spawned tasks, but `Builder` requires the unstable `tokio_unstable` cfg
/// flag. The top-level sync entry points are documented here instead. If
/// `tokio_unstable` is enabled in the future, add `.name()` to the `tokio::spawn`
/// calls in this module (boost download, header sync forwarder, spawn_blocking).
pub async fn sync_to_tip(
    peer_manager: &mut PeerManager,
    header_store: &Arc<SqliteHeaderStore>,
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    guards: &SyncGuards,
    progress: Option<SyncProgressFn>,
    peer_count_ref: Option<Arc<AtomicU32>>,
    boost_cache_override: Option<std::path::PathBuf>,
) -> Result<u64, CoreError> {
    // Try to acquire the sync guard
    if !guards.try_acquire_sync() {
        return Err(CoreError::SyncInProgress);
    }
    let _guard = SyncDropGuard {
        guards,
        flag: SyncFlag::Syncing,
    };

    // FIX #1184: Check broadcasting flag
    if guards
        .is_broadcasting
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(CoreError::BroadcastingInProgress);
    }

    // Check for full re-download flag (migration v4)
    {
        let db_check = db.clone();
        let needs_redownload =
            tokio::task::spawn_blocking(move || db_check.check_and_clear_redownload_flag())
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;

        if needs_redownload {
            eprintln!("[ZipherX] Full re-download requested: clearing delta store");
            delta_store
                .clear_delta_bundle(true, false)
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            eprintln!("[ZipherX] Delta store cleared, will re-download from peers");
        }
    }

    // Report starting
    if let Some(ref p) = progress {
        p(SyncStatus::HeaderSync {
            current_height: 0,
            target_height: 0,
        });
    }

    // ================================================================
    // Step 1: Start boost download AND peer connection CONCURRENTLY.
    //
    // The boost download is a 2GB HTTP download from GitHub — completely
    // independent of P2P peers. Starting it before/during peer connection
    // saves the 15-45s spent waiting for peer timeouts.
    // ================================================================

    let boost_cache_dir = if !sk_bytes.is_empty() {
        // Use explicit override (e.g. Android external storage) or default derivation
        boost_cache_override.or_else(|| {
            delta_store
                .base_dir()
                .parent()
                .map(|p| p.join("BoostCache"))
        })
    } else {
        None
    };

    // Determine if boost download is needed (quick filesystem check)
    let boost_needs_download = if let Some(ref boost_dir) = boost_cache_dir {
        let boost_file = boost_dir.join("zipherx_boost_v1.bin");
        let manifest_file = boost_dir.join("zipherx_boost_manifest.json");
        !boost_file.exists()
            || !manifest_file.exists()
            || std::fs::metadata(&boost_file).map(|m| m.len()).unwrap_or(0) < 100_000_000
    } else {
        false
    };

    // Check for boost update (only if boost already exists and no download needed)
    let mut update_tag: Option<String> = None;
    if !boost_needs_download {
        if let Some(ref boost_dir) = boost_cache_dir {
            let manifest_file = boost_dir.join("zipherx_boost_manifest.json");
            if manifest_file.exists() {
                let mp = manifest_file.to_string_lossy().to_string();
                match boost_download::check_for_boost_update(&mp).await {
                    Ok(Some(tag)) => {
                        eprintln!(
                            "[ZipherX] New boost available (tag: {}), clearing old cache...",
                            tag,
                        );
                        let boost_file = boost_dir.join("zipherx_boost_v1.bin");
                        let _ = std::fs::remove_file(&boost_file);
                        let _ = std::fs::remove_file(&manifest_file);

                        // Reset DB tree state so boost_scan re-processes with new data.
                        let db_c = db.clone();
                        let _ = tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                            db_c.save_tree_state(&[], 0)
                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                            db_c.set_delta_bundle_verified(false)
                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                            Ok(())
                        })
                        .await;

                        update_tag = Some(tag);
                    }
                    Ok(None) => {} // Up to date
                    Err(e) => {
                        eprintln!("[ZipherX] Boost update check failed (non-fatal): {e}");
                    }
                }
            }
        }
    }

    // Spawn boost download as a concurrent task (runs in parallel with peer connection)
    let boost_download_handle = if boost_needs_download || update_tag.is_some() {
        if let Some(ref boost_dir) = boost_cache_dir {
            eprintln!("[ZipherX] Downloading boost file (contains 2.5M headers + outputs)...");
            if let Some(ref p) = progress {
                p(SyncStatus::BoostDownload {
                    downloaded_bytes: 0,
                    total_bytes: 0,
                });
            }
            let download_progress: Option<boost_download::DownloadProgressFn> =
                progress.as_ref().map(|p| {
                    let p = p.clone();
                    Arc::new(move |downloaded: u64, total: u64, _label: &str| {
                        eprintln!(
                            "[ZipherX] Boost download: {} / {} MB",
                            downloaded / (1024 * 1024),
                            total / (1024 * 1024),
                        );
                        p(SyncStatus::BoostDownload {
                            downloaded_bytes: downloaded,
                            total_bytes: total,
                        });
                    }) as Arc<dyn Fn(u64, u64, &str) + Send + Sync>
                });

            let boost_dir_clone = boost_dir.clone();
            let tag_clone = update_tag.clone();
            Some(tokio::spawn(async move {
                let tag_ref = tag_clone.as_deref();
                boost_download::download_boost_file_if_needed(
                    &boost_dir_clone,
                    download_progress,
                    tag_ref,
                )
                .await
            }))
        } else {
            None
        }
    } else {
        None
    };

    // Step 1b: Connect to peers (runs concurrently with boost download)
    if peer_manager.connected_count() == 0 {
        let mut last_err = None;
        for attempt in 1..=3 {
            tracing::info!("Peer connection attempt {}/3...", attempt);

            match peer_manager.connect_with_counter(peer_count_ref.as_ref()).await {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    tracing::warn!("Connection attempt {}/3 failed: {e}", attempt);
                    last_err = Some(e);
                    if attempt < 3 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                }
            }
        }

        if let Some(e) = last_err {
            // If boost download is running, don't fail — we still need it
            if boost_download_handle.is_none() {
                return Err(CoreError::Network(e));
            }
            eprintln!(
                "[ZipherX] Peer connection failed but boost download in progress, continuing..."
            );
        }
    }

    let peer_count = peer_manager.connected_count();
    if let Some(ref pc) = peer_count_ref {
        pc.store(peer_count as u32, Ordering::Relaxed);
    }
    tracing::info!("{} peers connected", peer_count);

    // Step 1c: Await boost download result (may already be done if peers were slow)
    let mut boost_download_result: Option<(String, String)> = None;
    if let Some(handle) = boost_download_handle {
        match handle.await {
            Ok(Ok(result)) => {
                boost_download_result = Some(result);
            }
            Ok(Err(e)) => {
                eprintln!("[ZipherX] Boost download failed: {e}");
            }
            Err(e) => {
                eprintln!("[ZipherX] Boost download task panicked: {e}");
            }
        }
    }

    // Now check peer count — we need at least 1 peer for header sync
    if peer_count == 0 {
        return Err(CoreError::Network(
            zipherx_network::types::NetworkError::NoPeersAvailable,
        ));
    }

    // ================================================================
    // Step 2: Load boost headers into HeaderStore (if not already loaded).
    //
    // The boost file contains 2.5M Sapling-era block headers (Section 7).
    // Loading them into HeaderStore eliminates the need for P2P header sync
    // across that range. P2P only fetches the small gap from boost height
    // to chain tip (~5K headers instead of 2.5M).
    // ================================================================

    if let Some(ref boost_dir) = boost_cache_dir {
        // Load headers from boost file into HeaderStore (if not already loaded)
        let boost_file = boost_dir.join("zipherx_boost_v1.bin");
        let manifest_file = boost_dir.join("zipherx_boost_manifest.json");

        if boost_file.exists() && manifest_file.exists() {
            let manifest_path = manifest_file.to_string_lossy().to_string();
            if let Ok(manifest) = boost_download::parse_manifest(&manifest_path) {
                let current_h = header_store
                    .get_latest_height()
                    .map_err(|e| CoreError::Storage(e.to_string()))?
                    .unwrap_or(0);

                if current_h < manifest.chain_height {
                    eprintln!(
                        "[ZipherX] HeaderStore at height {} < boost height {}, loading headers from boost file...",
                        current_h, manifest.chain_height
                    );
                    if let Some(ref p) = progress {
                        p(SyncStatus::BoostLoad {
                            loaded: 0,
                            total: manifest.chain_height,
                        });
                    }

                    // Enable bulk import mode for 10-50x faster inserts
                    header_store
                        .begin_bulk_import()
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                    // Use existing HeaderStoreAdapter (Arc-based, Send+Sync) for spawn_blocking
                    let adapter = Arc::new(HeaderStoreAdapter(header_store.clone()));
                    let bf = boost_file.to_string_lossy().to_string();
                    let m = manifest.clone();

                    // Build progress callback that reports BoostLoad status
                    let progress_for_load: Option<boost_download::BoostLoadProgressFn> =
                        progress.as_ref().map(|p| {
                            let p = p.clone();
                            Box::new(move |loaded: u64, total: u64| {
                                p(SyncStatus::BoostLoad { loaded, total });
                            }) as boost_download::BoostLoadProgressFn
                        });

                    let load_result = tokio::task::spawn_blocking(move || {
                        boost_download::load_boost_headers_with_progress(
                            &bf,
                            &m,
                            adapter.as_ref(),
                            progress_for_load,
                        )
                    })
                    .await
                    .map_err(|e| CoreError::RuntimeError(e.to_string()))?;

                    // Checkpoint WAL before rebuilding indexes to reclaim disk space.
                    // During bulk import, the WAL file can grow to 500MB+ with 2.5M+
                    // header inserts. On Android emulators with limited disk, this
                    // causes "database or disk is full" when end_bulk_import tries to
                    // CREATE INDEX. Checkpointing first merges WAL back into the main
                    // DB file and truncates the WAL.
                    if let Err(e) = header_store.checkpoint_wal() {
                        eprintln!("[ZipherX] WAL checkpoint before index rebuild failed (non-fatal): {e}");
                    }

                    // Always restore safe pragmas, even on error
                    header_store
                        .end_bulk_import()
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                    let loaded = load_result?;

                    eprintln!(
                        "[ZipherX] Loaded {} headers from boost file into HeaderStore",
                        loaded
                    );
                }
            }
        }
    }

    // Step 3: Get current header height (now reflects boost headers if loaded)
    let current_header_height = header_store
        .get_latest_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?
        .unwrap_or(0);

    // Step 3b: Reconnect peers if they dropped during boost load (can take 15+ min).
    // Check for zombie peers: appear connected but listeners are dead (reader consumed,
    // dispatcher inactive). These pass is_connected() but can't handle header requests.
    {
        let ready = peer_manager.get_ready_peers();
        let zombie_count = ready.iter().filter(|p| !p.is_listener_active()).count();
        let live_count = ready.len() - zombie_count;

        if live_count == 0 {
            eprintln!(
                "[ZipherX] No live peers after boost load ({} zombies, {} disconnected), reconnecting...",
                zombie_count,
                peer_manager.peers.len() - ready.len(),
            );
            // Disconnect zombies so connect() starts fresh
            peer_manager.disconnect_all().await;
            for attempt in 1..=3 {
                match peer_manager.connect().await {
                    Ok(()) => break,
                    Err(e) => {
                        eprintln!("[ZipherX] Reconnect attempt {}/3 failed: {e}", attempt);
                        if attempt < 3 {
                            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        }
                    }
                }
            }
            let pc = peer_manager.connected_count();
            eprintln!("[ZipherX] Reconnected: {} peers", pc);
            if pc == 0 {
                return Err(CoreError::Network(
                    zipherx_network::types::NetworkError::NoPeersAvailable,
                ));
            }
        }
    }

    // Step 4: P2P header sync — only fetches gap from boost height to chain tip.
    // With boost headers loaded, this is ~5K headers (seconds) instead of ~3M (50+ min).
    if let Some(ref p) = progress {
        p(SyncStatus::HeaderSync {
            current_height: current_header_height,
            target_height: 0,
        });
    }

    let header_sync = HeaderSync::new(Arc::new(HeaderStoreAdapter(header_store.clone())));
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::channel::<zipherx_network::header_sync::HeaderSyncProgress>(32);

    let progress_clone = progress.clone();
    let progress_forwarder = tokio::spawn(async move {
        while let Some(hp) = progress_rx.recv().await {
            if let Some(ref p) = progress_clone {
                p(SyncStatus::HeaderSync {
                    current_height: hp.current_height,
                    target_height: hp.target_height,
                });
            }
        }
    });

    let start_height = if current_header_height > 0 {
        current_header_height + 1
    } else {
        0
    };

    let headers_synced = header_sync
        .sync_headers(peer_manager, start_height, None, Some(progress_tx))
        .await
        .map_err(|e| CoreError::Network(e))?;

    let _ = progress_forwarder.await;

    // Step 5: Get final header height after sync
    let final_height = header_store
        .get_latest_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?
        .unwrap_or(0);

    tracing::info!(
        "Header sync complete: {} P2P headers synced, tip at {}",
        headers_synced,
        final_height
    );

    // ====================================================================
    // Step 5: Delta sync — download blocks with new Sapling outputs
    //
    // CHUNKED: Fetches and persists in batches of DELTA_CHUNK_SIZE so that
    // progress survives app restarts. Each chunk updates delta_end_height
    // in the manifest, so the next run resumes from the last saved chunk.
    // ====================================================================

    /// Maximum blocks per delta sync chunk. Persisted after each chunk.
    const DELTA_CHUNK_SIZE: usize = 5_000;

    // Determine delta sync range: from delta bundle end (or boost height, or Sapling activation) to tip
    let delta_end = delta_store
        .get_delta_end_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?;
    let delta_start = if delta_end > 0 {
        delta_end + 1
    } else {
        // If we have a boost manifest, start from boost chain_height + 1 instead of Sapling.
        // The boost file already covers all outputs up to chain_height.
        let boost_height = boost_download_result
            .as_ref()
            .and_then(|(_, manifest_path)| {
                boost_download::parse_manifest(manifest_path)
                    .ok()
                    .map(|m| m.chain_height)
            });
        if let Some(bh) = boost_height {
            eprintln!("[ZipherX] Delta sync: using boost chain_height {} as start (skipping Sapling→boost range)", bh);
            bh + 1
        } else {
            SAPLING_ACTIVATION_HEIGHT
        }
    };

    if delta_start <= final_height {
        if let Some(ref p) = progress {
            p(SyncStatus::DeltaSync {
                current_height: delta_start,
                target_height: final_height,
            });
        }

        // Find blocks where the Sapling root changed (blocks with new outputs)
        let blocks_with_outputs = header_store
            .get_blocks_with_new_outputs(delta_start, final_height)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let total_output_blocks = blocks_with_outputs.len();
        eprintln!(
            "[ZipherX] Delta sync: {} blocks with new outputs in range {}-{}",
            total_output_blocks, delta_start, final_height
        );

        if total_output_blocks > 0 {
            let pacing = PacingConfig::default();
            let num_chunks = (total_output_blocks + DELTA_CHUNK_SIZE - 1) / DELTA_CHUNK_SIZE;
            let mut grand_total_cmus: u64 = 0;
            let mut grand_total_nullifiers: u64 = 0;
            let mut chunks_completed: usize = 0;

            eprintln!(
                "[ZipherX] Processing in {} chunks of up to {} blocks each",
                num_chunks, DELTA_CHUNK_SIZE
            );

            for (chunk_idx, chunk) in blocks_with_outputs.chunks(DELTA_CHUNK_SIZE).enumerate() {
                let chunk_start_height = chunk.first().map(|&(h, _)| h).unwrap_or(0);
                let chunk_end_height = chunk.last().map(|&(h, _)| h).unwrap_or(0);

                if let Some(ref p) = progress {
                    p(SyncStatus::DeltaSync {
                        current_height: chunk_start_height,
                        target_height: final_height,
                    });
                }

                eprintln!(
                    "[ZipherX] Chunk {}/{}: fetching {} blocks (heights {}-{})",
                    chunk_idx + 1,
                    num_chunks,
                    chunk.len(),
                    chunk_start_height,
                    chunk_end_height,
                );

                // Fetch this chunk via P2P
                let fetch_result =
                    async_block_fetch::fetch_blocks_by_hashes(peer_manager, chunk, &pacing).await?;

                // Update peer count after block fetch
                if let Some(ref pc) = peer_count_ref {
                    pc.store(peer_manager.connected_count() as u32, Ordering::Relaxed);
                }

                let blocks_received = fetch_result.blocks.len();
                eprintln!(
                    "[ZipherX] Chunk {}/{}: received {}/{} blocks in {} rounds",
                    chunk_idx + 1,
                    num_chunks,
                    blocks_received,
                    chunk.len(),
                    fetch_result.rounds,
                );

                if blocks_received == 0 {
                    eprintln!(
                        "[ZipherX] Chunk {}: 0 blocks received, stopping delta sync",
                        chunk_idx + 1
                    );
                    break;
                }

                // Extract CMUs, spends, and roots from fetched blocks
                let mut delta_outputs: Vec<DeltaOutput> = Vec::new();
                let mut delta_nullifiers: Vec<DeltaNullifier> = Vec::new();
                let mut sapling_roots: Vec<(u64, Vec<u8>)> = Vec::new();
                let mut chunk_cmus: u64 = 0;
                let mut max_fetched_height: u64 = chunk_start_height;

                for block in &fetch_result.blocks {
                    if block.final_sapling_root != [0u8; 32] {
                        sapling_roots.push((block.height, block.final_sapling_root.to_vec()));
                    }

                    for (idx, output) in block.outputs.iter().enumerate() {
                        // Bounds check: block.height is u64 but DeltaOutput.height is u32.
                        // Heights beyond u32::MAX would silently truncate and corrupt data.
                        if block.height > u32::MAX as u64 {
                            return Err(CoreError::RuntimeError(format!(
                                "Block height {} exceeds u32::MAX, cannot store as DeltaOutput",
                                block.height,
                            )));
                        }
                        delta_outputs.push(DeltaOutput {
                            height: block.height as u32,
                            index: idx as u32,
                            cmu: output.cmu.to_vec(),
                            epk: output.epk.to_vec(),
                            ciphertext: output.ciphertext.clone(),
                            txid: output.txid.to_vec(),
                        });
                        chunk_cmus += 1;
                    }

                    for spend in &block.spends {
                        delta_nullifiers.push(DeltaNullifier {
                            height: block.height as u32,
                            txid: spend.txid.to_vec(),
                            nullifier: spend.nullifier.to_vec(),
                        });
                    }

                    if block.height > max_fetched_height {
                        max_fetched_height = block.height;
                    }
                }

                // Persist this chunk immediately — survives app restart
                // Use no_dedup: delta sync processes blocks sequentially, no revisits
                if !delta_outputs.is_empty() {
                    delta_store
                        .append_outputs_no_dedup(
                            &delta_outputs,
                            chunk_start_height,
                            max_fetched_height,
                            None,
                        )
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                }

                if !sapling_roots.is_empty() {
                    delta_store
                        .append_sapling_roots_batch(&sapling_roots)
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                    header_store
                        .store_sapling_roots(&sapling_roots)
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                }

                if !delta_nullifiers.is_empty() {
                    delta_store
                        .append_nullifiers(&delta_nullifiers)
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                }

                grand_total_cmus += chunk_cmus;
                grand_total_nullifiers += delta_nullifiers.len() as u64;
                chunks_completed += 1;

                eprintln!(
                    "[ZipherX] Chunk {}/{} persisted: {} CMUs, {} nullifiers (cumulative: {} CMUs)",
                    chunk_idx + 1,
                    num_chunks,
                    chunk_cmus,
                    delta_nullifiers.len(),
                    grand_total_cmus,
                );

                if let Some(ref p) = progress {
                    p(SyncStatus::DeltaSync {
                        current_height: max_fetched_height,
                        target_height: final_height,
                    });
                }
            }

            eprintln!(
                "[ZipherX] Delta sync complete: {} chunks, {} CMUs, {} nullifiers stored",
                chunks_completed, grand_total_cmus, grand_total_nullifiers
            );
        } else {
            eprintln!("[ZipherX] No blocks with new outputs in range — delta already caught up");
        }
    } else {
        eprintln!(
            "[ZipherX] Delta sync: already caught up (delta_end={}, tip={})",
            delta_end, final_height
        );
    }

    // ====================================================================
    // Step 6: Catch-up scan — trial-decrypt ALL unscanned delta outputs
    //
    // Reads from the delta store (persisted on disk) in chunks.
    // This handles both:
    //   a) Outputs downloaded in previous sessions but never scanned
    //   b) Outputs just downloaded in Step 5 above
    //
    // The tree_height in DB tracks how many CMUs have been appended.
    // If tree_height < delta output count, there are unscanned outputs.
    // ====================================================================

    /// Maximum delta outputs to process per scan chunk.
    /// Keep low to avoid OOM on memory-constrained Android devices.
    const SCAN_CHUNK_SIZE: usize = 10_000;

    let mut total_notes_found: u32 = 0;
    let mut total_spent_found: usize = 0;
    // FIX #1300: Track whether tree root mismatch was detected during block scan.
    // If true, reset last_scanned to boost height AFTER Step 9 so that the next
    // sync downloads ALL post-boost blocks (filling in missing CMUs).
    let mut tree_needs_reset = false;
    #[allow(unused_assignments)]
    let mut boost_height_for_reset: u64 = 0;
    #[allow(unused_assignments)]
    let mut boost_output_count_for_reset: u64 = 0;

    if !sk_bytes.is_empty() {
        // RC-8: Gate address logging behind compile-time #[cfg] to ensure spending key
        // addresses are NEVER compiled into release binaries (not just dead-code-eliminated).
        #[cfg(debug_assertions)]
        {
            if let Ok((addr_bytes, _)) = zipherx_crypto::keys::derive_address(sk_bytes, 0) {
                if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                    eprintln!("[ZipherX] Sync: spending key default address = {}", addr);
                }
            }
        }

        // One-time migration: clear notes with all-zero txids from before
        // synthetic txids were introduced. Resets tree_height to 0 so the
        // catch-up scan re-processes everything with proper txids + anchors.
        {
            let db_clone = db.clone();
            let migrated = tokio::task::spawn_blocking(move || db_clone.fix_zero_txid_notes())
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            if migrated > 0 {
                eprintln!(
                    "[ZipherX] Migration: cleared {} notes with zero txids, tree_height reset to 0",
                    migrated,
                );
            }
        }

        // ================================================================
        // Step 6a: Boost file scan — loads ALL outputs from pre-computed
        // boost file for correct tree positions → correct nullifiers →
        // correct spend detection → correct balance.
        //
        // Boost file was already downloaded + headers loaded in Step 2.
        // ================================================================
        let boost_result = boost_scan_if_needed(
            delta_store,
            db.clone(),
            sk_bytes,
            &progress,
            boost_download_result,
            boost_cache_dir.as_deref(),
        )
        .await?;

        if let Some((bch, boost_output_count)) = boost_result {
            // Update delta manifest end_height to boost chain height so that
            // subsequent syncs start delta_sync from bch+1 instead of Sapling.
            // Without this, get_delta_end_height() returns 0 and delta_sync
            // re-downloads all blocks from SAPLING_ACTIVATION_HEIGHT every time.
            if delta_store.get_delta_end_height().unwrap_or(0) < bch {
                eprintln!(
                    "[ZipherX] Updating delta manifest end_height to boost chain height {}",
                    bch
                );
                delta_store
                    .set_end_height(bch)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
            }

            // After boost scan, the tree_height is set to boost_output_count
            // (e.g. 1,047,160). The delta store only has ~3K outputs which
            // are a subset of the boost range, so the catch-up scan below
            // will correctly skip (tree_height > delta_output_count).
            eprintln!(
                "[ZipherX] Boost height={}, output_count={}, scanning post-boost delta outputs...",
                bch, boost_output_count
            );

            // Load the commitment tree from DB into global mutex.
            // On first run, boost_scan_if_needed() loads it from the boost file.
            // On subsequent runs, it returns early (tree_height >= output_count)
            // but the tree is only in DB, not in the global mutex. Post-boost
            // operations (delta CMU append, tree validation) need it in memory.
            {
                let db_c = db.clone();
                let (tree_state, db_tree_height) =
                    tokio::task::spawn_blocking(move || -> Result<_, CoreError> {
                        let state = db_c
                            .get_tree_state()
                            .map_err(|e| CoreError::Storage(e.to_string()))?;
                        let height = db_c
                            .get_tree_height()
                            .map_err(|e| CoreError::Storage(e.to_string()))?;
                        Ok((state, height))
                    })
                    .await
                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                if let Some(ref state_bytes) = tree_state {
                    // Try direct deserialization first; if it fails, try stripping
                    // the 8-byte position prefix (older DB entries from boost file
                    // may have included it before the fix).
                    match commitment_tree::deserialize(state_bytes) {
                        Ok(()) => {}
                        Err(_) if state_bytes.len() > 8 => {
                            eprintln!(
                                "[ZipherX] Tree deserialize failed ({} bytes), retrying without 8B prefix...",
                                state_bytes.len(),
                            );
                            commitment_tree::deserialize(&state_bytes[8..]).map_err(|e| {
                                CoreError::Crypto(format!("Tree deserialize (stripped): {e}"))
                            })?;
                            // Re-save without prefix so future loads work directly
                            let stripped_len = state_bytes.len() - 8;
                            let stripped = state_bytes[8..].to_vec();
                            let db_c2 = db.clone();
                            let h = db_tree_height;
                            tokio::task::spawn_blocking(move || {
                                db_c2.save_tree_state(&stripped, h)
                            })
                            .await
                            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                            .map_err(|e| CoreError::Storage(e.to_string()))?;
                            eprintln!(
                                "[ZipherX] Re-saved tree state without prefix ({} bytes)",
                                stripped_len
                            );
                        }
                        Err(e) => {
                            return Err(CoreError::Crypto(format!("Tree deserialize: {e}")));
                        }
                    }
                    // Set TREE_POSITION from DB so size() returns the correct value
                    commitment_tree::set_position(db_tree_height)
                        .map_err(|e| CoreError::Crypto(format!("Set position: {e}")))?;
                    eprintln!(
                        "[ZipherX] Commitment tree loaded from DB: {} CMUs",
                        db_tree_height,
                    );
                } else {
                    eprintln!("[ZipherX] No tree state in DB — tree operations will be skipped");
                }
            }

            // ====== DIAGNOSTIC: Validate tree root against HeaderStore ======
            // The boost file's tree root should match the blockchain's finalsaplingroot
            // at the same height. If not, the boost file is missing outputs.
            {
                match header_store.get_sapling_root(bch) {
                    Ok(Some(blockchain_root)) => {
                        // Parse manifest tree root (hex string → bytes)
                        if let Ok(manifest) = boost_download::parse_manifest(
                            &delta_store
                                .base_dir()
                                .parent()
                                .map(|p| p.join("BoostCache").join("zipherx_boost_manifest.json"))
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        ) {
                            let manifest_root_hex = &manifest.tree_root;
                            let blockchain_root_hex = hex::encode(&blockchain_root);
                            let blockchain_root_reversed_hex = hex::encode(
                                blockchain_root.iter().rev().copied().collect::<Vec<u8>>(),
                            );

                            let matches = *manifest_root_hex == blockchain_root_hex
                                || *manifest_root_hex == blockchain_root_reversed_hex;

                            if matches {
                                eprintln!(
                                    "[ZipherX] Boost tree root matches blockchain at height {}",
                                    bch,
                                );
                            } else {
                                // Boost file may not include every output up to its height.
                                // Delta CMUs compensate — only warn at debug level.
                                eprintln!(
                                    "[ZipherX] Boost tree root differs from blockchain at height {} (delta CMUs compensate)",
                                    bch,
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        eprintln!(
                            "[ZipherX] DIAG tree root: no finalsaplingroot in HeaderStore at height {} (headers not loaded yet?)",
                            bch,
                        );
                    }
                    Err(e) => {
                        eprintln!("[ZipherX] DIAG tree root: HeaderStore error: {}", e);
                    }
                }
            }

            // ============================================================
            // Load verified delta CMUs into tree (no P2P needed)
            // If the delta bundle was previously validated (boost + post-boost
            // tree root matched blockchain), load delta CMUs directly from
            // the delta store without re-downloading blocks.
            //
            // WITNESS CREATION: If any unspent notes lack witnesses, we use
            // individual append() instead of append_batch() so we can call
            // witness_current() when we encounter a note's CMU. The witness
            // is auto-updated as subsequent CMUs are appended, so the final
            // witness root = final tree root = correct anchor for spending.
            // ============================================================
            {
                let db_c = db.clone();
                let delta_verified =
                    tokio::task::spawn_blocking(move || db_c.get_delta_bundle_verified())
                        .await
                        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                if delta_verified {
                    // Get total delta CMU count without loading all data into memory.
                    // output_count() is file-size based (fast, no allocation).
                    let total_delta_cmus = delta_store
                        .output_count()
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                    let current_tree_size = commitment_tree::size()
                        .map_err(|e| CoreError::Crypto(format!("Tree size: {e}")))?;
                    let already_in_tree = if current_tree_size > boost_output_count {
                        (current_tree_size - boost_output_count) as usize
                    } else {
                        0
                    };

                    // Decision phase: determine skip count and whether to process.
                    // This avoids loading all CMUs into memory at once.
                    let (skip_count, should_process) = if total_delta_cmus == 0 {
                        (0usize, false)
                    } else if already_in_tree > 0 && already_in_tree < total_delta_cmus {
                        eprintln!(
                            "[ZipherX] Skipping {} delta CMUs already in tree (tree_size={}, boost={}), appending {} new",
                            already_in_tree, current_tree_size, boost_output_count,
                            total_delta_cmus - already_in_tree,
                        );
                        (already_in_tree, true)
                    } else if already_in_tree >= total_delta_cmus {
                        // All delta CMUs appear to be in tree already.
                        // Check if the tree root is actually valid.
                        let current_root = commitment_tree::root()
                            .map_err(|e| CoreError::Crypto(format!("Tree root: {e}")))?;
                        let root_valid = header_store
                            .contains_sapling_root(&current_root)
                            .unwrap_or(false);

                        if root_valid {
                            // FIX #1300: Tree root is valid, but check if any notes
                            // have stale/invalid witness anchors. If so, we must rebuild
                            // the tree to re-create witnesses with the correct anchor.
                            let db_c = db.clone();
                            let hs_ref = header_store.clone();
                            let has_invalid_anchors = tokio::task::spawn_blocking(move || -> bool {
                                let notes = match db_c.get_all_unspent_notes(0) {
                                    Ok(n) => n,
                                    Err(_) => return false,
                                };
                                for note in &notes {
                                    if note.witness.is_none()
                                        || note.witness.as_ref().map(|w| w.len()).unwrap_or(0) < 100
                                    {
                                        return true; // Missing witness
                                    }
                                    if let Some(ref anchor) = note.anchor {
                                        if anchor.len() == 32 {
                                            let valid = hs_ref
                                                .contains_sapling_root(anchor)
                                                .unwrap_or(false);
                                            if !valid {
                                                return true; // Invalid anchor
                                            }
                                        }
                                    } else {
                                        return true; // No anchor
                                    }
                                }
                                false
                            })
                            .await
                            .unwrap_or(false);

                            if has_invalid_anchors {
                                eprintln!(
                                    "[ZipherX] Tree root valid but notes have invalid/missing anchors — forcing tree rebuild for witness re-creation",
                                );
                                if let Some(ref p) = progress {
                                    p(SyncStatus::WitnessUpdate {
                                        notes_updated: 0,
                                        total_notes: 0,
                                    });
                                }
                                // Force rebuild from boost + all delta CMUs
                                let boost_cache = if let Some(ref bcd) = boost_cache_dir {
                                    bcd.clone()
                                } else {
                                    delta_store
                                        .base_dir()
                                        .parent()
                                        .map(|p| p.join("BoostCache"))
                                        .ok_or_else(|| {
                                            CoreError::Storage("Cannot determine BoostCache path".into())
                                        })?
                                };
                                let manifest_path = boost_cache.join("zipherx_boost_manifest.json");
                                let boost_file_path = boost_cache.join("zipherx_boost_v1.bin");

                                if manifest_path.exists() && boost_file_path.exists() {
                                    let manifest_str = manifest_path.to_string_lossy().to_string();
                                    let boost_str = boost_file_path.to_string_lossy().to_string();
                                    let manifest = boost_download::parse_manifest(&manifest_str)?;
                                    let tree_section =
                                        boost_download::get_section(&manifest, BOOST_SECTION_TREE)
                                            .ok_or_else(|| {
                                                CoreError::Storage(
                                                    "Boost manifest missing tree section".into(),
                                                )
                                            })?
                                            .clone();

                                    let tree_data = tokio::task::spawn_blocking(move || {
                                        boost_download::read_section(&boost_str, &tree_section)
                                    })
                                    .await
                                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                                    let tree_bytes = if tree_data.len() > 8 {
                                        &tree_data[8..]
                                    } else {
                                        &tree_data[..]
                                    };

                                    commitment_tree::deserialize(tree_bytes).map_err(|e| {
                                        CoreError::Crypto(format!("Boost tree deserialize: {e}"))
                                    })?;
                                    commitment_tree::set_position(boost_output_count)
                                        .map_err(|e| CoreError::Crypto(format!("Set position: {e}")))?;

                                    eprintln!(
                                        "[ZipherX] Boost tree reloaded for witness rebuild (size={}), appending ALL {} delta CMUs...",
                                        boost_output_count, total_delta_cmus,
                                    );
                                    (0, true) // Rebuild from scratch
                                } else {
                                    eprintln!(
                                        "[ZipherX] Boost file not found — cannot rebuild witnesses. Full Rescan required.",
                                    );
                                    (0, false)
                                }
                            } else {
                                eprintln!(
                                    "[ZipherX] All {} delta CMUs already in tree (tree_size={}, boost={}), root valid, all anchors OK — skipping",
                                    total_delta_cmus, current_tree_size, boost_output_count,
                                );
                                (0, false)
                            }
                        } else {
                            // Tree is CORRUPTED — rebuild from boost file + delta CMUs.
                            eprintln!(
                                "[ZipherX] *** Tree root INVALID (tree_size={}) — rebuilding from boost + delta ***",
                                current_tree_size,
                            );

                            let boost_cache = if let Some(ref bcd) = boost_cache_dir {
                                bcd.clone()
                            } else {
                                delta_store
                                    .base_dir()
                                    .parent()
                                    .map(|p| p.join("BoostCache"))
                                    .ok_or_else(|| {
                                        CoreError::Storage("Cannot determine BoostCache path".into())
                                    })?
                            };
                            let manifest_path = boost_cache.join("zipherx_boost_manifest.json");
                            let boost_file_path = boost_cache.join("zipherx_boost_v1.bin");

                            if manifest_path.exists() && boost_file_path.exists() {
                                let manifest_str = manifest_path.to_string_lossy().to_string();
                                let boost_str = boost_file_path.to_string_lossy().to_string();
                                let manifest = boost_download::parse_manifest(&manifest_str)?;
                                let tree_section =
                                    boost_download::get_section(&manifest, BOOST_SECTION_TREE)
                                        .ok_or_else(|| {
                                            CoreError::Storage(
                                                "Boost manifest missing tree section".into(),
                                            )
                                        })?
                                        .clone();

                                let tree_data = tokio::task::spawn_blocking(move || {
                                    boost_download::read_section(&boost_str, &tree_section)
                                })
                                .await
                                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                                let tree_bytes = if tree_data.len() > 8 {
                                    &tree_data[8..]
                                } else {
                                    &tree_data[..]
                                };

                                commitment_tree::deserialize(tree_bytes).map_err(|e| {
                                    CoreError::Crypto(format!("Boost tree deserialize: {e}"))
                                })?;
                                commitment_tree::set_position(boost_output_count)
                                    .map_err(|e| CoreError::Crypto(format!("Set position: {e}")))?;

                                eprintln!(
                                    "[ZipherX] Boost tree reloaded (size={}), appending ALL {} delta CMUs...",
                                    boost_output_count, total_delta_cmus,
                                );

                                // Append ALL delta CMUs fresh to the clean boost tree
                                (0, true)
                            } else {
                                eprintln!(
                                    "[ZipherX] *** Boost file not found for tree repair — tree remains corrupted ***",
                                );
                                (0, false)
                            }
                        }
                    } else {
                        (0, true)
                    };

                    // Execution phase: paginated CMU loading + tree append.
                    // Pages of 10K records (~7MB each) to stay within Android heap.
                    const DELTA_CMU_PAGE: usize = 10_000;

                    if should_process {
                        eprintln!(
                            "[ZipherX] Loading verified delta CMUs into tree (paginated, skip={}, total={})...",
                            skip_count, total_delta_cmus,
                        );

                        // Check for unspent notes needing witnesses (or with invalid anchors)
                        let db_c = db.clone();
                        let all_unspent_notes = tokio::task::spawn_blocking(
                            move || -> Result<Vec<zipherx_storage::types::Note>, CoreError> {
                                db_c.get_all_unspent_notes(0)
                                    .map_err(|e| CoreError::Storage(e.to_string()))
                            },
                        )
                        .await
                        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                        let notes_needing_witnesses: Vec<(i64, Vec<u8>, u64)> = all_unspent_notes
                            .into_iter()
                            .filter(|n| {
                                if n.witness.is_none()
                                    || n.witness.as_ref().map(|w| w.len()).unwrap_or(0) < 100
                                {
                                    return true;
                                }
                                if let Some(ref anchor) = n.anchor {
                                    if anchor.len() == 32 {
                                        let valid = header_store
                                            .contains_sapling_root(anchor)
                                            .unwrap_or(false);
                                        if !valid {
                                            return true;
                                        }
                                    }
                                }
                                false
                            })
                            .filter(|n| n.cmu.len() == 32)
                            .map(|n| (n.id, n.cmu, n.value))
                            .collect();

                        let need_witnesses = !notes_needing_witnesses.is_empty();

                        let mut note_cmu_set: HashSet<[u8; 32]> = HashSet::new();
                        let mut note_cmu_to_id: HashMap<[u8; 32], (i64, u64)> = HashMap::new();
                        if need_witnesses {
                            eprintln!(
                                "[ZipherX] {} notes need witnesses — using individual append with witness creation",
                                notes_needing_witnesses.len(),
                            );
                            for (note_id, cmu, value) in &notes_needing_witnesses {
                                let mut cmu_arr = [0u8; 32];
                                cmu_arr.copy_from_slice(cmu);
                                note_cmu_set.insert(cmu_arr);
                                note_cmu_to_id.insert(cmu_arr, (*note_id, *value));
                            }
                            commitment_tree::clear_witnesses()
                                .map_err(|e| CoreError::Crypto(format!("Clear witnesses: {e}")))?;
                        }

                        let mut witness_map: Vec<([u8; 32], u64)> = Vec::new();
                        let mut global_cmu_idx: usize = 0;
                        let mut page_offset: usize = 0;

                        loop {
                            let page = delta_store
                                .load_cmus_for_range_paged(
                                    bch + 1,
                                    final_height,
                                    page_offset,
                                    DELTA_CMU_PAGE,
                                )
                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                            if page.is_empty() {
                                break;
                            }
                            let page_len = page.len();

                            if need_witnesses {
                                // Individual append with witness creation
                                for (_h, cmu) in &page {
                                    if global_cmu_idx < skip_count {
                                        global_cmu_idx += 1;
                                        continue;
                                    }
                                    global_cmu_idx += 1;
                                    if cmu.len() != 32 {
                                        continue;
                                    }
                                    let mut cmu_arr = [0u8; 32];
                                    cmu_arr.copy_from_slice(cmu);

                                    commitment_tree::append(&cmu_arr).map_err(|e| {
                                        CoreError::Crypto(format!("Delta CMU append: {e}"))
                                    })?;

                                    if note_cmu_set.contains(&cmu_arr) {
                                        let witness_idx = commitment_tree::witness_current()
                                            .map_err(|e| {
                                                CoreError::Crypto(format!("Witness create: {e}"))
                                            })?;
                                        witness_map.push((cmu_arr, witness_idx));

                                        if let Some(&(note_id, value)) =
                                            note_cmu_to_id.get(&cmu_arr)
                                        {
                                            eprintln!(
                                                "[ZipherX]   Witness created for note id={} (value={} zatoshis)",
                                                note_id, value,
                                            );
                                        }
                                    }
                                }
                            } else {
                                // Fast path: batch append (skip first skip_count CMUs)
                                let cmu_bytes: Vec<u8> = page
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, _)| {
                                        let idx = global_cmu_idx;
                                        global_cmu_idx += 1;
                                        idx >= skip_count
                                    })
                                    .flat_map(|(_, (_, cmu))| cmu.iter().copied())
                                    .collect();
                                if !cmu_bytes.is_empty() {
                                    commitment_tree::append_batch(&cmu_bytes).map_err(|e| {
                                        CoreError::Crypto(format!("Delta CMU append: {e}"))
                                    })?;
                                }
                            }

                            page_offset += page_len;
                            if page_len < DELTA_CMU_PAGE {
                                break;
                            }
                        }

                        // Update tree height in DB
                        let new_size = commitment_tree::size()
                            .map_err(|e| CoreError::Crypto(format!("Tree size: {e}")))?;
                        let tree_data = commitment_tree::serialize()
                            .map_err(|e| CoreError::Crypto(format!("Tree serialize: {e}")))?;
                        let db_c = db.clone();
                        tokio::task::spawn_blocking(move || {
                            db_c.save_tree_state(&tree_data, new_size)
                        })
                        .await
                        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                        eprintln!(
                            "[ZipherX] Delta CMUs loaded: tree_size={}",
                            new_size,
                        );

                        // FIX #1300: Validate tree root BEFORE storing witnesses.
                        // If the combined (boost + delta) tree root doesn't match any
                        // blockchain sapling root, the delta store is incomplete. Storing
                        // witnesses with an invalid anchor would block ALL future sends
                        // (anchor validation in async_send.rs is a hard error).
                        let post_load_root = commitment_tree::root()
                            .map_err(|e| CoreError::Crypto(format!("Tree root: {e}")))?;
                        let root_valid = header_store
                            .contains_sapling_root(&post_load_root)
                            .unwrap_or(false);

                        if root_valid {
                            eprintln!(
                                "[ZipherX] Tree root validated OK — storing witnesses",
                            );

                            // Serialize and store witnesses in DB (only if root is valid)
                            if !witness_map.is_empty() {
                                let mut witness_updates: Vec<(i64, Vec<u8>, [u8; 32])> = Vec::new();
                                for &(cmu_arr, witness_idx) in &witness_map {
                                    let wb = commitment_tree::get_witness_serialized(witness_idx)
                                        .map_err(|e| {
                                            CoreError::Crypto(format!("Witness serialize: {e}"))
                                        })?;
                                    let anchor = commitment_tree::get_witness_root(witness_idx)
                                        .map_err(|e| CoreError::Crypto(format!("Witness root: {e}")))?;

                                    if let Some(&(note_id, _value)) = note_cmu_to_id.get(&cmu_arr) {
                                        match commitment_tree::verify_witness_consistency(&wb) {
                                            Ok(()) => {
                                                eprintln!(
                                                    "[ZipherX]   Witness for note id={}: {} bytes, anchor={}... OK",
                                                    note_id, wb.len(), hex::encode(&anchor[..8]),
                                                );
                                                witness_updates.push((note_id, wb, anchor));
                                            }
                                            Err(e) => {
                                                eprintln!(
                                                    "[ZipherX]   Witness for note id={}: FAILED consistency: {}",
                                                    note_id, e,
                                                );
                                            }
                                        }
                                    }
                                }

                                if !witness_updates.is_empty() {
                                    let update_count = witness_updates.len();
                                    let db_c = db.clone();
                                    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                                        for (note_id, witness, anchor) in &witness_updates {
                                            db_c.update_note_witness(*note_id, witness)
                                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                                            db_c.update_note_anchor(*note_id, anchor)
                                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                                        }
                                        Ok(())
                                    })
                                    .await
                                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                                    eprintln!(
                                        "[ZipherX] Stored {} witnesses during delta CMU loading",
                                        update_count,
                                    );
                                }
                            } else if need_witnesses {
                                eprintln!(
                                    "[ZipherX] Note CMUs not found in delta range — witnesses will be created by rebuild_witnesses_if_needed()",
                                );
                            }
                        } else {
                            // Tree root INVALID — delta store is incomplete.
                            // DO NOT store witnesses (they'd have invalid anchors).
                            // Reset delta_verified so full block scan fills in missing CMUs.
                            eprintln!(
                                "[ZipherX] *** Tree root NOT found in blockchain — delta store incomplete ***",
                            );
                            eprintln!(
                                "[ZipherX] *** NOT storing witnesses (invalid anchors). Resetting delta_verified for full block scan ***",
                            );

                            // Clear any existing witnesses for these notes (safety: prevent
                            // stale anchors from previous syncs from persisting)
                            if !witness_map.is_empty() {
                                let note_ids: Vec<i64> = witness_map.iter()
                                    .filter_map(|(cmu_arr, _)| note_cmu_to_id.get(cmu_arr).map(|(id, _)| *id))
                                    .collect();
                                if !note_ids.is_empty() {
                                    let cleared_count = note_ids.len();
                                    let db_c = db.clone();
                                    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                                        for note_id in &note_ids {
                                            db_c.clear_witness_for_note(*note_id)
                                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                                        }
                                        Ok(())
                                    })
                                    .await
                                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                                    eprintln!(
                                        "[ZipherX] Cleared {} note witness(es) with invalid anchors",
                                        cleared_count,
                                    );
                                }
                            }

                            let db_c = db.clone();
                            tokio::task::spawn_blocking(move || {
                                db_c.set_delta_bundle_verified(false)
                            })
                            .await
                            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                            .map_err(|e| CoreError::Storage(e.to_string()))?;
                        }
                    }
                }
            }

            // ============================================================
            // Post-boost delta scan: find notes at heights > boost height
            // The boost file covers up to bch (e.g. 3,011,251). Any delta
            // outputs above that height are new and need trial decryption.
            // ============================================================
            let post_boost_result = post_boost_delta_scan(
                delta_store,
                db.clone(),
                sk_bytes,
                bch,
                boost_output_count,
                final_height,
                &progress,
            )
            .await;

            match post_boost_result {
                Ok((notes, spent)) => {
                    total_notes_found += notes;
                    total_spent_found += spent;
                }
                Err(e) => {
                    eprintln!("[ZipherX] Post-boost delta scan error (non-fatal): {e}");
                }
            }

            // ============================================================
            // ============================================================
            // Post-boost full block scan: download ALL blocks in post-boost
            // range to find BOTH nullifiers (spends) AND shielded outputs
            // (received notes). The delta store only has ~24 post-boost
            // outputs — many blocks with our notes were never fetched.
            // ============================================================
            let full_scan_result = post_boost_full_block_scan(
                peer_manager,
                header_store,
                delta_store,
                db.clone(),
                sk_bytes,
                bch,
                boost_output_count,
                final_height,
                &progress,
            )
            .await;

            boost_height_for_reset = bch;
            boost_output_count_for_reset = boost_output_count;
            match full_scan_result {
                Ok((marked, notes, tree_valid)) => {
                    if marked > 0 {
                        total_spent_found += marked;
                    }
                    if notes > 0 {
                        total_notes_found += notes;
                    }
                    if marked > 0 || notes > 0 {
                        eprintln!(
                            "[ZipherX] Post-boost full block scan: {} notes found, {} notes marked spent",
                            notes, marked,
                        );
                    }
                    if !tree_valid {
                        tree_needs_reset = true;
                    }
                }
                Err(e) => {
                    eprintln!("[ZipherX] Post-boost full block scan error (non-fatal): {e}");
                }
            }
        }

        let total_delta_outputs = delta_store
            .output_count()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            as u64;

        let db_clone = db.clone();
        let tree_height = tokio::task::spawn_blocking(move || db_clone.get_tree_height())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if tree_height < total_delta_outputs {
            let unscanned = total_delta_outputs - tree_height;
            eprintln!(
                "[ZipherX] Catch-up scan: {} unscanned outputs (tree_height={}, delta_outputs={})",
                unscanned, tree_height, total_delta_outputs
            );

            // Load or initialize the commitment tree
            let db_clone = db.clone();
            let tree_state = tokio::task::spawn_blocking(move || db_clone.get_tree_state())
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;

            if let Some(ref state_bytes) = tree_state {
                // Handle older DB entries that may still have the 8-byte position prefix
                match commitment_tree::deserialize(state_bytes) {
                    Ok(()) => {}
                    Err(_) if state_bytes.len() > 8 => {
                        eprintln!(
                            "[ZipherX] Catch-up tree deserialize failed, retrying without 8B prefix...",
                        );
                        commitment_tree::deserialize(&state_bytes[8..]).map_err(|e| {
                            CoreError::Crypto(format!("Tree deserialize (stripped): {e}"))
                        })?;
                    }
                    Err(e) => {
                        return Err(CoreError::Crypto(format!("Tree deserialize: {e}")));
                    }
                }
                // Set TREE_POSITION from DB tree_height
                commitment_tree::set_position(tree_height)
                    .map_err(|e| CoreError::Crypto(format!("Set position: {e}")))?;
            } else {
                commitment_tree::init()
                    .map_err(|e| CoreError::Crypto(format!("Tree init: {e}")))?;
            }

            let mut scan_offset = tree_height as usize;
            let mut current_tree_position = tree_height;
            let num_scan_chunks = ((unscanned as usize) + SCAN_CHUNK_SIZE - 1) / SCAN_CHUNK_SIZE;

            eprintln!(
                "[ZipherX] Scanning in {} chunks of up to {} outputs",
                num_scan_chunks, SCAN_CHUNK_SIZE,
            );

            let mut chunk_num: usize = 0;

            while scan_offset < total_delta_outputs as usize {
                chunk_num += 1;

                // Load a page of delta outputs
                let chunk_outputs = delta_store
                    .load_outputs_paged(scan_offset, SCAN_CHUNK_SIZE)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;

                if chunk_outputs.is_empty() {
                    break;
                }

                let chunk_size = chunk_outputs.len();
                let first_height = chunk_outputs.first().map(|o| o.height as u64).unwrap_or(0);
                let last_height = chunk_outputs.last().map(|o| o.height as u64).unwrap_or(0);

                eprintln!(
                    "[ZipherX] Scan chunk {}/{}: {} outputs (heights {}-{})",
                    chunk_num, num_scan_chunks, chunk_size, first_height, last_height,
                );

                if let Some(ref p) = progress {
                    p(SyncStatus::BlockScan {
                        current_height: first_height,
                        target_height: final_height,
                        notes_found: total_notes_found,
                    });
                }

                // Load only this chunk's nullifiers for spend detection
                let chunk_nullifiers = delta_store
                    .load_nullifiers_for_height_range(first_height, last_height)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
                let mut nullifier_map: std::collections::BTreeMap<u64, Vec<(Vec<u8>, Vec<u8>)>> =
                    std::collections::BTreeMap::new();
                for nf in &chunk_nullifiers {
                    nullifier_map
                        .entry(nf.height as u64)
                        .or_default()
                        .push((nf.txid.clone(), nf.nullifier.clone()));
                }

                // Reconstruct CompactBlocks from delta output records.
                // Group outputs by height, include nullifiers for spend detection.
                let blocks = reconstruct_compact_blocks(&chunk_outputs, &nullifier_map);

                // Drop source data eagerly — blocks already cloned all ciphertexts
                drop(chunk_outputs);
                drop(chunk_nullifiers);
                drop(nullifier_map);

                // Trial decryption
                let scan_result =
                    scanner::scan_blocks(&blocks, sk_bytes, current_tree_position, None)?;

                let chunk_notes = scan_result.new_notes.len();
                let chunk_spent = scan_result.spent_nullifiers.len();

                // Build set of positions where we need witnesses
                let our_positions: HashSet<u64> = scan_result
                    .new_notes
                    .iter()
                    .map(|n| n.tree_position)
                    .collect();

                // Extract CMUs for tree appending, then drop blocks to free ciphertext memory
                let block_cmus: Vec<[u8; 32]> = blocks
                    .iter()
                    .flat_map(|b| b.outputs.iter().map(|o| o.cmu))
                    .collect();
                drop(blocks);

                // Clear witnesses from previous chunk
                commitment_tree::clear_witnesses()
                    .map_err(|e| CoreError::Crypto(format!("Clear witnesses: {e}")))?;

                // Append CMUs to tree and create witnesses for discovered notes
                let mut position = current_tree_position;
                let mut witness_map: Vec<(u64, u64)> = Vec::new();

                for cmu in &block_cmus {
                    commitment_tree::append(cmu)
                        .map_err(|e| CoreError::Crypto(format!("Tree append: {e}")))?;

                    if our_positions.contains(&position) {
                        let witness_idx = commitment_tree::witness_current()
                            .map_err(|e| CoreError::Crypto(format!("Witness create: {e}")))?;
                        witness_map.push((position, witness_idx));
                    }

                    position += 1;
                }
                drop(block_cmus);

                // Build witness data map
                let mut witness_data: std::collections::HashMap<u64, Vec<u8>> =
                    std::collections::HashMap::new();
                for &(tree_pos, witness_idx) in &witness_map {
                    match commitment_tree::get_witness_serialized(witness_idx) {
                        Ok(bytes) => match commitment_tree::get_witness_root(witness_idx) {
                            Ok(_anchor) => {
                                witness_data.insert(tree_pos, bytes);
                            }
                            Err(e) => {
                                eprintln!(
                                    "[ZipherX] Witness at position {} invalid root: {e}",
                                    tree_pos
                                );
                            }
                        },
                        Err(e) => {
                            eprintln!("[ZipherX] Failed to serialize witness at {}: {e}", tree_pos);
                        }
                    }
                }

                // Persist notes to DB
                if !scan_result.new_notes.is_empty() {
                    for n in &scan_result.new_notes {
                        eprintln!(
                            "[ZipherX]   Note: height={}, value={} zatoshis, position={}",
                            n.height, n.note.value, n.tree_position,
                        );
                    }

                    let db_clone = db.clone();
                    let notes = scan_result.new_notes.clone();
                    let wd = witness_data.clone();
                    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                        for note in &notes {
                            let wb = wd.get(&note.tree_position);

                            let memo_str = if note.note.memo.iter().all(|&b| b == 0) {
                                None
                            } else {
                                let trimmed: Vec<u8> = note
                                    .note
                                    .memo
                                    .iter()
                                    .copied()
                                    .take_while(|&b| b != 0)
                                    .collect();
                                String::from_utf8(trimmed).ok()
                            };

                            // Txid: display order = reversed wire order.
                            // Synthetic txid from height+index (set in
                            // reconstruct_compact_blocks) ensures uniqueness.
                            let mut txid_display = note.txid;
                            txid_display.reverse();
                            let txid_hex = hex::encode(txid_display);

                            let note_id = db_clone
                                .insert_note(
                                    0,
                                    note.height,
                                    &note.cmu,
                                    note.note.value,
                                    Some(&note.nullifier),
                                    Some(&note.note.rcm),
                                    Some(&note.epk),
                                    Some(&note.ciphertext),
                                    memo_str.as_deref(),
                                    Some(&note.note.diversifier),
                                    wb.map(|w| w.as_slice()),
                                    Some(&txid_hex),
                                    Some(note.tree_position),
                                )
                                .map_err(|e| CoreError::Storage(e.to_string()))?;

                            // Set anchor using note_id directly (avoids nullifier lookup)
                            if note_id > 0 {
                                if let Some(witness_bytes) = wb {
                                    if let Ok(anchor) =
                                        zipherx_crypto::witness::witness_root(witness_bytes)
                                    {
                                        let _ = db_clone.update_note_anchor(note_id, &anchor);
                                    }
                                }
                            }

                            eprintln!(
                                "[ZipherX]   DB insert: note_id={}, txid={}, value={}",
                                note_id,
                                &txid_hex[..16],
                                note.note.value,
                            );

                            // Insert received transaction record
                            db_clone
                                .insert_transaction(
                                    &txid_hex,
                                    note.height,
                                    None,
                                    TxType::Received,
                                    note.note.value,
                                    0,
                                    None,
                                    memo_str.as_deref(),
                                    TxStatus::Confirmed,
                                )
                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
                }

                // Mark spent notes
                if !scan_result.spent_nullifiers.is_empty() {
                    let db_clone = db.clone();
                    let nullifiers = scan_result.spent_nullifiers.clone();
                    let spent_count =
                        tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
                            let mut count = 0;
                            for (nullifier, txid_bytes) in &nullifiers {
                                let mut txid_display = *txid_bytes;
                                txid_display.reverse();
                                let txid_hex = hex::encode(txid_display);
                                match db_clone.mark_note_spent(nullifier, &txid_hex, 0) {
                                    Ok(true) => count += 1,
                                    Ok(false) => {}
                                    Err(e) => {
                                        eprintln!("[ZipherX] Error marking note spent: {e}");
                                    }
                                }
                            }
                            Ok(count)
                        })
                        .await
                        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                    if spent_count > 0 {
                        total_spent_found += spent_count;
                    }
                }

                // Serialize and save tree state after each chunk (crash-safe)
                let tree_state_bytes = commitment_tree::serialize()
                    .map_err(|e| CoreError::Crypto(format!("Tree serialize: {e}")))?;
                {
                    let db_clone = db.clone();
                    let new_tree_height = position;
                    tokio::task::spawn_blocking(move || {
                        db_clone.save_tree_state(&tree_state_bytes, new_tree_height)
                    })
                    .await
                    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
                }

                total_notes_found += chunk_notes as u32;
                current_tree_position = position;
                scan_offset += chunk_size;

                eprintln!(
                    "[ZipherX] Scan chunk {}/{}: {} notes, {} spent, tree_height={}",
                    chunk_num, num_scan_chunks, chunk_notes, chunk_spent, current_tree_position,
                );
            }

            eprintln!(
                "[ZipherX] Catch-up scan complete: {} notes found, {} spent, tree_height={}",
                total_notes_found, total_spent_found, current_tree_position,
            );
        } else {
            eprintln!(
                "[ZipherX] Block scan: tree up to date (tree_height={}, delta_outputs={})",
                tree_height, total_delta_outputs,
            );
        }
    } else {
        eprintln!("[ZipherX] Block scan skipped: no spending key provided");
    }

    // ====================================================================
    // Step 8b: Targeted block scan for pending transactions
    //
    // After a send, notes are marked spent but the change note only exists
    // in the mined block. The delta store only captures blocks where the
    // sapling root changed — the change note's block may be missing.
    // This step downloads ALL recent blocks from peers to find change notes.
    // Only runs when there are pending (unconfirmed) sent transactions.
    // ====================================================================
    if !sk_bytes.is_empty() {
        // Reconnect peers if needed — incremental syncs may have disconnected
        // after header/delta sync, but we need peers to fetch full blocks.
        if peer_manager.connected_count() == 0 {
            eprintln!("[ZipherX] Step 8b: no connected peers, reconnecting...");
            peer_manager.disconnect_all().await;
            if let Err(e) = peer_manager.connect().await {
                eprintln!("[ZipherX] Step 8b: peer reconnect failed (non-fatal): {e}");
            }
        }

        if peer_manager.connected_count() > 0 {
            let scan_result = scan_blocks_for_pending_txs(
                peer_manager,
                header_store,
                delta_store,
                db.clone(),
                sk_bytes,
                final_height,
                &progress,
            )
            .await;

            match scan_result {
                Ok((notes, spent)) => {
                    if notes > 0 || spent > 0 {
                        total_notes_found += notes;
                        total_spent_found += spent;
                        eprintln!(
                            "[ZipherX] Pending TX scan: {} notes found, {} notes marked spent",
                            notes, spent,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("[ZipherX] Pending TX scan error (non-fatal): {e}");
                }
            }
        } else {
            eprintln!("[ZipherX] Step 8b: skipped — could not reconnect peers");
        }
    }

    // ====================================================================
    // Step 9: Update confirmations + last scanned height
    // ====================================================================

    eprintln!(
        "[ZipherX] Step 9: updating last_scanned_height={}, confirmations...",
        final_height
    );

    if final_height > 0 {
        let db_clone = db.clone();
        let h = final_height;
        tokio::task::spawn_blocking(move || {
            db_clone.update_last_scanned_height(h)?;
            db_clone.update_all_confirmations(h)?;
            Ok::<(), zipherx_storage::types::StorageError>(())
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        // Backfill missing timestamps from header store
        // Transactions inserted by boost_scan/delta_scan may have NULL timestamps.
        // Look up block times from the header store and update them.
        let db_clone = db.clone();
        let heights =
            tokio::task::spawn_blocking(move || db_clone.get_heights_needing_timestamps())
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;

        if !heights.is_empty() {
            let mut updates: Vec<(u64, u64)> = Vec::new();
            for h in &heights {
                if let Ok(Some(ts)) = header_store.get_block_time(*h) {
                    if ts > 0 {
                        updates.push((*h, ts));
                    }
                }
            }
            if !updates.is_empty() {
                let db_clone = db.clone();
                let backfilled = tokio::task::spawn_blocking(move || {
                    let mut total = 0usize;
                    for (h, ts) in &updates {
                        total += db_clone.set_timestamps_for_height(*h, *ts).unwrap_or(0);
                    }
                    Ok::<usize, zipherx_storage::types::StorageError>(total)
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;
                eprintln!(
                    "[ZipherX] Step 9: backfilled timestamps for {} transactions ({} heights)",
                    backfilled,
                    heights.len(),
                );
            }
        }
    }

    // FIX #1300: After Step 9, if tree root mismatch was detected, reset
    // last_scanned to boost height AND reload the tree from boost file.
    // This forces the next sync to download ALL post-boost blocks,
    // filling in the missing CMUs that caused the tree root mismatch.
    if tree_needs_reset && boost_height_for_reset > 0 {
        eprintln!(
            "[ZipherX] FIX #1300: Resetting last_scanned to boost height {} for full post-boost rescan",
            boost_height_for_reset,
        );
        let db_c = db.clone();
        let bch = boost_height_for_reset;
        let _ = tokio::task::spawn_blocking(move || {
            db_c.update_last_scanned_height(bch)
        })
        .await;

        // Reload tree from boost file to discard partial/wrong CMUs
        let boost_cache = delta_store
            .base_dir()
            .parent()
            .map(|p| p.join("BoostCache"));
        if let Some(bc) = boost_cache {
            let boost_file = bc.join("zipherx_boost_v1.bin");
            let manifest_file = bc.join("zipherx_boost_manifest.json");
            if boost_file.exists() && manifest_file.exists() {
                let mf_str = manifest_file.to_string_lossy().to_string();
                let bf_str = boost_file.to_string_lossy().to_string();
                if let Ok(mf) = boost_download::parse_manifest(&mf_str) {
                    if let Some(ts) = boost_download::get_section(&mf, BOOST_SECTION_TREE) {
                        let ts_clone = ts.clone();
                        if let Ok(td) = tokio::task::spawn_blocking(move || {
                            boost_download::read_section(&bf_str, &ts_clone)
                        }).await.unwrap_or(Err(CoreError::Storage("spawn failed".into()))) {
                            let tree_bytes = if td.len() > 8 { &td[8..] } else { &td[..] };
                            if commitment_tree::deserialize(tree_bytes).is_ok() {
                                let _ = commitment_tree::set_position(boost_output_count_for_reset);
                                if let Ok(tree_data) = commitment_tree::serialize() {
                                    let db_c2 = db.clone();
                                    let boc = boost_output_count_for_reset;
                                    let _ = tokio::task::spawn_blocking(move || {
                                        db_c2.save_tree_state(&tree_data, boc)
                                    }).await;
                                }
                                eprintln!(
                                    "[ZipherX] Tree reset to boost state (size={}). Next sync will rescan all post-boost blocks.",
                                    boost_output_count_for_reset,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Verify: check DB state before signalling completion
    {
        let db_clone = db.clone();
        #[allow(unused_variables)]
        let (
            note_count,
            spent_count,
            total_value,
            tx_count,
            tx_types,
            note_txids,
            unspent_detail,
            spent_detail,
        ) = tokio::task::spawn_blocking(move || {
            let unspent = db_clone.get_all_unspent_notes(0).unwrap_or_default();
            let all = db_clone.get_all_notes(0).unwrap_or_default();
            let spent_notes: Vec<_> = all.iter().filter(|n| n.is_spent).collect();
            let spent = spent_notes.len();
            let total: u64 = unspent.iter().map(|n| n.value).sum();
            let txids: Vec<String> = unspent
                .iter()
                .map(|n| {
                    n.received_txid
                        .as_deref()
                        .unwrap_or("NULL")
                        .chars()
                        .take(16)
                        .collect()
                })
                .collect();
            let tx_count = db_clone.get_transaction_count().unwrap_or(0);
            let tx_types = db_clone.get_transaction_type_counts().unwrap_or((0, 0, 0));

            // Detailed note values for balance audit
            let mut unspent_vals: Vec<(u64, u64, String)> = unspent
                .iter()
                .map(|n| {
                    (
                        n.value,
                        n.height as u64,
                        n.received_txid
                            .as_deref()
                            .unwrap_or("?")
                            .chars()
                            .take(16)
                            .collect(),
                    )
                })
                .collect();
            unspent_vals.sort_by_key(|(v, _, _)| *v);

            let mut spent_vals: Vec<(u64, u64, String, String)> = spent_notes
                .iter()
                .map(|n| {
                    (
                        n.value,
                        n.height as u64,
                        n.received_txid
                            .as_deref()
                            .unwrap_or("?")
                            .chars()
                            .take(16)
                            .collect(),
                        n.spent_in_tx
                            .as_deref()
                            .unwrap_or("?")
                            .chars()
                            .take(16)
                            .collect(),
                    )
                })
                .collect();
            spent_vals.sort_by_key(|(v, _, _, _)| *v);

            (
                unspent.len(),
                spent,
                total,
                tx_count,
                tx_types,
                txids,
                unspent_vals,
                spent_vals,
            )
        })
        .await
        .unwrap_or((0, 0, 0, 0, (0, 0, 0), vec![], vec![], vec![]));
        eprintln!(
            "[ZipherX] DB check: {} unspent + {} spent = {} notes",
            note_count,
            spent_count,
            note_count + spent_count,
        );
        eprintln!(
            "[ZipherX] DB check: {} tx_history rows ({} sent, {} received, {} change)",
            tx_count, tx_types.0, tx_types.1, tx_types.2,
        );

        // RC-8: Gate balance/note audit behind compile-time #[cfg] to ensure
        // wallet balances and note values are NEVER logged in release builds.
        #[cfg(debug_assertions)]
        {
            eprintln!(
                "[ZipherX] DB check: total unspent value {} zatoshis",
                total_value,
            );
            eprintln!(
                "[ZipherX] DB AUDIT: {} unspent notes:",
                unspent_detail.len()
            );
            for (val, h, txid) in &unspent_detail {
                eprintln!(
                    "[ZipherX]   UNSPENT: value={:>12} ({:.8} ZCL), height={}, txid={}...",
                    val,
                    *val as f64 / 100_000_000.0,
                    h,
                    txid
                );
            }
            eprintln!("[ZipherX] DB AUDIT: {} spent notes:", spent_detail.len());
            for (val, h, rtxid, stxid) in &spent_detail {
                eprintln!(
                    "[ZipherX]   SPENT:   value={:>12}, height={}, recv_tx={}..., spent_tx={}...",
                    val, h, rtxid, stxid,
                );
            }

            eprintln!(
                "[ZipherX] DB AUDIT: total_unspent={} zatoshis ({:.8} ZCL)",
                total_value,
                total_value as f64 / 100_000_000.0,
            );

            eprintln!("[ZipherX] DB check: unspent note txids: {:?}", note_txids);
        }
    }

    // ============================================================
    // Witness rebuild: create witnesses for unspent notes that
    // don't have them yet. Required for spending — notes without
    // witnesses show in total balance but spendable=0.
    // FIX #1300: Skip when tree_needs_reset — delta store is known
    // incomplete, witnesses would get invalid anchors. Next sync
    // downloads all post-boost blocks and rebuilds correctly.
    // ============================================================
    if tree_needs_reset {
        eprintln!(
            "[ZipherX] Skipping witness rebuild — tree reset pending, next sync will rebuild correctly",
        );
    }
    if !tree_needs_reset {
    match rebuild_witnesses_if_needed(delta_store, db.clone(), header_store, boost_cache_dir.as_deref()).await {
        Ok(rebuilt) => {
            if rebuilt > 0 {
                eprintln!(
                    "[ZipherX] Witness rebuild: {} notes now have witnesses (spendable!)",
                    rebuilt,
                );
            }
        }
        Err(e) => {
            eprintln!("[ZipherX] Witness rebuild error (non-fatal): {e}");
        }
    }
    } // end if !tree_needs_reset

    // ============================================================
    // Witness-based nullifier recompute: for ALL unspent notes
    // with valid witnesses, extract the note's tree position from
    // the witness and recompute the nullifier. This is the
    // DEFINITIVE fix for wrong nullifiers caused by incomplete
    // delta store position counting.
    //
    // The witness position is guaranteed correct when the anchor
    // is validated against the blockchain's finalsaplingroot.
    // Using this position produces the same nullifier that the
    // TX builder (transaction.rs) computes during spending, and
    // that the blockchain reveals when the note is spent.
    // ============================================================
    if !sk_bytes.is_empty() {
        let db_c = db.clone();
        let all_unspent = tokio::task::spawn_blocking(move || db_c.get_all_unspent_notes(0))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        // Filter to notes with valid witnesses (length >= 100)
        let notes_with_witnesses: Vec<&zipherx_storage::types::Note> = all_unspent
            .iter()
            .filter(|n| n.witness.as_ref().map(|w| w.len()).unwrap_or(0) >= 100)
            .collect();

        if !notes_with_witnesses.is_empty() {
            // RC-2: Spending key clone for nullifier recompute — zeroized after use
            // inside the spawn_blocking closure via explicit .zeroize() call.
            let sk = sk_bytes.to_vec();
            let db_c = db.clone();

            // Collect data needed for nullifier recomputation
            let notes_data: Vec<(
                Vec<u8>,
                Option<Vec<u8>>,
                u64,
                Option<Vec<u8>>,
                Vec<u8>,
                Option<u64>,
            )> = notes_with_witnesses
                .iter()
                .map(|n| {
                    (
                        n.cmu.clone(),
                        n.diversifier.clone(),
                        n.value,
                        n.rcm.clone(),
                        n.witness.clone().unwrap_or_default(),
                        n.position,
                    )
                })
                .collect();

            let witness_fixed = tokio::task::spawn_blocking(move || -> Result<u32, CoreError> {
                let mut sk = sk; // RC-2: take ownership for zeroization
                let mut fixed = 0u32;
                for (cmu, diversifier, value, rcm, witness_bytes, old_position) in &notes_data {
                    // Extract position from witness
                    let witness_pos = match commitment_tree::witnessed_position_from_bytes(witness_bytes) {
                        Ok(pos) => pos,
                        Err(e) => {
                            eprintln!(
                                "[ZipherX]   Witness position error for cmu={}...: {}",
                                hex::encode(&cmu[..8.min(cmu.len())]), e,
                            );
                            continue;
                        }
                    };

                    // Skip if position hasn't changed
                    if *old_position == Some(witness_pos) {
                        continue;
                    }

                    let div_vec = match diversifier {
                        Some(d) if d.len() == 11 => d,
                        _ => continue,
                    };
                    let rcm_vec = match rcm {
                        Some(r) if r.len() == 32 => r,
                        _ => continue,
                    };

                    let mut div_arr = [0u8; 11];
                    div_arr.copy_from_slice(div_vec);
                    let mut rcm_arr = [0u8; 32];
                    rcm_arr.copy_from_slice(rcm_vec);

                    match zipherx_crypto::notes::compute_nullifier(
                        &sk, &div_arr, *value, &rcm_arr, witness_pos, false,
                    ) {
                        Ok(nf) => {
                            eprintln!(
                                "[ZipherX]   Witness recompute: cmu={}... value={} old_pos={:?} witness_pos={} nf={}...",
                                hex::encode(&cmu[..8.min(cmu.len())]), value, old_position, witness_pos,
                                hex::encode(&nf[..8]),
                            );
                            match db_c.update_note_nullifier_by_cmu(cmu, &nf, witness_pos) {
                                Ok(true) => fixed += 1,
                                Ok(false) => {}
                                Err(e) => eprintln!("[ZipherX]   Witness nf update error: {}", e),
                            }
                        }
                        Err(e) => eprintln!("[ZipherX]   Witness nf compute error: {}", e),
                    }
                }
                sk.zeroize(); // RC-2: Explicit zeroization of spending key material
                Ok(fixed)
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

            if witness_fixed > 0 {
                eprintln!(
                    "[ZipherX] Witness-based nullifier recompute: FIXED {} notes with correct witness positions",
                    witness_fixed,
                );

                // RECHECK: cross-ref ALL delta nullifiers against the
                // now-correct DB nullifiers to catch spends that were
                // previously missed due to wrong nullifier positions.
                let all_delta_nfs = delta_store
                    .load_nullifiers_for_height_range(0, u64::MAX)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;

                let delta_nf_count = all_delta_nfs.len();
                if delta_nf_count > 0 {
                    let db_c = db.clone();
                    let recheck_marked = tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
                        let mut marked = 0usize;
                        let mut spends_by_tx: HashMap<String, (u64, u64)> = HashMap::new();

                        for nf in &all_delta_nfs {
                            let mut txid_display = nf.txid.clone();
                            txid_display.reverse();
                            let txid_hex = hex::encode(&txid_display);

                            let spent_value = db_c
                                .cross_ref_nullifier_spend(&nf.nullifier, &txid_hex, nf.height as u64)
                                .map_err(|e| CoreError::Storage(e.to_string()))?;

                            if let Some(value) = spent_value {
                                marked += 1;
                                eprintln!(
                                    "[ZipherX]   WITNESS RECHECK SPEND: block {} spent note worth {} zatoshis (tx={}...)",
                                    nf.height, value, &txid_hex[..16.min(txid_hex.len())],
                                );
                                let entry = spends_by_tx.entry(txid_hex).or_insert((0, nf.height as u64));
                                entry.0 += value;
                            }
                        }

                        // Record send transactions for newly-discovered spends
                        for (txid_hex, (total_value, height)) in &spends_by_tx {
                            let _ = db_c.insert_transaction(
                                txid_hex,
                                *height,
                                None,
                                TxType::Sent,
                                *total_value,
                                0, // fee unknown for discovered spends
                                None,
                                None,
                                TxStatus::Confirmed,
                            );
                        }

                        Ok(marked)
                    })
                    .await
                    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                    if recheck_marked > 0 {
                        eprintln!(
                            "[ZipherX] Witness RECHECK: marked {} notes as spent after nullifier fix",
                            recheck_marked,
                        );
                    } else {
                        eprintln!(
                            "[ZipherX] Witness RECHECK: no additional spends found ({} nullifiers checked)",
                            delta_nf_count,
                        );
                    }
                }
            } else {
                eprintln!(
                    "[ZipherX] Witness-based nullifier recompute: all {} notes already have correct positions",
                    notes_with_witnesses.len(),
                );
            }
        }
    }

    // Post-sync spendable check: if we have unspent notes but none are spendable,
    // witnesses are broken. Clear the verified flag so next sync rebuilds properly.
    {
        let db_c = db.clone();
        let (total_bal, spendable_bal) = tokio::task::spawn_blocking(move || {
            let total = db_c.get_total_unspent_balance(0).unwrap_or(0);
            let spendable = db_c.get_balance(0).unwrap_or(0);
            (total, spendable)
        })
        .await
        // RC-6: Log the error instead of silently defaulting to (0, 0).
        .unwrap_or_else(|e| {
            eprintln!("[ZipherX] Warning: post-sync balance check failed: {:?}", e);
            (0, 0)
        });

        if total_bal > 0 && spendable_bal == 0 {
            eprintln!(
                "[ZipherX] WARNING: total_balance={} but spendable=0 — witnesses missing, clearing verified flag for next sync",
                total_bal,
            );
            let db_c = db.clone();
            let _ =
                tokio::task::spawn_blocking(move || db_c.set_delta_bundle_verified(false)).await;
        } else {
            eprintln!(
                "[ZipherX] Post-sync balance: total={}, spendable={} — OK",
                total_bal, spendable_bal,
            );
        }
    }

    // FIX #1300: Auto-recover notes from expired/failed sends.
    // After sync to tip, check for notes marked spent for TXs that were never mined.
    // If 20+ blocks have passed (Zclassic TX expiry), restore notes automatically.
    {
        let db_recovery = db.clone();
        let recovery_height = final_height;
        let recovery_result = tokio::task::spawn_blocking(move || {
            db_recovery.auto_recover_expired_sends(recovery_height, 20)
        })
        .await;

        match recovery_result {
            Ok(Ok((count, value))) if count > 0 => {
                eprintln!(
                    "[ZipherX] AUTO-RECOVERY: Restored {} note(s) worth {} zatoshis from expired sends",
                    count, value
                );
                // Re-read balance after recovery
                let db_bal = db.clone();
                if let Ok(bal) = tokio::task::spawn_blocking(move || db_bal.get_balance(0)).await {
                    if let Ok(spendable) = bal {
                        eprintln!(
                            "[ZipherX] Post-recovery spendable balance: {}",
                            spendable
                        );
                    }
                }
            }
            Ok(Err(e)) => {
                eprintln!("[ZipherX] Auto-recovery check failed: {e}");
            }
            Err(e) => {
                eprintln!("[ZipherX] Auto-recovery task failed: {e}");
            }
            _ => {} // No notes to recover
        }
    }

    eprintln!("[ZipherX] Sync complete, sending Complete event to UI");

    // Ensure block listeners are active for post-sync mempool detection.
    // Listeners may have died during long sync operations (boost scan, block download).
    if !peer_manager.has_active_block_listeners() {
        eprintln!("[ZipherX] Restarting block listeners for mempool detection...");
        peer_manager.start_all_block_listeners().await;
    }

    if let Some(ref p) = progress {
        p(SyncStatus::Complete {
            height: final_height,
        });
    }

    tracing::info!(
        "Sync complete: tip={}, notes_found={}, spent_found={}",
        final_height,
        total_notes_found,
        total_spent_found,
    );

    Ok(final_height)
}

// ============================================================================
// Witness Rebuild
// ============================================================================

/// Create witnesses for unspent notes that don't have them.
///
/// Witnesses are required for spending — notes without witnesses appear in
/// total balance but have spendable=0. This function:
/// 1. Checks for unspent notes without witnesses
/// 2. Loads the boost tree from the boost file on disk
/// 3. Re-appends delta CMUs one-by-one, creating witnesses at note positions
/// 4. Verifies the final tree root matches the expected root
/// 5. Stores witnesses in the DB
///
/// Returns the number of notes that received witnesses.
pub async fn rebuild_witnesses_if_needed(
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    _header_store: &SqliteHeaderStore,
    boost_cache_override: Option<&std::path::Path>,
) -> Result<usize, CoreError> {
    // Step 1: Check for unspent notes without witnesses
    let db_c = db.clone();
    let all_unspent = tokio::task::spawn_blocking(move || db_c.get_all_unspent_notes(0))
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    // Check if any notes are missing witnesses
    let missing_witnesses = all_unspent
        .iter()
        .any(|n| n.witness.is_none() || n.witness.as_ref().map(|w| w.len()).unwrap_or(0) < 100);

    // Check if all existing anchors are the SAME — Sapling requires all spends
    // in a TX to share the same anchor. Different anchors = different tree states.
    let mut anchors_differ = false;
    let mut has_invalid_anchor = false;
    let mut first_anchor: Option<&[u8]> = None;
    for n in &all_unspent {
        if let Some(ref anchor) = n.anchor {
            if anchor.len() == 32 {
                // FIX #1300: Check if anchor is actually valid on blockchain
                if !has_invalid_anchor {
                    let valid = _header_store
                        .contains_sapling_root(anchor)
                        .unwrap_or(false);
                    if !valid {
                        eprintln!(
                            "[ZipherX] Witness rebuild: note id={} has INVALID anchor {} — forcing rebuild",
                            n.id, hex::encode(anchor),
                        );
                        has_invalid_anchor = true;
                    }
                }
                if let Some(first) = first_anchor {
                    if first != anchor.as_slice() {
                        anchors_differ = true;
                    }
                } else {
                    first_anchor = Some(anchor);
                }
            }
        }
    }

    // Rebuild ALL witnesses when any are missing, anchors differ, or anchors are invalid
    let notes_needing_witnesses: Vec<&zipherx_storage::types::Note> =
        if missing_witnesses || anchors_differ || has_invalid_anchor {
            all_unspent.iter().collect()
        } else {
            Vec::new()
        };

    if notes_needing_witnesses.is_empty() {
        return Ok(0);
    }

    eprintln!(
        "[ZipherX] Witness rebuild: {} unspent notes need witnesses",
        notes_needing_witnesses.len(),
    );

    // Collect note CMUs for matching during tree replay
    let mut note_cmu_set: HashSet<[u8; 32]> = HashSet::new();
    let mut note_cmu_to_id: HashMap<[u8; 32], (i64, u64)> = HashMap::new(); // cmu → (note_id, value)
    for note in &notes_needing_witnesses {
        if note.cmu.len() == 32 {
            let mut cmu_arr = [0u8; 32];
            cmu_arr.copy_from_slice(&note.cmu);
            note_cmu_set.insert(cmu_arr);
            note_cmu_to_id.insert(cmu_arr, (note.id, note.value));
        }
    }

    if note_cmu_set.is_empty() {
        eprintln!("[ZipherX] Witness rebuild: no valid CMUs found, skipping");
        return Ok(0);
    }

    // Step 2: Find the boost file and manifest
    // Use explicit override path first (Android: external storage differs from delta store).
    let boost_cache = if let Some(override_dir) = boost_cache_override {
        override_dir.to_path_buf()
    } else {
        delta_store
            .base_dir()
            .parent()
            .map(|p| p.join("BoostCache"))
            .ok_or_else(|| CoreError::Storage("Cannot determine BoostCache path".into()))?
    };

    let manifest_path = boost_cache.join("zipherx_boost_manifest.json");
    let boost_file_path = boost_cache.join("zipherx_boost_v1.bin");

    if !manifest_path.exists() || !boost_file_path.exists() {
        eprintln!("[ZipherX] Witness rebuild: boost file not found, skipping");
        return Ok(0);
    }

    let manifest_str = manifest_path.to_string_lossy().to_string();
    let boost_str = boost_file_path.to_string_lossy().to_string();

    // Parse manifest to get sections
    let manifest = boost_download::parse_manifest(&manifest_str)?;
    let tree_section = boost_download::get_section(&manifest, BOOST_SECTION_TREE)
        .ok_or_else(|| CoreError::Storage("Boost manifest missing tree section".into()))?
        .clone();
    let outputs_section = boost_download::get_section(&manifest, BOOST_SECTION_OUTPUTS)
        .ok_or_else(|| CoreError::Storage("Boost manifest missing outputs section".into()))?
        .clone();

    let boost_output_count = manifest.output_count;

    // Check if any notes are in the boost range (position < boost_output_count).
    // These notes' CMUs are inside the boost tree, NOT in the delta store,
    // so we must replay boost CMUs from the outputs section to create witnesses.
    let has_boost_range_notes = notes_needing_witnesses
        .iter()
        .any(|n| n.position.map(|p| p < boost_output_count).unwrap_or(true));

    // Save current tree state for restoration after witness creation.
    let db_c = db.clone();
    let (saved_tree_state, saved_tree_height) =
        tokio::task::spawn_blocking(move || -> Result<_, CoreError> {
            let state = db_c
                .get_tree_state()
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            let height = db_c
                .get_tree_height()
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            Ok((state, height))
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    let mut witness_map: Vec<([u8; 32], u64)> = Vec::new();

    if has_boost_range_notes {
        // ============================================================
        // BOOST-RANGE WITNESS CREATION
        // Notes discovered during boost_scan have CMUs inside the boost
        // tree. We must replay ALL boost CMUs from the outputs section
        // (streaming 32-byte CMUs from 684-byte records) to create
        // witnesses at the correct tree positions.
        // ============================================================
        eprintln!(
            "[ZipherX] Witness rebuild: notes in boost range — streaming {} boost CMUs from outputs section",
            boost_output_count,
        );

        // Start from empty tree (we need witnesses at intermediate positions)
        commitment_tree::init().map_err(|e| CoreError::Crypto(format!("Init tree: {e}")))?;

        // Stream CMUs from boost outputs section (684 bytes per record, CMU at offset 8..40)
        let boost_path_for_stream = boost_str.clone();
        let outputs_offset = outputs_section.offset;
        let outputs_count = outputs_section.count as usize;
        // Read boost CMUs in pages to avoid loading 1.7GB into memory
        const BOOST_CMU_PAGE_SIZE: usize = 50_000;
        let mut boost_cmus_appended: usize = 0;

        let boost_cmu_pages =
            tokio::task::spawn_blocking(move || -> Result<Vec<Vec<[u8; 32]>>, CoreError> {
                use std::io::{BufReader, Read, Seek, SeekFrom};
                let file = std::fs::File::open(&boost_path_for_stream)
                    .map_err(|e| CoreError::Storage(format!("Open boost file: {e}")))?;
                let mut reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer

                // Seek to start of outputs section, then read sequentially
                reader
                    .seek(SeekFrom::Start(outputs_offset))
                    .map_err(|e| CoreError::Storage(format!("Seek to outputs: {e}")))?;

                let mut pages: Vec<Vec<[u8; 32]>> = Vec::new();
                let mut page: Vec<[u8; 32]> = Vec::with_capacity(BOOST_CMU_PAGE_SIZE);
                let mut record_buf = [0u8; 684];

                for i in 0..outputs_count {
                    reader
                        .read_exact(&mut record_buf)
                        .map_err(|e| CoreError::Storage(format!("Read output {i}: {e}")))?;

                    let mut cmu = [0u8; 32];
                    cmu.copy_from_slice(&record_buf[8..40]);
                    page.push(cmu);

                    if page.len() >= BOOST_CMU_PAGE_SIZE {
                        pages.push(std::mem::replace(
                            &mut page,
                            Vec::with_capacity(BOOST_CMU_PAGE_SIZE),
                        ));
                    }
                }
                if !page.is_empty() {
                    pages.push(page);
                }
                Ok(pages)
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

        for page in &boost_cmu_pages {
            for cmu_arr in page {
                commitment_tree::append(cmu_arr)
                    .map_err(|e| CoreError::Crypto(format!("Boost CMU append: {e}")))?;
                boost_cmus_appended += 1;

                if note_cmu_set.contains(cmu_arr) {
                    let witness_idx = commitment_tree::witness_current()
                        .map_err(|e| CoreError::Crypto(format!("Witness create: {e}")))?;
                    witness_map.push((*cmu_arr, witness_idx));

                    if let Some(&(note_id, _value)) = note_cmu_to_id.get(cmu_arr) {
                        eprintln!(
                            "[ZipherX]   Created witness for note id={} at boost position {} (CMU {}...)",
                            note_id, boost_cmus_appended - 1, hex::encode(&cmu_arr[..8]),
                        );
                    }
                }
            }
        }

        eprintln!(
            "[ZipherX] Witness rebuild: appended {} boost CMUs, created {} witnesses",
            boost_cmus_appended,
            witness_map.len(),
        );

        // Now append delta CMUs to update the witnesses to the chain tip
        let delta_cmus = delta_store
            .load_cmus_for_range(manifest.chain_height + 1, u64::MAX)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if !delta_cmus.is_empty() {
            eprintln!(
                "[ZipherX] Witness rebuild: appending {} delta CMUs to update witnesses...",
                delta_cmus.len(),
            );

            let mut seen_cmus: HashSet<(u32, [u8; 32])> = HashSet::new();
            for (h, cmu) in &delta_cmus {
                if cmu.len() != 32 {
                    continue;
                }
                let mut cmu_arr = [0u8; 32];
                cmu_arr.copy_from_slice(cmu);
                if !seen_cmus.insert((*h, cmu_arr)) {
                    continue;
                }

                commitment_tree::append(&cmu_arr)
                    .map_err(|e| CoreError::Crypto(format!("Delta CMU append: {e}")))?;

                // Also check for post-boost notes that need witnesses
                if note_cmu_set.contains(&cmu_arr)
                    && !witness_map.iter().any(|(c, _)| c == &cmu_arr)
                {
                    let witness_idx = commitment_tree::witness_current()
                        .map_err(|e| CoreError::Crypto(format!("Witness create: {e}")))?;
                    witness_map.push((cmu_arr, witness_idx));

                    if let Some(&(note_id, _value)) = note_cmu_to_id.get(&cmu_arr) {
                        eprintln!(
                            "[ZipherX]   Created witness for note id={} in delta range (CMU {}...)",
                            note_id,
                            hex::encode(&cmu_arr[..8]),
                        );
                    }
                }
            }
        }
    } else {
        // ============================================================
        // DELTA-ONLY WITNESS CREATION (original path)
        // All notes are post-boost — load boost tree and append delta CMUs.
        // ============================================================
        let boost_str_for_tree = boost_str.clone();
        let tree_data = tokio::task::spawn_blocking(move || {
            boost_download::read_section(&boost_str_for_tree, &tree_section)
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

        let tree_bytes = if tree_data.len() > 8 {
            &tree_data[8..]
        } else {
            &tree_data[..]
        };

        eprintln!(
            "[ZipherX] Witness rebuild: loading boost tree ({} bytes), output_count={}",
            tree_bytes.len(),
            boost_output_count,
        );

        commitment_tree::deserialize(tree_bytes)
            .map_err(|e| CoreError::Crypto(format!("Boost tree deserialize: {e}")))?;
        commitment_tree::set_position(boost_output_count)
            .map_err(|e| CoreError::Crypto(format!("Set position: {e}")))?;
        commitment_tree::clear_witnesses()
            .map_err(|e| CoreError::Crypto(format!("Clear witnesses: {e}")))?;

        let delta_cmus = delta_store
            .load_cmus_for_range(manifest.chain_height + 1, u64::MAX)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        eprintln!(
            "[ZipherX] Witness rebuild: appending {} delta CMUs with witness creation...",
            delta_cmus.len(),
        );

        let mut seen_cmus: HashSet<(u32, [u8; 32])> = HashSet::new();
        let mut dedup_skipped: usize = 0;
        for (h, cmu) in &delta_cmus {
            if cmu.len() != 32 {
                continue;
            }
            let mut cmu_arr = [0u8; 32];
            cmu_arr.copy_from_slice(cmu);
            if !seen_cmus.insert((*h, cmu_arr)) {
                dedup_skipped += 1;
                continue;
            }

            commitment_tree::append(&cmu_arr)
                .map_err(|e| CoreError::Crypto(format!("Witness rebuild append: {e}")))?;

            if note_cmu_set.contains(&cmu_arr) {
                let witness_idx = commitment_tree::witness_current()
                    .map_err(|e| CoreError::Crypto(format!("Witness create: {e}")))?;
                witness_map.push((cmu_arr, witness_idx));

                if let Some(&(note_id, _value)) = note_cmu_to_id.get(&cmu_arr) {
                    eprintln!(
                        "[ZipherX]   Created witness for note id={} at CMU {}...",
                        note_id,
                        hex::encode(&cmu_arr[..8]),
                    );
                }
            }
        }

        if dedup_skipped > 0 {
            eprintln!(
                "[ZipherX] Witness rebuild: skipped {} duplicate CMUs",
                dedup_skipped,
            );
        }
    }

    // Check if we found all note CMUs
    if witness_map.len() < note_cmu_set.len() {
        eprintln!(
            "[ZipherX] Witness rebuild: only found {}/{} note CMUs",
            witness_map.len(),
            note_cmu_set.len(),
        );
    }

    if witness_map.is_empty() {
        eprintln!("[ZipherX] Witness rebuild: no witnesses created (note CMUs not found)");
        // Restore original tree
        if let Some(ref state_bytes) = saved_tree_state {
            let _ = commitment_tree::deserialize(state_bytes);
            let _ = commitment_tree::set_position(saved_tree_height);
        }
        return Ok(0);
    }

    // DIAGNOSTIC: Log tree root after rebuild and compare with blockchain
    {
        let rebuilt_root = commitment_tree::root()
            .map_err(|e| CoreError::Crypto(format!("Tree root: {e}")))?;
        let rebuilt_size = commitment_tree::size()
            .map_err(|e| CoreError::Crypto(format!("Tree size: {e}")))?;
        let root_hex = hex::encode(&rebuilt_root);
        let root_rev_hex = hex::encode(rebuilt_root.iter().rev().copied().collect::<Vec<u8>>());
        eprintln!(
            "[ZipherX] DIAG witness rebuild: tree_size={}, root={}",
            rebuilt_size, root_hex,
        );
        eprintln!(
            "[ZipherX] DIAG witness rebuild: root_reversed={}",
            root_rev_hex,
        );

        // Check what the header store thinks the root should be
        let root_found = _header_store
            .contains_sapling_root(&rebuilt_root)
            .unwrap_or(false);
        eprintln!(
            "[ZipherX] DIAG witness rebuild: root in header store? {}",
            root_found,
        );

        // Try to get the actual blockchain root at chain tip for comparison
        if let Ok(tip) = _header_store.get_latest_height() {
            if let Some(tip_height) = tip {
                if let Ok(Some(blockchain_root)) = _header_store.get_sapling_root(tip_height) {
                    let blockchain_hex = hex::encode(&blockchain_root);
                    let blockchain_rev_hex = hex::encode(blockchain_root.iter().rev().copied().collect::<Vec<u8>>());
                    eprintln!(
                        "[ZipherX] DIAG: blockchain root at tip {}: {}",
                        tip_height, blockchain_hex,
                    );
                    eprintln!(
                        "[ZipherX] DIAG: blockchain root reversed: {}",
                        blockchain_rev_hex,
                    );
                    if root_hex == blockchain_hex || root_hex == blockchain_rev_hex
                        || root_rev_hex == blockchain_hex || root_rev_hex == blockchain_rev_hex
                    {
                        eprintln!("[ZipherX] DIAG: *** ROOT MATCHES (byte order issue in contains_sapling_root) ***");
                    } else {
                        eprintln!(
                            "[ZipherX] DIAG: *** ROOT MISMATCH — delta CMUs incomplete or boost file corrupted ***",
                        );
                    }
                } else {
                    eprintln!("[ZipherX] DIAG: no sapling root in header store at tip {}", tip_height);
                }
            }
        }
    }

    // Step 7: Serialize witnesses and validate internal consistency (FIX #827).
    // Also validate anchor against header store — invalid anchors block sends.
    let mut witness_updates: Vec<(i64, Vec<u8>, [u8; 32])> = Vec::new();
    for &(cmu_arr, witness_idx) in &witness_map {
        let wb = commitment_tree::get_witness_serialized(witness_idx)
            .map_err(|e| CoreError::Crypto(format!("Witness serialize: {e}")))?;
        let anchor = commitment_tree::get_witness_root(witness_idx)
            .map_err(|e| CoreError::Crypto(format!("Witness root: {e}")))?;

        if let Some(&(note_id, _value)) = note_cmu_to_id.get(&cmu_arr) {
            // FIX #827: Verify internal consistency (deserializable + has path)
            match commitment_tree::verify_witness_consistency(&wb) {
                Ok(()) => {
                    eprintln!(
                        "[ZipherX]   Witness for note id={}: {} bytes, anchor={}... OK",
                        note_id,
                        wb.len(),
                        hex::encode(&anchor[..8]),
                    );
                    witness_updates.push((note_id, wb, anchor));
                }
                Err(e) => {
                    eprintln!(
                        "[ZipherX]   Witness for note id={}: FAILED consistency check: {}",
                        note_id, e,
                    );
                }
            }
        }
    }

    // Step 8: ALWAYS restore the original tree from DB.
    // The rebuilt tree is temporary — we only needed it for witness creation.
    // The original DB tree is the authoritative state with all CMUs.
    if let Some(ref state_bytes) = saved_tree_state {
        commitment_tree::deserialize(state_bytes)
            .map_err(|e| CoreError::Crypto(format!("Restore tree: {e}")))?;
        commitment_tree::set_position(saved_tree_height)
            .map_err(|e| CoreError::Crypto(format!("Restore position: {e}")))?;
        eprintln!(
            "[ZipherX] Witness rebuild: restored original tree (size={})",
            saved_tree_height,
        );
    }

    // FIX #1300: Validate all witness anchors against header store before storing.
    // If the rebuilt tree root doesn't match any blockchain root, storing these
    // witnesses would permanently block sends (anchor validation in async_send.rs).
    if !witness_updates.is_empty() {
        let sample_anchor = &witness_updates[0].2;
        let anchor_valid = _header_store
            .contains_sapling_root(sample_anchor)
            .unwrap_or(false);
        if !anchor_valid {
            eprintln!(
                "[ZipherX] Witness rebuild: anchor {} NOT found in header store — NOT storing witnesses (tree data corrupted)",
                hex::encode(sample_anchor),
            );
            eprintln!(
                "[ZipherX] Witness rebuild: Full Rescan required to fix commitment tree",
            );
            return Ok(0);
        }
    }

    if witness_updates.is_empty() {
        eprintln!("[ZipherX] Witness rebuild: no valid witnesses created");
        return Ok(0);
    }

    // Step 9: Store validated witnesses in DB (anchors confirmed valid)
    let update_count = witness_updates.len();
    let db_c = db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
        for (note_id, witness, anchor) in &witness_updates {
            db_c.update_note_witness(*note_id, witness)
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            db_c.update_note_anchor(*note_id, anchor)
                .map_err(|e| CoreError::Storage(e.to_string()))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    eprintln!(
        "[ZipherX] Witness rebuild complete: {} witnesses stored (anchors validated against HeaderStore)",
        update_count,
    );

    Ok(update_count)
}

// ============================================================================
// HeaderStore Adapter
// ============================================================================

/// Adapter to use SqliteHeaderStore through the HeaderStore trait.
///
/// HeaderSync needs Arc<dyn HeaderStore>. This wrapper holds an Arc<SqliteHeaderStore>
/// so it can be shared safely across threads without raw pointers.
struct HeaderStoreAdapter(Arc<SqliteHeaderStore>);

impl HeaderStore for HeaderStoreAdapter {
    fn get_latest_height(&self) -> Result<Option<u64>, zipherx_network::types::NetworkError> {
        self.0.get_latest_height()
    }

    fn get_header(
        &self,
        height: u64,
    ) -> Result<
        Option<zipherx_network::header_sync::StoredHeader>,
        zipherx_network::types::NetworkError,
    > {
        self.0.get_header(height)
    }

    fn get_header_hash(
        &self,
        height: u64,
    ) -> Result<Option<[u8; 32]>, zipherx_network::types::NetworkError> {
        self.0.get_header_hash(height)
    }

    fn store_headers(
        &self,
        headers: Vec<(u64, zipherx_network::header_sync::StoredHeader)>,
    ) -> Result<(), zipherx_network::types::NetworkError> {
        self.0.store_headers(headers)
    }

    fn count_headers_in_range(
        &self,
        from: u64,
        to: u64,
    ) -> Result<usize, zipherx_network::types::NetworkError> {
        self.0.count_headers_in_range(from, to)
    }

    fn truncate_above(&self, height: u64) -> Result<(), zipherx_network::types::NetworkError> {
        self.0.truncate_above(height).map(|_| ())
    }
}

// ============================================================================
// Boost File Scan
// ============================================================================

/// Scan the boost file if it exists and hasn't been loaded yet.
///
/// The boost file contains ALL shielded outputs and spends up to a known height.
/// This gives correct tree positions → correct nullifiers → correct spend detection.
///
/// Returns `Some((boost_chain_height, boost_output_count))` if the boost scan was performed, `None` if skipped.
/// `pre_downloaded` is the result from a parallel download task, if one was started.
async fn boost_scan_if_needed(
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    progress: &Option<SyncProgressFn>,
    pre_downloaded: Option<(String, String)>,
    boost_cache_override: Option<&std::path::Path>,
) -> Result<Option<(u64, u64)>, CoreError> {
    // Use pre-downloaded paths if available, otherwise find on disk
    let (boost_file_str, manifest_file_str) = if let Some(paths) = pre_downloaded {
        eprintln!("[ZipherX] Boost scan: using pre-downloaded boost file");
        paths
    } else {
        // Use explicit override path first, then fall back to delta store derivation.
        // On Android, the boost file is in external storage (override) while delta
        // store is in internal storage — paths differ.
        let boost_cache_dir = if let Some(override_dir) = boost_cache_override {
            override_dir.to_path_buf()
        } else {
            match delta_store.base_dir().parent() {
                Some(parent) => parent.join("BoostCache"),
                None => return Ok(None),
            }
        };

        // Try to use existing files without downloading
        let boost_file = boost_cache_dir.join("zipherx_boost_v1.bin");
        let manifest_file = boost_cache_dir.join("zipherx_boost_manifest.json");

        eprintln!(
            "[ZipherX] Boost scan: checking {}",
            boost_file.display(),
        );

        if boost_file.exists() && manifest_file.exists() {
            let size = std::fs::metadata(&boost_file).map(|m| m.len()).unwrap_or(0);
            if size > 100_000_000 {
                (
                    boost_file.to_string_lossy().into_owned(),
                    manifest_file.to_string_lossy().into_owned(),
                )
            } else {
                eprintln!(
                    "[ZipherX] Boost scan: file too small ({} bytes), skipping",
                    size
                );
                return Ok(None);
            }
        } else {
            eprintln!(
                "[ZipherX] Boost scan: no boost file at {}, skipping",
                boost_cache_dir.display(),
            );
            return Ok(None);
        }
    };

    // Parse manifest
    let manifest = boost_download::parse_manifest(&manifest_file_str)?;

    // RC-22: Validate manifest chain_height >= Sapling activation height.
    // A manifest with a chain_height below Sapling activation is invalid and
    // would cause incorrect tree positions and nullifier computation.
    if manifest.chain_height < SAPLING_ACTIVATION_HEIGHT {
        return Err(CoreError::Storage(format!(
            "Boost manifest chain_height {} is below Sapling activation height {}",
            manifest.chain_height, SAPLING_ACTIVATION_HEIGHT,
        )));
    }

    eprintln!(
        "[ZipherX] Boost scan: manifest loaded — {} outputs, {} spends, chain_height={}",
        manifest.output_count, manifest.spend_count, manifest.chain_height,
    );

    // Check if already loaded (tree_height >= boost_output_count)
    let db_clone = db.clone();
    let tree_height = tokio::task::spawn_blocking(move || db_clone.get_tree_height())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    if tree_height >= manifest.output_count {
        eprintln!(
            "[ZipherX] Boost scan: already loaded (tree_height={} >= output_count={})",
            tree_height, manifest.output_count,
        );
        // Return (chain_height, output_count) so post-boost scans use correct tree positions
        return Ok(Some((manifest.chain_height, manifest.output_count)));
    }

    // Find sections
    let outputs_section = boost_download::get_section(&manifest, BOOST_SECTION_OUTPUTS)
        .ok_or_else(|| CoreError::Storage("Boost manifest missing outputs section".into()))?
        .clone();
    let spends_section = boost_download::get_section(&manifest, BOOST_SECTION_SPENDS)
        .ok_or_else(|| CoreError::Storage("Boost manifest missing spends section".into()))?
        .clone();
    let tree_section = boost_download::get_section(&manifest, BOOST_SECTION_TREE)
        .ok_or_else(|| CoreError::Storage("Boost manifest missing tree section".into()))?
        .clone();

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: 0,
            target_height: manifest.chain_height,
            notes_found: 0,
        });
    }

    eprintln!(
        "[ZipherX] Boost scan: loading {} outputs ({} MB), {} spends ({} MB) from boost file",
        manifest.output_count,
        outputs_section.size / (1024 * 1024),
        manifest.spend_count,
        spends_section.size / (1024 * 1024),
    );

    // Memory-mapped I/O for the outputs section (~1.75 GB).
    // Instead of loading the entire section into a Vec (which caused OOM on Android),
    // we mmap it so the OS pages data in/out as needed. Peak RSS drops from ~2 GB to ~300 MB.
    // Spends (~200 MB) are still read into memory (needed for HashMap building).
    // Tree data (~80 MB) is deferred until after the scan to avoid concurrent pressure.
    let boost_path = boost_file_str.clone();
    let boost_path2 = boost_path.clone();

    let outputs_section_clone = outputs_section.clone();
    let spends_section_clone = spends_section.clone();

    // Step 1: mmap outputs + read spends (tree deferred)
    let outputs_mmap = boost_download::mmap_section(&boost_path, &outputs_section_clone)?;
    let spends_data = tokio::task::spawn_blocking(move || {
        boost_download::read_section(&boost_path2, &spends_section_clone)
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    eprintln!(
        "[ZipherX] Boost scan: mmap {} bytes outputs, read {} bytes spends (tree deferred)",
        outputs_mmap.len(),
        spends_data.len(),
    );

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: manifest.chain_height / 4,
            target_height: manifest.chain_height,
            notes_found: 0,
        });
    }

    // RC-8: Gate address logging behind compile-time #[cfg] to ensure spending key
    // addresses are NEVER compiled into release binaries.
    #[cfg(debug_assertions)]
    {
        if let Ok((addr_bytes, _)) = zipherx_crypto::keys::derive_address(sk_bytes, 0) {
            if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                eprintln!(
                    "[ZipherX] Boost scan: spending key default address = {}",
                    addr
                );
            }
        }
    }

    // Report boost scan phase to UI
    let output_count = outputs_mmap.len() / 684; // BOOST_OUTPUT_SIZE = 684
    if let Some(ref p) = progress {
        p(SyncStatus::BoostScan {
            outputs_total: output_count as u64,
        });
    }
    eprintln!(
        "[ZipherX] Boost scan: scanning {} outputs (CPU-intensive trial decryption)...",
        output_count,
    );

    // Parallel boost scan via Rayon (in spawn_blocking since it's CPU-bound).
    //
    // RC-18: Use Arc<Mmap> to pass the mmap into spawn_blocking without copying.
    // The mmap is backed by the OS page cache — only accessed pages are loaded
    // into RAM, keeping peak RSS far below the 1.75 GB section size.
    // Previous code called `.to_vec()` which copied the entire region into heap,
    // causing OOM on Android devices with limited RAM.
    // RC-2: Spending key clone for boost scan — zeroized inside spawn_blocking.
    let sk = sk_bytes.to_vec();
    let outputs_mmap = Arc::new(outputs_mmap);
    let spends_for_scan = spends_data;
    let (scan_result, boost_notes) = tokio::task::spawn_blocking(move || {
        let mut sk = sk; // RC-2: take ownership for zeroization
        let result =
            zipherx_crypto::boost_scan::scan_boost_outputs(&sk, &outputs_mmap, &spends_for_scan);
        sk.zeroize(); // RC-2: Explicit zeroization of spending key material
        result
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Crypto(e.to_string()))?;

    // RC-8: Balance values only in debug builds; release just logs counts.
    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] Boost scan: {} notes found, {} spent, balance = {} zatoshis",
        scan_result.notes_found, scan_result.notes_spent, scan_result.unspent_balance,
    );
    #[cfg(not(debug_assertions))]
    eprintln!(
        "[ZipherX] Boost scan: {} notes found, {} spent",
        scan_result.notes_found, scan_result.notes_spent,
    );

    // Step 2: Now read tree data (only ~80 MB, after outputs mmap is released)
    let boost_path3 = boost_file_str.clone();
    let tree_data = tokio::task::spawn_blocking(move || {
        boost_download::read_section(&boost_path3, &tree_section)
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
    eprintln!(
        "[ZipherX] Boost scan: read {} bytes tree data",
        tree_data.len()
    );

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: manifest.chain_height / 2,
            target_height: manifest.chain_height,
            notes_found: scan_result.notes_found,
        });
    }

    // Clear old data and insert boost notes
    let tree_data_for_validation = tree_data.clone(); // Keep a copy for tree root validation
    let db_clone = db.clone();
    let boost_output_count = manifest.output_count;
    let chain_height = manifest.chain_height;

    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
        // Step 1: Clear all notes, tx history, and tree state
        db_clone
            .clear_notes_and_history()
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        eprintln!("[ZipherX] Boost scan: cleared old notes and history");

        // Step 2: Insert notes into notes table + mark spent
        for note in &boost_notes {
            let mut txid_display = note.received_txid;
            txid_display.reverse();
            let txid_hex = hex::encode(txid_display);

            db_clone
                .insert_note(
                    0,                               // account_id
                    note.height as u64,              // height
                    &note.cmu,                       // cmu
                    note.value,                      // value
                    Some(&note.nullifier),            // nullifier (auto-hashed by VUL-009)
                    Some(&note.rcm),                 // rcm
                    None,                            // epk (not in BoostScanNote)
                    None,                            // ciphertext (not needed post-scan)
                    None,                            // memo
                    Some(&note.diversifier),          // diversifier
                    None,                            // witness (built later)
                    Some(&txid_hex),                 // received_txid
                    Some(note.position),             // position
                )
                .map_err(|e| CoreError::Storage(e.to_string()))?;

            if note.is_spent {
                let mut spent_txid_display = note.spent_txid;
                spent_txid_display.reverse();
                let spent_txid_hex = hex::encode(spent_txid_display);

                db_clone
                    .mark_note_spent(
                        &note.nullifier,
                        &spent_txid_hex,
                        note.spent_height as u64,
                    )
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
            }
        }

        // Step 3: Build clean transaction history by aggregating per-TX
        //
        // Problem: per-note entries show confusing "change" amounts.
        // Fix: group notes by txid, compute net amounts, show one
        // clean entry per real transaction.
        //
        // A note received in the SAME txid that spent our other notes
        // is change — don't show it as a separate "received" entry.

        use std::collections::{HashMap, HashSet};

        // Collect all spent_txids (raw wire bytes) for change detection
        let spent_txid_set: HashSet<[u8; 32]> = boost_notes
            .iter()
            .filter(|n| n.is_spent)
            .map(|n| n.spent_txid)
            .collect();

        // Group received notes by received_txid (wire bytes)
        let mut received_by_tx: HashMap<[u8; 32], (u64, u32)> = HashMap::new();
        for note in &boost_notes {
            let entry = received_by_tx
                .entry(note.received_txid)
                .or_insert((0u64, 0u32));
            entry.0 += note.value;
            entry.1 = entry.1.max(note.height);
        }

        // Group spent notes by spent_txid → total input value + height
        let mut inputs_by_tx: HashMap<[u8; 32], (u64, u32)> = HashMap::new();
        for note in boost_notes.iter().filter(|n| n.is_spent) {
            let entry = inputs_by_tx
                .entry(note.spent_txid)
                .or_insert((0u64, 0u32));
            entry.0 += note.value;
            entry.1 = entry.1.max(note.spent_height);
        }

        let mut tx_received_count = 0u32;
        let mut tx_sent_count = 0u32;

        // Insert "received" entries — skip change outputs (received in a TX
        // that also spent our notes).
        for (recv_txid, (total_value, height)) in &received_by_tx {
            if spent_txid_set.contains(recv_txid) {
                // This is change from our own send — skip
                continue;
            }
            let mut txid_display = *recv_txid;
            txid_display.reverse();
            let txid_hex = hex::encode(txid_display);

            db_clone
                .insert_transaction(
                    &txid_hex,
                    *height as u64,
                    None,
                    TxType::Received,
                    *total_value,
                    0,
                    None,
                    None,
                    TxStatus::Confirmed,
                )
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            tx_received_count += 1;
        }

        // Insert "sent" entries — net amount (inputs - change - fee)
        for (spent_txid, (total_input, height)) in &inputs_by_tx {
            // Change = value of notes we received in this same TX
            let change_value = received_by_tx
                .get(spent_txid)
                .map(|(v, _)| *v)
                .unwrap_or(0);

            // RC-9: HARDCODED FEE — This 10,000 zatoshi (0.0001 ZCL) fee is an
            // approximation used for display purposes. The actual fee paid in the
            // transaction may differ. TODO: compute the real fee from TX inputs
            // minus outputs when full transaction data is available.
            let fee = 10_000u64;
            let net_sent = total_input
                .saturating_sub(change_value)
                .saturating_sub(fee);

            let mut txid_display = *spent_txid;
            txid_display.reverse();
            let txid_hex = hex::encode(txid_display);

            db_clone
                .insert_transaction(
                    &txid_hex,
                    *height as u64,
                    None,
                    TxType::Sent,
                    net_sent,
                    fee,
                    None,
                    None,
                    TxStatus::Confirmed,
                )
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            tx_sent_count += 1;
        }

        eprintln!(
            "[ZipherX] Boost scan: inserted {} notes, {} received TXs, {} sent TXs",
            boost_notes.len(), tx_received_count, tx_sent_count,
        );

        // Verify actual DB count matches expected (detects INSERT OR IGNORE collisions)
        let actual_count = db_clone.count_unspent_notes(0).unwrap_or(0)
            + db_clone.get_all_notes(0).map(|n| n.iter().filter(|n| n.is_spent).count()).unwrap_or(0);
        let expected_count = boost_notes.len();
        if actual_count != expected_count {
            eprintln!(
                "[ZipherX] *** WARNING: DB has {} notes but expected {} — {} notes lost to INSERT OR IGNORE (CMU collision?)",
                actual_count, expected_count, expected_count - actual_count,
            );
        } else {
            eprintln!(
                "[ZipherX] Boost scan: DB note count verified: {} == expected {}",
                actual_count, expected_count,
            );
        }

        // Step 3: Save the serialized tree from boost file
        // The boost file's tree section has an 8-byte position prefix (u64 LE = CMU count)
        // prepended by serialize_tree before the actual tree data.
        // read_commitment_tree() expects raw tree data starting with Optional<Node>,
        // so we must strip the 8-byte prefix before saving.
        let raw_tree = if tree_data.len() > 8 {
            &tree_data[8..]
        } else {
            &tree_data[..]
        };
        db_clone
            .save_tree_state(raw_tree, boost_output_count)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        eprintln!(
            "[ZipherX] Boost scan: saved tree state ({} bytes, stripped 8B prefix from {} bytes), tree_height={}",
            raw_tree.len(), tree_data.len(), boost_output_count,
        );

        // Update confirmations
        db_clone
            .update_all_confirmations(chain_height)
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(())
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: manifest.chain_height,
            target_height: manifest.chain_height,
            notes_found: scan_result.notes_found,
        });
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] Boost scan complete: {} received, {} spent, unspent = {} zatoshis",
        scan_result.total_received, scan_result.total_spent, scan_result.unspent_balance,
    );

    // ====== DIAGNOSTIC: Validate boost tree root against blockchain ======
    // This is the definitive completeness check. If the tree root from the
    // boost file matches the finalsaplingroot from the blockchain at the
    // same height, ALL outputs are present (the root encodes every CMU).
    {
        // Strip the 8-byte position prefix from boost file's tree section
        let tree_stripped = tree_data_for_validation
            .get(8..)
            .unwrap_or(&tree_data_for_validation);
        match commitment_tree::root_from_serialized(tree_stripped) {
            Ok(root_bytes) => {
                let root_hex = hex::encode(&root_bytes);
                eprintln!(
                    "[ZipherX] DIAG tree root validation: boost tree root = {}",
                    root_hex,
                );
                // Compare with manifest's tree_root (check both byte orders)
                let root_rev_hex =
                    hex::encode(root_bytes.iter().rev().copied().collect::<Vec<u8>>());
                if root_hex == manifest.tree_root || root_rev_hex == manifest.tree_root {
                    eprintln!("[ZipherX] DIAG tree root: MATCHES manifest tree_root");
                } else {
                    eprintln!(
                        "[ZipherX] *** TREE ROOT MISMATCH with manifest: tree={}, manifest={}",
                        root_hex, manifest.tree_root,
                    );
                }
            }
            Err(e) => {
                eprintln!("[ZipherX] DIAG tree root: failed to get root: {}", e);
            }
        }
    }

    Ok(Some((manifest.chain_height, manifest.output_count)))
}

// ============================================================================
// Post-Boost Delta Scan
// ============================================================================

/// Scan delta outputs above the boost height for notes received after
/// the boost snapshot.
///
/// The boost file covers all outputs up to `boost_chain_height`. Any delta
/// outputs with height > boost_chain_height are post-boost and need scanning.
/// We trial-decrypt them without tree operations (witnesses can be built later).
/// This ensures correct balance for recent transactions.
///
/// Returns (notes_found, spent_found).
async fn post_boost_delta_scan(
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    boost_chain_height: u64,
    boost_output_count: u64,
    chain_tip: u64,
    progress: &Option<SyncProgressFn>,
) -> Result<(u32, usize), CoreError> {
    // Load ALL delta outputs (sparse, typically ~3K records, ~2MB)
    let total_outputs = delta_store
        .output_count()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    if total_outputs == 0 {
        eprintln!("[ZipherX] Post-boost scan: no delta outputs");
        return Ok((0, 0));
    }

    let all_outputs = delta_store
        .load_outputs_paged(0, total_outputs)
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    // Filter to outputs above boost height
    let post_boost_outputs: Vec<&DeltaOutput> = all_outputs
        .iter()
        .filter(|o| (o.height as u64) > boost_chain_height)
        .collect();

    if post_boost_outputs.is_empty() {
        eprintln!(
            "[ZipherX] Post-boost scan: no outputs above boost height {}",
            boost_chain_height,
        );
        return Ok((0, 0));
    }

    eprintln!(
        "[ZipherX] Post-boost scan: {} outputs above height {} (from {} total delta outputs)",
        post_boost_outputs.len(),
        boost_chain_height,
        total_outputs,
    );

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: boost_chain_height,
            target_height: chain_tip,
            notes_found: 0,
        });
    }

    // Load only post-boost nullifiers for spend detection (height-filtered, avoids full load)
    let post_boost_nullifiers = delta_store
        .load_nullifiers_for_height_range(boost_chain_height + 1, chain_tip)
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    // Build height → nullifiers map for CompactBlock reconstruction
    let mut nullifier_map: std::collections::BTreeMap<u64, Vec<(Vec<u8>, Vec<u8>)>> =
        std::collections::BTreeMap::new();
    for nf in &post_boost_nullifiers {
        nullifier_map
            .entry(nf.height as u64)
            .or_default()
            .push((nf.txid.clone(), nf.nullifier.clone()));
    }

    // Build owned Vec for reconstruct_compact_blocks (it expects &[DeltaOutput])
    let owned_outputs: Vec<DeltaOutput> = post_boost_outputs.into_iter().cloned().collect();

    // Reconstruct compact blocks and trial-decrypt
    let blocks = reconstruct_compact_blocks(&owned_outputs, &nullifier_map);

    eprintln!(
        "[ZipherX] Post-boost scan: {} blocks reconstructed, scanning...",
        blocks.len(),
    );

    // Use boost_output_count as starting tree position. Post-boost CMUs
    // start at position boost_output_count in the commitment tree.
    // scan_blocks increments position for each output in each block,
    // so positions are correct as long as the delta store is complete
    // (no missing blocks in the range). On first run, delta may have
    // gaps, but the full block scan fills them and corrects nullifiers.
    let scan_result = scanner::scan_blocks(&blocks, sk_bytes, boost_output_count, None)?;

    let notes_found = scan_result.new_notes.len() as u32;
    let spent_count = scan_result.spent_nullifiers.len();

    eprintln!(
        "[ZipherX] Post-boost scan: {} notes found, {} spends detected",
        notes_found, spent_count,
    );

    // Insert discovered notes into DB (without witnesses — balance via getTotalUnspentBalance)
    if !scan_result.new_notes.is_empty() {
        let db_clone = db.clone();
        let notes = scan_result.new_notes.clone();
        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            for note in &notes {
                let memo_str = if note.note.memo.iter().all(|&b| b == 0) {
                    None
                } else {
                    let trimmed: Vec<u8> = note
                        .note
                        .memo
                        .iter()
                        .copied()
                        .take_while(|&b| b != 0)
                        .collect();
                    String::from_utf8(trimmed).ok()
                };

                let mut txid_display = note.txid;
                txid_display.reverse();
                let txid_hex = hex::encode(txid_display);

                eprintln!(
                    "[ZipherX]   Post-boost note: height={}, value={} zatoshis, txid={}...",
                    note.height,
                    note.note.value,
                    &txid_hex[..16],
                );

                db_clone
                    .insert_note(
                        0,
                        note.height,
                        &note.cmu,
                        note.note.value,
                        Some(&note.nullifier),
                        Some(&note.note.rcm),
                        Some(&note.epk),
                        Some(&note.ciphertext),
                        memo_str.as_deref(),
                        Some(&note.note.diversifier),
                        None, // No witness — will be built later
                        Some(&txid_hex),
                        None, // No tree position — approximate
                    )
                    .map_err(|e| CoreError::Storage(e.to_string()))?;

                // Insert received transaction record
                db_clone
                    .insert_transaction(
                        &txid_hex,
                        note.height,
                        None,
                        TxType::Received,
                        note.note.value,
                        0,
                        None,
                        memo_str.as_deref(),
                        TxStatus::Confirmed,
                    )
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
            }
            Ok(())
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
    }

    // Mark notes spent by post-boost nullifiers AND create "sent" TX history entries.
    // Without "sent" entries, the history filter hides change outputs but has nothing
    // to show for the send itself — making the TX completely invisible.
    if !scan_result.spent_nullifiers.is_empty() {
        let db_clone = db.clone();
        let nullifiers = scan_result.spent_nullifiers.clone();
        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            let mut spends_by_tx: HashMap<String, (u64, u64)> = HashMap::new();

            for (nullifier, txid_bytes) in &nullifiers {
                let mut txid_display = *txid_bytes;
                txid_display.reverse();
                let txid_hex = hex::encode(txid_display);

                // Look up note value BEFORE marking spent (need the value for history)
                let note_value = db_clone
                    .get_note_by_nullifier(nullifier)
                    .ok()
                    .flatten()
                    .map(|n| n.value)
                    .unwrap_or(0);

                let _ = db_clone.mark_note_spent(nullifier, &txid_hex, 0);

                if note_value > 0 {
                    let entry = spends_by_tx.entry(txid_hex).or_insert((0, 0));
                    entry.0 += note_value;
                }
            }

            // Insert "sent" TX history entries for aggregated spends.
            // Use height from the matching "received" entry (same txid = same block).
            for (txid_hex, (total_value, _)) in &spends_by_tx {
                let height = db_clone
                    .get_transaction_by_txid(txid_hex)
                    .ok()
                    .flatten()
                    .map(|r| r.height)
                    .unwrap_or(0);

                // RC-9: HARDCODED FEE — approximation for display. See RC-9 note above.
                let _ = db_clone.insert_transaction(
                    txid_hex,
                    height,
                    None,
                    TxType::Sent,
                    *total_value,
                    10_000,
                    None,
                    None,
                    TxStatus::Confirmed,
                );
            }

            if !spends_by_tx.is_empty() {
                eprintln!(
                    "[ZipherX] Post-boost scan: created {} 'sent' TX history entries from spend detection",
                    spends_by_tx.len(),
                );
            }

            Ok(())
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
    }

    // ================================================================
    // Cross-reference ALL delta nullifiers against unspent notes
    // ================================================================
    // The boost file's spend section may be incomplete — some on-chain spends
    // might be missing.  The delta store has nullifiers from blocks it downloaded
    // (those with new sapling outputs).  Cross-reference ALL of them against
    // unspent notes in the DB to catch any the boost scan missed.
    let mut cross_ref_count = 0usize;
    if !post_boost_nullifiers.is_empty() {
        eprintln!(
            "[ZipherX] Cross-ref: checking {} post-boost delta nullifiers against unspent notes",
            post_boost_nullifiers.len(),
        );

        let db_clone = db.clone();
        let nf_data: Vec<(Vec<u8>, Vec<u8>, u32)> = post_boost_nullifiers
            .iter()
            .map(|nf| (nf.nullifier.clone(), nf.txid.clone(), nf.height))
            .collect();

        cross_ref_count = tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
            let mut marked = 0usize;
            // Aggregate spends by txid for clean TX history entries
            let mut spends_by_tx: HashMap<String, (u64, u64)> = HashMap::new(); // txid_hex → (total_value, height)

            for (nullifier, txid_raw, height) in &nf_data {
                let mut txid_display = txid_raw.clone();
                txid_display.reverse();
                let txid_hex = hex::encode(&txid_display);

                let spent_value = db_clone
                    .cross_ref_nullifier_spend(nullifier, &txid_hex, *height as u64)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;

                if let Some(value) = spent_value {
                    marked += 1;
                    eprintln!(
                        "[ZipherX]   Cross-ref hit: block {} spent note worth {} zatoshis (tx={}...)",
                        height, value, &txid_hex[..16],
                    );
                    let entry = spends_by_tx.entry(txid_hex).or_insert((0, *height as u64));
                    entry.0 += value;
                }
            }

            // Insert aggregated "sent" TX history entries for newly detected spends
            for (txid_hex, (total_value, height)) in &spends_by_tx {
                // RC-9: HARDCODED FEE — approximation for display. See RC-9 note above.
                let _ = db_clone.insert_transaction(
                    txid_hex,
                    *height,
                    None,
                    TxType::Sent,
                    *total_value,
                    10_000,
                    None,
                    None,
                    TxStatus::Confirmed,
                );
            }

            Ok(marked)
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

        if cross_ref_count > 0 {
            eprintln!(
                "[ZipherX] Cross-ref complete: {} notes marked spent by delta nullifiers",
                cross_ref_count,
            );
        } else {
            eprintln!(
                "[ZipherX] Cross-ref: 0 matches in {} delta nullifiers — boost file spends may be complete, \
                 or delta store lacks spend-only blocks",
                post_boost_nullifiers.len(),
            );
        }
    }

    if notes_found > 0 || spent_count > 0 || cross_ref_count > 0 {
        eprintln!(
            "[ZipherX] Post-boost scan complete: {} notes, {} scan-spends, {} cross-ref spends",
            notes_found, spent_count, cross_ref_count,
        );
    }

    Ok((notes_found, spent_count + cross_ref_count))
}

// ============================================================================
// Post-Boost Full Block Scan (outputs + nullifiers)
// ============================================================================

/// Download ALL blocks from boost_height+1 to chain_tip, extract both shielded
/// outputs (for note discovery) and nullifiers (for spend detection).
///
/// The delta store only captured ~24 post-boost outputs from blocks where
/// `finalsaplingroot` changed. Many blocks with our received notes were never
/// fetched. This function downloads every block in the post-boost range,
/// trial-decrypts all outputs to find received notes, and cross-references
/// nullifiers against unspent notes.
///
/// Returns (notes_marked_spent, notes_found, tree_root_valid).
async fn post_boost_full_block_scan(
    peer_manager: &mut PeerManager,
    header_store: &SqliteHeaderStore,
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    boost_chain_height: u64,
    boost_output_count: u64,
    chain_tip: u64,
    progress: &Option<SyncProgressFn>,
) -> Result<(usize, u32, bool), CoreError> {
    if boost_chain_height >= chain_tip {
        return Ok((0, 0, true));
    }

    // Check if delta bundle already verified (boost + post-boost = effective boost)
    let db_c = db.clone();
    let (is_verified, last_scanned) = tokio::task::spawn_blocking(move || {
        let v = db_c.get_delta_bundle_verified()?;
        let ss = db_c.get_sync_state()?;
        Ok::<_, zipherx_storage::types::StorageError>((v, ss.last_scanned_height))
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Storage(e.to_string()))?;

    let delta_end = delta_store
        .get_delta_end_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    // If we already scanned up to or past chain_tip, skip entirely
    if last_scanned >= chain_tip {
        eprintln!(
            "[ZipherX] Post-boost: last_scanned={} >= chain_tip={} — already scanned",
            last_scanned, chain_tip,
        );
        return Ok((0, 0, true));
    }

    if is_verified && delta_end >= chain_tip {
        eprintln!(
            "[ZipherX] Post-boost: delta verified up to {}, chain_tip={} — already complete",
            delta_end, chain_tip,
        );
        return Ok((0, 0, true));
    }

    // Use the best known checkpoint: max of (delta_end if verified, last_scanned, boost_chain_height)
    let scan_start = if is_verified && delta_end > boost_chain_height {
        let best = delta_end.max(last_scanned);
        eprintln!(
            "[ZipherX] Post-boost: delta verified={}, last_scanned={}, scanning from {} to chain_tip={}",
            delta_end, last_scanned, best + 1, chain_tip,
        );
        best + 1
    } else if last_scanned > boost_chain_height {
        eprintln!(
            "[ZipherX] Post-boost: resuming from last_scanned={} to chain_tip={}",
            last_scanned, chain_tip,
        );
        last_scanned + 1
    } else {
        boost_chain_height + 1
    };

    // Count blocks without loading them (avoids ~90MB upfront allocation)
    let initial_count = header_store
        .count_block_hashes_in_range(scan_start, chain_tip)
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    if initial_count == 0 {
        eprintln!(
            "[ZipherX] Full block scan: no headers in range {}-{}, skipping",
            scan_start, chain_tip
        );
        return Ok((0, 0, true));
    }

    eprintln!(
        "[ZipherX] Full block scan: {} blocks to download in post-boost range {}-{}",
        initial_count, scan_start, chain_tip,
    );

    if let Some(ref p) = progress {
        p(SyncStatus::BlockScan {
            current_height: scan_start,
            target_height: chain_tip,
            notes_found: 0,
        });
    }

    // The boost scan loads ~750MB and does parallel Rayon decryption, which can
    // take 30-90+ seconds. During this time, block listener tasks may hit the
    // 120s idle timeout and die. Once dead, the reader is consumed (take()) and
    // can NEVER be restarted — start_block_listener silently fails.
    //
    // The only fix is a FULL reconnect to get fresh Peer objects with fresh readers.
    let has_listeners = peer_manager.has_active_block_listeners();
    let ready_count = peer_manager.connected_count();
    eprintln!(
        "[ZipherX] Full block scan: {} ready peers, listeners_active={}",
        ready_count, has_listeners,
    );

    if !has_listeners {
        eprintln!(
            "[ZipherX] Full block scan: listeners dead (boost scan took too long), reconnecting..."
        );
        // Disconnect zombie peers first — they pass is_connected() but can't serve data.
        // Without this, connect() sees enough peers and skips reconnection entirely.
        if ready_count > 0 {
            eprintln!(
                "[ZipherX] Disconnecting {} zombie peers before reconnect",
                ready_count,
            );
            peer_manager.disconnect_all().await;
        }
        match peer_manager.connect().await {
            Ok(()) => {
                peer_manager.start_all_block_listeners().await;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                eprintln!(
                    "[ZipherX] Full block scan: reconnected, {} peers, listeners={}",
                    peer_manager.connected_count(),
                    peer_manager.has_active_block_listeners(),
                );
            }
            Err(e) => {
                eprintln!("[ZipherX] Full block scan: reconnect failed: {e}");
                return Ok((0, 0, true));
            }
        }
    }

    // ============================================================
    // Nullifier recomputation BEFORE block scan: fix positions and
    // nullifiers for ALL post-boost notes. During per-chunk scanning,
    // we used position 0 (approximate). Now that ALL outputs are in
    // the delta store, we can compute EXACT positions.
    //
    // This MUST run before the block scan so that when we encounter
    // nullifiers (spends) in blocks, cross_ref_nullifier_spend can
    // match them against correctly-computed nullifiers in the DB.
    // Without this, self-sends and spends of post-boost notes fail
    // to be detected, causing balance inflation.
    // ============================================================
    const CMU_PAGE_SIZE: usize = 50_000;
    {
        let db_clone = db.clone();
        let bch = boost_chain_height;
        let unspent_notes = tokio::task::spawn_blocking(move || db_clone.get_all_unspent_notes(0))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let post_boost_notes: Vec<&zipherx_storage::types::Note> =
            unspent_notes.iter().filter(|n| n.height > bch).collect();

        if !post_boost_notes.is_empty() {
            let note_cmu_set: HashSet<Vec<u8>> =
                post_boost_notes.iter().map(|n| n.cmu.clone()).collect();

            let mut cmu_position_map: HashMap<Vec<u8>, u64> = HashMap::new();
            {
                let mut nf_page_offset: usize = 0;
                let mut unique_cmu_idx: u64 = 0;
                // Deduplicate CMUs when counting positions — duplicates in the
                // delta store (from overlapping delta_sync + block_scan) would
                // inflate positions and produce wrong nullifiers.
                let mut seen_cmus: HashSet<(u32, Vec<u8>)> = HashSet::new();
                loop {
                    let page = delta_store
                        .load_cmus_for_range_paged(
                            boost_chain_height + 1,
                            chain_tip,
                            nf_page_offset,
                            CMU_PAGE_SIZE,
                        )
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                    if page.is_empty() {
                        break;
                    }
                    let page_len = page.len();
                    for (height, cmu) in page {
                        // Skip duplicate (height, cmu) pairs
                        if !seen_cmus.insert((height, cmu.clone())) {
                            continue;
                        }
                        if note_cmu_set.contains(&cmu) {
                            cmu_position_map.insert(cmu, boost_output_count + unique_cmu_idx);
                        }
                        unique_cmu_idx += 1;
                    }
                    nf_page_offset += page_len;
                    if page_len < CMU_PAGE_SIZE {
                        break;
                    }
                }
            }

            if !cmu_position_map.is_empty() {
                eprintln!(
                    "[ZipherX] Pre-scan nullifier recompute: {} post-boost unspent notes, {} matched CMU positions",
                    post_boost_notes.len(), cmu_position_map.len(),
                );

                // RC-2: Spending key clone — zeroized after nullifier recompute.
                let sk = sk_bytes.to_vec();
                let db_clone = db.clone();
                let notes_data: Vec<(Vec<u8>, Option<Vec<u8>>, u64, Option<Vec<u8>>, Option<u64>)> =
                    post_boost_notes
                        .iter()
                        .map(|n| {
                            (
                                n.cmu.clone(),
                                n.diversifier.clone(),
                                n.value,
                                n.rcm.clone(),
                                n.position,
                            )
                        })
                        .collect();
                let cmu_map = cmu_position_map;

                let fixed_count = tokio::task::spawn_blocking(move || -> Result<u32, CoreError> {
                    let mut sk = sk; // RC-2: take ownership for zeroization
                    let mut count = 0u32;
                    for (cmu, diversifier, value, rcm, old_position) in &notes_data {
                        let correct_pos = match cmu_map.get(cmu) {
                            Some(&pos) => pos,
                            None => continue,
                        };

                        if *old_position == Some(correct_pos) {
                            continue;
                        }

                        let div_vec = match diversifier {
                            Some(d) if d.len() == 11 => d,
                            _ => continue,
                        };
                        let rcm_vec = match rcm {
                            Some(r) if r.len() == 32 => r,
                            _ => continue,
                        };

                        let mut div_arr = [0u8; 11];
                        div_arr.copy_from_slice(div_vec);
                        let mut rcm_arr = [0u8; 32];
                        rcm_arr.copy_from_slice(rcm_vec);

                        match zipherx_crypto::notes::compute_nullifier(
                            &sk, &div_arr, *value, &rcm_arr, correct_pos, false,
                        ) {
                            Ok(nf) => {
                                eprintln!(
                                    "[ZipherX]   Recompute: cmu={}... value={} old_pos={:?} new_pos={} nf={}...",
                                    hex::encode(&cmu[..8]), value, old_position, correct_pos,
                                    hex::encode(&nf[..8]),
                                );
                                match db_clone.update_note_nullifier_by_cmu(cmu, &nf, correct_pos) {
                                    Ok(true) => {
                                        count += 1;
                                    }
                                    Ok(false) => {}
                                    Err(e) => {
                                        eprintln!(
                                            "[ZipherX]   Nullifier update error: {}",
                                            e,
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[ZipherX]   Nullifier compute error: {}",
                                    e,
                                );
                            }
                        }
                    }
                    sk.zeroize(); // RC-2: Explicit zeroization of spending key material
                    Ok(count)
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                if fixed_count > 0 {
                    eprintln!(
                        "[ZipherX] Pre-scan nullifier recompute: FIXED {} notes with correct positions",
                        fixed_count,
                    );
                } else {
                    eprintln!(
                        "[ZipherX] Pre-scan nullifier recompute: all {} notes already have correct positions",
                        post_boost_notes.len(),
                    );
                }
            }
        }
    }

    // Download blocks with cursor-based pagination to avoid loading all 2M+
    // block hashes into memory at once. Each cursor page loads BATCH_SIZE
    // hashes from SQLite, fetches them from peers with retry, then advances.
    // 3 peers × 128 blocks/peer = 384 blocks per fetch round.
    // fetch_blocks_by_hashes distributes round-robin across all connected peers,
    // each getting up to MAX_BLOCKS_PER_PEER (128). Feeding fewer blocks than
    // peers×128 wastes peer bandwidth and slows the scan.
    const BATCH_SIZE: usize = 384;
    const MAX_BATCH_RETRIES: usize = 5;
    let pacing = PacingConfig::default();
    let mut total_nullifiers_found: u64 = 0;
    let mut total_marked_spent: usize = 0;
    let mut total_notes_found: u32 = 0;
    let mut total_shielded_outputs: u64 = 0;
    let mut total_shielded_spends: u64 = 0;
    let mut total_fetched: usize = 0;
    let mut total_unfetched: usize = 0;
    // Collect CMUs of our discovered notes for witness creation during tree building
    let mut all_discovered_note_cmus: Vec<[u8; 32]> = Vec::new();
    let mut cursor_offset: usize = 0;

    while cursor_offset < initial_count {
        // Load only BATCH_SIZE hashes from SQLite per cursor page
        let batch = header_store
            .get_block_hashes_in_range_paged(scan_start, chain_tip, BATCH_SIZE, cursor_offset)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        if batch.is_empty() {
            break;
        }
        cursor_offset += batch.len();

        // Inner retry loop for this batch (max BATCH_SIZE items, not 2.26M)
        let mut batch_remaining = batch;
        let mut batch_retry: usize = 0;
        let mut batch_consecutive_zero: usize = 0;

        while !batch_remaining.is_empty() && batch_retry < MAX_BATCH_RETRIES {
            batch_retry += 1;

            let fetch_result =
                async_block_fetch::fetch_blocks_by_hashes(peer_manager, &batch_remaining, &pacing)
                    .await?;

            let received = fetch_result.blocks.len();
            if received == 0 {
                batch_consecutive_zero += 1;
                if batch_consecutive_zero >= MAX_BATCH_RETRIES {
                    break;
                }
                // Peers may have gone stale — full reconnect at attempt 3
                if batch_consecutive_zero == 3 {
                    let ready_count = peer_manager.connected_count();
                    let has_listeners = peer_manager.has_active_block_listeners();
                    eprintln!(
                        "[ZipherX] Full block scan: 3 empty fetches, full reconnect... ({} ready, listeners={})",
                        ready_count, has_listeners,
                    );
                    // Zombie detection: peers pass is_connected() but listeners are dead.
                    // disconnect_all() so connect() starts fresh instead of seeing stale peers.
                    if !has_listeners && ready_count > 0 {
                        eprintln!(
                            "[ZipherX] Disconnecting {} zombie peers (connected but listeners dead)",
                            ready_count,
                        );
                        peer_manager.disconnect_all().await;
                    }
                    if let Ok(()) = peer_manager.connect().await {
                        peer_manager.start_all_block_listeners().await;
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        // Reset counter so fresh peers get a fair chance
                        batch_consecutive_zero = 0;
                        batch_retry = batch_retry.saturating_sub(3);
                    }
                } else {
                    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                }
                continue;
            }
            batch_consecutive_zero = 0;
            total_fetched += received;

            // Remove received heights from batch_remaining (O(500) not O(2.26M))
            let received_heights: HashSet<u64> =
                fetch_result.blocks.iter().map(|b| b.height).collect();
            batch_remaining.retain(|&(h, _)| !received_heights.contains(&h));

            // Store ALL shielded outputs to delta store (no dedup file read)
            let mut chunk_delta_outputs: Vec<DeltaOutput> = Vec::new();
            for block in &fetch_result.blocks {
                // Bounds check: block.height is u64 but DeltaOutput.height is u32.
                // Heights beyond u32::MAX would silently truncate and corrupt data.
                if block.height > u32::MAX as u64 {
                    return Err(CoreError::RuntimeError(format!(
                        "Block height {} exceeds u32::MAX, cannot store as DeltaOutput",
                        block.height,
                    )));
                }
                total_shielded_outputs += block.outputs.len() as u64;
                total_shielded_spends += block.spends.len() as u64;
                for (output_idx, output) in block.outputs.iter().enumerate() {
                    chunk_delta_outputs.push(DeltaOutput {
                        height: block.height as u32,
                        index: output_idx as u32,
                        cmu: output.cmu.to_vec(),
                        epk: output.epk.to_vec(),
                        ciphertext: output.ciphertext.clone(),
                        txid: output.txid.to_vec(),
                    });
                }
            }

            if !chunk_delta_outputs.is_empty() {
                let min_h = chunk_delta_outputs
                    .iter()
                    .map(|o| o.height as u64)
                    .min()
                    .expect("chunk_delta_outputs confirmed non-empty");
                let max_h = chunk_delta_outputs
                    .iter()
                    .map(|o| o.height as u64)
                    .max()
                    .expect("chunk_delta_outputs confirmed non-empty");
                delta_store
                    .append_outputs_no_dedup(&chunk_delta_outputs, min_h, max_h, None)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
            }

            // Extract nullifiers from fetched blocks
            let mut chunk_nullifiers: Vec<DeltaNullifier> = Vec::new();
            for block in &fetch_result.blocks {
                for spend in &block.spends {
                    chunk_nullifiers.push(DeltaNullifier {
                        height: block.height as u32,
                        txid: spend.txid.to_vec(),
                        nullifier: spend.nullifier.to_vec(),
                    });
                }
            }

            // Try to decrypt outputs from fetched blocks (find received notes)
            let mut chunk_notes: Vec<scanner::DiscoveredNote> = Vec::new();
            for block in &fetch_result.blocks {
                if block.outputs.is_empty() {
                    continue;
                }
                let (notes, _spent) = scanner::process_block(block, sk_bytes, 0)?;
                if !notes.is_empty() {
                    for note in &notes {
                        let mut txid_display = note.txid;
                        txid_display.reverse();
                        let txid_hex = hex::encode(txid_display);
                        eprintln!(
                            "[ZipherX]   Full block scan note: height={}, value={} zatoshis, txid={}...",
                            note.height, note.note.value, &txid_hex[..16],
                        );
                    }
                    chunk_notes.extend(notes);
                }
            }

            let chunk_notes_count = chunk_notes.len() as u32;
            let chunk_nf_count = chunk_nullifiers.len();

            eprintln!(
                "[ZipherX] Full block scan: +{} blocks ({}/{} total), {} nullifiers, {} notes, batch_remaining={}",
                received, total_fetched, initial_count,
                chunk_nf_count, chunk_notes_count, batch_remaining.len(),
            );

            // Build height → timestamp map from fetched blocks for TX history
            let height_timestamps: HashMap<u64, u32> = fetch_result
                .blocks
                .iter()
                .map(|b| (b.height, b.timestamp))
                .collect();

            // Insert discovered notes into DB
            if !chunk_notes.is_empty() {
                for note in &chunk_notes {
                    all_discovered_note_cmus.push(note.cmu);
                }
                total_notes_found += chunk_notes_count;
                let db_clone = db.clone();
                let ts_map = height_timestamps.clone();
                tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                    for note in &chunk_notes {
                        let memo_str = if note.note.memo.iter().all(|&b| b == 0) {
                            None
                        } else {
                            let trimmed: Vec<u8> = note
                                .note
                                .memo
                                .iter()
                                .copied()
                                .take_while(|&b| b != 0)
                                .collect();
                            String::from_utf8(trimmed).ok()
                        };

                        let mut txid_display = note.txid;
                        txid_display.reverse();
                        let txid_hex = hex::encode(txid_display);

                        let block_ts = ts_map.get(&note.height).map(|&t| t as u64);

                        db_clone
                            .insert_note(
                                0,
                                note.height,
                                &note.cmu,
                                note.note.value,
                                Some(&note.nullifier),
                                Some(&note.note.rcm),
                                Some(&note.epk),
                                Some(&note.ciphertext),
                                memo_str.as_deref(),
                                Some(&note.note.diversifier),
                                None,
                                Some(&txid_hex),
                                None,
                            )
                            .map_err(|e| CoreError::Storage(e.to_string()))?;

                        db_clone
                            .insert_transaction(
                                &txid_hex,
                                note.height,
                                block_ts,
                                TxType::Received,
                                note.note.value,
                                0,
                                None,
                                memo_str.as_deref(),
                                TxStatus::Confirmed,
                            )
                            .map_err(|e| CoreError::Storage(e.to_string()))?;
                    }
                    Ok(())
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
            }

            // Process nullifiers: persist + cross-reference against unspent notes
            if !chunk_nullifiers.is_empty() {
                total_nullifiers_found += chunk_nf_count as u64;

                delta_store
                    .append_nullifiers(&chunk_nullifiers)
                    .map_err(|e| CoreError::Storage(e.to_string()))?;

                let db_clone = db.clone();
                let nf_data: Vec<(Vec<u8>, Vec<u8>, u32)> = chunk_nullifiers
                    .iter()
                    .map(|nf| (nf.nullifier.clone(), nf.txid.clone(), nf.height))
                    .collect();

                let ts_map2 = height_timestamps;
                let chunk_marked = tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
                    let mut marked = 0usize;
                    let mut spends_by_tx: HashMap<String, (u64, u64)> = HashMap::new();

                    for (nullifier, txid_raw, height) in &nf_data {
                        let mut txid_display = txid_raw.clone();
                        txid_display.reverse();
                        let txid_hex = hex::encode(&txid_display);

                        let spent_value = db_clone
                            .cross_ref_nullifier_spend(nullifier, &txid_hex, *height as u64)
                            .map_err(|e| CoreError::Storage(e.to_string()))?;

                        if let Some(value) = spent_value {
                            marked += 1;
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[ZipherX]   SPEND FOUND: block {} spent note worth {} zatoshis (tx={}...)",
                                height, value, &txid_hex[..16],
                            );
                            let entry = spends_by_tx.entry(txid_hex).or_insert((0, *height as u64));
                            entry.0 += value;
                        }
                    }

                    for (txid_hex, (total_value, height)) in &spends_by_tx {
                        let block_ts = ts_map2.get(&(*height as u64)).map(|&t| t as u64);
                        let _ = db_clone.insert_transaction(
                            txid_hex,
                            *height,
                            block_ts,
                            TxType::Sent,
                            *total_value,
                            10_000,
                            None,
                            None,
                            TxStatus::Confirmed,
                        );
                    }

                    Ok(marked)
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                total_marked_spent += chunk_marked;
            }

            if let Some(ref p) = progress {
                let progress_height = received_heights.into_iter().max().unwrap_or(scan_start);
                p(SyncStatus::BlockScan {
                    current_height: progress_height,
                    target_height: chain_tip,
                    notes_found: total_notes_found,
                });
            }

            // Brief pacing between fetch attempts
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        } // end inner retry loop

        // Track unfetched blocks from this batch
        if !batch_remaining.is_empty() {
            total_unfetched += batch_remaining.len();
            eprintln!(
                "[ZipherX] *** WARN: {} blocks unfetched in batch (cursor {}-{}) ***",
                batch_remaining.len(),
                cursor_offset.saturating_sub(BATCH_SIZE),
                cursor_offset,
            );
        }
    } // end cursor loop

    if total_unfetched > 0 {
        eprintln!(
            "[ZipherX] *** WARN: {} total blocks unfetched — spends in these blocks will be MISSED ***",
            total_unfetched,
        );
        eprintln!(
            "[ZipherX]   *** This can cause INFLATED BALANCE — notes spent in unfetched blocks appear unspent ***",
        );
    }

    eprintln!(
        "[ZipherX] Full block scan complete: {} blocks, {} shielded outputs, {} shielded spends, {} nullifiers, {} notes found, {} notes marked spent",
        total_fetched, total_shielded_outputs, total_shielded_spends,
        total_nullifiers_found, total_notes_found, total_marked_spent,
    );

    // ============================================================
    // Tree validation: boost tree + post-boost CMUs → validate root
    // The tree was loaded from the boost file during boost_scan_if_needed().
    // Now append ALL post-boost CMUs and validate the combined root against
    // the blockchain's finalsaplingroot. If valid, mark as verified —
    // boost + post-boost becomes the new effective boost.
    // ============================================================
    let mut tree_root_valid = true; // Assume valid unless proven otherwise
    if total_fetched > 0 && total_shielded_outputs > 0 {
        eprintln!(
            "[ZipherX] Building combined tree: boost + {} post-boost outputs...",
            total_shielded_outputs,
        );

        // Load post-boost CMUs from delta store in pages to avoid OOM.
        // Each page is ~2 MB (50,000 records × ~40 bytes each).
        {
            let pre_size = commitment_tree::size()
                .map_err(|e| CoreError::Crypto(format!("Tree size: {e}")))?;

            // Skip CMUs already in tree to prevent double-append.
            let already_in_tree = if pre_size > boost_output_count {
                (pre_size - boost_output_count) as usize
            } else {
                0
            };

            let has_notes = !all_discovered_note_cmus.is_empty();
            let our_note_cmu_set: HashSet<[u8; 32]> =
                all_discovered_note_cmus.iter().cloned().collect();

            // Clear any stale witnesses from previous operations
            commitment_tree::clear_witnesses()
                .map_err(|e| CoreError::Crypto(format!("Clear witnesses: {e}")))?;

            let mut witness_map: Vec<([u8; 32], u64)> = Vec::new();
            let mut global_idx: usize = 0; // tracks position across pages for skip logic
            let mut appended_total: usize = 0;
            let mut dedup_skipped: usize = 0;
            let mut cmu_page_offset: usize = 0;

            // INVARIANT: delta store may contain duplicates from delta_sync + block_scan.
            // Deduplication via HashSet<(height, cmu)> is REQUIRED for correct position counting.
            // Track (height, cmu) pairs to deduplicate — delta_sync and block_scan
            // both append to the same delta store with no_dedup, so duplicates
            // are possible when they process overlapping height ranges.
            let mut seen_cmus: HashSet<(u32, [u8; 32])> = HashSet::new();

            loop {
                let page = delta_store
                    .load_cmus_for_range_paged(
                        scan_start,
                        chain_tip,
                        cmu_page_offset,
                        CMU_PAGE_SIZE,
                    )
                    .map_err(|e| CoreError::Storage(e.to_string()))?;
                if page.is_empty() {
                    break;
                }
                let page_len = page.len();

                for (h, cmu) in &page {
                    if cmu.len() != 32 {
                        global_idx += 1;
                        continue;
                    }
                    let mut cmu_arr = [0u8; 32];
                    cmu_arr.copy_from_slice(cmu);

                    // Always track in dedup set (even for skipped CMUs) so that
                    // duplicates spanning the skip boundary are caught.
                    let is_new = seen_cmus.insert((*h, cmu_arr));

                    // Skip CMUs already in tree
                    if global_idx < already_in_tree {
                        global_idx += 1;
                        if !is_new {
                            dedup_skipped += 1;
                        }
                        continue;
                    }
                    global_idx += 1;

                    // Deduplicate: skip if we've already seen this (height, cmu) pair
                    if !is_new {
                        dedup_skipped += 1;
                        continue;
                    }

                    commitment_tree::append(&cmu_arr)
                        .map_err(|e| CoreError::Crypto(format!("Tree append: {e}")))?;
                    appended_total += 1;

                    if has_notes && our_note_cmu_set.contains(&cmu_arr) {
                        let witness_idx = commitment_tree::witness_current()
                            .map_err(|e| CoreError::Crypto(format!("Witness create: {e}")))?;
                        witness_map.push((cmu_arr, witness_idx));
                        eprintln!(
                            "[ZipherX]   Created witness for note CMU {}... (idx={})",
                            hex::encode(&cmu_arr[..8]),
                            witness_idx,
                        );
                    }
                }

                cmu_page_offset += page_len;

                // Last page was smaller than limit — no more data
                if page_len < CMU_PAGE_SIZE {
                    break;
                }
            }

            if dedup_skipped > 0 {
                eprintln!(
                    "[ZipherX] Tree building: skipped {} duplicate CMUs (delta_sync + block_scan overlap)",
                    dedup_skipped,
                );
            }

            if already_in_tree > 0 {
                eprintln!(
                    "[ZipherX] Skipped {} CMUs already in tree (size={}, boost={}), appended {} new",
                    already_in_tree, pre_size, boost_output_count, appended_total,
                );
            } else {
                eprintln!(
                    "[ZipherX] Appended {} post-boost CMUs to tree (was size: {}, our notes: {})",
                    appended_total,
                    pre_size,
                    all_discovered_note_cmus.len(),
                );
            }
            let tree_size = commitment_tree::size()
                .map_err(|e| CoreError::Crypto(format!("Tree size: {e}")))?;

            // Get combined root
            let combined_root = commitment_tree::root()
                .map_err(|e| CoreError::Crypto(format!("Tree root: {e}")))?;
            let combined_root_hex = hex::encode(&combined_root);

            eprintln!(
                "[ZipherX] Combined tree: size={}, root={}",
                tree_size,
                &combined_root_hex[..16],
            );

            // Validate against HeaderStore's finalsaplingroot at chain_tip
            match header_store.get_sapling_root(chain_tip) {
                Ok(Some(blockchain_root)) => {
                    let blockchain_hex = hex::encode(&blockchain_root);
                    let blockchain_rev_hex =
                        hex::encode(blockchain_root.iter().rev().copied().collect::<Vec<u8>>());

                    let matches = combined_root_hex == blockchain_hex
                        || combined_root_hex == blockchain_rev_hex;

                    if matches {
                        eprintln!(
                            "[ZipherX] TREE ROOT VALIDATED at height {} — boost + post-boost is COMPLETE",
                            chain_tip,
                        );

                        // Save validated tree state to DB + set DeltaBundleVerified
                        let tree_data = commitment_tree::serialize()
                            .map_err(|e| CoreError::Crypto(format!("Serialize: {e}")))?;
                        let db_c = db.clone();
                        let ts = tree_size;
                        tokio::task::spawn_blocking(move || {
                            db_c.save_tree_state(&tree_data, ts)?;
                            db_c.set_delta_bundle_verified(true)?;
                            Ok::<(), zipherx_storage::types::StorageError>(())
                        })
                        .await
                        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                        // Update delta manifest with validated root + end_height
                        delta_store
                            .update_manifest_verified(chain_tip, &combined_root_hex)
                            .map_err(|e| CoreError::Storage(e.to_string()))?;

                        eprintln!(
                            "[ZipherX] Effective boost: boost({}) + delta({}) = {} total CMUs, verified=true",
                            boost_output_count,
                            appended_total,
                            tree_size,
                        );

                        // ============================================================
                        // Witness storage: serialize witnesses and update DB notes
                        // Witnesses are valid because tree root is validated against
                        // blockchain. Each witness root == combined_root == anchor.
                        // ============================================================
                        if !witness_map.is_empty() {
                            eprintln!(
                                "[ZipherX] Storing {} witnesses for discovered notes...",
                                witness_map.len(),
                            );

                            // Serialize all witnesses
                            let mut witness_data: Vec<([u8; 32], Vec<u8>, [u8; 32])> = Vec::new();
                            for &(cmu_arr, witness_idx) in &witness_map {
                                match commitment_tree::get_witness_serialized(witness_idx) {
                                    Ok(wb) => {
                                        match commitment_tree::get_witness_root(witness_idx) {
                                            Ok(anchor) => {
                                                eprintln!(
                                                    "[ZipherX]   Witness {}: {} bytes, anchor={}...",
                                                    witness_idx, wb.len(),
                                                    hex::encode(&anchor[..8]),
                                                );
                                                witness_data.push((cmu_arr, wb, anchor));
                                            }
                                            Err(e) => {
                                                eprintln!(
                                                    "[ZipherX]   Witness {} root error: {e}",
                                                    witness_idx,
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[ZipherX]   Witness {} serialize error: {e}",
                                            witness_idx,
                                        );
                                    }
                                }
                            }

                            // Update DB: look up notes by CMU, set witness + anchor
                            if !witness_data.is_empty() {
                                let db_c = db.clone();
                                tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                                    let all_notes = db_c.get_all_unspent_notes(0)
                                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                                    for (cmu_arr, wb, anchor) in &witness_data {
                                        // Find note by CMU match
                                        let matching_note = all_notes.iter().find(|n| {
                                            n.cmu.len() == 32 && n.cmu[..] == cmu_arr[..]
                                        });

                                        if let Some(note) = matching_note {
                                            db_c.update_note_witness(note.id, wb)
                                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                                            db_c.update_note_anchor(note.id, anchor)
                                                .map_err(|e| CoreError::Storage(e.to_string()))?;
                                            #[cfg(debug_assertions)]
                                            eprintln!(
                                                "[ZipherX]   Updated note id={} (value={} zatoshis) with witness + anchor",
                                                note.id, note.value,
                                            );
                                        } else {
                                            eprintln!(
                                                "[ZipherX]   WARN: no matching unspent note for CMU {}...",
                                                hex::encode(&cmu_arr[..8]),
                                            );
                                        }
                                    }
                                    Ok(())
                                })
                                .await
                                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
                            }
                        }
                    } else {
                        eprintln!(
                            "[ZipherX] *** TREE ROOT MISMATCH at height {} ***",
                            chain_tip,
                        );
                        eprintln!("[ZipherX]   combined  = {}", combined_root_hex,);
                        eprintln!(
                            "[ZipherX]   blockchain = {} (rev: {})",
                            blockchain_hex, blockchain_rev_hex,
                        );

                        // FIX #1300: Signal tree root mismatch to caller.
                        // The caller will reset last_scanned to boost height AFTER
                        // Step 9 (which would otherwise overwrite any reset we do here).
                        tree_root_valid = false;
                        eprintln!(
                            "[ZipherX] Tree root invalid — will reset for full post-boost rescan",
                        );
                    }
                }
                Ok(None) => {
                    eprintln!(
                        "[ZipherX] Tree validation: no finalsaplingroot at height {} (headers not synced?)",
                        chain_tip,
                    );
                }
                Err(e) => {
                    eprintln!("[ZipherX] Tree validation error: {}", e);
                }
            }
        }
    }

    // ============================================================
    // Post-scan nullifier recompute + cross-ref RECHECK
    //
    // After the block scan AND tree building, re-examine ALL unspent
    // post-boost notes. Recompute their nullifiers with correct
    // positions (using deduped delta CMU scan — same logic as tree
    // building), then cross-ref ALL delta nullifiers against the
    // now-correct DB nullifiers.
    //
    // This catches cases the pre-scan recompute missed:
    //   - Notes discovered during the block scan (not present at
    //     pre-scan time)
    //   - Position errors from delta store changes during block scan
    //   - Any notes inserted with position=0 by process_block
    //     RC-18: Notes discovered during block scan are initially stored with
    //     position=0 (approximate). This is intentional — the correct position
    //     is computed during this post-scan recompute pass, which has access to
    //     the complete delta CMU data needed for accurate position calculation.
    // ============================================================
    {
        let db_clone = db.clone();
        let bch = boost_chain_height;
        let unspent_notes = tokio::task::spawn_blocking(move || db_clone.get_all_unspent_notes(0))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let post_boost_unspent: Vec<&zipherx_storage::types::Note> =
            unspent_notes.iter().filter(|n| n.height > bch).collect();

        if !post_boost_unspent.is_empty() {
            let note_cmu_set: HashSet<Vec<u8>> =
                post_boost_unspent.iter().map(|n| n.cmu.clone()).collect();

            // Compute correct positions with dedup (matching tree building logic)
            let mut cmu_position_map: HashMap<Vec<u8>, u64> = HashMap::new();
            {
                let mut page_offset: usize = 0;
                let mut unique_idx: u64 = 0;
                let mut seen: HashSet<(u32, Vec<u8>)> = HashSet::new();
                loop {
                    let page = delta_store
                        .load_cmus_for_range_paged(
                            boost_chain_height + 1,
                            chain_tip,
                            page_offset,
                            CMU_PAGE_SIZE,
                        )
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                    if page.is_empty() {
                        break;
                    }
                    let page_len = page.len();
                    for (height, cmu) in page {
                        if !seen.insert((height, cmu.clone())) {
                            continue; // skip duplicate
                        }
                        if note_cmu_set.contains(&cmu) {
                            cmu_position_map.insert(cmu, boost_output_count + unique_idx);
                        }
                        unique_idx += 1;
                    }
                    page_offset += page_len;
                    if page_len < CMU_PAGE_SIZE {
                        break;
                    }
                }
            }

            if !cmu_position_map.is_empty() {
                eprintln!(
                    "[ZipherX] Post-scan nullifier recompute: {} unspent post-boost notes, {} matched positions",
                    post_boost_unspent.len(), cmu_position_map.len(),
                );

                // RC-2: Spending key clone — zeroized after nullifier recompute.
                let sk = sk_bytes.to_vec();
                let db_clone = db.clone();
                let notes_data: Vec<(Vec<u8>, Option<Vec<u8>>, u64, Option<Vec<u8>>, Option<u64>)> =
                    post_boost_unspent
                        .iter()
                        .map(|n| {
                            (
                                n.cmu.clone(),
                                n.diversifier.clone(),
                                n.value,
                                n.rcm.clone(),
                                n.position,
                            )
                        })
                        .collect();
                let cmu_map = cmu_position_map;

                let fixed_count = tokio::task::spawn_blocking(move || -> Result<u32, CoreError> {
                    let mut sk = sk; // RC-2: take ownership for zeroization
                    let mut count = 0u32;
                    #[allow(unused_variables)]
                    for (cmu, diversifier, value, rcm, old_position) in &notes_data {
                        let correct_pos = match cmu_map.get(cmu) {
                            Some(&pos) => pos,
                            None => continue,
                        };

                        let div_vec = match diversifier {
                            Some(d) if d.len() == 11 => d,
                            _ => continue,
                        };
                        let rcm_vec = match rcm {
                            Some(r) if r.len() == 32 => r,
                            _ => continue,
                        };

                        let mut div_arr = [0u8; 11];
                        div_arr.copy_from_slice(div_vec);
                        let mut rcm_arr = [0u8; 32];
                        rcm_arr.copy_from_slice(rcm_vec);

                        match zipherx_crypto::notes::compute_nullifier(
                            &sk, &div_arr, *value, &rcm_arr, correct_pos, false,
                        ) {
                            Ok(nf) => {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "[ZipherX]   Post-scan recompute: cmu={}... value={} old_pos={:?} pos={} nf={}...",
                                    hex::encode(&cmu[..8]), value, old_position, correct_pos,
                                    hex::encode(&nf[..8]),
                                );
                                match db_clone.update_note_nullifier_by_cmu(cmu, &nf, correct_pos) {
                                    Ok(true) => count += 1,
                                    Ok(false) => {}
                                    Err(e) => eprintln!("[ZipherX]   Post-scan nf update error: {}", e),
                                }
                            }
                            Err(e) => eprintln!("[ZipherX]   Post-scan nf compute error: {}", e),
                        }
                    }
                    sk.zeroize(); // RC-2: Explicit zeroization of spending key material
                    Ok(count)
                })
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

                eprintln!(
                    "[ZipherX] Post-scan nullifier recompute: updated {} notes",
                    fixed_count,
                );
            }
        }

        // Cross-ref RECHECK: load ALL delta nullifiers and re-check against
        // the now-correct DB nullifiers. This catches spends that the per-batch
        // cross-ref missed because nullifiers were wrong at scan time.
        let all_delta_nfs = delta_store
            .load_nullifiers_for_height_range(boost_chain_height + 1, chain_tip)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        let delta_nf_count = all_delta_nfs.len();

        if !all_delta_nfs.is_empty() {
            let db_clone = db.clone();
            let recheck_marked = tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
                let mut marked = 0usize;
                let mut spends_by_tx: HashMap<String, (u64, u64)> = HashMap::new();

                for nf in &all_delta_nfs {
                    let mut txid_display = nf.txid.clone();
                    txid_display.reverse();
                    let txid_hex = hex::encode(&txid_display);

                    let spent_value = db_clone
                        .cross_ref_nullifier_spend(&nf.nullifier, &txid_hex, nf.height as u64)
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                    if let Some(value) = spent_value {
                        marked += 1;
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[ZipherX]   RECHECK SPEND: block {} spent note worth {} zatoshis (tx={}...)",
                            nf.height, value, &txid_hex[..16],
                        );
                        let entry = spends_by_tx.entry(txid_hex).or_insert((0, nf.height as u64));
                        entry.0 += value;
                    }
                }

                // Record send transactions for newly-discovered spends
                for (txid_hex, (total_value, height)) in &spends_by_tx {
                    // RC-9: HARDCODED FEE — approximation for display. See RC-9 note above.
                    let _ = db_clone.insert_transaction(
                        txid_hex,
                        *height,
                        None,
                        TxType::Sent,
                        *total_value,
                        10_000,
                        None,
                        None,
                        TxStatus::Confirmed,
                    );
                }

                Ok(marked)
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

            if recheck_marked > 0 {
                total_marked_spent += recheck_marked;
                eprintln!(
                    "[ZipherX] Post-scan RECHECK: marked {} additional notes as spent (total now: {})",
                    recheck_marked, total_marked_spent,
                );
            } else {
                eprintln!(
                    "[ZipherX] Post-scan RECHECK: no additional spends found ({} nullifiers checked)",
                    delta_nf_count,
                );
            }
        }
    }

    Ok((total_marked_spent, total_notes_found, tree_root_valid))
}

// ============================================================================
// Targeted Block Scan for Pending Transactions
// ============================================================================

/// Download and scan recent blocks when there are pending sent transactions.
///
/// After a send, input notes are marked spent but the change note only exists
/// in the mined block. The delta store only captures blocks where the sapling
/// root changed, so the change note's block may be missing. This function
/// downloads ALL blocks between last_scanned_height+1 and chain_tip from peers,
/// trial-decrypts them, and inserts any discovered notes (including change notes).
///
/// Only runs when there are unconfirmed sent transactions in the DB.
async fn scan_blocks_for_pending_txs(
    peer_manager: &mut PeerManager,
    header_store: &Arc<SqliteHeaderStore>,
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    chain_tip: u64,
    _progress: &Option<SyncProgressFn>,
) -> Result<(u32, usize), CoreError> {
    // Check for pending sent transactions
    let db_c = db.clone();
    let pending_txs = tokio::task::spawn_blocking(move || db_c.get_pending_transactions())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let has_pending_sends = pending_txs
        .iter()
        .any(|tx| tx.tx_type == TxType::Sent);

    if !has_pending_sends {
        return Ok((0, 0));
    }

    eprintln!(
        "[ZipherX] Pending TX scan: {} pending transactions, scanning recent blocks...",
        pending_txs.len(),
    );

    // Get last scanned height
    let db_c = db.clone();
    let last_scanned = tokio::task::spawn_blocking(move || {
        db_c.get_sync_state().map(|s| s.last_scanned_height)
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::Storage(e.to_string()))?;

    let scan_start = if last_scanned > 0 {
        last_scanned + 1
    } else {
        // Fallback: scan last 20 blocks
        chain_tip.saturating_sub(20)
    };

    if scan_start > chain_tip {
        eprintln!(
            "[ZipherX] Pending TX scan: already scanned up to {} (tip={})",
            last_scanned, chain_tip,
        );
        return Ok((0, 0));
    }

    // Get block hashes from header store
    let block_count = header_store
        .count_block_hashes_in_range(scan_start, chain_tip)
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    if block_count == 0 {
        eprintln!(
            "[ZipherX] Pending TX scan: no headers in range {}-{}",
            scan_start, chain_tip,
        );
        return Ok((0, 0));
    }

    eprintln!(
        "[ZipherX] Pending TX scan: downloading {} blocks ({}-{})",
        block_count, scan_start, chain_tip,
    );

    // Ensure block listeners are active
    if !peer_manager.has_active_block_listeners() {
        peer_manager.start_all_block_listeners().await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let pacing = PacingConfig::default();
    let mut total_notes_found: u32 = 0;
    let mut total_marked_spent: usize = 0;
    let mut cursor_offset: usize = 0;

    const BATCH_SIZE: usize = 384;

    while cursor_offset < block_count {
        let batch = header_store
            .get_block_hashes_in_range_paged(scan_start, chain_tip, BATCH_SIZE, cursor_offset)
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        if batch.is_empty() {
            break;
        }
        cursor_offset += batch.len();

        let fetch_result =
            async_block_fetch::fetch_blocks_by_hashes(peer_manager, &batch, &pacing).await?;

        let received = fetch_result.blocks.len();
        if received == 0 {
            eprintln!("[ZipherX] Pending TX scan: 0 blocks received, stopping");
            break;
        }

        // Store outputs to delta store for future syncs
        let mut chunk_delta_outputs: Vec<DeltaOutput> = Vec::new();
        let mut chunk_nullifiers: Vec<DeltaNullifier> = Vec::new();

        for block in &fetch_result.blocks {
            if block.height > u32::MAX as u64 {
                continue;
            }
            for (output_idx, output) in block.outputs.iter().enumerate() {
                chunk_delta_outputs.push(DeltaOutput {
                    height: block.height as u32,
                    index: output_idx as u32,
                    cmu: output.cmu.to_vec(),
                    epk: output.epk.to_vec(),
                    ciphertext: output.ciphertext.clone(),
                    txid: output.txid.to_vec(),
                });
            }
            for spend in &block.spends {
                chunk_nullifiers.push(DeltaNullifier {
                    height: block.height as u32,
                    txid: spend.txid.to_vec(),
                    nullifier: spend.nullifier.to_vec(),
                });
            }
        }

        // Persist to delta store
        if !chunk_delta_outputs.is_empty() {
            let min_h = chunk_delta_outputs.iter().map(|o| o.height as u64).min().expect("chunk_delta_outputs confirmed non-empty");
            let max_h = chunk_delta_outputs.iter().map(|o| o.height as u64).max().expect("chunk_delta_outputs confirmed non-empty");
            delta_store
                .append_outputs_no_dedup(&chunk_delta_outputs, min_h, max_h, None)
                .map_err(|e| CoreError::Storage(e.to_string()))?;
        }

        if !chunk_nullifiers.is_empty() {
            delta_store
                .append_nullifiers(&chunk_nullifiers)
                .map_err(|e| CoreError::Storage(e.to_string()))?;
        }

        // Trial-decrypt outputs from fetched blocks
        let mut chunk_notes: Vec<scanner::DiscoveredNote> = Vec::new();
        for block in &fetch_result.blocks {
            if block.outputs.is_empty() {
                continue;
            }
            let (notes, _spent) = scanner::process_block(block, sk_bytes, 0)?;
            chunk_notes.extend(notes);
        }

        // Build height → timestamp map
        let height_timestamps: HashMap<u64, u32> = fetch_result
            .blocks
            .iter()
            .map(|b| (b.height, b.timestamp))
            .collect();

        // Insert discovered notes into DB
        if !chunk_notes.is_empty() {
            for note in &chunk_notes {
                let mut txid_display = note.txid;
                txid_display.reverse();
                eprintln!(
                    "[ZipherX]   Pending TX scan note: height={}, value={} zatoshis, txid={}...",
                    note.height, note.note.value, &hex::encode(txid_display)[..16],
                );
            }
            total_notes_found += chunk_notes.len() as u32;
            let db_clone = db.clone();
            let ts_map = height_timestamps.clone();
            tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                for note in &chunk_notes {
                    let memo_str = if note.note.memo.iter().all(|&b| b == 0) {
                        None
                    } else {
                        let trimmed: Vec<u8> = note.note.memo.iter().copied().take_while(|&b| b != 0).collect();
                        String::from_utf8(trimmed).ok()
                    };

                    let mut txid_display = note.txid;
                    txid_display.reverse();
                    let txid_hex = hex::encode(txid_display);
                    let block_ts = ts_map.get(&note.height).map(|&t| t as u64);

                    db_clone
                        .insert_note(
                            0,
                            note.height,
                            &note.cmu,
                            note.note.value,
                            Some(&note.nullifier),
                            Some(&note.note.rcm),
                            Some(&note.epk),
                            Some(&note.ciphertext),
                            memo_str.as_deref(),
                            Some(&note.note.diversifier),
                            None,
                            Some(&txid_hex),
                            None,
                        )
                        .map_err(|e| CoreError::Storage(e.to_string()))?;

                    db_clone
                        .insert_transaction(
                            &txid_hex,
                            note.height,
                            block_ts,
                            TxType::Received,
                            note.note.value,
                            0,
                            None,
                            memo_str.as_deref(),
                            TxStatus::Confirmed,
                        )
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                }
                Ok(())
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
        }

        // Cross-reference nullifiers against unspent notes
        if !chunk_nullifiers.is_empty() {
            let db_clone = db.clone();
            let nf_data: Vec<(Vec<u8>, Vec<u8>, u32)> = chunk_nullifiers
                .iter()
                .map(|nf| (nf.nullifier.clone(), nf.txid.clone(), nf.height))
                .collect();
            let chunk_marked = tokio::task::spawn_blocking(move || -> Result<usize, CoreError> {
                let mut marked = 0usize;
                for (nullifier, txid_raw, height) in &nf_data {
                    let mut txid_display = txid_raw.clone();
                    txid_display.reverse();
                    let txid_hex = hex::encode(&txid_display);
                    let spent_value = db_clone
                        .cross_ref_nullifier_spend(nullifier, &txid_hex, *height as u64)
                        .map_err(|e| CoreError::Storage(e.to_string()))?;
                    if spent_value.is_some() {
                        marked += 1;
                    }
                }
                Ok(marked)
            })
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))??;
            total_marked_spent += chunk_marked;
        }

        eprintln!(
            "[ZipherX] Pending TX scan batch: +{} blocks, {} notes, {} spent",
            received, total_notes_found, total_marked_spent,
        );
    }

    eprintln!(
        "[ZipherX] Pending TX scan complete: {} notes found, {} marked spent",
        total_notes_found, total_marked_spent,
    );

    Ok((total_notes_found, total_marked_spent))
}

// ============================================================================
// Compact Block Reconstruction
// ============================================================================

/// Reconstruct CompactBlock objects from delta store output records.
///
/// Groups DeltaOutput records by block height and includes nullifier spends
/// from the height-indexed map. Used by the catch-up scan to feed delta
/// store data into the block scanner.
fn reconstruct_compact_blocks(
    outputs: &[DeltaOutput],
    nullifier_map: &std::collections::BTreeMap<u64, Vec<(Vec<u8>, Vec<u8>)>>,
) -> Vec<CompactBlock> {
    use std::collections::BTreeMap;

    // Group outputs by height, sorted by index within each height
    let mut blocks_map: BTreeMap<u64, Vec<(u32, ShieldedOutput)>> = BTreeMap::new();

    for output in outputs {
        let height = output.height as u64;
        let cmu: [u8; 32] = output.cmu[..32].try_into().unwrap_or([0u8; 32]);
        let epk: [u8; 32] = output.epk[..32].try_into().unwrap_or([0u8; 32]);

        // Use real txid from delta store (v2) or synthetic fallback (v1 legacy)
        let txid: [u8; 32] = if output.txid.len() >= 32 && output.txid.iter().any(|&b| b != 0) {
            output.txid[..32].try_into().unwrap_or([0u8; 32])
        } else {
            // v1 legacy: synthetic txid from (height, index)
            let mut synthetic = [0u8; 32];
            synthetic[..4].copy_from_slice(&output.height.to_le_bytes());
            synthetic[4..8].copy_from_slice(&output.index.to_le_bytes());
            synthetic
        };

        let shielded = ShieldedOutput {
            txid,
            cmu,
            epk,
            ciphertext: output.ciphertext.clone(),
            cv: [0u8; 32],
        };
        blocks_map
            .entry(height)
            .or_default()
            .push((output.index, shielded));
    }

    blocks_map
        .into_iter()
        .map(|(height, mut indexed_outputs)| {
            // Sort by output index within each block
            indexed_outputs.sort_by_key(|(idx, _)| *idx);
            let block_outputs: Vec<ShieldedOutput> =
                indexed_outputs.into_iter().map(|(_, o)| o).collect();

            // Include nullifiers (spends) for this height
            let spends: Vec<ShieldedSpend> = nullifier_map
                .get(&height)
                .map(|nfs| {
                    nfs.iter()
                        .map(|(txid, nullifier)| ShieldedSpend {
                            txid: txid[..32].try_into().unwrap_or([0u8; 32]),
                            nullifier: nullifier[..32].try_into().unwrap_or([0u8; 32]),
                        })
                        .collect()
                })
                .unwrap_or_default();

            CompactBlock {
                height,
                hash: [0u8; 32],
                timestamp: 0,
                final_sapling_root: [0u8; 32],
                outputs: block_outputs,
                spends,
            }
        })
        .collect()
}

// ============================================================================
// Delta Sync
// ============================================================================

/// Sync delta bundle from current end height to chain tip.
pub async fn sync_delta_bundle_if_needed(
    _peer_manager: &PeerManager,
    header_store: &SqliteHeaderStore,
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    _config: &DeltaSyncConfig,
    guards: &SyncGuards,
) -> Result<DeltaSyncResult, CoreError> {
    if guards.is_syncing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(CoreError::SyncInProgress);
    }

    let manifest = delta_store
        .get_manifest()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let end_height = manifest.as_ref().map(|m| m.end_height).unwrap_or(0);

    let db_clone = db.clone();
    let sync_state = tokio::task::spawn_blocking(move || db_clone.get_sync_state())
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let header_height = header_store
        .get_latest_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?
        .unwrap_or(0);

    if sync_state.delta_bundle_verified && end_height >= header_height {
        return Ok(DeltaSyncResult {
            end_height,
            cmus_appended: 0,
            roots_stored: 0,
            was_noop: true,
        });
    }

    let range = sync::calculate_delta_sync_range(end_height, header_height, header_height);

    match range {
        None => Ok(DeltaSyncResult {
            end_height,
            cmus_appended: 0,
            roots_stored: 0,
            was_noop: true,
        }),
        Some((_start, _end)) => {
            let blocks_fetched: u64 = 0;
            let cmus_appended: u64 = 0;

            let validated_height = sync::validate_delta_sync_result(
                blocks_fetched,
                cmus_appended,
                end_height,
                end_height,
            );

            let new_end = validated_height.unwrap_or(end_height);

            Ok(DeltaSyncResult {
                end_height: new_end,
                cmus_appended,
                roots_stored: 0,
                was_noop: blocks_fetched == 0,
            })
        }
    }
}

// ============================================================================
// Gap Fill
// ============================================================================

/// Fill internal gaps in the delta bundle.
pub async fn gap_fill_delta_bundle(
    _peer_manager: &PeerManager,
    header_store: &SqliteHeaderStore,
    delta_store: &DeltaCMUStore,
    _db: Arc<WalletDatabase>,
    guards: &SyncGuards,
) -> Result<GapFillResult, CoreError> {
    if !guards.try_acquire_gap_fill() {
        return Err(CoreError::GapFillInProgress);
    }
    let _guard = SyncDropGuard {
        guards,
        flag: SyncFlag::GapFilling,
    };

    let _header_height = header_store
        .get_latest_height()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let manifest = delta_store
        .get_manifest()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let manifest = match manifest {
        Some(m) => m,
        None => {
            return Ok(GapFillResult {
                gaps_found: 0,
                gaps_filled: 0,
                cmus_recovered: 0,
            })
        }
    };

    let cmus = delta_store
        .load_cmus()
        .map_err(|e| CoreError::Storage(e.to_string()))?;

    let heights: Vec<u64> = cmus.iter().map(|(h, _)| *h as u64).collect();
    let gaps = sync::detect_gaps(&heights, manifest.start_height, manifest.end_height);

    Ok(GapFillResult {
        gaps_found: gaps.len(),
        gaps_filled: 0,
        cmus_recovered: 0,
    })
}

// ============================================================================
// Background Sync
// ============================================================================

/// Run background sync — called periodically to catch up to chain tip.
pub async fn background_sync(
    peer_manager: &mut PeerManager,
    header_store: &Arc<SqliteHeaderStore>,
    delta_store: &DeltaCMUStore,
    db: Arc<WalletDatabase>,
    sk_bytes: &[u8],
    guards: &SyncGuards,
) -> Result<(), CoreError> {
    if !guards.can_background_sync() {
        return Ok(());
    }

    sync_to_tip(
        peer_manager,
        header_store,
        delta_store,
        db,
        sk_bytes,
        guards,
        None,
        None,
        None,
    )
    .await?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn make_test_guards() -> SyncGuards {
        SyncGuards::new()
    }

    #[tokio::test]
    async fn test_sync_acquires_guard() {
        let guards = make_test_guards();
        assert!(!guards.is_syncing.load(Ordering::SeqCst));

        guards.is_syncing.store(true, Ordering::SeqCst);

        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = Arc::new(SqliteHeaderStore::open_in_memory().unwrap());
        let temp_dir =
            std::env::temp_dir().join(format!("zipherx_test_sync_{}", rand::random::<u64>()));
        let ds = DeltaCMUStore::new(&temp_dir).unwrap();

        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(pm_config);
        let result = sync_to_tip(&mut pm, &hs, &ds, db, &[], &guards, None, None, None).await;

        assert!(matches!(result, Err(CoreError::SyncInProgress)));
    }

    #[tokio::test]
    async fn test_sync_blocked_during_broadcast() {
        let guards = make_test_guards();
        guards.is_broadcasting.store(true, Ordering::SeqCst);

        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = Arc::new(SqliteHeaderStore::open_in_memory().unwrap());
        let temp_dir =
            std::env::temp_dir().join(format!("zipherx_test_bcast_{}", rand::random::<u64>()));
        let ds = DeltaCMUStore::new(&temp_dir).unwrap();

        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(pm_config);
        let result = sync_to_tip(&mut pm, &hs, &ds, db, &[], &guards, None, None, None).await;

        assert!(matches!(result, Err(CoreError::BroadcastingInProgress)));
    }

    #[tokio::test]
    async fn test_delta_sync_skips_when_caught_up() {
        let guards = make_test_guards();
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("zipherx_test_delta_{}", rand::random::<u64>()));
        let ds = DeltaCMUStore::new(&temp_dir).unwrap();
        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let config = DeltaSyncConfig::default();

        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let result = sync_delta_bundle_if_needed(
            &PeerManager::new(pm_config),
            &hs,
            &ds,
            db,
            &config,
            &guards,
        )
        .await;

        let r = result.unwrap();
        assert!(r.was_noop);
        assert_eq!(r.cmus_appended, 0);
    }

    #[tokio::test]
    async fn test_gap_fill_acquires_guard() {
        let guards = make_test_guards();
        guards.is_gap_filling.store(true, Ordering::SeqCst);

        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = SqliteHeaderStore::open_in_memory().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("zipherx_test_gap_{}", rand::random::<u64>()));
        let ds = DeltaCMUStore::new(&temp_dir).unwrap();

        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let result =
            gap_fill_delta_bundle(&PeerManager::new(pm_config), &hs, &ds, db, &guards).await;

        assert!(matches!(result, Err(CoreError::GapFillInProgress)));
    }

    #[tokio::test]
    async fn test_background_sync_respects_guards() {
        let guards = make_test_guards();
        guards.is_broadcasting.store(true, Ordering::SeqCst);

        let db = Arc::new(WalletDatabase::open_in_memory().unwrap());
        let hs = Arc::new(SqliteHeaderStore::open_in_memory().unwrap());
        let temp_dir =
            std::env::temp_dir().join(format!("zipherx_test_bg_{}", rand::random::<u64>()));
        let ds = DeltaCMUStore::new(&temp_dir).unwrap();

        let pm_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(pm_config);
        let result = background_sync(&mut pm, &hs, &ds, db, &[], &guards).await;

        assert!(result.is_ok());
    }
}
