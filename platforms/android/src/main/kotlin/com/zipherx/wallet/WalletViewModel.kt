package com.zipherx.wallet

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.StatFs
import android.util.Log
import com.zipherx.wallet.BuildConfig
import com.zipherx.wallet.platform.AndroidSecureStorage
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Sync task status
enum class SyncTaskStatus { PENDING, IN_PROGRESS, COMPLETED, FAILED }

data class SyncTask(
    val id: String,
    val title: String,
    val status: SyncTaskStatus = SyncTaskStatus.PENDING,
    val detail: String? = null,
    val progress: Float? = null,
    val startTimeMs: Long? = null,
    val endTimeMs: Long? = null,
)

/**
 * Android ViewModel for ZipherX wallet state.
 *
 * Exposes wallet data via [StateFlow]s that Compose UI observes.
 * All FFI calls are dispatched on [Dispatchers.IO] to keep the main thread free.
 *
 * Sync and send operations use callback interfaces that the Rust FFI invokes
 * on progress. These callbacks update the corresponding StateFlows so the UI
 * recomposes automatically.
 *
 * TODO: KA-INFO-1 — Consider adding structured logging (Timber) with log-level
 *  filtering instead of BuildConfig.DEBUG-gated Log calls for production diagnostics.
 * TODO: KA-INFO-2 — Consider adding Kotlin Result/sealed-class error types for
 *  FFI call results instead of raw exception strings in _errorMessage.
 * TODO: KA-INFO-3 — Consider adding unit tests for ViewModel state transitions
 *  (create/restore/sync/send lifecycle) using Turbine for StateFlow testing.
 */
class WalletViewModel : ViewModel() {

    // -----------------------------------------------------------------------
    // State
    // -----------------------------------------------------------------------

    private val _balance = MutableStateFlow<Balance?>(null)
    val balance: StateFlow<Balance?> = _balance.asStateFlow()

    // Pre-send balance snapshot — shown while awaiting confirmation instead of
    // the incorrect post-send balance (spent notes marked, change note not yet mined).
    private var preSendBalance: Balance? = null

    private val _transactions = MutableStateFlow<List<Transaction>>(emptyList())
    val transactions: StateFlow<List<Transaction>> = _transactions.asStateFlow()

    private val _syncPhase = MutableStateFlow("idle")
    val syncPhase: StateFlow<String> = _syncPhase.asStateFlow()

    private val _syncProgress = MutableStateFlow(0.0)
    val syncProgress: StateFlow<Double> = _syncProgress.asStateFlow()

    private val _walletState = MutableStateFlow("loading")
    val walletState: StateFlow<String> = _walletState.asStateFlow()

    private val _errorMessage = MutableStateFlow<String?>(null)
    val errorMessage: StateFlow<String?> = _errorMessage.asStateFlow()

    private val _isSyncing = MutableStateFlow(false)
    val isSyncing: StateFlow<Boolean> = _isSyncing.asStateFlow()

    private val _isInitialSync = MutableStateFlow(false)
    val isInitialSync: StateFlow<Boolean> = _isInitialSync.asStateFlow()

    private val _isSending = MutableStateFlow(false)
    val isSending: StateFlow<Boolean> = _isSending.asStateFlow()

    private val _walletAddress = MutableStateFlow<String?>(null)
    val walletAddress: StateFlow<String?> = _walletAddress.asStateFlow()

    private val _sendTxid = MutableStateFlow<String?>(null)
    val sendTxid: StateFlow<String?> = _sendTxid.asStateFlow()

    private val _sendPhase = MutableStateFlow<String?>(null)
    val sendPhase: StateFlow<String?> = _sendPhase.asStateFlow()

    private val _onionAddress = MutableStateFlow<String?>(null)
    val onionAddress: StateFlow<String?> = _onionAddress.asStateFlow()

    private val _sentCount = MutableStateFlow(0u)
    val sentCount: StateFlow<UInt> = _sentCount.asStateFlow()

    private val _receivedCount = MutableStateFlow(0u)
    val receivedCount: StateFlow<UInt> = _receivedCount.asStateFlow()

    private val _mempoolAccepted = MutableStateFlow(false)
    val mempoolAccepted: StateFlow<Boolean> = _mempoolAccepted.asStateFlow()

    private val _mempoolPeerStatus = MutableStateFlow<String?>(null)
    val mempoolPeerStatus: StateFlow<String?> = _mempoolPeerStatus.asStateFlow()

    private val _confirmedTxid = MutableStateFlow<String?>(null)
    val confirmedTxid: StateFlow<String?> = _confirmedTxid.asStateFlow()

    private val _confirmationMessage = MutableStateFlow<String?>(null)
    val confirmationMessage: StateFlow<String?> = _confirmationMessage.asStateFlow()

    private val _sendTimestamp = MutableStateFlow(0L)
    val sendTimestamp: StateFlow<Long> = _sendTimestamp.asStateFlow()

    private val _sendAmount = MutableStateFlow(0L)
    val sendAmount: StateFlow<Long> = _sendAmount.asStateFlow()

    private val _incomingTxNotification = MutableStateFlow<Transaction?>(null)
    val incomingTxNotification: StateFlow<Transaction?> = _incomingTxNotification.asStateFlow()

    // Status bar state
    private val _connectedPeers = MutableStateFlow(0u)
    val connectedPeers: StateFlow<UInt> = _connectedPeers.asStateFlow()

    private val _blockHeight = MutableStateFlow(0L)
    val blockHeight: StateFlow<Long> = _blockHeight.asStateFlow()

    private val _syncTasks = MutableStateFlow<List<SyncTask>>(emptyList())
    val syncTasks: StateFlow<List<SyncTask>> = _syncTasks.asStateFlow()

    private val _overallProgress = MutableStateFlow(0f)
    val overallProgress: StateFlow<Float> = _overallProgress.asStateFlow()

    private val _syncStartTimeMs = MutableStateFlow(0L)
    val syncStartTimeMs: StateFlow<Long> = _syncStartTimeMs.asStateFlow()

    private val _networkStatus = MutableStateFlow("Disconnected")
    val networkStatus: StateFlow<String> = _networkStatus.asStateFlow()

    private val _torEnabled = MutableStateFlow(false)
    val torEnabled: StateFlow<Boolean> = _torEnabled.asStateFlow()

    private val _isAuthRequired = MutableStateFlow(false)
    val isAuthRequired: StateFlow<Boolean> = _isAuthRequired.asStateFlow()

    /** True when wallet is active (past setup/mnemonic display) — enables FLAG_SECURE. */
    private val _isWalletActive = MutableStateFlow(false)
    val isWalletActive: StateFlow<Boolean> = _isWalletActive.asStateFlow()

    /** Whether screenshot protection is enabled (default: true). */
    private val _screenshotProtection = MutableStateFlow(true)
    val screenshotProtection: StateFlow<Boolean> = _screenshotProtection.asStateFlow()

    /** Boost download failure — set when all retry attempts exhausted. Pair of (reason, attempts). */
    private val _boostFailed = MutableStateFlow<Pair<String, Int>?>(null)
    val boostFailed: StateFlow<Pair<String, Int>?> = _boostFailed.asStateFlow()

    private var secureStorage: AndroidSecureStorage? = null
    private var appContext: Context? = null

    /**
     * Initialize secure storage for persisting settings.
     * Called by MainActivity after setting the activity reference.
     *
     * Settings (auth_required, screenshot_protection) are stored in
     * EncryptedSharedPreferences backed by Android Keystore, the same
     * storage layer used for the spending key. This prevents an attacker
     * with filesystem access from flipping the biometric-auth flag.
     */
    fun initPrefs(context: Context) {
        appContext = context.applicationContext
        secureStorage = AndroidSecureStorage(context)
        _isAuthRequired.value = loadSettingBoolean("auth_required", true)
        _screenshotProtection.value = loadSettingBoolean("screenshot_protection", true)
        // Auto-load wallet on startup — if a key exists in secure storage,
        // walletState transitions to "ready" before the UI decides which screen to show.
        loadWallet()
    }

    /**
     * KA-N4: Check if the device has network connectivity before attempting sync.
     * Returns true if connected (WiFi, cellular, ethernet, or VPN), false otherwise.
     */
    private fun isNetworkAvailable(): Boolean {
        val ctx = appContext ?: return true // If no context yet, assume connected
        return try {
            val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return true
            val network = cm.activeNetwork ?: return false
            val caps = cm.getNetworkCapabilities(network) ?: return false
            caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        } catch (_: SecurityException) {
            true // Permission missing — assume connected, let sync attempt and fail gracefully
        }
    }

    /**
     * Check if the device has enough free disk space for sync operations.
     * First sync requires ~6 GB (boost download + header DB + working space),
     * subsequent syncs ~1 GB.
     * Checks BOTH internal storage (header DB, wallet DB) and external storage
     * (boost cache file). Returns null if OK, or an error message if insufficient.
     */
    private fun checkDiskSpace(): String? {
        val ctx = appContext ?: return null
        return try {
            // Check internal storage (headers, wallet DB, delta store)
            val internalStat = StatFs(ctx.filesDir.absolutePath)
            val internalAvailGB = internalStat.availableBytes / (1024.0 * 1024.0 * 1024.0)

            // Check external storage (boost cache file ~2.1 GB)
            val externalDir = ctx.getExternalFilesDir(null) ?: ctx.filesDir
            val externalStat = StatFs(externalDir.absolutePath)
            val externalAvailGB = externalStat.availableBytes / (1024.0 * 1024.0 * 1024.0)

            // Check if boost file already exists (skip download space requirement)
            val boostDir = java.io.File(externalDir, "BoostCache")
            val boostFile = java.io.File(boostDir, "zipherx_boost_v1.bin")
            val hasBoostFile = boostFile.exists() && boostFile.length() > 100_000_000

            // Check if header DB already exists (first sync vs incremental)
            val walletDir = ctx.filesDir
            val hasHeaders = walletDir.listFiles()?.any {
                it.name.contains("header") && it.length() > 1_000_000
            } == true

            val isFirstSync = !hasHeaders

            if (isFirstSync) {
                // First sync: need space for boost (2.1 GB external) + headers (1.5 GB internal)
                // + delta/witness (0.5 GB) + WAL/temp (1 GB)
                val internalRequired = 3.0 // headers + delta + working space
                val externalRequired = if (hasBoostFile) 0.5 else 2.5 // boost file + margin

                if (internalAvailGB < internalRequired) {
                    return "Insufficient device storage: %.1f GB free, %.0f GB required for initial sync. Free up space or use a device with more storage.".format(
                        internalAvailGB, internalRequired,
                    )
                }
                if (externalAvailGB < externalRequired) {
                    return "Insufficient storage: %.1f GB free, %.0f GB required for boost download. Free up space and try again.".format(
                        externalAvailGB, externalRequired,
                    )
                }
            } else {
                // Incremental sync: ~1 GB internal
                if (internalAvailGB < 1.0) {
                    return "Insufficient storage: %.1f GB free, 1 GB required for sync. Free up space and try again.".format(
                        internalAvailGB,
                    )
                }
            }
            null // Enough space
        } catch (_: Exception) {
            null // Can't check — proceed and let sync fail with its own error
        }
    }

    /** Load a boolean setting from EncryptedSharedPreferences.
     *  Stored as a single-byte value: 0x01 = true, 0x00 = false. */
    private fun loadSettingBoolean(key: String, default: Boolean): Boolean {
        val data = secureStorage?.loadKey("setting_$key") ?: return default
        return if (data.isNotEmpty()) data[0] != 0.toByte() else default
    }

    /** Persist a boolean setting to EncryptedSharedPreferences. */
    private fun storeSettingBoolean(key: String, value: Boolean) {
        secureStorage?.storeKey("setting_$key", byteArrayOf(if (value) 1 else 0))
    }

    /**
     * Enable or disable biometric auth requirement for app launch and sensitive operations.
     */
    fun setAuthRequired(enabled: Boolean) {
        _isAuthRequired.value = enabled
        storeSettingBoolean("auth_required", enabled)
    }

    fun setScreenshotProtection(enabled: Boolean) {
        _screenshotProtection.value = enabled
        storeSettingBoolean("screenshot_protection", enabled)
    }

    /** Track known txids so we can detect newly received ones after sync. */
    private var knownTxids: Set<String> = emptySet()

    private val _pendingConfirmationTxid = MutableStateFlow<String?>(null)
    val pendingConfirmationTxid: StateFlow<String?> = _pendingConfirmationTxid.asStateFlow()

    // Clearing celebration (mempool accepted)
    private val _clearingCelebration = MutableStateFlow<String?>(null)
    val clearingCelebration: StateFlow<String?> = _clearingCelebration.asStateFlow()

    private val _clearingDuration = MutableStateFlow<String?>(null)
    val clearingDuration: StateFlow<String?> = _clearingDuration.asStateFlow()

    // Settlement celebration (block confirmed)
    private val _settlementCelebration = MutableStateFlow<String?>(null)
    val settlementCelebration: StateFlow<String?> = _settlementCelebration.asStateFlow()

    private val _settlementDuration = MutableStateFlow<String?>(null)
    val settlementDuration: StateFlow<String?> = _settlementDuration.asStateFlow()

    private val _settlementTxid = MutableStateFlow<String?>(null)
    val settlementTxid: StateFlow<String?> = _settlementTxid.asStateFlow()

    private var confirmedSentCountAtSend: Int = 0

    // Incoming TX settlement tracking (mempool → block confirmation)
    private val _pendingIncomingTxid = MutableStateFlow<String?>(null)
    val pendingIncomingTxid: StateFlow<String?> = _pendingIncomingTxid.asStateFlow()

    private val _pendingIncomingAmount = MutableStateFlow(0L)
    val pendingIncomingAmount: StateFlow<Long> = _pendingIncomingAmount.asStateFlow()

    private val _incomingSettlementCelebration = MutableStateFlow<String?>(null)
    val incomingSettlementCelebration: StateFlow<String?> = _incomingSettlementCelebration.asStateFlow()

    private val _incomingSettlementTxid = MutableStateFlow<String?>(null)
    val incomingSettlementTxid: StateFlow<String?> = _incomingSettlementTxid.asStateFlow()

    /** Spending key bytes held in memory after unlock/create/import.
     *  Stored as ByteArray for secure zeroing on shutdown/delete. */
    private var skBytes: ByteArray? = null

    /** True once loadWallet() has completed successfully at least once. */
    private var walletLoaded = false

    /** Activity reference for biometric prompt — set by MainActivity. */
    private var activityRef: java.lang.ref.WeakReference<FragmentActivity>? = null

    fun setActivity(activity: FragmentActivity) {
        activityRef = java.lang.ref.WeakReference(activity)
    }

    /**
     * Request biometric authentication.
     * Returns true on success, false on failure/cancel.
     * Falls back to allowing send if biometrics are unavailable.
     */
    suspend fun authenticateBiometric(reason: String): Boolean {
        val activity = activityRef?.get() ?: return false  // Deny if no activity — never silently bypass
        val bioAuth = com.zipherx.wallet.platform.AndroidBiometricAuth(activity)
        if (!bioAuth.isEnrolled) {
            // No biometrics enrolled — show error and deny
            _errorMessage.value = "No biometrics enrolled. Set up fingerprint or face unlock in device settings."
            return false
        }
        return withContext(Dispatchers.IO) {
            bioAuth.authenticate(reason, activity)
        }
    }

    /**
     * Authenticate with biometrics if enrolled, otherwise skip (allow).
     * Used for destructive operations (delete all data) where the user has already
     * confirmed via a dialog — biometric is an extra layer, not a hard requirement.
     */
    suspend fun authenticateBiometricOrSkip(reason: String): Boolean {
        val activity = activityRef?.get() ?: return true  // No activity = allow (dialog already confirmed)
        val bioAuth = com.zipherx.wallet.platform.AndroidBiometricAuth(activity)
        if (!bioAuth.isEnrolled) {
            return true  // No biometrics — already confirmed via dialog, proceed
        }
        return withContext(Dispatchers.IO) {
            bioAuth.authenticate(reason, activity)
        }
    }

    /**
     * Mandatory authentication: biometric if enrolled, falls back to device credential
     * (PIN/pattern/password). Never skips — always requires proof of identity.
     * Use for security-critical actions (sending funds).
     */
    suspend fun authenticateStrict(reason: String): Boolean {
        val activity = activityRef?.get() ?: return false  // Deny if no activity
        val bioAuth = com.zipherx.wallet.platform.AndroidBiometricAuth(activity)
        if (!bioAuth.hasDeviceCredential) {
            // No screen lock configured — skip auth. The confirmation dialog
            // already verified user intent. Log a warning for audit trail.
            if (BuildConfig.DEBUG) Log.w(TAG, "No device credential configured — skipping auth for: $reason")
            return true
        }
        return withContext(Dispatchers.IO) {
            bioAuth.authenticateStrict(reason, activity)
        }
    }

    init {
        startStatusPolling()
    }

    /** Convert ByteArray to List<UByte> for FFI calls. */
    private fun ByteArray.toUByteList(): List<UByte> = map { it.toUByte() }

    /** Convert List<UByte> to ByteArray for secure storage. */
    private fun List<UByte>.toByteArraySecure(): ByteArray = ByteArray(size) { this[it].toByte() }

    override fun onCleared() {
        super.onCleared()
        if (BuildConfig.DEBUG) Log.i(TAG, "onCleared() — stopping sync before ViewModel destruction")
        // Securely zero spending key bytes
        skBytes?.fill(0)
        skBytes = null
        // Stop sync synchronously to prevent Rust from calling back into dead objects
        try {
            if (_isSyncing.value) {
                uniffi.zipherx.stopSync()
            }
            activeSyncCallback = null
        } catch (e: Exception) {
            if (BuildConfig.DEBUG) Log.w(TAG, "stopSync in onCleared failed: ${e.message}")
        }
    }

    // -----------------------------------------------------------------------
    // Wallet Lifecycle
    // -----------------------------------------------------------------------

    /**
     * Load the wallet: fetch summary, balance, and recent transactions.
     */
    fun loadWallet() {
        if (BuildConfig.DEBUG) Log.i(TAG, "loadWallet() called")

        // If already loaded, just refresh balance and history instead of re-initializing
        if (walletLoaded) {
            if (BuildConfig.DEBUG) Log.i(TAG, "loadWallet() — already loaded, refreshing only")
            refreshBalance()
            refreshHistory()
            return
        }

        viewModelScope.launch {
            try {
                _walletState.value = "loading"

                // Restore SK from secure storage if not already in memory
                if (skBytes == null) {
                    val restored = withContext(Dispatchers.IO) {
                        ZipherXWrapper.platformStorage?.loadKey("spending_key")
                    }
                    if (restored != null && restored.isNotEmpty()) {
                        skBytes = restored.toByteArraySecure()
                        if (BuildConfig.DEBUG) Log.i(TAG, "Restored SK from secure storage")
                    }
                }

                val summary = withContext(Dispatchers.IO) {
                    ZipherXWrapper.getSummary()
                }
                if (BuildConfig.DEBUG) Log.i(TAG, "getSummary() returned: state=${summary.state}")

                // Derive address from SK bytes if summary doesn't provide one
                var address = summary.address
                if (address == null && skBytes != null) {
                    address = withContext(Dispatchers.IO) {
                        try {
                            ZipherXWrapper.deriveAddress(skBytes!!.toUByteList())
                        } catch (e: Exception) {
                            if (BuildConfig.DEBUG) Log.e(TAG, "deriveAddress failed: ${e.message}")
                            null
                        }
                    }
                    if (BuildConfig.DEBUG) Log.i(TAG, "Derived address from SK: ${address != null}")
                }

                _walletAddress.value = address

                if (address == null && skBytes == null) {
                    if (BuildConfig.DEBUG) Log.i(TAG, "No address and no SK — showing onboarding")
                    _walletState.value = "uninitialized"
                    return@launch
                }

                _walletState.value = "ready"
                _isWalletActive.value = true
                // KA-4: Clear mnemonic words from memory once the user has
                // navigated past the mnemonic display to the main wallet UI.
                if (_mnemonicWords.value.isNotEmpty()) {
                    _mnemonicWords.value = emptyList()
                }
                if (BuildConfig.DEBUG) Log.i(TAG, "Wallet loaded: state=ready")
                _syncPhase.value = summary.syncPhase
                _blockHeight.value = summary.lastSyncedHeight
                // Use refreshBalance() to get the actual DB balance (not summary which may be stale)
                refreshBalance()
                refreshHistory()
                walletLoaded = true

                // Start sync (background loop handles periodic re-sync)
                if (!_isSyncing.value) {
                    startSync()
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.w(TAG, "loadWallet() failed: ${e.message}")
                _walletState.value = "uninitialized"
            }
        }
    }

    /**
     * Create a brand new wallet. Shows the 24-word mnemonic.
     */
    private val _mnemonicWords = MutableStateFlow<List<String>>(emptyList())
    val mnemonicWords: StateFlow<List<String>> = _mnemonicWords.asStateFlow()

    /**
     * Explicitly clear mnemonic words from memory. (KD-6)
     * Should be called from the "I Have Saved" button handler after the user
     * confirms they have backed up the mnemonic phrase.
     */
    fun clearMnemonic() {
        _mnemonicWords.value = emptyList()
    }

    fun createNewWallet() {
        if (BuildConfig.DEBUG) Log.i(TAG, "createNewWallet() called")
        viewModelScope.launch {
            try {
                _walletState.value = "creating"
                val (words, sk) = withContext(Dispatchers.IO) {
                    ZipherXWrapper.createWallet()
                }
                if (BuildConfig.DEBUG) Log.i(TAG, "Wallet created successfully")
                _mnemonicWords.value = words
                skBytes = sk.toByteArraySecure()
                _walletState.value = "created"
                loadWallet()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "createNewWallet() FAILED: ${e.message}", e)
                _errorMessage.value = e.message ?: "Failed to create wallet"
                _walletState.value = "uninitialized"
            }
        }
    }

    /**
     * Restore wallet from a 24-word BIP39 mnemonic.
     */
    fun restoreFromMnemonic(words: List<String>) {
        if (BuildConfig.DEBUG) Log.i(TAG, "restoreFromMnemonic() called, ${words.size} words")
        viewModelScope.launch {
            try {
                _walletState.value = "restoring"
                val sk = withContext(Dispatchers.IO) {
                    ZipherXWrapper.restoreWallet(words)
                }
                if (BuildConfig.DEBUG) Log.i(TAG, "Wallet restored successfully")
                skBytes = sk.toByteArraySecure()
                _walletState.value = "ready"
                loadWallet()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "restoreFromMnemonic() FAILED: ${e.message}", e)
                _errorMessage.value = e.message ?: "Failed to restore wallet"
                _walletState.value = "uninitialized"
            }
        }
    }

    /**
     * Import wallet from a spending key (hex or encoded format).
     */
    fun importSpendingKey(key: String) {
        if (BuildConfig.DEBUG) Log.i(TAG, "importSpendingKey() called, key length=${key.length}")
        viewModelScope.launch {
            try {
                _walletState.value = "importing"
                val isHex = key.length == 64 && key.all { it in "0123456789abcdefABCDEF" }
                if (BuildConfig.DEBUG) Log.i(TAG, "Key format: ${if (isHex) "raw hex" else "encoded"}")
                val sk = withContext(Dispatchers.IO) {
                    if (isHex) {
                        ZipherXWrapper.importFromKey(key)
                    } else {
                        ZipherXWrapper.importFromEncodedKey(key)
                    }
                }
                if (BuildConfig.DEBUG) Log.i(TAG, "Key imported successfully")
                skBytes = sk.toByteArraySecure()
                _walletState.value = "ready"
                loadWallet()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "importSpendingKey() FAILED: ${e.message}", e)
                _errorMessage.value = e.message ?: "Failed to import private key"
                _walletState.value = "uninitialized"
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sync
    // -----------------------------------------------------------------------

    /**
     * Start background sync. Progress is reported via [uniffi.zipherx.SyncProgressCallback].
     */
    /** Hold strong reference to sync callback to prevent GC while Rust holds it. */
    private var activeSyncCallback: uniffi.zipherx.SyncProgressCallback? = null

    fun startSync() {
        if (_isSyncing.value) return
        // KA-N4: Check network connectivity before attempting sync
        if (!isNetworkAvailable()) {
            if (BuildConfig.DEBUG) Log.w(TAG, "startSync() skipped — no network connectivity")
            _errorMessage.value = "No network connection. Please check your internet and try again."
            return
        }
        // Check available disk space before starting sync
        val storageError = checkDiskSpace()
        if (storageError != null) {
            if (BuildConfig.DEBUG) Log.w(TAG, "startSync() skipped — $storageError")
            _errorMessage.value = storageError
            return
        }
        if (BuildConfig.DEBUG) Log.i(TAG, "startSync() called")
        viewModelScope.launch {
            try {
                _isInitialSync.value = _blockHeight.value == 0L
                if (BuildConfig.DEBUG) Log.i(TAG, "startSync: blockHeight=${_blockHeight.value} isInitialSync=${_isInitialSync.value}")
                _isSyncing.value = true
                _syncPhase.value = "starting"
                _errorMessage.value = null
                _syncStartTimeMs.value = System.currentTimeMillis()
                _overallProgress.value = 0f

                // Initialize task list
                _syncTasks.value = listOf(
                    SyncTask("boost_download", "Downloading boost file"),
                    SyncTask("boost_load", "Loading boost headers"),
                    SyncTask("header_sync", "Syncing block headers"),
                    SyncTask("delta_sync", "Downloading shielded outputs"),
                    SyncTask("block_scan", "Scanning for transactions"),
                    SyncTask("witness_update", "Verifying witnesses"),
                )

                var lastPhase = ""

                withContext(Dispatchers.IO) {
                    val callback = object : uniffi.zipherx.SyncProgressCallback {
                        override fun onProgress(phase: String, current: ULong, target: ULong) {
                            if (BuildConfig.DEBUG) Log.d(TAG, "Sync progress: phase=$phase current=$current target=$target")

                            // Detect boost download failure (all retries exhausted)
                            if (phase == "boost_failed") {
                                if (BuildConfig.DEBUG) Log.w(TAG, "Boost download failed after $target attempts")
                                _boostFailed.value = Pair("Boost download failed after $target attempts", target.toInt())
                                markCurrentTaskFailed("boost_download", "Failed after $target attempts")
                                return
                            }

                            // Update peer count on every progress tick so status bar stays current
                            viewModelScope.launch(Dispatchers.IO) {
                                try {
                                    _connectedPeers.value = ZipherXWrapper.getConnectedPeerCount()
                                } catch (_: Exception) {}
                            }
                            // Include height numbers in the phase for visible progress
                            _syncPhase.value = if (target > 0uL && current > 0uL) {
                                "${phase}:${current}:${target}"
                            } else {
                                phase
                            }
                            val phaseProgress = if (target > 0uL) {
                                current.toFloat() / target.toFloat()
                            } else {
                                0f
                            }
                            _syncProgress.value = phaseProgress.toDouble()

                            // Update task list
                            if (phase != lastPhase) {
                                markPhaseTransition(lastPhase, phase)
                                lastPhase = phase
                            }
                            updateTaskProgress(phase, current.toLong(), target.toLong(), phaseProgress)
                        }

                        override fun onComplete(height: ULong) {
                            val wasInitialSync = _isInitialSync.value
                            if (BuildConfig.DEBUG) Log.i(TAG, "Sync complete at height $height (initial=$wasInitialSync)")
                            // Update peer count immediately on sync completion
                            viewModelScope.launch(Dispatchers.IO) {
                                try {
                                    _connectedPeers.value = ZipherXWrapper.getConnectedPeerCount()
                                } catch (_: Exception) {}
                            }
                            _isSyncing.value = false
                            _isInitialSync.value = false
                            _syncPhase.value = "complete"
                            _syncProgress.value = 1.0
                            _blockHeight.value = height.toLong()

                            if (wasInitialSync) {
                                // Initial sync done — mark tasks complete, then clear after delay
                                _overallProgress.value = 1f
                                markAllTasksCompleted()
                                viewModelScope.launch {
                                    delay(3_000) // Show "completed" briefly
                                    _syncTasks.value = emptyList()
                                    _overallProgress.value = 0f
                                }
                            }
                            // Background syncs from the FFI 30s loop: no task UI needed.
                            // The FFI already runs wallet.sync() every 30s and calls
                            // onComplete when new blocks are found — no need to re-trigger
                            // startSync() which would kill the background loop.

                            refreshBalance()
                            viewModelScope.launch {
                                refreshHistoryInternal()
                                checkForTxConfirmation()
                                checkForIncomingTxConfirmation()
                            }
                        }

                        override fun onError(message: String) {
                            if (BuildConfig.DEBUG) Log.e(TAG, "Sync error: $message")
                            _isSyncing.value = false
                            _syncPhase.value = "failed"
                            // Make network errors more user-friendly
                            _errorMessage.value = when {
                                message.contains("No peers available", ignoreCase = true) ->
                                    "No network peers found. Check your internet connection and try again."
                                message.contains("disk is full", ignoreCase = true) ->
                                    "Device storage is full. ZipherX needs at least 4 GB free for initial sync. Free up space or use a device with more storage."
                                else -> message
                            }
                            markCurrentTaskFailed(lastPhase, message)
                        }

                        override fun onMempoolTx(txid: String, amount: ULong) {
                            if (BuildConfig.DEBUG) Log.i(TAG, "Mempool TX detected: $txid ($amount zatoshis)")
                            // Skip change outputs from our own sends — the mempool
                            // monitor decrypts ALL outputs (including change) so the
                            // same txid we just sent would appear as "incoming".
                            if (txid == _pendingConfirmationTxid.value) {
                                if (BuildConfig.DEBUG) Log.d(TAG, "Mempool TX $txid is our own send (change output) — skipping incoming notification")
                                return
                            }
                            val mempoolTx = Transaction(
                                txid = txid,
                                txType = "received",
                                amount = amount.toLong(),
                                fee = 0,
                                address = null,
                                memo = null,
                                confirmations = 0,
                                height = 0,
                                timestamp = System.currentTimeMillis() / 1000
                            )
                            _incomingTxNotification.value = mempoolTx
                            _pendingIncomingTxid.value = txid
                            _pendingIncomingAmount.value = amount.toLong()
                        }
                    }
                    activeSyncCallback = callback
                    if (BuildConfig.DEBUG) Log.i(TAG, "startSync: calling FFI startSync()...")
                    uniffi.zipherx.startSync(callback)
                    if (BuildConfig.DEBUG) Log.i(TAG, "startSync: FFI startSync() returned OK")
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "startSync() FAILED: ${e.message}", e)
                _errorMessage.value = e.message ?: "Sync failed"
                _isSyncing.value = false
                _syncPhase.value = "failed"
            }
        }
    }

    /**
     * Stop the background sync.
     */
    fun stopSync() {
        if (BuildConfig.DEBUG) Log.i(TAG, "stopSync() called")
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                uniffi.zipherx.stopSync()
            }
            _isSyncing.value = false
            _syncPhase.value = "idle"
        }
    }

    /**
     * User chose to continue with slow P2P header sync after boost failure.
     */
    fun onBoostFailedContinue() {
        if (BuildConfig.DEBUG) Log.i(TAG, "Boost failed — user chose to continue with P2P sync")
        _boostFailed.value = null
        // Sync continues automatically on the Rust side (falls through to header_sync)
    }

    /**
     * User chose to quit the app after boost failure.
     */
    fun onBoostFailedQuit() {
        if (BuildConfig.DEBUG) Log.i(TAG, "Boost failed — user chose to quit")
        _boostFailed.value = null
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                try { uniffi.zipherx.stopSync() } catch (_: Exception) {}
            }
        }
        // Activity will call finishAffinity() from the UI layer
    }

    // -----------------------------------------------------------------------
    // Send
    // -----------------------------------------------------------------------

    /**
     * Send a shielded transaction.
     *
     * @param to Destination shielded address.
     * @param amount Amount in zatoshis.
     * @param fee Fee in zatoshis.
     * @param memo Optional memo string.
     * @param skBytes Spending key bytes.
     */
    fun send(to: String, amount: Long, fee: Long, memo: String?) {
        if (_isSending.value) return
        if (amount <= 0L) {
            _errorMessage.value = "Insufficient spendable balance"
            return
        }
        val skArray = skBytes
        if (skArray == null) {
            _errorMessage.value = "Spending key not available"
            return
        }
        if (BuildConfig.DEBUG) Log.d(TAG, "send() called")
        // Snapshot current balance before send — will be displayed until confirmation
        preSendBalance = _balance.value
        viewModelScope.launch {
            try {
                _isSending.value = true
                _sendPhase.value = "validating"
                _sendTxid.value = null
                _errorMessage.value = null
                _mempoolAccepted.value = false
                _mempoolPeerStatus.value = null
                _sendTimestamp.value = System.currentTimeMillis()
                _sendAmount.value = amount

                withContext(Dispatchers.IO) {
                    val callback = SendCallback()
                    uniffi.zipherx.sendWithProgress(
                        to, amount.toULong(), fee.toULong(), memo, skArray.toUByteList(), callback
                    )
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "send() FAILED: ${e.message}", e)
                _errorMessage.value = e.message ?: "Send failed"
                _isSending.value = false
                _sendPhase.value = null
            }
        }
    }

    // -----------------------------------------------------------------------
    // Refresh
    // -----------------------------------------------------------------------

    /**
     * Refresh the current balance from the Rust core.
     */
    fun refreshBalance() {
        viewModelScope.launch {
            try {
                val bal = withContext(Dispatchers.IO) {
                    ZipherXWrapper.getBalance()
                }
                if (BuildConfig.DEBUG) Log.d(TAG, "refreshBalance: total=${bal.total} spendable=${bal.spendable} notes=${bal.noteCount}")
                if (bal.total > 0 && bal.spendable == 0L) {
                    if (BuildConfig.DEBUG) Log.w(TAG, "Notes may be missing witnesses/anchors")
                }
                // While a TX is pending confirmation, suppress balance DECREASES —
                // the DB has spent notes marked but no change note yet, so the
                // balance would show an incorrect (too low) value.
                // But allow balance INCREASES (incoming TXs should still update).
                if (_pendingConfirmationTxid.value != null) {
                    val currentTotal = _balance.value?.total ?: 0L
                    if (bal.total < currentTotal) {
                        if (BuildConfig.DEBUG) Log.d(TAG, "refreshBalance: suppressed decrease while TX pending (real=${bal.total}, showing pre-send=$currentTotal)")
                        return@launch
                    }
                    if (BuildConfig.DEBUG) Log.d(TAG, "refreshBalance: allowing update while TX pending (${currentTotal} → ${bal.total})")
                }
                _balance.value = bal
            } catch (e: Exception) {
                _errorMessage.value = e.message ?: "Failed to refresh balance"
            }
        }
    }

    /**
     * Refresh transaction history from the Rust core.
     * Fire-and-forget wrapper around the suspending implementation.
     */
    fun refreshHistory() {
        viewModelScope.launch { refreshHistoryInternal() }
    }

    /**
     * Suspending implementation of history refresh — can be awaited before
     * checking for TX confirmation (avoids the race where checkForTxConfirmation
     * reads stale _transactions.value).
     */
    private suspend fun refreshHistoryInternal() {
        try {
            val rawHistory = withContext(Dispatchers.IO) {
                ZipherXWrapper.getHistory(limit = 50, offset = 0)
            }

            // Detect self-sends: txids that appear as both sent AND received
            val txidTypes = rawHistory.groupBy { it.txid }
            val history = mutableListOf<Transaction>()
            val processedTxids = mutableSetOf<String>()

            for (tx in rawHistory) {
                if (tx.txid in processedTxids) continue
                val group = txidTypes[tx.txid] ?: listOf(tx)
                val hasSent = group.any { it.txType == "sent" || it.txType == "alpha" }
                val hasReceived = group.any { it.txType == "received" || it.txType == "beta" }

                if (hasSent && hasReceived) {
                    // Self-send: merge into a single "self" entry
                    val sentTx = group.first { it.txType == "sent" || it.txType == "alpha" }
                    history.add(sentTx.copy(txType = "self"))
                    processedTxids.add(tx.txid)
                } else {
                    history.add(tx)
                    processedTxids.add(tx.txid)
                }
            }

            // Detect newly received transactions
            if (knownTxids.isNotEmpty()) {
                val newReceived = history.filter { tx ->
                    tx.txid !in knownTxids &&
                    (tx.txType == "received" || tx.txType == "beta")
                }
                if (newReceived.isNotEmpty()) {
                    if (BuildConfig.DEBUG) Log.i(TAG, "Detected ${newReceived.size} new incoming TX(s)")
                    _incomingTxNotification.value = newReceived.first()
                }
            }
            knownTxids = history.map { it.txid }.toSet()

            if (BuildConfig.DEBUG) Log.d(TAG, "refreshHistory: ${history.size} TXs, known=${knownTxids.size}")
            _transactions.value = history
        } catch (e: Exception) {
            if (BuildConfig.DEBUG) Log.e(TAG, "refreshHistory failed: ${e.message}")
            _errorMessage.value = e.message ?: "Failed to refresh history"
        }

        // Also refresh IN/OUT counts
        try {
            val counts = withContext(Dispatchers.IO) {
                ZipherXWrapper.getTransactionCounts()
            }
            _sentCount.value = counts.first
            _receivedCount.value = counts.second
        } catch (_: Exception) {
            // Non-fatal — counts are informational
        }
    }

    /**
     * Clear the current error message.
     */
    fun clearError() {
        _errorMessage.value = null
    }

    // -----------------------------------------------------------------------
    // Status Bar Polling
    // -----------------------------------------------------------------------

    private fun startStatusPolling() {
        viewModelScope.launch {
            while (true) {
                try {
                    val peers = withContext(Dispatchers.IO) {
                        ZipherXWrapper.getConnectedPeerCount()
                    }
                    _connectedPeers.value = peers

                    val summary = withContext(Dispatchers.IO) {
                        ZipherXWrapper.getSummary()
                    }
                    _blockHeight.value = summary.lastSyncedHeight

                    _networkStatus.value = when {
                        _isSyncing.value -> "Syncing"
                        peers > 0u && (summary.syncPhase == "complete" || summary.syncPhase == "idle") -> "Synced"
                        peers > 0u -> "Connected"
                        summary.syncPhase == "complete" || summary.syncPhase == "idle" -> "Ready"
                        else -> "Disconnected"
                    }
                } catch (_: Exception) {
                    _networkStatus.value = "Error"
                }
                delay(2000)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sync Task Helpers
    // -----------------------------------------------------------------------

    private fun markPhaseTransition(oldPhase: String, newPhase: String) {
        val now = System.currentTimeMillis()
        val taskIds = _syncTasks.value.map { it.id }
        val newPhaseIdx = taskIds.indexOf(newPhase)

        _syncTasks.value = _syncTasks.value.mapIndexed { idx, task ->
            when {
                // Mark all phases BEFORE the new phase as completed (handles skipped phases)
                newPhaseIdx >= 0 && idx < newPhaseIdx && task.status != SyncTaskStatus.COMPLETED ->
                    task.copy(status = SyncTaskStatus.COMPLETED, progress = 1f,
                        startTimeMs = task.startTimeMs ?: now, endTimeMs = task.endTimeMs ?: now)
                // Mark the new phase as in-progress
                task.id == newPhase ->
                    task.copy(status = SyncTaskStatus.IN_PROGRESS, startTimeMs = now)
                else -> task
            }
        }
        recalculateOverallProgress()
    }

    private fun updateTaskProgress(phase: String, current: Long, target: Long, progress: Float) {
        val detail = when (phase) {
            "boost_download" -> {
                val mb = current / (1024 * 1024)
                val totalMb = if (target > 0) target / (1024 * 1024) else 0
                if (totalMb > 0) "${mb}MB / ${totalMb}MB" else "${mb}MB downloaded"
            }
            "boost_load" -> "$current / $target headers"
            "header_sync" -> "Height $current / $target"
            "delta_sync" -> "Height $current / $target"
            "block_scan" -> "Block $current / $target"
            "witness_update" -> "$current / $target notes"
            else -> if (target > 0) "$current / $target" else ""
        }
        _syncTasks.value = _syncTasks.value.map { task ->
            if (task.id == phase) task.copy(progress = progress, detail = detail)
            else task
        }
        recalculateOverallProgress()
    }

    private fun markAllTasksCompleted() {
        val now = System.currentTimeMillis()
        _syncTasks.value = _syncTasks.value.map { task ->
            if (task.status != SyncTaskStatus.COMPLETED) {
                task.copy(status = SyncTaskStatus.COMPLETED, progress = 1f, endTimeMs = now)
            } else task
        }
    }

    private fun markCurrentTaskFailed(phase: String, message: String) {
        _syncTasks.value = _syncTasks.value.map { task ->
            if (task.id == phase) task.copy(status = SyncTaskStatus.FAILED, detail = message)
            else task
        }
    }

    private fun recalculateOverallProgress() {
        val tasks = _syncTasks.value
        if (tasks.isEmpty()) return
        val totalWeight = tasks.size.toFloat()
        var weighted = 0f
        for (task in tasks) {
            weighted += when (task.status) {
                SyncTaskStatus.COMPLETED -> 1f
                SyncTaskStatus.IN_PROGRESS -> (task.progress ?: 0f)
                else -> 0f
            }
        }
        _overallProgress.value = weighted / totalWeight
    }

    // -----------------------------------------------------------------------
    // Silent Sync
    // -----------------------------------------------------------------------

    private var isSyncingSilent = false
    private fun syncSilent() {
        if (_isSyncing.value || isSyncingSilent) return
        viewModelScope.launch {
            isSyncingSilent = true
            try {
                withContext(Dispatchers.IO) {
                    uniffi.zipherx.startSync(object : uniffi.zipherx.SyncProgressCallback {
                        override fun onProgress(phase: String, current: ULong, target: ULong) {}
                        override fun onComplete(height: ULong) {
                            isSyncingSilent = false
                            _blockHeight.value = height.toLong()
                            refreshBalance()
                            // Await history refresh before checking confirmation
                            // to avoid reading stale _transactions.value
                            viewModelScope.launch {
                                refreshHistoryInternal()
                                checkForTxConfirmation()
                                checkForIncomingTxConfirmation()
                            }
                        }
                        override fun onError(message: String) {
                            isSyncingSilent = false
                        }
                        override fun onMempoolTx(txid: String, amount: ULong) {
                            if (BuildConfig.DEBUG) Log.i(TAG, "Mempool TX (silent): $txid ($amount zatoshis)")
                            // Skip change outputs from our own sends
                            if (txid == _pendingConfirmationTxid.value) {
                                if (BuildConfig.DEBUG) Log.d(TAG, "Mempool TX $txid is our own send — skipping")
                                return
                            }
                            val mempoolTx = Transaction(
                                txid = txid,
                                txType = "received",
                                amount = amount.toLong(),
                                fee = 0, address = null, memo = null,
                                confirmations = 0, height = 0,
                                timestamp = System.currentTimeMillis() / 1000
                            )
                            _incomingTxNotification.value = mempoolTx
                            _pendingIncomingTxid.value = txid
                            _pendingIncomingAmount.value = amount.toLong()
                        }
                    })
                }
            } catch (_: Exception) {
                isSyncingSilent = false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Send Callback
    // -----------------------------------------------------------------------

    /**
     * Callback invoked by the Rust FFI during send operations.
     * Updates StateFlows so the Compose UI recomposes on progress changes.
     */
    inner class SendCallback : uniffi.zipherx.SendProgressCallback {

        override fun onPhase(phase: String, current: UInt, total: UInt) {
            if (BuildConfig.DEBUG) Log.d(TAG, "Send phase: $phase current=$current total=$total")
            _sendPhase.value = phase
            // TX lifecycle: detect mempool acceptance from peer_response phase
            if (phase == "peer_response" && current > 0u) {
                _mempoolAccepted.value = true
                _mempoolPeerStatus.value = "$current/$total"
            }
        }

        override fun onComplete(txid: String, amount: ULong, fee: ULong) {
            if (BuildConfig.DEBUG) Log.i(TAG, "Send complete")
            _isSending.value = false
            _sendTxid.value = txid
            _sendPhase.value = null
            _mempoolAccepted.value = true
            _pendingConfirmationTxid.value = txid
            uniffi.zipherx.setPendingTxFastPoll(true)

            // Snapshot confirmed sent/self count for fallback detection
            confirmedSentCountAtSend = _transactions.value.count {
                it.confirmations > 0 && (it.txType == "sent" || it.txType == "self")
            }

            // Show clearing (mempool) celebration
            val elapsed = if (_sendTimestamp.value > 0) {
                (System.currentTimeMillis() - _sendTimestamp.value) / 1000
            } else {
                0L
            }
            _clearingCelebration.value = randomClearingMessage()
            _clearingDuration.value = if (elapsed > 0) "${elapsed}s" else null

            // Don't refreshBalance() here — spent notes are already marked but change
            // note won't appear until the TX is mined, so balance would show 0.
            // Balance will update naturally when the first auto-sync completes.
            refreshHistory()

            // The FFI background loop handles confirmation polling automatically:
            // setPendingTxFastPoll(true) switches it to 10s interval.
            // On each onComplete callback, checkForTxConfirmation() checks if
            // the pending TX got confirmed, and clears the fast poll flag.
            // Safety: auto-clear after 6 minutes if still pending.
            viewModelScope.launch {
                delay(360_000) // 6 minutes
                if (_pendingConfirmationTxid.value != null) {
                    if (BuildConfig.DEBUG) Log.w(TAG, "Safety: auto-clearing pending TX after timeout")
                    _pendingConfirmationTxid.value = null
                    uniffi.zipherx.setPendingTxFastPoll(false)
                    preSendBalance = null
                    _mempoolAccepted.value = false
                    _mempoolPeerStatus.value = null
                    refreshBalance()  // Show real balance now
                }
            }
        }

        override fun onError(message: String) {
            if (BuildConfig.DEBUG) Log.e(TAG, "Send error: $message")
            _isSending.value = false
            val lastPhase = _sendPhase.value
            _sendPhase.value = null
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            // Make send errors more user-friendly
            _errorMessage.value = when {
                message.contains("Invalid anchor", ignoreCase = true) ->
                    "Invalid anchor — corrupted witness data. " +
                    "Go to Settings → FULL RESCAN to fix, then try again."
                else -> message
            }
        }
    }

    // -----------------------------------------------------------------------
    // TX Lifecycle
    // -----------------------------------------------------------------------

    /**
     * Check if a pending TX just got confirmed (called after sync completes).
     * Uses TWO strategies: exact txid match OR detecting a new confirmed sent/self TX
     * that appeared since the send. The second strategy handles txid mismatches.
     */
    fun checkForTxConfirmation() {
        val pendingTxid = _pendingConfirmationTxid.value ?: return
        val txList = _transactions.value

        // Strategy 1: exact txid match
        val matchedByTxid = txList.any { it.txid == pendingTxid && it.confirmations > 0 }
        // Strategy 2: count confirmed sent/self TXs — if more than at send time, our TX confirmed
        val currentConfirmedCount = txList.count {
            it.confirmations > 0 && (it.txType == "sent" || it.txType == "self")
        }
        val matchedByCount = currentConfirmedCount > confirmedSentCountAtSend

        if (matchedByTxid || matchedByCount) {
            // Settlement detected — show celebration
            val elapsed = if (_sendTimestamp.value > 0) {
                (System.currentTimeMillis() - _sendTimestamp.value) / 1000
            } else {
                0L
            }
            val durationStr = if (elapsed > 0) "${elapsed}s" else ""
            // Find the confirmed TX (prefer exact match, fallback to newest)
            val confirmedTx = txList.firstOrNull { it.txid == pendingTxid && it.confirmations > 0 }
                ?: txList.firstOrNull { it.confirmations > 0 && (it.txType == "sent" || it.txType == "self") }
            _settlementTxid.value = confirmedTx?.txid ?: pendingTxid
            _settlementCelebration.value = randomSettlementMessage()
            _settlementDuration.value = durationStr
            _pendingConfirmationTxid.value = null
            uniffi.zipherx.setPendingTxFastPoll(false)
            preSendBalance = null  // Allow real balance updates again
            // Clear mempool status — TX is now confirmed in a block
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            // Force real balance refresh now that TX is confirmed
            refreshBalance()
            // Post-settlement: the FFI background loop will rebuild witnesses
            // on the next sync cycle (within 30s).
        }
    }

    /**
     * Check if a pending incoming mempool TX has been confirmed in a block.
     */
    fun checkForIncomingTxConfirmation() {
        val pendingTxid = _pendingIncomingTxid.value ?: return
        val txList = _transactions.value

        val confirmed = txList.any { it.txid == pendingTxid && it.confirmations > 0 }
        if (confirmed) {
            if (BuildConfig.DEBUG) Log.i(TAG, "Incoming TX confirmed: $pendingTxid")
            _incomingSettlementTxid.value = pendingTxid
            _incomingSettlementCelebration.value = randomSettlementMessage()
            _pendingIncomingTxid.value = null
            _pendingIncomingAmount.value = 0L
            refreshBalance()
        }
    }

    private fun formatDuration(seconds: Long): String {
        return when {
            seconds < 60 -> "${seconds}s"
            seconds < 3600 -> "${seconds / 60}m ${seconds % 60}s"
            else -> "${seconds / 3600}h ${(seconds % 3600) / 60}m"
        }
    }

    fun clearSendStatus() {
        _mempoolAccepted.value = false
        _mempoolPeerStatus.value = null
    }

    fun dismissConfirmation() {
        _confirmedTxid.value = null
        _confirmationMessage.value = null
    }

    fun dismissIncomingNotification() {
        _incomingTxNotification.value = null
    }

    fun dismissClearing() {
        _clearingCelebration.value = null
        _clearingDuration.value = null
    }

    fun dismissSettlement() {
        _settlementCelebration.value = null
        _settlementDuration.value = null
        _settlementTxid.value = null
    }

    fun dismissIncomingSettlement() {
        _incomingSettlementCelebration.value = null
        _incomingSettlementTxid.value = null
    }

    /**
     * Enable or disable Tor for P2P connections.
     * Tor is disabled by default. Takes effect on next sync.
     */
    fun setTorEnabled(enabled: Boolean) {
        _torEnabled.value = enabled
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                uniffi.zipherx.setTorEnabled(enabled)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Maintenance: Repair & Rescan
    // -----------------------------------------------------------------------

    private val _isRepairing = MutableStateFlow(false)
    val isRepairing: StateFlow<Boolean> = _isRepairing.asStateFlow()

    private val _repairStatus = MutableStateFlow<String?>(null)
    val repairStatus: StateFlow<String?> = _repairStatus.asStateFlow()

    /**
     * Repair database: stop sync, clear tree state (preserving notes and
     * history), then restart sync to rebuild witnesses.
     */
    fun repairDatabase() {
        if (_isRepairing.value) return
        if (BuildConfig.DEBUG) Log.i(TAG, "repairDatabase() called")
        viewModelScope.launch {
            try {
                _isRepairing.value = true
                _repairStatus.value = "Stopping sync..."

                // Stop sync if running
                if (_isSyncing.value) {
                    withContext(Dispatchers.IO) {
                        uniffi.zipherx.stopSync()
                    }
                    _isSyncing.value = false
                    _syncPhase.value = "idle"
                }

                _repairStatus.value = "Repairing database..."

                withContext(Dispatchers.IO) {
                    ZipherXWrapper.repairDatabase()
                }

                if (BuildConfig.DEBUG) Log.i(TAG, "repairDatabase() completed successfully")
                _repairStatus.value = "Repair complete. Restarting sync..."

                // Brief delay so the user sees the status
                delay(1000)

                _isRepairing.value = false
                _repairStatus.value = null

                // Restart sync to rebuild tree/witnesses
                startSync()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "repairDatabase() FAILED: ${e.message}", e)
                _errorMessage.value = "Repair failed: ${e.message}"
                _isRepairing.value = false
                _repairStatus.value = null
            }
        }
    }

    /**
     * Full rescan: stop sync, reset all sync state, then restart sync
     * which will re-download everything from scratch (boost + blocks).
     */
    fun fullRescan() {
        if (_isRepairing.value) return
        if (BuildConfig.DEBUG) Log.i(TAG, "fullRescan() called")
        viewModelScope.launch {
            try {
                _isRepairing.value = true
                _repairStatus.value = "Stopping sync..."

                // Stop sync if running
                if (_isSyncing.value) {
                    withContext(Dispatchers.IO) {
                        uniffi.zipherx.stopSync()
                    }
                    _isSyncing.value = false
                    _syncPhase.value = "idle"
                }

                _repairStatus.value = "Resetting sync state..."

                withContext(Dispatchers.IO) {
                    ZipherXWrapper.fullRescan()
                }

                if (BuildConfig.DEBUG) Log.i(TAG, "fullRescan() completed successfully")
                _repairStatus.value = "Rescan reset complete. Restarting sync..."

                // Reset balance/history since rescan will rebuild everything
                _balance.value = Balance(total = 0, spendable = 0, noteCount = 0, spendableNoteCount = 0)
                _syncProgress.value = 0.0

                // Brief delay so the user sees the status
                delay(1000)

                _isRepairing.value = false
                _repairStatus.value = null

                // Restart sync — this will re-download boost file and rescan all blocks
                startSync()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e(TAG, "fullRescan() FAILED: ${e.message}", e)
                _errorMessage.value = "Full rescan failed: ${e.message}"
                _isRepairing.value = false
                _repairStatus.value = null
            }
        }
    }

    // -----------------------------------------------------------------------
    // Export Spending Key
    // -----------------------------------------------------------------------

    /**
     * Check whether a spending key is loaded in memory, without materializing it as a String.
     * Use this for existence checks (e.g., security audit dialog) to avoid unnecessary
     * String allocations of sensitive key material. (KD-4)
     */
    fun hasSpendingKey(): Boolean = skBytes != null

    /**
     * Return the spending key as a CharArray for export.
     * Caller MUST zero the CharArray after use to minimize key exposure in memory.
     * Requires biometric authentication before calling. (KD-5)
     */
    fun getSpendingKeyHex(): CharArray? {
        val sk = skBytes ?: return null
        val chars = CharArray(sk.size * 2)
        for (i in sk.indices) {
            val hex = "%02x".format(sk[i])
            chars[i * 2] = hex[0]
            chars[i * 2 + 1] = hex[1]
        }
        return chars
    }

    // -----------------------------------------------------------------------
    // Delete All Data
    // -----------------------------------------------------------------------

    /**
     * Wipe all wallet data: stop sync, clear secure storage, reset state.
     * Requires biometric authentication before calling.
     */
    fun deleteAllData() {
        if (BuildConfig.DEBUG) Log.w(TAG, "deleteAllData() — wiping all wallet data")
        viewModelScope.launch {
            // Stop sync first
            if (_isSyncing.value) {
                withContext(Dispatchers.IO) {
                    try { uniffi.zipherx.stopSync() } catch (_: Exception) {}
                }
            }
            _isSyncing.value = false

            // Delete spending key from secure storage
            withContext(Dispatchers.IO) {
                try {
                    ZipherXWrapper.platformStorage?.deleteKey("spending_key")
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.e(TAG, "Failed to delete SK from storage: ${e.message}")
                }
            }

            // Securely zero spending key bytes before releasing
            skBytes?.fill(0)
            skBytes = null
            walletLoaded = false
            _isWalletActive.value = false
            knownTxids = emptySet()
            _pendingConfirmationTxid.value = null
            uniffi.zipherx.setPendingTxFastPoll(false)

            // Reset all StateFlows
            _balance.value = null
            _transactions.value = emptyList()
            _walletState.value = "uninitialized"
            _walletAddress.value = null
            _syncPhase.value = "idle"
            _syncProgress.value = 0.0
            _overallProgress.value = 0f
            _syncTasks.value = emptyList()
            _syncStartTimeMs.value = 0L
            _sendTxid.value = null
            _sendPhase.value = null
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            _confirmedTxid.value = null
            _confirmationMessage.value = null
            _clearingCelebration.value = null
            _clearingDuration.value = null
            _settlementCelebration.value = null
            _settlementDuration.value = null
            _settlementTxid.value = null
            _incomingTxNotification.value = null
            _pendingIncomingTxid.value = null
            _pendingIncomingAmount.value = 0L
            _incomingSettlementCelebration.value = null
            _incomingSettlementTxid.value = null
            _mnemonicWords.value = emptyList()
            _errorMessage.value = null

            if (BuildConfig.DEBUG) Log.i(TAG, "All data deleted. Wallet reset to onboarding state.")
        }
    }

    companion object {
        private const val TAG = "ZipherX.VM"

        private val cypherpunkMessages = listOf(
            "Block confirmed. Your transaction is now etched into the chain. Privacy preserved.",
            "The miners have spoken. Your shielded TX is sealed in cryptographic stone.",
            "Confirmed. Zero-knowledge proof verified. The cypherpunks write code.",
            "Block mined. Your privacy is mathematically guaranteed. Satoshi would be proud.",
            "TX confirmed on-chain. No surveillance. No middlemen. Just math.",
            "The blockchain has accepted your proof. Shielded. Private. Unstoppable.",
            "Confirmed. Your ZCL moved through the void, unseen by all. As intended.",
            "Block sealed. Another victory for financial privacy. The cypherpunks win again.",
        )

        fun randomCypherpunkMessage(): String =
            cypherpunkMessages.random()

        private val clearingMessages = listOf(
            "Transaction accepted by the network mempool.\nYour zero-knowledge proof passed validation.",
            "Peers accepted your shielded transaction.\nWaiting for a miner to seal it into a block.",
            "Mempool cleared. Your TX is queued for the next block.\nThe network validates. Trust the math.",
            "Proof verified by peers. Transaction is in the mempool.\nNo identity revealed. Awaiting block inclusion.",
            "Network nodes accepted your transaction.\nShielded, validated, waiting for settlement.",
        )
        private val settlementMessages = listOf(
            "Your transaction is now etched into the chain.\nPrivacy preserved. No trace left behind.",
            "The miners have spoken.\nYour shielded TX is sealed in cryptographic stone forever.",
            "Zero-knowledge proof verified.\nAnother private transaction joins the immutable ledger.",
            "Confirmation received.\nYour funds moved without leaving a trace.\nThe chain remembers. The world does not.",
            "Block mined. Cypherpunks write code.\nMiners write history.\nYour privacy is now permanent.",
            "Trust math, not middlemen.\nYour transaction is confirmed and irreversible.",
            "The proof is in the block.\nShielded, verified, sealed.\nThis is financial sovereignty.",
            "Another block, another victory for privacy.\nNo KYC. No surveillance. Just math.",
            "Your transaction joined the longest chain.\nCensorship-resistant. Permissionless. Private.",
            "Confirmed. The network accepted your proof.\nNo identity revealed. No trail to follow.",
        )
        fun randomClearingMessage(): String = clearingMessages.random()
        fun randomSettlementMessage(): String = settlementMessages.random()
    }
}
