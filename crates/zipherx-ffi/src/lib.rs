//! ZipherX FFI bindings — UniFFI interface for cross-platform access.
//!
//! This crate generates native bindings for:
//! - Swift (iOS/macOS) via UniFFI Swift scaffolding
//! - Kotlin (Android) via UniFFI Kotlin scaffolding
//! - C# (Windows) via UniFFI C# scaffolding (future)
//!
//! All functions exposed here match the UDL interface definition.
//!
//! Phase 1: Crypto, mnemonic, address, balance, validation, boost scan.
//! Phase 9: Runtime, wallet lifecycle, sync, send, repair, Tor, platform.

uniffi::include_scaffolding!("zipherx");

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use zipherx_core::async_send::SendPhase;
use zipherx_core::async_wallet::AsyncWallet;
use zipherx_core::send::SendRequest;
use zipherx_core::sync::SyncStatus;
use zipherx_core::wallet::WalletConfig;
use zipherx_core::{runtime, CoreError};

/// RAII wrapper that zeros the contained `Vec<u8>` on drop.
/// Uses `write_volatile` to prevent the compiler from eliding the zeroing.
struct SecureVec(Vec<u8>);
impl Drop for SecureVec {
    fn drop(&mut self) {
        // Shrink to len so capacity bytes are also covered by zeroization
        self.0.shrink_to_fit();
        for byte in self.0.iter_mut() {
            // SAFETY: `byte` is a valid, aligned, initialized reference inside the Vec.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        // Fence to ensure volatile writes are not reordered past this point.
        std::sync::atomic::fence(Ordering::SeqCst);
    }
}
impl std::ops::Deref for SecureVec {
    type Target = Vec<u8>;
    fn deref(&self) -> &Vec<u8> {
        &self.0
    }
}
impl std::ops::DerefMut for SecureVec {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

// ============================================================================
// Error Type
// ============================================================================

// TODO (RF-24): Add numeric error codes to each variant for programmatic
// handling on the Swift/Kotlin side. UniFFI flat enums lose the `msg` field
// across the FFI boundary (see RF-15 note in zipherx.udl), so error codes
// would give callers a machine-readable way to distinguish error causes.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("Crypto error: {msg}")]
    CryptoError { msg: String },
    #[error("Network error: {msg}")]
    NetworkError { msg: String },
    #[error("Storage error: {msg}")]
    StorageError { msg: String },
    #[error("Invalid input: {msg}")]
    InvalidInput { msg: String },
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Wallet locked")]
    WalletLocked,
    #[error("Wallet not initialized")]
    NotInitialized,
    #[error("Sync in progress")]
    SyncInProgress,
    #[error("Runtime error: {msg}")]
    RuntimeError { msg: String },
    #[error("Broadcast failed: {msg}")]
    BroadcastFailed { msg: String },
    #[error("Invalid anchor")]
    InvalidAnchor,
}

impl From<CoreError> for WalletError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Crypto(msg) => WalletError::CryptoError { msg },
            CoreError::Network(ne) => WalletError::NetworkError {
                msg: ne.to_string(),
            },
            CoreError::Storage(msg) => WalletError::StorageError { msg },
            CoreError::Platform(pe) => WalletError::StorageError {
                msg: pe.to_string(),
            },
            CoreError::WalletNotInitialized => WalletError::NotInitialized,
            CoreError::WalletLocked => WalletError::WalletLocked,
            CoreError::InsufficientBalance { .. } => WalletError::InsufficientBalance,
            CoreError::InvalidAnchor => WalletError::InvalidAnchor,
            CoreError::BroadcastFailed(msg) => WalletError::BroadcastFailed { msg },
            CoreError::SyncInProgress => WalletError::SyncInProgress,
            CoreError::RepairInProgress => WalletError::SyncInProgress,
            CoreError::RuntimeNotInitialized => WalletError::NotInitialized,
            CoreError::RuntimeShutdown => WalletError::RuntimeError {
                msg: "Runtime shut down".into(),
            },
            CoreError::RuntimeError(msg) => WalletError::RuntimeError { msg },
            CoreError::BroadcastingInProgress => WalletError::SyncInProgress,
            CoreError::GapFillInProgress => WalletError::SyncInProgress,
            CoreError::InvalidWitness(msg) => WalletError::CryptoError { msg },
            CoreError::ProverNotInitialized => WalletError::NotInitialized,
            CoreError::TransactionBuildFailed(msg) => WalletError::CryptoError { msg },
        }
    }
}

impl From<zipherx_tor::TorError> for WalletError {
    fn from(e: zipherx_tor::TorError) -> Self {
        WalletError::NetworkError { msg: e.to_string() }
    }
}

// ============================================================================
// Data Types (must match UDL dictionaries)
// ============================================================================

pub struct BalanceInfo {
    pub total: u64,
    pub spendable: u64,
    pub note_count: u32,
    pub spendable_note_count: u32,
}

pub struct NoteInfo {
    pub id: i64,
    /// RF-19: Accepts i64 to detect negative DB corruption at the FFI boundary.
    /// Negative values are logged as warnings and the note is skipped.
    pub value: i64,
    pub is_spent: bool,
    pub has_witness: bool,
    pub has_anchor: bool,
}

pub struct BoostScanResultFFI {
    pub total_received: u64,
    pub total_spent: u64,
    pub unspent_balance: u64,
    pub notes_found: u32,
    pub notes_spent: u32,
    pub spends_checked: u32,
}

pub struct WalletConfigFFI {
    pub db_path: String,
    pub header_store_path: String,
    pub delta_store_dir: String,
    pub spend_params_path: String,
    pub output_params_path: String,
    pub account_index: u32,
    /// Optional 32-byte encryption key for SQLCipher database encryption.
    pub db_encryption_key: Option<Vec<u8>>,
    /// Optional directory for boost file cache (large files: 2-4 GB).
    /// On Android, should point to external storage to avoid filling internal storage.
    pub boost_cache_dir: Option<String>,
}

pub struct WalletSummaryFFI {
    pub state: String,
    pub address: Option<String>,
    pub total_balance: u64,
    pub spendable_balance: u64,
    pub note_count: u32,
    pub last_synced_height: u64,
    pub chain_tip: u64,
    pub startup_mode: Option<String>,
    pub sync_phase: String,
}

pub struct TransactionDisplayFFI {
    pub txid: String,
    pub tx_type: String,
    pub amount: u64,
    pub fee: u64,
    pub address: Option<String>,
    pub memo: Option<String>,
    pub confirmations: u64,
    pub height: u64,
    pub timestamp: u64,
}

pub struct TransactionCountsFFI {
    pub sent_count: u32,
    pub received_count: u32,
}

pub struct SendResultFFI {
    pub txid: String,
    pub amount: u64,
    pub fee: u64,
    pub change_value: u64,
    pub notes_used: u32,
}

pub struct ConnectedPeerInfoFFI {
    pub address: String,
    pub protocol_version: u32,
    pub user_agent: String,
    pub start_height: u32,
}

pub struct BannedPeerInfoFFI {
    pub host: String,
    pub reason: String,
    pub is_permanent: bool,
    pub remaining_seconds: u64,
}

pub struct TransparentUtxoFFI {
    pub txid: String,
    pub output_index: u32,
    pub address: String,
    pub value: u64,
    pub height: u64,
    pub is_change: bool,
}

// ============================================================================
// Data Types — Funded Transparent Key Export & WIF Import
// ============================================================================

pub struct FundedTransparentKeyFFI {
    pub address: String,
    pub wif: String,
    pub balance: u64,
    pub is_change: bool,
    pub is_imported: bool,
}

pub struct WifValidationResultFFI {
    pub valid: bool,
    pub address: String,
    pub error_message: String,
}

pub struct WifImportEntryFFI {
    pub address: String,
}

pub struct WifImportErrorFFI {
    pub address: String,
    pub error_message: String,
}

pub struct WifImportResultFFI {
    pub imported: Vec<WifImportEntryFFI>,
    pub errors: Vec<WifImportErrorFFI>,
    pub duplicates: Vec<String>,
}

// ============================================================================
// Callback Interfaces (must match UDL callback interfaces)
// ============================================================================

pub trait SyncProgressCallback: Send + Sync {
    fn on_progress(&self, phase: String, current: u64, target: u64);
    fn on_complete(&self, height: u64);
    fn on_error(&self, message: String);
    fn on_mempool_tx(&self, txid: String, amount: u64);
}

pub trait SendProgressCallback: Send + Sync {
    fn on_phase(&self, phase: String, current: u32, total: u32);
    fn on_complete(&self, txid: String, amount: u64, fee: u64);
    fn on_error(&self, message: String);
}

pub trait PlatformStorageCallback: Send + Sync {
    fn load_key(&self, key: String) -> Option<Vec<u8>>;
    fn store_key(&self, key: String, value: Vec<u8>) -> bool;
    fn delete_key(&self, key: String) -> bool;
    fn has_key(&self, key: String) -> bool;
}

// ============================================================================
// Global State
// ============================================================================

/// Global wallet instance. Uses `OnceLock` which can only be set once per process.
/// **Limitation:** Once initialized, the wallet cannot be replaced or reset.
/// To switch wallets, the process must be restarted. A future `close_wallet()`
/// function would require replacing `OnceLock` with a `Mutex<Option<...>>`.
static WALLET: OnceLock<AsyncWallet> = OnceLock::new();
static PLATFORM_STORAGE: Mutex<Option<Box<dyn PlatformStorageCallback>>> = Mutex::new(None);
static TOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// Concurrency guard: prevents concurrent `send_with_progress` calls.
static IS_SENDING: AtomicBool = AtomicBool::new(false);

/// RF-23: Cached sync progress (f64 stored as u64 bits). Updated by sync callback.
static SYNC_PROGRESS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// When true, the background sync loop uses a 10s interval instead of 30s
/// to detect TX confirmations faster. Set by mobile platforms after broadcast,
/// cleared when confirmation is detected.
static PENDING_TX_FAST_POLL: AtomicBool = AtomicBool::new(false);

/// RF-25: Guard to prevent concurrent `initialize_wallet` calls.
/// `OnceLock::set` is safe, but two threads can race past the `is_some()`
/// check and both attempt heavyweight wallet initialization. This atomic
/// ensures only one thread enters the initialization path.
static WALLET_INITIALIZING: AtomicBool = AtomicBool::new(false);

fn get_wallet() -> Result<&'static AsyncWallet, WalletError> {
    WALLET.get().ok_or(WalletError::NotInitialized)
}

// ============================================================================
// Phase 1: Namespace Functions (crypto, mnemonic, address, balance)
// ============================================================================

/// Get the library version.
fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Generate a new 24-word BIP39 mnemonic phrase.
fn generate_mnemonic() -> Result<String, WalletError> {
    zipherx_crypto::mnemonic::generate()
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Convert a mnemonic phrase to a 64-byte seed.
fn mnemonic_to_seed(phrase: String) -> Result<Vec<u8>, WalletError> {
    zipherx_crypto::mnemonic::to_seed(&phrase)
        .map(|seed| seed.to_vec())
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Validate a BIP39 mnemonic phrase.
fn validate_mnemonic(phrase: String) -> bool {
    zipherx_crypto::mnemonic::validate(&phrase)
}

/// Derive a spending key from a seed.
///
/// # Security
/// The returned bytes are secret key material. Callers MUST zero the
/// corresponding `ByteArray` / `Data` immediately after use to limit
/// exposure in memory.
fn derive_spending_key(seed: Vec<u8>, account_index: u32) -> Result<Vec<u8>, WalletError> {
    let seed = SecureVec(seed);
    zipherx_crypto::keys::derive_spending_key(&seed, account_index)
        .map(|z| z.to_vec()) // Unwrap Zeroizing for FFI; the Zeroizing wrapper zeros the original on drop
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Derive a shielded payment address from a spending key.
///
/// # Security
/// `sk_bytes` is secret key material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use.
fn derive_address(sk_bytes: Vec<u8>, diversifier_index: u64) -> Result<String, WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    let (addr_bytes, _actual_index) =
        zipherx_crypto::keys::derive_address(&sk_bytes, diversifier_index)
            .map_err(|e| WalletError::CryptoError { msg: e.to_string() })?;
    zipherx_crypto::address::encode_address(&addr_bytes)
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Decode a bech32-encoded spending key (secret-extended-key-main1...).
fn decode_spending_key(encoded: String) -> Result<Vec<u8>, WalletError> {
    zipherx_crypto::keys::decode_spending_key(&encoded)
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Encode a raw spending key to bech32 format.
///
/// # Security
/// `sk_bytes` is secret key material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use. The returned bech32 string
/// also encodes the full spending key and should be treated as a secret.
fn encode_spending_key(sk_bytes: Vec<u8>) -> Result<String, WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    zipherx_crypto::keys::encode_spending_key(&sk_bytes)
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Validate a Zclassic shielded address.
fn validate_address(address: String) -> bool {
    zipherx_crypto::address::validate_address(&address)
}

/// Compute balance from a list of notes (FIX #1210: total includes all unspent).
fn compute_balance_from_notes(notes: Vec<NoteInfo>) -> Result<BalanceInfo, WalletError> {
    let mut total: u64 = 0;
    let mut spendable: u64 = 0;
    let mut note_count: u32 = 0;
    let mut spendable_count: u32 = 0;

    for note in &notes {
        // RF-19: Detect negative note values (possible DB corruption).
        // Skip the note and log a warning instead of erroring the entire computation.
        if note.value < 0 {
            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] WARNING: note id={} has negative value {} — possible DB corruption, skipping",
                note.id, note.value,
            );
            continue;
        }
        let value = note.value as u64;

        if !note.is_spent {
            total = total.saturating_add(value);
            note_count = note_count.saturating_add(1);
            if note.has_witness && note.has_anchor {
                spendable = spendable.saturating_add(value);
                spendable_count = spendable_count.saturating_add(1);
            }
        }
    }

    Ok(BalanceInfo {
        total,
        spendable,
        note_count,
        spendable_note_count: spendable_count,
    })
}

/// Validate a send request before building the transaction.
fn validate_send_request_params(
    to_address: String,
    amount: u64,
    fee: u64,
    memo: Option<String>,
) -> Result<(), WalletError> {
    // RF-20: Enforce Sapling memo field limit (512 bytes) at FFI boundary.
    if let Some(ref m) = memo {
        if m.len() > 512 {
            return Err(WalletError::InvalidInput {
                msg: format!("Memo exceeds 512-byte limit ({} bytes)", m.len()),
            });
        }
    }

    let request = SendRequest {
        to_address,
        amount_zatoshis: amount,
        fee_zatoshis: fee,
        memo,
    };
    zipherx_core::send::validate_send_request(&request)
        .map_err(|e| WalletError::InvalidInput { msg: e.to_string() })
}

/// Scan boost file outputs for wallet notes (Rayon parallel).
fn scan_boost_outputs(
    sk_bytes: Vec<u8>,
    outputs_data: Vec<u8>,
    spends_data: Vec<u8>,
) -> Result<BoostScanResultFFI, WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    let (result, _notes) =
        zipherx_crypto::boost_scan::scan_boost_outputs(&sk_bytes, &outputs_data, &spends_data)
            .map_err(|e| WalletError::CryptoError { msg: e.to_string() })?;

    Ok(BoostScanResultFFI {
        total_received: result.total_received,
        total_spent: result.total_spent,
        unspent_balance: result.unspent_balance,
        notes_found: result.notes_found,
        notes_spent: result.notes_spent,
        spends_checked: result.spends_checked,
    })
}

// ============================================================================
// Phase 9: Runtime
// ============================================================================

/// Initialize the global tokio runtime.
fn initialize_runtime() -> Result<(), WalletError> {
    runtime::initialize_runtime().map_err(WalletError::from)
}

/// Shut down the runtime (no new tasks accepted).
fn shutdown_runtime() {
    runtime::shutdown_runtime();
}

/// Check if the runtime is ready.
fn is_runtime_ready() -> bool {
    runtime::is_runtime_ready()
}

// ============================================================================
// Phase 9: Wallet Lifecycle
// ============================================================================

/// Initialize the wallet with the given configuration.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn initialize_wallet(config: WalletConfigFFI) -> Result<(), WalletError> {
    // RF-25: Prevent double initialization and concurrent init races.
    if WALLET.get().is_some() {
        return Ok(()); // Already initialized
    }
    if WALLET_INITIALIZING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // Another thread is already initializing — return Ok to avoid error noise
        return Ok(());
    }

    // RF-9: Validate db_encryption_key length before proceeding
    if let Some(ref key) = config.db_encryption_key {
        if !key.is_empty() && key.len() != 32 {
            WALLET_INITIALIZING.store(false, Ordering::SeqCst);
            return Err(WalletError::InvalidInput {
                msg: format!(
                    "db_encryption_key must be exactly 32 bytes, got {}",
                    key.len()
                ),
            });
        }
    }

    // Security: Wrap db_encryption_key in Zeroizing so it is zeroed on drop
    // (e.g., if initialization fails). The Vec is moved into WalletConfig via
    // Zeroizing::into_inner() — no copy, the Zeroizing guard transfers ownership.
    // NOTE: UniFFI deserialization may leave copies of the key in its internal
    // buffers; we cannot zeroize those from here.
    let mut config = config;
    let db_key = config.db_encryption_key.take();
    let wallet_config = WalletConfig {
        db_path: config.db_path,
        header_store_path: config.header_store_path,
        delta_store_dir: config.delta_store_dir,
        spend_params_path: config.spend_params_path,
        output_params_path: config.output_params_path,
        account_index: config.account_index,
        db_encryption_key: db_key,
        boost_cache_dir: config.boost_cache_dir,
    };

    let wallet = match runtime::block_on(AsyncWallet::initialize(wallet_config))
        .map_err(WalletError::from)
        .and_then(|r| r.map_err(WalletError::from))
    {
        Ok(w) => w,
        Err(e) => {
            WALLET_INITIALIZING.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    // If spending key exists in platform storage, wallet is Ready (not Uninitialized)
    let has_key = match PLATFORM_STORAGE.lock() {
        Ok(guard) => guard
            .as_ref()
            .and_then(|s| s.load_key("spending_key".to_string()))
            .map(|k| !k.is_empty())
            .unwrap_or(false),
        Err(_) => false,
    };
    if has_key {
        wallet
            .core
            .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
    }

    let _ = WALLET.set(wallet);
    // Leave WALLET_INITIALIZING=true — wallet is now set, future calls exit early via is_some()
    Ok(())
}

/// Check if the wallet is initialized.
fn is_wallet_initialized() -> bool {
    WALLET.get().is_some()
}

/// Create a new wallet, returning the 24-word mnemonic.
///
/// # Security
/// The returned word list IS the wallet's master secret. Callers MUST
/// minimize the lifetime of this data in memory — display it to the user
/// once, then discard the list as soon as possible. Do NOT persist the
/// mnemonic in plaintext logs, preferences, or analytics.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn create_wallet_new() -> Result<Vec<String>, WalletError> {
    let wallet = get_wallet()?;
    let words = wallet.create_wallet().map_err(WalletError::from)?;
    wallet
        .core
        .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
    Ok(words)
}

/// Restore a wallet from a mnemonic phrase.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn restore_wallet(words: Vec<String>) -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    wallet.restore_wallet(&words).map_err(WalletError::from)?;
    wallet
        .core
        .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
    Ok(())
}

/// Import a wallet from raw spending key bytes.
///
/// # Security
/// `sk_bytes` is secret key material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn import_wallet_from_key(sk_bytes: Vec<u8>) -> Result<(), WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    let wallet = get_wallet()?;
    wallet
        .import_wallet_from_key(&sk_bytes)
        .map_err(WalletError::from)?;
    wallet
        .core
        .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
    Ok(())
}

/// Get the number of connected peers (lock-free atomic read).
fn get_connected_peer_count() -> Result<u32, WalletError> {
    let wallet = get_wallet()?;
    Ok(wallet.get_connected_peer_count())
}

/// Get the wallet's shielded address.
///
/// # Security
/// `sk_bytes` is secret key material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_wallet_address(sk_bytes: Vec<u8>) -> Result<String, WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    let wallet = get_wallet()?;
    wallet.get_address(&sk_bytes).map_err(WalletError::from)
}

/// Get the current balance.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_balance() -> Result<BalanceInfo, WalletError> {
    #[cfg(debug_assertions)]
    eprintln!("[ZipherX] FFI get_balance() called");
    let wallet = get_wallet()?;
    let balance = runtime::block_on(wallet.get_balance())
        .map_err(|e| {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] FFI get_balance() runtime error: {e}");
            WalletError::from(e)
        })?
        .map_err(|e| {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] FFI get_balance() wallet error: {e}");
            WalletError::from(e)
        })?;

    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] FFI get_balance() → total={}, spendable={}, notes={}, spendable_notes={}",
        balance.total, balance.spendable, balance.note_count, balance.spendable_note_count,
    );

    Ok(BalanceInfo {
        total: balance.total,
        spendable: balance.spendable,
        note_count: u32::try_from(balance.note_count).unwrap_or(u32::MAX),
        spendable_note_count: u32::try_from(balance.spendable_note_count).unwrap_or(u32::MAX),
    })
}

/// Get a summary of the wallet state.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_wallet_summary() -> Result<WalletSummaryFFI, WalletError> {
    let wallet = get_wallet()?;
    let summary = runtime::block_on(wallet.get_summary())
        .map_err(WalletError::from)?
        .map_err(WalletError::from)?;

    let state_str = format!("{:?}", summary.state);
    let startup_str = summary.startup_mode.map(|m| format!("{:?}", m));
    let sync_phase = sync_status_to_phase(&summary.sync_status);
    let (total_balance, spendable_balance, note_count) = if let Some(ref b) = summary.balance {
        (
            b.total,
            b.spendable,
            u32::try_from(b.note_count).unwrap_or(u32::MAX),
        )
    } else {
        (0, 0, 0)
    };

    Ok(WalletSummaryFFI {
        state: state_str,
        address: summary.address,
        total_balance,
        spendable_balance,
        note_count,
        last_synced_height: summary.last_synced_height,
        chain_tip: summary.chain_tip,
        startup_mode: startup_str,
        sync_phase,
    })
}

/// Get transaction history.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_transaction_history(
    limit: u32,
    offset: u32,
) -> Result<Vec<TransactionDisplayFFI>, WalletError> {
    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] FFI get_transaction_history(limit={}, offset={}) called",
        limit, offset
    );
    let wallet = get_wallet()?;
    let records =
        runtime::block_on(wallet.get_transaction_history(limit as usize, offset as usize))
            .map_err(|e| {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX] FFI get_transaction_history() runtime error: {e}");
                WalletError::from(e)
            })?
            .map_err(|e| {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX] FFI get_transaction_history() wallet error: {e}");
                WalletError::from(e)
            })?;

    #[cfg(debug_assertions)]
    {
        let sent_count = records.iter().filter(|r| r.tx_type == "sent").count();
        let recv_count = records.iter().filter(|r| r.tx_type == "received").count();
        let other_count = records.len() - sent_count - recv_count;
        eprintln!(
            "[ZipherX] FFI get_transaction_history() → {} records ({} received, {} sent, {} other)",
            records.len(),
            recv_count,
            sent_count,
            other_count,
        );
        for (i, r) in records.iter().take(5).enumerate() {
            eprintln!(
                "[ZipherX]   tx[{}]: type={}, amount={}, height={}, txid={}...",
                i,
                r.tx_type,
                r.amount,
                r.height,
                &r.txid[..16.min(r.txid.len())],
            );
        }
    }

    Ok(records
        .into_iter()
        .map(|r| TransactionDisplayFFI {
            txid: r.txid,
            tx_type: r.tx_type,
            amount: r.amount,
            fee: r.fee,
            address: r.address,
            memo: r.memo,
            confirmations: r.confirmations,
            height: r.height,
            timestamp: r.timestamp,
        })
        .collect())
}

/// Get total IN (received) and OUT (sent) transaction counts.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_transaction_counts() -> Result<TransactionCountsFFI, WalletError> {
    let wallet = get_wallet()?;
    let (sent, received) = runtime::block_on(wallet.get_transaction_counts())
        .map_err(WalletError::from)?
        .map_err(WalletError::from)?;
    Ok(TransactionCountsFFI {
        sent_count: sent,
        received_count: received,
    })
}

// ============================================================================
// Phase 9: Sync
// ============================================================================

/// Handle for the combined initial-sync + background-monitoring task.
/// `stop_sync()` aborts this to cancel both phases.
///
/// NOTE: If a panic occurs while this mutex is held, subsequent `lock()`
/// calls will return `Err(PoisonError)`. The current code gracefully
/// degrades (skips abort / store) but sync may not be stoppable until
/// the process is restarted.
static SYNC_TASK: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

/// Start syncing the wallet. Progress is reported via callback.
///
/// Phase 1: Initial sync with full progress reporting (header sync, delta,
/// boost scan, block scan). Calls `on_complete(height)` when done.
///
/// Phase 2: Background monitoring loop — re-syncs every 75 seconds
/// (= 1 Zclassic block time). Only calls `on_complete` when new blocks
/// are found, which triggers UI balance + history refresh.
///
/// Loads the spending key from platform storage (Keychain) to enable
/// trial decryption during block scan. If no key is stored, only
/// headers + delta download run (no note discovery).
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn start_sync(callback: Box<dyn SyncProgressCallback>) -> Result<(), WalletError> {
    // RF-18: When TOR_ONLY_MODE is enabled, verify Tor is ready before any
    // network operation. Prevents boost download and sync from bypassing Tor.
    if zipherx_tor::client::is_tor_only_mode() {
        if !zipherx_tor::client::is_socks_running() {
            return Err(WalletError::NetworkError {
                msg: "Tor-only mode is enabled but Tor SOCKS5 proxy is not running. \
                      Start Tor before syncing."
                    .into(),
            });
        }
    }

    let wallet = get_wallet()?;

    // Abort any existing sync + background task
    // NOTE: If SYNC_TASK mutex is poisoned, we skip the abort and proceed —
    // the old task may still be running but we cannot stop it.
    if let Ok(mut guard) = SYNC_TASK.lock() {
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }

    // ── Tor initialization (opt-in, disabled by default) ──────────────
    // When enabled: detect system Tor SOCKS5 proxy + init hidden service.
    // If a verified SOCKS5 proxy is found on 9050/9150/9250, route
    // all P2P through Tor. Otherwise, fall back to direct connections.
    // DNS discovery is also skipped when Tor active (prevents DNS leak).
    if is_tor_enabled() {
        // Start Tor (idempotent if already running).
        // RF-13: `start_tor` returns `Result<Result<u16, TorError>, CoreError>`.
        // The outer Result comes from `runtime::block_on` (runtime errors),
        // the inner Result comes from the Tor client itself. Both are matched
        // here to surface the correct error path.
        match runtime::block_on(zipherx_tor::client::start_tor(None)) {
            Ok(Ok(socks_port)) => {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] Tor started, SOCKS5 port: {socks_port}");

                // Init hidden service (.onion address generation)
                let tor_dir = zipherx_tor::client::get_tor_data_dir();
                match zipherx_tor::hidden_service::init_hidden_service(tor_dir) {
                    Ok(onion) => {
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX-Tor] Onion address: {onion}");
                        let _ = onion;
                    }
                    Err(e) => {
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX-Tor] Hidden service init failed: {e}");
                        let _ = e;
                    }
                }

                // Probe SOCKS5 proxy — only route through Tor if it's actually listening
                let proxy_addr = std::net::SocketAddr::from(([127, 0, 0, 1], socks_port));
                let probe_result = runtime::block_on(async {
                    tokio::time::timeout(
                        tokio::time::Duration::from_secs(2),
                        tokio::net::TcpStream::connect(proxy_addr),
                    )
                    .await
                });

                match probe_result {
                    Ok(Ok(Ok(_stream))) => {
                        // Real SOCKS5 proxy is listening — route P2P through Tor
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX-Tor] SOCKS5 proxy verified — routing P2P through Tor");
                        if let Ok(Ok(())) = runtime::block_on(async {
                            let mut pm = wallet.peer_manager.lock().await;
                            let config = zipherx_network::peer::Socks5Config { proxy_addr };
                            pm.set_socks5_config(config);
                            Ok::<(), CoreError>(())
                        }) {}
                    }
                    _ => {
                        // Proxy disappeared between start_tor() and probe — fail instead of silent clearnet fallback
                        return Err(WalletError::NetworkError {
                            msg: "Tor is enabled but SOCKS5 proxy is not reachable. Disable Tor or fix Tor configuration.".into(),
                        });
                    }
                }
            }
            Ok(Err(e)) => {
                return Err(WalletError::NetworkError {
                    msg: format!("Tor is enabled but failed to start: {e}. Disable Tor or fix Tor configuration."),
                });
            }
            Err(e) => {
                return Err(WalletError::NetworkError {
                    msg: format!("Tor is enabled but runtime error: {e}. Disable Tor or fix Tor configuration."),
                });
            }
        }
    }

    let callback = Arc::new(callback);

    // Load spending key from Keychain for trial decryption
    let sk_bytes: SecureVec = SecureVec(match PLATFORM_STORAGE.lock() {
        Ok(guard) => guard
            .as_ref()
            .and_then(|s| s.load_key("spending_key".to_string()))
            .unwrap_or_default(),
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("WARNING: PLATFORM_STORAGE mutex poisoned: {:?}", e);
            // Still try to recover from the poisoned lock
            let guard = e.into_inner();
            guard
                .as_ref()
                .and_then(|s| s.load_key("spending_key".to_string()))
                .unwrap_or_default()
        }
    });

    // Load wallet seed for transparent address scanning (BIP-44 derivation).
    // Without the seed, transparent UTXOs won't be discovered during sync.
    let seed_bytes: Option<SecureVec> = match PLATFORM_STORAGE.lock() {
        Ok(guard) => guard
            .as_ref()
            .and_then(|s| s.load_key("wallet_seed".to_string()))
            .map(SecureVec),
        Err(e) => {
            let guard = e.into_inner();
            guard
                .as_ref()
                .and_then(|s| s.load_key("wallet_seed".to_string()))
                .map(SecureVec)
        }
    };

    #[cfg(debug_assertions)]
    eprintln!(
        "[ZipherX] FFI start_sync: sk_bytes={} bytes, seed={} bytes",
        sk_bytes.len(),
        seed_bytes.as_ref().map_or(0, |s| s.len()),
    );

    // RF-6: Warn if spending key is unavailable — sync will proceed but
    // note discovery (trial decryption) will be disabled.
    if sk_bytes.is_empty() {
        let cb = callback.clone();
        cb.on_error("Spending key not available — note discovery disabled".to_string());
    }

    let progress_fn: zipherx_core::async_sync::SyncProgressFn = {
        let cb = callback.clone();
        Arc::new(move |status: SyncStatus| {
            let (phase, current, target) = sync_status_to_progress(&status);
            // RF-23: Update cached sync progress for get_sync_progress()
            let ratio = if target > 0 {
                (current as f64 / target as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            SYNC_PROGRESS.store(ratio.to_bits(), Ordering::Relaxed);
            cb.on_progress(phase, current, target);
        })
    };

    // Set state to Syncing while sync is in progress
    wallet
        .core
        .set_state(zipherx_core::wallet::WalletLifecycleState::Syncing);

    let cb_complete = callback.clone();
    let cb_error = callback.clone();
    let cb_mempool = callback.clone();
    let cb_bg = callback;
    // NOTE: `.to_vec()` on SecureVec goes through Deref<Target=[u8]> and creates
    // an intermediate heap allocation. The new SecureVec wraps it immediately and
    // will zero it on drop. The brief window where the copy exists unprotected
    // is an inherent limitation of Rust's allocator (no guaranteed zeroing of freed
    // memory). Mitigated by the short lifetime and SecureVec's volatile zeroing.
    let sk_bg = SecureVec(sk_bytes.to_vec());
    let seed_bg: Option<SecureVec> = seed_bytes.as_ref().map(|s| SecureVec(s.to_vec()));

    // Event-driven mempool detection: set callback on peer manager BEFORE sync.
    // Block listeners handle inv→getdata→tx internally and fire this callback
    // with raw TX bytes. Trial decryption happens synchronously in the callback.
    {
        let sk_mempool = sk_bg.0.clone();
        let cb_mp = cb_mempool.clone();
        // Build transparent address set with seed-derived + imported addresses,
        // matching the egui setup (sync.rs setup_mempool_detector).
        let addr_set = {
            use zipherx_core::scanner::TransparentAddressSet;

            let mut set = if let Some(ref seed) = seed_bg {
                TransparentAddressSet::from_seed(&seed.0, 0, 20)
            } else {
                TransparentAddressSet::empty()
            };

            // Load imported transparent addresses from DB
            let db = wallet.db.clone();
            let imported = runtime::block_on(async {
                tokio::task::spawn_blocking(move || db.get_imported_transparent_addresses())
                    .await
                    .unwrap_or(Err(zipherx_storage::types::StorageError::QueryFailed(
                        "spawn_blocking failed".into(),
                    )))
            })
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();
            for (db_id, addr) in &imported {
                set.add_imported(addr.clone(), *db_id);
            }
            if !imported.is_empty() {
                eprintln!(
                    "[ZipherX] FFI mempool detector: {} imported transparent addresses added",
                    imported.len()
                );
            }

            if set.addresses().is_empty() && set.imported_addresses().is_empty() {
                None
            } else {
                Some(set)
            }
        };
        let detector = if let Some(addr_set) = addr_set {
            zipherx_core::mempool_monitor::MempoolDetector::new_with_address_set(
                sk_mempool,
                addr_set,
                std::sync::Arc::new(move |info: zipherx_core::mempool_monitor::MempoolTxInfo| {
                    cb_mp.on_mempool_tx(info.txid, info.amount);
                }),
            )
        } else {
            zipherx_core::mempool_monitor::MempoolDetector::new(
                sk_mempool,
                std::sync::Arc::new(move |info: zipherx_core::mempool_monitor::MempoolTxInfo| {
                    cb_mp.on_mempool_tx(info.txid, info.amount);
                }),
            )
        };
        let mempool_callback = detector.into_callback();
        runtime::block_on(async {
            let mut pm = wallet.peer_manager.lock().await;
            pm.set_on_mempool_tx_data(mempool_callback);
        })
        .ok();
    }

    // New-block notification: when any peer sends inv MSG_BLOCK, wake the
    // background sync loop immediately instead of waiting for the 30s timer.
    let new_block_notify = std::sync::Arc::new(tokio::sync::Notify::new());
    {
        let notify = new_block_notify.clone();
        runtime::block_on(async {
            let mut pm = wallet.peer_manager.lock().await;
            pm.set_on_new_block(std::sync::Arc::new(move || {
                notify.notify_one();
            }));
        })
        .ok();
    }

    #[cfg(debug_assertions)]
    eprintln!("[ZipherX] FFI: mempool detector + new-block notify set (event-driven)");

    let handle = runtime::spawn(async move {
        // ── Pause Tor during initial sync (boost download + delta sync) ──
        // The boost file is ~2 GB from GitHub — downloading through Tor's
        // SOCKS5 proxy caps throughput at ~1 MB/s. Pausing Tor lets the
        // download run at full speed. Tor is restarted after sync completes
        // so all subsequent P2P traffic is still routed through Tor.
        // RF-18: NEVER pause Tor when TOR_ONLY_MODE is active — user explicitly
        // requested all traffic go through Tor, even at the cost of slower sync.
        let tor_only = zipherx_tor::client::is_tor_only_mode();
        let tor_was_running =
            !tor_only && is_tor_enabled() && zipherx_tor::client::is_socks_running();
        if tor_was_running {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Pausing Tor for boost download (direct connection for speed)");
            let _ = zipherx_tor::client::stop_tor().await;
            // Clear SOCKS5 config so P2P connects direct during sync
            {
                let mut pm = wallet.peer_manager.lock().await;
                pm.clear_socks5_config();
            }
        }

        // Phase 1: Initial sync with progress UI
        // Use sync_with_transparent when seed is available (enables transparent
        // UTXO discovery via BIP-44 address derivation from seed).
        let sync_result = if let Some(ref seed) = seed_bg {
            wallet
                .sync_with_transparent(&sk_bg, seed, Some(progress_fn))
                .await
        } else {
            wallet.sync(&sk_bg, Some(progress_fn)).await
        };

        // ── Restore Tor after initial sync ──────────────────────────────
        // This block runs on both sync success AND failure so that P2P
        // traffic is never left on clearnet when the user enabled Tor.
        // NOTE: If sync panics (e.g. overflow with overflow-checks=true),
        // the entire async task aborts and this block is skipped. The
        // release profile sets panic="abort" so a panic terminates the
        // process anyway, making the Tor-restart moot in that case.
        if tor_was_running {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Initial sync complete — restarting Tor");
            // RF-13: Unlike the outer `start_tor` call which goes through
            // `runtime::block_on` (returning `Result<Result<..>>>`), this call
            // is already inside an async block so it returns a single
            // `Result<u16, TorError>` directly.
            match zipherx_tor::client::start_tor(None).await {
                Ok(socks_port) => {
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] Tor restarted, SOCKS5 port: {socks_port}");
                    // Re-apply SOCKS5 config to PeerManager
                    let proxy_addr = std::net::SocketAddr::from(([127, 0, 0, 1], socks_port));
                    {
                        let mut pm = wallet.peer_manager.lock().await;
                        let config = zipherx_network::peer::Socks5Config { proxy_addr };
                        pm.set_socks5_config(config);
                    }
                }
                Err(e) => {
                    // SECURITY: Tor restart failed — P2P traffic will continue
                    // over clearnet. Notify the user via the error callback so
                    // they are aware of the privacy degradation.
                    let msg =
                        format!("Tor restart failed after sync: {e} — P2P continues without Tor");
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] {msg}");
                    cb_error.on_error(msg);
                }
            }
        }

        match sync_result {
            Ok(height) => {
                // Set state back to Ready after successful sync
                wallet
                    .core
                    .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
                #[cfg(debug_assertions)]
                eprintln!(
                    "[ZipherX] FFI: sync succeeded, calling on_complete({})",
                    height
                );
                cb_complete.on_complete(height);

                // Auto-repair: if notes are missing witnesses after initial sync,
                // clear tree state + witnesses via repair_database() then re-sync.
                // The repair sync rebuilds witnesses from boost/delta data.
                if let Ok(balance) = wallet.get_balance().await {
                    if balance.note_count > 0
                        && balance.note_count > balance.spendable_note_count
                    {
                        let missing = balance.note_count - balance.spendable_note_count;
                        eprintln!(
                            "[ZipherX] FFI: {}/{} notes spendable — repairing {} witnesses",
                            balance.spendable_note_count, balance.note_count, missing,
                        );
                        // Inform user that repair is starting
                        cb_complete.on_progress(
                            "repairing_witnesses".into(),
                            0,
                            missing as u64,
                        );
                        match wallet.repair_database().await {
                            Ok(()) => {
                                eprintln!("[ZipherX] FFI: tree state cleared, re-syncing for witness rebuild");
                                // Create progress callback for repair sync so UI shows progress
                                let cb_repair = cb_complete.clone();
                                let repair_progress_fn: zipherx_core::async_sync::SyncProgressFn =
                                    Arc::new(move |status: SyncStatus| {
                                        let (phase, current, target) = sync_status_to_progress(&status);
                                        // Prefix phase with "repair:" so UI can show "Repairing..."
                                        let repair_phase = format!("repair:{}", phase);
                                        cb_repair.on_progress(repair_phase, current, target);
                                    });
                                match if let Some(ref seed) = seed_bg {
                                    wallet.sync_with_transparent(&sk_bg, seed, Some(repair_progress_fn)).await
                                } else {
                                    wallet.sync(&sk_bg, Some(repair_progress_fn)).await
                                } {
                                    Ok(h) => {
                                        eprintln!(
                                            "[ZipherX] FFI: witness repair complete at height {}",
                                            h,
                                        );
                                        cb_complete.on_complete(h);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[ZipherX] FFI: witness repair sync failed: {}",
                                            e,
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "[ZipherX] FFI: repair_database failed: {}",
                                    e,
                                );
                            }
                        }
                    }
                }

                // Phase 2a: Request mempool inventories from all connected peers.
                // BIP 35: "mempool" message tells peers to send inv for ALL their
                // mempool TXs. Without this, peers only announce TXs that arrive
                // AFTER the connection — TXs already in their mempool are silent.
                // The on_mempool_tx_data callback (set before sync) handles
                // inv→getdata→tx→trial-decrypt automatically via block listeners.
                {
                    let pm = wallet.peer_manager.lock().await;
                    pm.request_mempool_from_all().await;
                }

                // Phase 2b: Background monitoring loop (no progress UI)
                // Wakes on: inv MSG_BLOCK (instant), fast-poll 10s, normal 30s
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX] FFI: starting background sync loop");
                let mut last_height = height;
                loop {
                    let interval = if PENDING_TX_FAST_POLL.load(Ordering::Relaxed) {
                        10
                    } else {
                        30
                    };
                    // Wait for EITHER the timer OR a new-block notification from peers.
                    // When a peer sends inv MSG_BLOCK, new_block_notify fires and we
                    // sync immediately instead of waiting for the full interval.
                    let inv_triggered = tokio::select! {
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(interval)) => false,
                        _ = new_block_notify.notified() => {
                            #[cfg(debug_assertions)]
                            eprintln!("[ZipherX] FFI: new block announced by peer — instant sync");
                            // Delay to let the block propagate to all peers before syncing.
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            true
                        }
                    };

                    let h_before = last_height;

                    match if let Some(ref seed) = seed_bg {
                        wallet.sync_with_transparent(&sk_bg, seed, None).await
                    } else {
                        wallet.sync(&sk_bg, None).await
                    } {
                        Ok(h) => {
                            // Request mempool after each sync — sync may reconnect
                            // peers (zombie detection), and new peers need a fresh
                            // BIP 35 mempool request to announce their mempool TXs.
                            {
                                let pm = wallet.peer_manager.lock().await;
                                pm.request_mempool_from_all().await;
                            }
                            if h > last_height {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "[ZipherX] FFI: background sync found new blocks ({} → {})",
                                    last_height, h,
                                );
                                last_height = h;
                                cb_bg.on_complete(h);
                            } else {
                                #[cfg(debug_assertions)]
                                eprintln!(
                                    "[ZipherX] FFI: background sync — no new blocks (height={})",
                                    h,
                                );
                            }
                        }
                        Err(e) => {
                            // Non-fatal: SyncInProgress (manual sync running), network glitch, etc.
                            // Always log (not just debug) so production issues are visible.
                            eprintln!("[ZipherX] FFI: background sync error (non-fatal): {}", e);
                            let _ = e;
                        }
                    }

                    // Retry logic for inv-triggered syncs that found 0 new blocks.
                    // Peers may not have propagated headers yet when the inv arrived.
                    // Matches the egui retry+reconnect escalation for parity.
                    if inv_triggered && last_height <= h_before {
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX] FFI: inv block but no new header — retrying in 10s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                        match if let Some(ref seed) = seed_bg {
                            wallet.sync_with_transparent(&sk_bg, seed, None).await
                        } else {
                            wallet.sync(&sk_bg, None).await
                        } {
                            Ok(h) if h > last_height => {
                                last_height = h;
                                cb_bg.on_complete(h);
                            }
                            _ => {}
                        }

                        // If retry also found nothing, reconnect for fresh peer heights.
                        // Skip if a send is in progress — it needs stable peer connections.
                        if last_height <= h_before
                            && !IS_SENDING.load(std::sync::atomic::Ordering::Relaxed)
                        {
                            eprintln!("[ZipherX] FFI: peers stale after inv — reconnecting");
                            {
                                let mut pm = wallet.peer_manager.lock().await;
                                pm.disconnect_all().await;
                                let _ = pm.connect().await;
                            }
                            // One more sync with fresh peers
                            match if let Some(ref seed) = seed_bg {
                                wallet.sync_with_transparent(&sk_bg, seed, None).await
                            } else {
                                wallet.sync(&sk_bg, None).await
                            } {
                                Ok(h) if h > last_height => {
                                    last_height = h;
                                    cb_bg.on_complete(h);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(e) => {
                // Set state back to Ready on sync failure (don't leave stuck on Syncing)
                wallet
                    .core
                    .set_state(zipherx_core::wallet::WalletLifecycleState::Ready);
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX] FFI: sync FAILED: {}", e);
                cb_error.on_error(e.to_string());
            }
        }
    })
    .map_err(WalletError::from)?;

    // Store handle so stop_sync() can abort it.
    // NOTE: If SYNC_TASK mutex is poisoned, we cannot store the handle and
    // the task will run until it completes or the process exits.
    if let Ok(mut guard) = SYNC_TASK.lock() {
        *guard = Some(handle);
    }

    Ok(())
}

/// Stop the sync + background monitoring task.
fn stop_sync() {
    // NOTE: If SYNC_TASK mutex is poisoned, we cannot abort the running task.
    if let Ok(mut guard) = SYNC_TASK.lock() {
        if let Some(handle) = guard.take() {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] FFI: stop_sync — aborting sync task");
            handle.abort();
        }
    }
}

/// RF-23: Get the current sync progress as a value in [0.0, 1.0].
/// Returns 0.0 when no sync is in progress.
fn get_sync_progress() -> f64 {
    let bits = SYNC_PROGRESS.load(Ordering::Relaxed);
    f64::from_bits(bits).clamp(0.0, 1.0)
}

/// Set fast-poll mode for the background sync loop.
/// Call with `true` after broadcasting a TX (10s interval),
/// `false` once confirmed (back to 30s).
fn set_pending_tx_fast_poll(enabled: bool) {
    PENDING_TX_FAST_POLL.store(enabled, Ordering::Relaxed);
    #[cfg(debug_assertions)]
    eprintln!("[ZipherX] FFI: pending TX fast poll = {enabled}");
}

// ============================================================================
// Phase 9: Send
// ============================================================================

/// Send a shielded transaction. Progress is reported via callback.
///
/// # Security
/// `sk_bytes` is secret key material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use.
///
/// # Concurrency
/// Only one send can be in flight at a time. If a send is already in
/// progress, this function returns `WalletError::SyncInProgress`.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
///
/// TODO (RF-21): This function blocks the calling thread via `runtime::spawn`.
/// A future version should expose a fully async send interface to avoid
/// tying up the caller's thread while the transaction is built and broadcast.
fn send_with_progress(
    to_address: String,
    amount: u64,
    fee: u64,
    memo: Option<String>,
    sk_bytes: Vec<u8>,
    callback: Box<dyn SendProgressCallback>,
) -> Result<(), WalletError> {
    let sk_bytes = SecureVec(sk_bytes);

    // RF-20: Enforce Sapling memo field limit (512 bytes) at FFI boundary.
    if let Some(ref m) = memo {
        if m.len() > 512 {
            return Err(WalletError::InvalidInput {
                msg: format!("Memo exceeds 512-byte limit ({} bytes)", m.len()),
            });
        }
    }

    // Finding 4: Verify Tor proxy when tor-only mode is active
    if zipherx_tor::client::is_tor_only_mode() {
        if !zipherx_tor::client::is_socks_running() {
            return Err(WalletError::NetworkError {
                msg: "Tor-only mode enabled but Tor SOCKS5 proxy is not running".into(),
            });
        }
    }

    // RF-12: Concurrency guard — prevent overlapping sends
    if IS_SENDING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(WalletError::SyncInProgress);
    }

    /// RAII guard that resets `IS_SENDING` to false on drop,
    /// ensuring the flag is always cleared even on early returns or panics.
    struct SendGuard;
    impl Drop for SendGuard {
        fn drop(&mut self) {
            IS_SENDING.store(false, Ordering::SeqCst);
        }
    }

    let wallet = get_wallet().map_err(|e| {
        IS_SENDING.store(false, Ordering::SeqCst);
        e
    })?;
    let callback = Arc::new(callback);

    let request = SendRequest {
        to_address,
        amount_zatoshis: amount,
        fee_zatoshis: fee,
        memo,
    };

    let progress_fn: zipherx_core::async_send::SendProgressFn = {
        let cb = callback.clone();
        Arc::new(move |phase: SendPhase| {
            let (phase_str, current, total) = send_phase_to_progress(&phase);
            cb.on_phase(phase_str, current, total);
        })
    };

    let cb_complete = callback.clone();
    let cb_error = callback;

    runtime::spawn(async move {
        // SendGuard lives for the duration of the async task — dropped on
        // completion (success or error), resetting IS_SENDING to false.
        let _guard = SendGuard;
        match wallet.send(request, &sk_bytes, Some(progress_fn)).await {
            Ok(result) => cb_complete.on_complete(result.txid, result.amount, result.fee),
            Err(e) => cb_error.on_error(e.to_string()),
        }
    })
    .map_err(|e| {
        IS_SENDING.store(false, Ordering::SeqCst);
        WalletError::from(e)
    })?;

    Ok(())
}

/// Send a transparent transaction (spending transparent UTXOs).
///
/// Supports t→t and t→z (shielding). Requires both the spending key
/// (for Sapling change) and the wallet seed (for transparent key derivation).
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn send_transparent_with_progress(
    to_address: String,
    amount: u64,
    fee: u64,
    memo: Option<String>,
    sk_bytes: Vec<u8>,
    seed: Vec<u8>,
    callback: Box<dyn SendProgressCallback>,
) -> Result<(), WalletError> {
    let sk_bytes = SecureVec(sk_bytes);
    let seed_secure = SecureVec(seed);

    if let Some(ref m) = memo {
        if m.len() > 512 {
            return Err(WalletError::InvalidInput {
                msg: format!("Memo exceeds 512-byte limit ({} bytes)", m.len()),
            });
        }
    }

    if zipherx_tor::client::is_tor_only_mode() {
        if !zipherx_tor::client::is_socks_running() {
            return Err(WalletError::NetworkError {
                msg: "Tor-only mode enabled but Tor SOCKS5 proxy is not running".into(),
            });
        }
    }

    if IS_SENDING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(WalletError::SyncInProgress);
    }

    struct SendGuard;
    impl Drop for SendGuard {
        fn drop(&mut self) {
            IS_SENDING.store(false, Ordering::SeqCst);
        }
    }

    let wallet = get_wallet().map_err(|e| {
        IS_SENDING.store(false, Ordering::SeqCst);
        e
    })?;
    let callback = Arc::new(callback);

    let request = SendRequest {
        to_address,
        amount_zatoshis: amount,
        fee_zatoshis: fee,
        memo,
    };

    let progress_fn: zipherx_core::async_send::SendProgressFn = {
        let cb = callback.clone();
        Arc::new(move |phase: SendPhase| {
            let (phase_str, current, total) = send_phase_to_progress(&phase);
            cb.on_phase(phase_str, current, total);
        })
    };

    let cb_complete = callback.clone();
    let cb_error = callback;

    runtime::spawn(async move {
        let _guard = SendGuard;
        match wallet
            .send_transparent(
                request,
                &sk_bytes,
                &seed_secure,
                Some(progress_fn),
                |encrypted_sk: &[u8]| {
                    // Try as raw 32-byte key first (egui stores raw secret bytes)
                    if encrypted_sk.len() == 32 {
                        return Ok(zeroize::Zeroizing::new(encrypted_sk.to_vec()));
                    }
                    // Try as WIF string (Android/iOS store WIF as UTF-8 bytes)
                    if let Ok(wif_str) = std::str::from_utf8(encrypted_sk) {
                        let wif_str = wif_str.trim();
                        if let Ok((sk_bytes, _)) =
                            zipherx_crypto::transparent::decode_wif(wif_str)
                        {
                            return Ok(sk_bytes);
                        }
                    }
                    Err(format!(
                        "Cannot decode imported key ({} bytes)",
                        encrypted_sk.len()
                    ))
                },
            )
            .await
        {
            Ok(result) => cb_complete.on_complete(result.txid, result.amount, result.fee),
            Err(e) => cb_error.on_error(e.to_string()),
        }
    })
    .map_err(|e| {
        IS_SENDING.store(false, Ordering::SeqCst);
        WalletError::from(e)
    })?;

    Ok(())
}

// ============================================================================
// Phase 9: Repair
// ============================================================================

/// Run database repair (clear tree state and rebuild).
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn repair_database() -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(wallet.repair_database())
        .map_err(WalletError::from)?
        .map_err(WalletError::from)
}

/// Full rescan — nuclear reset: delete ALL notes/history/state, rescan from scratch.
///
/// Matches the official ZipherX full rescan approach: deletes all notes (so boost
/// scan re-inserts them with correct positions), clears delta bundle (forces fresh
/// download), and resets scan progress to zero. The next sync re-discovers everything.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn full_rescan() -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(wallet.full_rescan())
        .map_err(WalletError::from)?
        .map_err(WalletError::from)
}

/// Nuclear rescan: clears ALL notes, history, transparent UTXOs, and resets
/// scan state to 0. The next sync will re-download and re-scan everything.
/// Use after WIF import to ensure imported addresses are fully scanned.
fn full_rescan_reset() -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(async {
        let db = wallet.db.clone();
        tokio::task::spawn_blocking(move || db.full_rescan_reset())
            .await
            .map_err(|e| WalletError::StorageError { msg: e.to_string() })?
            .map_err(|e| WalletError::StorageError { msg: e.to_string() })
    })?
}

// ============================================================================
// Phase 9: Tor
// ============================================================================

/// Enable or disable Tor for P2P connections.
/// Tor is disabled by default. Call with `true` to route traffic through Tor.
fn set_tor_enabled(enabled: bool) {
    TOR_ENABLED.store(enabled, Ordering::SeqCst);
    #[cfg(debug_assertions)]
    eprintln!("[ZipherX-Tor] Tor enabled = {enabled}");
}

/// Check whether Tor is enabled.
fn is_tor_enabled() -> bool {
    TOR_ENABLED.load(Ordering::SeqCst)
}

/// Start the embedded Tor client. Returns the SOCKS5 proxy port.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn start_tor_client(data_dir: Option<String>) -> Result<u16, WalletError> {
    let path = data_dir.map(std::path::PathBuf::from);
    runtime::block_on(zipherx_tor::client::start_tor(path))
        .map_err(WalletError::from)?
        .map_err(WalletError::from)
}

/// Stop the Tor client.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn stop_tor_client() -> Result<(), WalletError> {
    runtime::block_on(zipherx_tor::client::stop_tor())
        .map_err(WalletError::from)?
        .map_err(WalletError::from)
}

/// Get the Tor connection state (0=Disconnected, 1=Connecting, 2=Bootstrapping, 3=Connected, 4=Error).
fn get_tor_state() -> u8 {
    zipherx_tor::client::get_state() as u8
}

/// Get the Tor bootstrap progress (0-100).
fn get_tor_bootstrap_progress() -> u8 {
    zipherx_tor::client::get_bootstrap_progress()
}

/// Get the last Tor error message.
fn get_tor_error() -> Option<String> {
    zipherx_tor::client::get_last_error()
}

/// Get the SOCKS5 port of the running Tor client.
fn get_tor_socks_port() -> u16 {
    zipherx_tor::client::get_socks_port()
}

/// Route all P2P traffic through the Tor SOCKS5 proxy.
/// Call AFTER start_tor_client() returns a port.
/// Verifies the proxy is actually listening before configuring PeerManager.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn enable_p2p_tor(socks_port: u16) -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(async {
        // Verify the SOCKS5 proxy is actually reachable
        let proxy_addr = std::net::SocketAddr::from(([127, 0, 0, 1], socks_port));
        let probe = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            tokio::net::TcpStream::connect(proxy_addr),
        )
        .await;

        match probe {
            Ok(Ok(_stream)) => {
                // Proxy is listening — configure SOCKS5
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] SOCKS5 proxy verified at port {socks_port}");
                let mut pm = wallet.peer_manager.lock().await;
                pm.disconnect_all().await;
                let config = zipherx_network::peer::Socks5Config { proxy_addr };
                pm.set_socks5_config(config);
                drop(pm);
                wallet.connect_network().await
            }
            _ => {
                // RF-14: Proxy not reachable — fail instead of silently falling back to direct
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] SOCKS5 proxy NOT reachable at port {socks_port}");
                Err(CoreError::Network(
                    zipherx_network::types::NetworkError::ConnectionFailed(
                        "Tor SOCKS5 proxy not reachable — cannot enable Tor for P2P".into(),
                    ),
                ))
            }
        }
    })
    .map_err(WalletError::from)?
    .map_err(WalletError::from)
}

/// Disable Tor routing — revert to direct P2P connections.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn disable_p2p_tor() -> Result<(), WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.disconnect_all().await;
        pm.clear_socks5_config();
        drop(pm);
        wallet.connect_network().await
    })
    .map_err(WalletError::from)?
    .map_err(WalletError::from)
}

/// Initialize the hidden service and return the .onion address.
fn init_hidden_service(data_dir: Option<String>) -> Result<String, WalletError> {
    let dir = data_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(zipherx_tor::client::get_tor_data_dir);
    zipherx_tor::hidden_service::init_hidden_service(dir).map_err(WalletError::from)
}

/// Get the .onion address (None if not initialized).
fn get_onion_address() -> Option<String> {
    zipherx_tor::hidden_service::get_onion_address()
}

// ============================================================================
// Phase 9: Platform Storage
// ============================================================================

// ============================================================================
// Peer Management
// ============================================================================

/// Get connected peers info.
fn get_connected_peers() -> Result<Vec<ConnectedPeerInfoFFI>, WalletError> {
    let wallet = get_wallet()?;
    let infos = runtime::block_on(async {
        let pm = wallet.peer_manager.lock().await;
        pm.get_connected_peer_infos()
    })
    .map_err(|e| WalletError::RuntimeError { msg: e.to_string() })?;

    Ok(infos
        .into_iter()
        .map(|p| ConnectedPeerInfoFFI {
            address: p.address,
            protocol_version: p.protocol_version,
            user_agent: p.user_agent,
            start_height: p.start_height,
        })
        .collect())
}

/// Get banned peers info.
fn get_banned_peers() -> Result<Vec<BannedPeerInfoFFI>, WalletError> {
    let wallet = get_wallet()?;
    let infos = runtime::block_on(async {
        let pm = wallet.peer_manager.lock().await;
        pm.get_banned_peer_infos()
    })
    .map_err(|e| WalletError::RuntimeError { msg: e.to_string() })?;

    Ok(infos
        .into_iter()
        .map(|p| BannedPeerInfoFFI {
            host: p.host,
            reason: p.reason,
            is_permanent: p.is_permanent,
            remaining_seconds: p.remaining_seconds,
        })
        .collect())
}

/// Add a custom peer. Validates host is a valid IP (no hostnames to prevent DNS leaks).
fn add_custom_peer(host: String, port: u16) -> Result<bool, WalletError> {
    // Validate before acquiring lock (defense in depth)
    let host = host.trim().to_string();
    if host.is_empty() || host.len() > 253 {
        return Err(WalletError::InvalidInput {
            msg: "Invalid host".into(),
        });
    }

    let wallet = get_wallet()?;
    let result = runtime::block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.add_custom_peer(&host, port)
    })
    .map_err(|e| WalletError::RuntimeError { msg: e.to_string() })?;

    result.map_err(|e| WalletError::InvalidInput { msg: e })
}

/// Unban a peer. Returns true if the peer was banned and was removed.
fn unban_peer(host: String) -> Result<bool, WalletError> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err(WalletError::InvalidInput {
            msg: "Host is empty".into(),
        });
    }

    let wallet = get_wallet()?;
    let result = runtime::block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.unban_peer(&host)
    })
    .map_err(|e| WalletError::RuntimeError { msg: e.to_string() })?;

    Ok(result)
}

/// Disconnect a specific peer. Returns true if the peer was connected.
fn disconnect_peer(peer_id: String) -> Result<bool, WalletError> {
    let peer_id = peer_id.trim().to_string();
    if peer_id.is_empty() {
        return Err(WalletError::InvalidInput {
            msg: "Peer ID is empty".into(),
        });
    }

    let wallet = get_wallet()?;
    let result = runtime::block_on(async {
        let mut pm = wallet.peer_manager.lock().await;
        pm.disconnect_peer(&peer_id)
    })
    .map_err(|e| WalletError::RuntimeError { msg: e.to_string() })?;

    Ok(result)
}

/// Set the platform-specific secure storage implementation.
fn set_platform_storage(storage: Box<dyn PlatformStorageCallback>) {
    match PLATFORM_STORAGE.lock() {
        Ok(mut guard) => {
            *guard = Some(storage);
        }
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("WARNING: PLATFORM_STORAGE mutex poisoned: {:?}", e);
            // Still try to recover from the poisoned lock
            let mut guard = e.into_inner();
            *guard = Some(storage);
        }
    }
}

// ============================================================================
// Transparent Addresses
// ============================================================================

/// Derive a transparent (t1...) address from seed.
///
/// # Security
/// `seed` is master seed material. Callers MUST zero the corresponding
/// `ByteArray` / `Data` immediately after use.
fn derive_transparent_address(
    seed: Vec<u8>,
    account_index: u32,
    child_index: u32,
) -> Result<String, WalletError> {
    let seed = SecureVec(seed);
    zipherx_crypto::transparent::derive_transparent_address(&seed, account_index, child_index)
        .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Export the transparent private key in WIF (Wallet Import Format).
///
/// # Security
/// The returned string is a private key. Callers MUST zero the string
/// after displaying it to the user.
fn export_transparent_wif(
    seed: Vec<u8>,
    account_index: u32,
    child_index: u32,
) -> Result<String, WalletError> {
    let seed = SecureVec(seed);
    let wif = zipherx_crypto::transparent::export_transparent_wif(
        &seed,
        account_index,
        child_index,
        false,
    )
    .map_err(|e| WalletError::CryptoError { msg: e.to_string() })?;
    Ok((*wif).clone())
}

/// Derive a transparent change address (internal chain).
fn derive_transparent_change_address(
    seed: Vec<u8>,
    account_index: u32,
    child_index: u32,
) -> Result<String, WalletError> {
    let seed = SecureVec(seed);
    zipherx_crypto::transparent::derive_transparent_change_address(
        &seed,
        account_index,
        child_index,
    )
    .map_err(|e| WalletError::CryptoError { msg: e.to_string() })
}

/// Validate a transparent address (t1... or t3...).
fn validate_transparent_address(address: String) -> bool {
    zipherx_crypto::transparent::validate_transparent_address(&address)
}

/// Get the transparent balance from the wallet database.
fn get_transparent_balance() -> Result<u64, WalletError> {
    let wallet = get_wallet()?;
    runtime::block_on(wallet.get_transparent_balance())
        .map_err(|e| WalletError::from(e))?
        .map_err(|e| WalletError::from(e))
}

/// Get all unspent transparent UTXOs.
fn get_transparent_utxos() -> Result<Vec<TransparentUtxoFFI>, WalletError> {
    let wallet = get_wallet()?;
    let utxos = runtime::block_on(wallet.get_unspent_transparent_utxos())
        .map_err(|e| WalletError::from(e))?
        .map_err(|e| WalletError::from(e))?;

    Ok(utxos
        .into_iter()
        .map(|u| TransparentUtxoFFI {
            txid: hex::encode(&u.txid),
            output_index: u.output_index,
            address: u.address,
            value: u.value,
            height: u.height,
            is_change: u.is_change,
        })
        .collect())
}

/// Get all imported transparent addresses (regardless of balance).
///
/// Unlike `get_transparent_utxos` which only returns addresses with unspent UTXOs,
/// this returns every WIF-imported address. Used by the receive screen to show
/// a valid receive address even when all UTXOs are spent.
fn get_imported_transparent_addresses() -> Result<Vec<String>, WalletError> {
    let wallet = get_wallet()?;
    let db = wallet.db.clone();
    let addrs = runtime::block_on(async {
        tokio::task::spawn_blocking(move || db.get_imported_transparent_addresses())
            .await
            .unwrap_or(Err(zipherx_storage::types::StorageError::QueryFailed(
                "spawn_blocking failed".into(),
            )))
    })
    .map_err(|e| WalletError::from(e))?
    .map_err(|e| WalletError::StorageError {
        msg: e.to_string(),
    })?;
    Ok(addrs.into_iter().map(|(_id, addr)| addr).collect())
}

// ============================================================================
// Funded Transparent Key Export & WIF Import
// ============================================================================

/// Export all funded transparent addresses with their WIF private keys.
///
/// Loads the seed from platform storage and calls async_wallet's
/// export_funded_transparent_wifs. For imported keys, a no-op decrypt_fn
/// is used since mobile platforms pass pre-encrypted keys.
///
/// SECURITY: This function returns raw private keys in WIF format.
/// Callers MUST enforce biometric or password authentication before
/// invoking this function. The FFI layer does not gate access —
/// authentication is the caller's responsibility (Android: biometric
/// prompt in WalletViewModel, iOS: FaceID/TouchID in SettingsView).
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn export_funded_transparent_wifs() -> Result<Vec<FundedTransparentKeyFFI>, WalletError> {
    let wallet = get_wallet()?;

    // Load seed from platform storage
    let seed = match PLATFORM_STORAGE.lock() {
        Ok(guard) => guard
            .as_ref()
            .and_then(|s| s.load_key("wallet_seed".to_string())),
        Err(e) => {
            let guard = e.into_inner();
            guard
                .as_ref()
                .and_then(|s| s.load_key("wallet_seed".to_string()))
        }
    }
    .ok_or_else(|| WalletError::StorageError {
        msg: "Wallet seed not available — cannot export transparent keys".into(),
    })?;
    let seed = SecureVec(seed);

    let keys = runtime::block_on(
        wallet.export_funded_transparent_wifs(&seed, |encrypted_sk| {
            // Try as raw 32-byte key first (egui stores raw secret bytes)
            if encrypted_sk.len() == 32 {
                return Ok(zeroize::Zeroizing::new(encrypted_sk.to_vec()));
            }
            // Try as WIF string (Android/iOS store WIF as UTF-8 bytes)
            if let Ok(wif_str) = std::str::from_utf8(encrypted_sk) {
                let wif_str = wif_str.trim();
                if let Ok((sk_bytes, _)) =
                    zipherx_crypto::transparent::decode_wif(wif_str)
                {
                    return Ok(sk_bytes);
                }
            }
            Err(format!(
                "Cannot decode imported key ({} bytes)",
                encrypted_sk.len()
            ))
        }),
    )
    .map_err(WalletError::from)?
    .map_err(WalletError::from)?;

    Ok(keys
        .into_iter()
        .map(|k| FundedTransparentKeyFFI {
            address: k.address,
            wif: (*k.wif).clone(), // Extract from Zeroizing; original zeroed on drop
            balance: k.balance,
            is_change: k.is_change,
            is_imported: k.is_imported,
        })
        .collect())
}

/// Validate a list of WIF-encoded private keys.
///
/// Returns one result per input WIF: valid keys include the derived address,
/// invalid keys include an error description.
fn validate_wif_keys(wifs: Vec<String>) -> Result<Vec<WifValidationResultFFI>, WalletError> {
    if wifs.len() > 100 {
        return Err(WalletError::InvalidInput {
            msg: "Maximum 100 WIF keys per import".into(),
        });
    }
    let mut results = Vec::with_capacity(wifs.len());
    for wif in &wifs {
        let trimmed = wif.trim();
        if trimmed.is_empty() {
            continue;
        }
        match zipherx_crypto::transparent::decode_wif(trimmed) {
            Ok((_sk_bytes, address)) => {
                results.push(WifValidationResultFFI {
                    valid: true,
                    address,
                    error_message: String::new(),
                });
            }
            Err(e) => {
                results.push(WifValidationResultFFI {
                    valid: false,
                    address: String::new(),
                    error_message: e.to_string(),
                });
            }
        }
    }
    Ok(results)
}

/// Import WIF keys into the wallet database.
///
/// Each (encrypted_key, address) pair is stored in the imported_transparent_keys table.
/// The encrypted_key bytes are provided by the platform after encrypting the raw secret
/// key with the platform's secure storage mechanism.
///
/// Returns a summary of imported keys, errors, and duplicates.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn import_wif_keys(
    encrypted_keys: Vec<Vec<u8>>,
    addresses: Vec<String>,
) -> Result<WifImportResultFFI, WalletError> {
    if encrypted_keys.len() != addresses.len() {
        return Err(WalletError::InvalidInput {
            msg: format!(
                "Mismatched lengths: {} keys vs {} addresses",
                encrypted_keys.len(),
                addresses.len()
            ),
        });
    }

    let wallet = get_wallet()?;
    let db = wallet.db.clone();

    let result = runtime::block_on(async {
        tokio::task::spawn_blocking(move || {
            let mut imported = Vec::new();
            let mut errors = Vec::new();
            let mut duplicates = Vec::new();

            for (enc_key, addr) in encrypted_keys.iter().zip(addresses.iter()) {
                match db.store_imported_transparent_key(addr, enc_key) {
                    Ok(()) => {
                        imported.push(WifImportEntryFFI {
                            address: addr.clone(),
                        });
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        if err_str.contains("UNIQUE") || err_str.contains("duplicate") {
                            duplicates.push(addr.clone());
                        } else {
                            errors.push(WifImportErrorFFI {
                                address: addr.clone(),
                                error_message: err_str,
                            });
                        }
                    }
                }
            }

            Ok::<WifImportResultFFI, WalletError>(WifImportResultFFI {
                imported,
                errors,
                duplicates,
            })
        })
        .await
        .map_err(|e| WalletError::RuntimeError {
            msg: format!("spawn_blocking: {e}"),
        })?
    })
    .map_err(WalletError::from)?;

    result
}

/// Get the number of imported transparent keys in the wallet.
///
/// BLOCKING: This function blocks the calling thread. Call from a background thread.
fn get_imported_key_count() -> Result<u32, WalletError> {
    let wallet = get_wallet()?;
    let db = wallet.db.clone();

    runtime::block_on(async {
        tokio::task::spawn_blocking(move || {
            db.get_imported_key_count()
                .map_err(|e| WalletError::StorageError {
                    msg: e.to_string(),
                })
        })
        .await
        .map_err(|e| WalletError::RuntimeError {
            msg: format!("spawn_blocking: {e}"),
        })?
    })
    .map_err(WalletError::from)?
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Convert SyncStatus to a progress string and values.
fn sync_status_to_progress(status: &SyncStatus) -> (String, u64, u64) {
    match status {
        SyncStatus::Idle => ("idle".into(), 0, 0),
        SyncStatus::BoostDownload {
            downloaded_bytes,
            total_bytes,
        } => ("boost_download".into(), *downloaded_bytes, *total_bytes),
        SyncStatus::BoostLoad { loaded, total } => ("boost_load".into(), *loaded, *total),
        SyncStatus::HeaderSync {
            current_height,
            target_height,
        } => ("header_sync".into(), *current_height, *target_height),
        SyncStatus::DeltaSync {
            current_height,
            target_height,
        } => ("delta_sync".into(), *current_height, *target_height),
        SyncStatus::BoostScan { outputs_total } => ("boost_scan".into(), 0, *outputs_total),
        SyncStatus::BlockScan {
            current_height,
            target_height,
            ..
        } => ("block_scan".into(), *current_height, *target_height),
        SyncStatus::GapFill { gaps_remaining } => (
            "gap_fill".into(),
            u64::try_from(*gaps_remaining).unwrap_or(u64::MAX),
            0,
        ),
        SyncStatus::WitnessUpdate {
            notes_updated,
            total_notes,
        } => (
            "witness_update".into(),
            u64::try_from(*notes_updated).unwrap_or(u64::MAX),
            u64::try_from(*total_notes).unwrap_or(u64::MAX),
        ),
        SyncStatus::BoostFailed { .. } => ("boost_failed".into(), 0, 0),
        SyncStatus::ConfirmationsUpdated { height } => ("confirmations_updated".into(), *height, *height),
        SyncStatus::Complete { height } => ("complete".into(), *height, *height),
        SyncStatus::Failed(_) => ("failed".into(), 0, 0),
    }
}

/// Convert SyncStatus to a phase string for WalletSummaryFFI.
fn sync_status_to_phase(status: &SyncStatus) -> String {
    match status {
        SyncStatus::Idle => "idle".into(),
        SyncStatus::BoostDownload { .. } => "boost_download".into(),
        SyncStatus::BoostLoad { .. } => "boost_load".into(),
        SyncStatus::HeaderSync { .. } => "header_sync".into(),
        SyncStatus::DeltaSync { .. } => "delta_sync".into(),
        SyncStatus::BoostScan { .. } => "boost_scan".into(),
        SyncStatus::BlockScan { .. } => "block_scan".into(),
        SyncStatus::GapFill { .. } => "gap_fill".into(),
        SyncStatus::WitnessUpdate { .. } => "witness_update".into(),
        SyncStatus::BoostFailed { .. } => "boost_failed".into(),
        SyncStatus::ConfirmationsUpdated { .. } => "confirmations_updated".into(),
        SyncStatus::Complete { .. } => "complete".into(),
        SyncStatus::Failed(_) => "failed".into(),
    }
}

/// Convert SendPhase to progress values.
fn send_phase_to_progress(phase: &SendPhase) -> (String, u32, u32) {
    match phase {
        SendPhase::Validating => ("validating".into(), 0, 0),
        SendPhase::NoteSelection { count, .. } => (
            "note_selection".into(),
            u32::try_from(*count).unwrap_or(u32::MAX),
            0,
        ),
        SendPhase::WitnessValidation {
            note_index, total, ..
        } => (
            "witness_validation".into(),
            u32::try_from(*note_index).unwrap_or(u32::MAX),
            u32::try_from(*total).unwrap_or(u32::MAX),
        ),
        SendPhase::Building {
            spend_index,
            total_spends,
        } => ("building".into(), *spend_index, *total_spends),
        SendPhase::Broadcasting => ("broadcasting".into(), 0, 0),
        SendPhase::PeerResponse {
            accepted, total, ..
        } => ("peer_response".into(), *accepted, *total),
        SendPhase::Recording => ("recording".into(), 0, 0),
        SendPhase::Complete { .. } => ("complete".into(), 0, 0),
        SendPhase::Error { .. } => ("error".into(), 0, 0),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_version() {
        let v = get_version();
        assert!(!v.is_empty());
        assert!(v.contains('.'));
    }

    #[test]
    fn test_generate_and_validate_mnemonic() {
        let phrase = generate_mnemonic().unwrap();
        assert!(validate_mnemonic(phrase.clone()));
        assert!(!validate_mnemonic("not a valid phrase".into()));
    }

    #[test]
    fn test_mnemonic_to_seed_and_derive() {
        let phrase = generate_mnemonic().unwrap();
        let seed = mnemonic_to_seed(phrase).unwrap();
        assert_eq!(seed.len(), 64);

        let sk = derive_spending_key(seed, 0).unwrap();
        assert!(!sk.is_empty());

        let addr = derive_address(sk, 0).unwrap();
        assert!(validate_address(addr));
    }

    /// Generate a valid bech32-encoded "zs" address for testing.
    fn test_zs_address() -> String {
        use bech32::ToBase32;
        let dummy_data = vec![0xAAu8; 43]; // 43 bytes = Sapling payment address
        bech32::encode("zs", dummy_data.to_base32(), bech32::Variant::Bech32).unwrap()
    }

    #[test]
    fn test_validate_send_request_params_valid() {
        let addr = test_zs_address();
        let result = validate_send_request_params(addr, 50_000, 10_000, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_send_request_params_invalid() {
        let result = validate_send_request_params("bad".into(), 0, 10_000, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_balance_from_notes_empty() {
        let balance = compute_balance_from_notes(vec![]).unwrap();
        assert_eq!(balance.total, 0);
        assert_eq!(balance.spendable, 0);
    }

    #[test]
    fn test_compute_balance_from_notes_mixed() {
        let notes = vec![
            NoteInfo {
                id: 1,
                value: 100_000,
                is_spent: false,
                has_witness: true,
                has_anchor: true,
            },
            NoteInfo {
                id: 2,
                value: 50_000,
                is_spent: false,
                has_witness: false,
                has_anchor: false,
            },
            NoteInfo {
                id: 3,
                value: 200_000,
                is_spent: true,
                has_witness: true,
                has_anchor: true,
            },
        ];
        let balance = compute_balance_from_notes(notes).unwrap();
        assert_eq!(balance.total, 150_000); // 100k + 50k (spent excluded)
        assert_eq!(balance.spendable, 100_000); // only note with witness+anchor
        assert_eq!(balance.note_count, 2);
        assert_eq!(balance.spendable_note_count, 1);
    }

    #[test]
    fn test_compute_balance_from_notes_negative_skipped() {
        // RF-19: Negative values should be skipped (DB corruption indicator)
        let notes = vec![
            NoteInfo {
                id: 1,
                value: 100_000,
                is_spent: false,
                has_witness: true,
                has_anchor: true,
            },
            NoteInfo {
                id: 2,
                value: -50_000,
                is_spent: false,
                has_witness: true,
                has_anchor: true,
            },
        ];
        let balance = compute_balance_from_notes(notes).unwrap();
        assert_eq!(balance.total, 100_000); // negative note skipped
        assert_eq!(balance.note_count, 1);
    }

    #[test]
    fn test_runtime_init_and_ready() {
        // Runtime may already be initialized from other tests
        let _ = initialize_runtime();
        assert!(is_runtime_ready());
    }

    #[test]
    fn test_wallet_not_initialized() {
        // WALLET is a OnceLock, if not set, get_wallet returns NotInitialized
        // We can't test this reliably since the OnceLock may be set from other tests
        // Just verify the helper function doesn't panic
        let _ = is_wallet_initialized();
    }

    #[test]
    fn test_tor_state_queries() {
        let state = get_tor_state();
        assert!(state <= 4);
        let progress = get_tor_bootstrap_progress();
        assert!(progress <= 100);
        let _ = get_tor_error();
    }

    #[test]
    fn test_tor_enabled_toggle() {
        assert!(!is_tor_enabled());
        set_tor_enabled(true);
        assert!(is_tor_enabled());
        set_tor_enabled(false);
        assert!(!is_tor_enabled());
    }

    #[test]
    fn test_sync_status_to_phase() {
        assert_eq!(sync_status_to_phase(&SyncStatus::Idle), "idle");
        assert_eq!(
            sync_status_to_phase(&SyncStatus::Complete { height: 100 }),
            "complete"
        );
        assert!(sync_status_to_phase(&SyncStatus::Failed("test".into())).starts_with("failed"));
    }

    #[test]
    fn test_core_error_conversion() {
        let err: WalletError = CoreError::InvalidAnchor.into();
        assert!(matches!(err, WalletError::InvalidAnchor));

        let err: WalletError = CoreError::SyncInProgress.into();
        assert!(matches!(err, WalletError::SyncInProgress));

        let err: WalletError = CoreError::WalletLocked.into();
        assert!(matches!(err, WalletError::WalletLocked));
    }
}
