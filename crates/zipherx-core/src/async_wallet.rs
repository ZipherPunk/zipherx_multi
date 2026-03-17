//! Async wallet orchestrator — top-level API tying all async flows together.
//!
//! `AsyncWallet` is the main entry point for all wallet operations.
//! It holds Arc references to shared state (DB, header store, delta store,
//! peer manager) and delegates to the async sync, send, and scan modules.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use tokio::sync::Mutex as TokioMutex;
use tokio::task::JoinHandle;
use zeroize::{Zeroize, Zeroizing};

use crate::async_send::{self, SendProgressFn};
use crate::async_sync::{self, SyncProgressFn};
use crate::send::SendRequest;
use crate::sync::SyncStatus;
use crate::wallet::{
    BalanceInfo, TransactionDisplay, WalletConfig, WalletCore, WalletLifecycleState, WalletSummary,
};
use crate::CoreError;
use zipherx_network::header_sync::HeaderStore;
use zipherx_network::peer_manager::PeerManager;
use zipherx_storage::database::WalletDatabase;
use zipherx_storage::delta_cmu::DeltaCMUStore;
use zipherx_storage::header_store_impl::SqliteHeaderStore;

// ============================================================================
// AsyncWallet
// ============================================================================

/// Top-level async wallet orchestrator.
///
/// Holds Arc references to all shared state and provides a clean async API.
/// The `peer_manager` uses a tokio Mutex (held across awaits).
/// The `db` uses spawn_blocking (rusqlite is sync).
pub struct AsyncWallet {
    /// Core wallet logic (pure functions, sync)
    pub core: WalletCore,
    /// Wallet database (SQLCipher)
    pub db: Arc<WalletDatabase>,
    /// Block header store
    pub header_store: Arc<SqliteHeaderStore>,
    /// Delta CMU store
    pub delta_store: Arc<DeltaCMUStore>,
    /// P2P peer manager (tokio mutex for cross-await holding)
    pub peer_manager: Arc<TokioMutex<PeerManager>>,
    /// Lock-free peer count for UI reads (updated during sync)
    pub connected_peer_count: Arc<AtomicU32>,
    /// Background sync task handle
    background_handle: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl AsyncWallet {
    /// Initialize a new AsyncWallet with the given configuration.
    ///
    /// Opens databases, creates peer manager, and sets up internal state.
    /// Does NOT connect to the network — call `connect_network()` for that.
    pub async fn initialize(config: WalletConfig) -> Result<Self, CoreError> {
        // Open databases via spawn_blocking (rusqlite is sync)
        let db_path = config.db_path.clone();
        let db_key = config.db_encryption_key.clone();
        let db = tokio::task::spawn_blocking(move || {
            if db_path == ":memory:" {
                WalletDatabase::open_in_memory()
            } else {
                WalletDatabase::open(&db_path, db_key.as_deref())
            }
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        let hs_path = config.header_store_path.clone();
        let header_store = tokio::task::spawn_blocking(move || {
            if hs_path == ":memory:" {
                SqliteHeaderStore::open_in_memory()
            } else {
                SqliteHeaderStore::open(&hs_path)
            }
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        let ds_dir = config.delta_store_dir.clone();
        let delta_store = tokio::task::spawn_blocking(move || {
            let path = std::path::Path::new(&ds_dir);
            DeltaCMUStore::new(path)
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        let core = WalletCore::new(config);
        let peer_config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let peer_manager = PeerManager::new(peer_config);

        Ok(Self {
            core,
            db: Arc::new(db),
            header_store: Arc::new(header_store),
            delta_store: Arc::new(delta_store),
            peer_manager: Arc::new(TokioMutex::new(peer_manager)),
            connected_peer_count: Arc::new(AtomicU32::new(0)),
            background_handle: std::sync::Mutex::new(None),
        })
    }

    /// Connect to the P2P network.
    ///
    /// Discovers peers via DNS, connects, and performs handshakes.
    pub async fn connect_network(&self) -> Result<(), CoreError> {
        let mut pm = self.peer_manager.lock().await;
        pm.connect_with_counter(Some(&self.connected_peer_count))
            .await
            .map_err(|e| CoreError::Network(e))?;
        // Final update in case connect_with_counter missed any edge case
        self.connected_peer_count
            .store(pm.connected_count() as u32, Ordering::Relaxed);
        Ok(())
    }

    /// Get the connected peer count (lock-free, safe to call during sync).
    pub fn get_connected_peer_count(&self) -> u32 {
        self.connected_peer_count.load(Ordering::Relaxed)
    }

    /// Get the current wallet state.
    pub fn state(&self) -> WalletLifecycleState {
        self.core.state()
    }

    /// Create a new wallet, returning the 24-word mnemonic.
    pub fn create_wallet(&self) -> Result<Vec<String>, CoreError> {
        self.core.create_wallet()
    }

    /// Restore a wallet from a mnemonic phrase.
    pub fn restore_wallet(&self, words: &[String]) -> Result<(), CoreError> {
        self.core.restore_wallet(words)
    }

    /// Import a wallet from raw spending key bytes.
    pub fn import_wallet_from_key(&self, sk_bytes: &[u8]) -> Result<(), CoreError> {
        self.core.import_wallet_from_key(sk_bytes)
    }

    /// Get the wallet's shielded address.
    pub fn get_address(&self, sk_bytes: &[u8]) -> Result<String, CoreError> {
        self.core.get_address(sk_bytes)
    }

    /// Sync the wallet to the network tip.
    ///
    /// Runs header sync → delta sync → block scan → witness update.
    /// Pass `sk_bytes` from Keychain/Secure Enclave to enable trial decryption
    /// (block scan + witness creation). If empty, only headers + delta download run.
    pub async fn sync(
        &self,
        sk_bytes: &[u8],
        progress: Option<SyncProgressFn>,
    ) -> Result<u64, CoreError> {
        let peer_count_ref = self.connected_peer_count.clone();
        let mut pm = self.peer_manager.lock().await;

        // Update peer count before sync (so UI sees it while mutex is held)
        peer_count_ref.store(pm.connected_count() as u32, Ordering::Relaxed);

        let boost_cache = self
            .core
            .config
            .boost_cache_dir
            .as_ref()
            .map(std::path::PathBuf::from);
        // Even without a seed, check for imported WIF addresses to scan
        let imported_addrs = {
            let db_clone = self.db.clone();
            tokio::task::spawn_blocking(move || {
                db_clone.get_imported_transparent_addresses().unwrap_or_default()
            }).await.unwrap_or_default()
        };
        let t_addrs = if !imported_addrs.is_empty() {
            let mut addr_set = crate::scanner::TransparentAddressSet::empty();
            for (db_id, addr) in &imported_addrs {
                addr_set.add_imported(addr.clone(), *db_id);
            }
            Some(addr_set)
        } else {
            None
        };

        let result = async_sync::sync_to_tip(
            &mut pm,
            &self.header_store,
            &self.delta_store,
            self.db.clone(),
            sk_bytes,
            &self.core.guards,
            progress,
            Some(peer_count_ref.clone()),
            boost_cache,
            t_addrs.as_ref(),
        )
        .await;

        // Update peer count after sync completes
        peer_count_ref.store(pm.connected_count() as u32, Ordering::Relaxed);

        // Reset live_chain_tip to the actual synced height so it stays anchored
        // to reality. Without this, fetch_add(1) per inv MSG_BLOCK drifts
        // live_chain_tip above median+100 over long sessions, causing
        // get_consensus_height() to fall back to a stale peer_start_height
        // median — which makes header sync return 0 new headers.
        if let Ok(synced_height) = &result {
            pm.live_chain_tip.store(*synced_height, Ordering::Relaxed);
        }

        // Pre-initialize the Sapling prover after sync so that send() is instant.
        // The params files are cached on disk; this just loads them into memory.
        // Only runs once per app session (is_prover_ready() returns true afterwards).
        if result.is_ok() && !crate::async_prover::is_prover_ready() {
            let spend_path = self.core.config.spend_params_path.clone();
            let output_path = self.core.config.output_params_path.clone();
            tokio::spawn(async move {
                match crate::async_prover::ensure_prover_initialized(&spend_path, &output_path)
                    .await
                {
                    Ok(()) => {
                        eprintln!("[ZipherX] Sapling prover pre-initialized (ready to send)");
                    }
                    Err(e) => {
                        eprintln!(
                            "[ZipherX] Prover pre-init failed (will retry on send): {}",
                            e
                        );
                    }
                }
            });
        }

        result
    }

    /// Sync with transparent address scanning.
    ///
    /// Same as `sync()` but also scans for transparent UTXOs matching
    /// addresses derived from the seed.
    pub async fn sync_with_transparent(
        &self,
        sk_bytes: &[u8],
        seed: &[u8],
        progress: Option<SyncProgressFn>,
    ) -> Result<u64, CoreError> {
        use crate::scanner::TransparentAddressSet;

        let peer_count_ref = self.connected_peer_count.clone();
        let mut pm = self.peer_manager.lock().await;
        peer_count_ref.store(pm.connected_count() as u32, Ordering::Relaxed);

        let boost_cache = self
            .core
            .config
            .boost_cache_dir
            .as_ref()
            .map(std::path::PathBuf::from);

        // Pre-derive transparent addresses for scanning (gap limit = 20)
        let mut t_address_set = TransparentAddressSet::from_seed(seed, 0, 20);

        // Also include imported (WIF) transparent addresses
        {
            let db_clone = self.db.clone();
            if let Ok(imported_addrs) = tokio::task::spawn_blocking(move || {
                db_clone.get_imported_transparent_addresses()
            })
            .await
            .unwrap_or(Err(zipherx_storage::types::StorageError::QueryFailed(
                "spawn failed".into(),
            ))) {
                for (db_id, addr) in &imported_addrs {
                    t_address_set.add_imported(addr.clone(), *db_id);
                }
            }
        }

        let result = async_sync::sync_to_tip(
            &mut pm,
            &self.header_store,
            &self.delta_store,
            self.db.clone(),
            sk_bytes,
            &self.core.guards,
            progress,
            Some(peer_count_ref.clone()),
            boost_cache,
            Some(&t_address_set),
        )
        .await;

        peer_count_ref.store(pm.connected_count() as u32, Ordering::Relaxed);

        if let Ok(synced_height) = &result {
            pm.live_chain_tip.store(*synced_height, Ordering::Relaxed);
        }

        // Backfill transaction_history for any transparent UTXOs missing history entries
        // (handles upgrade from pre-transparent versions)
        if result.is_ok() {
            let db_clone = self.db.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = db_clone.backfill_transparent_history();
            })
            .await;
        }

        if result.is_ok() && !crate::async_prover::is_prover_ready() {
            let spend_path = self.core.config.spend_params_path.clone();
            let output_path = self.core.config.output_params_path.clone();
            tokio::spawn(async move {
                let _ =
                    crate::async_prover::ensure_prover_initialized(&spend_path, &output_path).await;
            });
        }

        result
    }

    /// Send a shielded transaction.
    pub async fn send(
        &self,
        request: SendRequest,
        sk_bytes: &[u8],
        progress: Option<SendProgressFn>,
    ) -> Result<crate::send::SendResult, CoreError> {
        // Lazy prover initialization: download params if needed, then load.
        // ~47MB spend + ~3.5MB output params. Safe to call multiple times
        // (returns immediately if already initialized).
        if !crate::async_prover::is_prover_ready() {
            eprintln!("[ZipherX] Prover not initialized — downloading/loading Sapling params...");
            crate::async_prover::ensure_prover_initialized(
                &self.core.config.spend_params_path,
                &self.core.config.output_params_path,
            )
            .await?;
            eprintln!("[ZipherX] Sapling prover initialized successfully");
        }

        let pm = self.peer_manager.lock().await;
        let chain_height = self
            .header_store
            .get_latest_height()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .unwrap_or(0);

        async_send::send_transaction(
            self.db.clone(),
            &pm,
            &self.header_store,
            &self.delta_store,
            sk_bytes,
            &request,
            &self.core.guards,
            progress,
            chain_height,
        )
        .await
    }

    /// Send a transparent transaction (spending transparent UTXOs).
    ///
    /// Supports t→t and t→z (shielding). Requires the wallet seed for
    /// deriving transparent private keys. For imported UTXOs, `decrypt_fn`
    /// is used to decrypt the stored secret key.
    pub async fn send_transparent(
        &self,
        request: SendRequest,
        sk_bytes: &[u8],
        seed: &[u8],
        progress: Option<SendProgressFn>,
        decrypt_fn: impl Fn(&[u8]) -> Result<Zeroizing<Vec<u8>>, String> + Send + 'static,
    ) -> Result<crate::send::SendResult, CoreError> {
        // Ensure prover is initialized (needed for shielded change/output)
        if !crate::async_prover::is_prover_ready() {
            eprintln!("[ZipherX] Prover not initialized — downloading/loading Sapling params...");
            crate::async_prover::ensure_prover_initialized(
                &self.core.config.spend_params_path,
                &self.core.config.output_params_path,
            )
            .await?;
            eprintln!("[ZipherX] Sapling prover initialized successfully");
        }

        let pm = self.peer_manager.lock().await;
        let chain_height = self
            .header_store
            .get_latest_height()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .unwrap_or(0);

        async_send::send_transparent_transaction(
            self.db.clone(),
            &pm,
            sk_bytes,
            seed,
            &request,
            &self.core.guards,
            progress,
            chain_height,
            decrypt_fn,
        )
        .await
    }

    /// Get the current balance.
    pub async fn get_balance(&self) -> Result<BalanceInfo, CoreError> {
        let db_clone = self.db.clone();
        let notes = tokio::task::spawn_blocking(move || db_clone.get_all_unspent_notes(0))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(WalletCore::compute_balance(&notes))
    }

    /// Get the transparent (t-address) balance.
    pub async fn get_transparent_balance(&self) -> Result<u64, CoreError> {
        let db_clone = self.db.clone();
        tokio::task::spawn_blocking(move || db_clone.get_transparent_balance())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Get all unspent transparent UTXOs.
    pub async fn get_unspent_transparent_utxos(
        &self,
    ) -> Result<Vec<zipherx_storage::types::TransparentUtxo>, CoreError> {
        let db_clone = self.db.clone();
        tokio::task::spawn_blocking(move || db_clone.get_unspent_transparent_utxos())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Get transaction history.
    pub async fn get_transaction_history(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TransactionDisplay>, CoreError> {
        let db_clone = self.db.clone();
        let records =
            tokio::task::spawn_blocking(move || db_clone.get_transaction_history(limit, offset))
                .await
                .map_err(|e| CoreError::RuntimeError(e.to_string()))?
                .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(records
            .iter()
            .map(|r| WalletCore::transaction_to_display(r))
            .collect())
    }

    /// Get total IN (received) and OUT (sent) transaction counts.
    pub async fn get_transaction_counts(&self) -> Result<(u32, u32), CoreError> {
        let db_clone = self.db.clone();
        tokio::task::spawn_blocking(move || db_clone.get_transaction_counts())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Get a summary of the wallet state.
    pub async fn get_summary(&self) -> Result<WalletSummary, CoreError> {
        let state = self.core.state();
        let db_clone = self.db.clone();
        let sync_state = tokio::task::spawn_blocking(move || db_clone.get_sync_state())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let header_height = self
            .header_store
            .get_latest_height()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .unwrap_or(0);

        // Populate balance from DB (same logic as get_balance())
        let db_clone2 = self.db.clone();
        let balance = tokio::task::spawn_blocking(move || db_clone2.get_all_unspent_notes(0))
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .ok()
            .map(|notes| WalletCore::compute_balance(&notes));

        Ok(WalletSummary {
            state,
            address: None,
            balance,
            last_synced_height: sync_state.last_scanned_height,
            chain_tip: header_height,
            startup_mode: None,
            sync_status: SyncStatus::Idle,
        })
    }

    /// Start background sync loop.
    ///
    /// Spawns a task that periodically syncs to the chain tip.
    /// Pass `sk_bytes` for trial decryption during background syncs.
    /// Call `stop_background_sync()` to cancel.
    ///
    /// RC-2: The `sk_bytes` Vec is moved into the spawned task and will be
    /// zeroized when the task is cancelled (via `stop_background_sync`).
    pub async fn start_background_sync(&self, sk_bytes: Vec<u8>) -> Result<(), CoreError> {
        let mut handle_guard = self
            .background_handle
            .lock()
            .map_err(|_| CoreError::RuntimeError("lock poisoned".into()))?;

        if handle_guard.is_some() {
            return Ok(()); // Already running
        }

        let pm = self.peer_manager.clone();
        let hs = self.header_store.clone();
        let ds = self.delta_store.clone();
        let db = self.db.clone();
        let guards = self.core.guards.clone();

        // Set up new-block notification channel so inv MSG_BLOCK wakes the sleep
        let new_block_notify = Arc::new(tokio::sync::Notify::new());
        {
            let notify = new_block_notify.clone();
            let mut pm_lock = pm.lock().await;
            pm_lock.set_on_new_block(Arc::new(move || {
                notify.notify_one();
            }));
        }

        let handle = tokio::spawn(async move {
            #[allow(unused_mut)]
            let mut sk_bytes = Zeroizing::new(sk_bytes);

            const MAX_SYNC_RETRIES: u32 = 10;
            const BACKOFF_SECS: u64 = 300;
            let mut consecutive_failures: u32 = 0;

            loop {
                let sleep_secs = if consecutive_failures >= MAX_SYNC_RETRIES {
                    BACKOFF_SECS
                } else {
                    30
                };

                // Wait for either the timer OR a new-block notification (whichever first)
                tokio::select! {
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)) => {}
                    _ = new_block_notify.notified() => {
                        eprintln!("[ZipherX] Background sync: inv MSG_BLOCK — immediate sync");
                        // Small delay to let duplicate inv messages from other peers arrive
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }

                let mut pm_lock = pm.lock().await;
                match async_sync::background_sync(
                    &mut pm_lock,
                    &hs,
                    &ds,
                    db.clone(),
                    &sk_bytes,
                    &guards,
                )
                .await
                {
                    Ok(()) => {
                        consecutive_failures = 0;
                        // Restart block listeners after sync — sync consumes readers
                        if pm_lock.connected_count() > 0 && !pm_lock.has_active_block_listeners() {
                            pm_lock.start_all_block_listeners().await;
                        }
                    }
                    Err(_) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        if consecutive_failures == MAX_SYNC_RETRIES {
                            eprintln!(
                                "[ZipherX] Background sync: {} consecutive failures, backing off to {}s interval",
                                MAX_SYNC_RETRIES, BACKOFF_SECS,
                            );
                        }
                    }
                }
            }
            #[allow(unreachable_code)]
            {
                sk_bytes.zeroize();
            }
        });

        *handle_guard = Some(handle);
        Ok(())
    }

    /// Stop background sync.
    pub fn stop_background_sync(&self) {
        if let Ok(mut handle_guard) = self.background_handle.lock() {
            if let Some(handle) = handle_guard.take() {
                handle.abort();
            }
        }
    }

    /// Run a database repair (light: clear tree + witnesses only).
    pub async fn repair_database(&self) -> Result<(), CoreError> {
        let acquired = self
            .core
            .guards
            .is_repairing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok();

        if !acquired {
            return Err(CoreError::RepairInProgress);
        }

        let db_clone = self.db.clone();
        let result = tokio::task::spawn_blocking(move || db_clone.clear_tree_state_for_rebuild())
            .await
            .map_err(|e| CoreError::RuntimeError(e.to_string()))?
            .map_err(|e| CoreError::Storage(e.to_string()));

        self.core
            .guards
            .is_repairing
            .store(false, std::sync::atomic::Ordering::SeqCst);

        result
    }

    /// Full rescan: clear tree state, witnesses, and force re-validation.
    ///
    /// Clears tree state + all witnesses + resets delta_bundle_verified.
    /// Next sync will rebuild the tree and witnesses from boost/delta data.
    pub async fn full_rescan(&self) -> Result<(), CoreError> {
        let acquired = self
            .core
            .guards
            .is_repairing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok();

        if !acquired {
            return Err(CoreError::RepairInProgress);
        }

        // Clear tree state + witnesses (notes preserved)
        let db_clone = self.db.clone();
        let result = tokio::task::spawn_blocking(move || {
            db_clone.clear_tree_state_for_rebuild()?;
            // Also reset delta_bundle_verified to force fresh tree validation
            db_clone.set_delta_bundle_verified(false)?;
            Ok::<(), zipherx_storage::types::StorageError>(())
        })
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Storage(e.to_string()));

        self.core
            .guards
            .is_repairing
            .store(false, std::sync::atomic::Ordering::SeqCst);

        result
    }

    /// Export WIF private keys for all transparent addresses that have unspent funds.
    ///
    /// For seed-derived addresses, the WIF is derived from the seed.
    /// For imported addresses, the encrypted secret key is decrypted via `decrypt_fn`.
    /// Returns a list of funded addresses with their WIF keys.
    pub async fn export_funded_transparent_wifs(
        &self,
        seed: &[u8],
        decrypt_fn: impl Fn(&[u8]) -> Result<Zeroizing<Vec<u8>>, String> + Send + 'static,
    ) -> Result<Vec<FundedTransparentKey>, CoreError> {
        let db = self.db.clone();
        let seed = seed.to_vec();
        tokio::task::spawn_blocking(move || {
            let funded = db
                .get_funded_transparent_addresses()
                .map_err(|e| CoreError::Storage(e.to_string()))?;
            let mut keys = Vec::new();
            for f in funded {
                let wif = if f.is_imported {
                    let encrypted_sk =
                        db.get_imported_transparent_secret(&f.address)
                            .map_err(|e| CoreError::Storage(e.to_string()))?
                            .ok_or_else(|| {
                                CoreError::Crypto(format!(
                                    "Imported key not found: {}",
                                    f.address
                                ))
                            })?;
                    let sk_bytes = decrypt_fn(&encrypted_sk).map_err(|e| {
                        CoreError::Crypto(format!("Decrypt failed: {e}"))
                    })?;
                    let wif_str =
                        zipherx_crypto::transparent::encode_wif(&sk_bytes)
                            .map_err(|e| CoreError::Crypto(e.to_string()))?;
                    (*wif_str).clone()
                } else {
                    let wif_str =
                        zipherx_crypto::transparent::export_transparent_wif(
                            &seed,
                            0,
                            f.child_index,
                            f.is_change,
                        )
                        .map_err(|e| CoreError::Crypto(e.to_string()))?;
                    (*wif_str).clone()
                };
                keys.push(FundedTransparentKey {
                    address: f.address,
                    wif,
                    balance: f.balance,
                    is_change: f.is_change,
                    is_imported: f.is_imported,
                });
            }
            Ok(keys)
        })
        .await
        .map_err(|e| CoreError::RuntimeError(format!("spawn_blocking: {e}")))?
    }
}

/// A funded transparent address with its WIF private key.
#[derive(Debug, Clone)]
pub struct FundedTransparentKey {
    /// Encoded transparent address (t1...).
    pub address: String,
    /// WIF-encoded private key. Caller should zeroize after use.
    pub wif: String,
    /// Total unspent balance in zatoshis.
    pub balance: u64,
    /// Whether this is a change address (internal derivation chain).
    pub is_change: bool,
    /// Whether this address was imported via WIF rather than derived.
    pub is_imported: bool,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WalletConfig {
        let unique = rand::random::<u64>();
        WalletConfig {
            db_path: ":memory:".into(),
            header_store_path: ":memory:".into(),
            delta_store_dir: std::env::temp_dir()
                .join(format!("zipherx_test_wallet_{}", unique))
                .to_string_lossy()
                .into_owned(),
            spend_params_path: "/tmp/spend.params".into(),
            output_params_path: "/tmp/output.params".into(),
            account_index: 0,
            db_encryption_key: None,
            boost_cache_dir: None,
        }
    }

    #[tokio::test]
    async fn test_wallet_initialize() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        assert_eq!(wallet.state(), WalletLifecycleState::Uninitialized);
    }

    #[tokio::test]
    async fn test_wallet_create() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        let words = wallet.create_wallet().unwrap();
        assert_eq!(words.len(), 24);
        assert_eq!(wallet.state(), WalletLifecycleState::Locked);
    }

    #[tokio::test]
    async fn test_wallet_get_balance_empty() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        let balance = wallet.get_balance().await.unwrap();
        assert_eq!(balance.total, 0);
        assert_eq!(balance.spendable, 0);
    }

    #[tokio::test]
    async fn test_wallet_get_history_empty() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        let history = wallet.get_transaction_history(10, 0).await.unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_wallet_get_summary() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        let summary = wallet.get_summary().await.unwrap();
        assert_eq!(summary.last_synced_height, 0);
        assert_eq!(summary.chain_tip, 0);
    }

    #[tokio::test]
    async fn test_wallet_background_sync_start_stop() {
        let wallet = AsyncWallet::initialize(test_config()).await.unwrap();
        wallet.start_background_sync(vec![]).await.unwrap();
        // Double start is OK
        wallet.start_background_sync(vec![]).await.unwrap();
        wallet.stop_background_sync();
    }
}
