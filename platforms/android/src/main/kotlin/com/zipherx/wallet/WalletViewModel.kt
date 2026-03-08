package com.zipherx.wallet

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
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

    private val _transactions = MutableStateFlow<List<Transaction>>(emptyList())
    val transactions: StateFlow<List<Transaction>> = _transactions.asStateFlow()

    private val _syncPhase = MutableStateFlow("idle")
    val syncPhase: StateFlow<String> = _syncPhase.asStateFlow()

    private val _syncProgress = MutableStateFlow(0.0)
    val syncProgress: StateFlow<Double> = _syncProgress.asStateFlow()

    private val _walletState = MutableStateFlow("uninitialized")
    val walletState: StateFlow<String> = _walletState.asStateFlow()

    private val _errorMessage = MutableStateFlow<String?>(null)
    val errorMessage: StateFlow<String?> = _errorMessage.asStateFlow()

    private val _isSyncing = MutableStateFlow(false)
    val isSyncing: StateFlow<Boolean> = _isSyncing.asStateFlow()

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
    }

    /**
     * KA-N4: Check if the device has network connectivity before attempting sync.
     * Returns true if connected (WiFi, cellular, ethernet, or VPN), false otherwise.
     */
    private fun isNetworkAvailable(): Boolean {
        val ctx = appContext ?: return true // If no context yet, assume connected
        val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return true
        val network = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(network) ?: return false
        return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
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
            // No biometrics enrolled — deny rather than silently bypass
            return false
        }
        return withContext(Dispatchers.IO) {
            bioAuth.authenticate(reason, activity)
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
                // Use refreshBalance() to get the actual DB balance (not summary which may be stale)
                refreshBalance()
                refreshHistory()
                walletLoaded = true
                // Auto-start sync if not already syncing
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
     * Start background sync. Progress is reported via [SyncCallback].
     */
    /** Hold strong reference to sync callback to prevent GC while Rust holds it. */
    private var activeSyncCallback: SyncCallback? = null

    fun startSync() {
        if (_isSyncing.value) return
        // KA-N4: Check network connectivity before attempting sync
        if (!isNetworkAvailable()) {
            if (BuildConfig.DEBUG) Log.w(TAG, "startSync() skipped — no network connectivity")
            _errorMessage.value = "No network connection. Please check your internet and try again."
            return
        }
        if (BuildConfig.DEBUG) Log.i(TAG, "startSync() called")
        viewModelScope.launch {
            try {
                _isSyncing.value = true
                _syncPhase.value = "starting"
                _errorMessage.value = null

                withContext(Dispatchers.IO) {
                    val callback = SyncCallback()
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
        val skArray = skBytes
        if (skArray == null) {
            _errorMessage.value = "Spending key not available"
            return
        }
        if (BuildConfig.DEBUG) Log.d(TAG, "send() called")
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
                if (bal.total > 0 && bal.spendable == 0L) {
                    if (BuildConfig.DEBUG) Log.w(TAG, "Notes may be missing witnesses/anchors")
                }
                _balance.value = bal
            } catch (e: Exception) {
                _errorMessage.value = e.message ?: "Failed to refresh balance"
            }
        }
    }

    /**
     * Refresh transaction history from the Rust core.
     */
    fun refreshHistory() {
        viewModelScope.launch {
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

                _transactions.value = history
            } catch (e: Exception) {
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
                        peers == 0u -> "Disconnected"
                        _isSyncing.value -> "Syncing"
                        summary.syncPhase == "complete" || summary.syncPhase == "idle" -> "Synced"
                        else -> "Connected"
                    }
                } catch (_: Exception) {
                    _networkStatus.value = "Error"
                }
                delay(5000)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sync Callback
    // -----------------------------------------------------------------------

    /**
     * Callback invoked by the Rust FFI during sync operations.
     * Updates StateFlows so the Compose UI recomposes on progress changes.
     */
    inner class SyncCallback : uniffi.zipherx.SyncProgressCallback {

        override fun onProgress(phase: String, current: ULong, target: ULong) {
            if (BuildConfig.DEBUG) Log.d(TAG, "Sync progress: phase=$phase current=$current target=$target")
            // Include height numbers in the phase for visible progress
            _syncPhase.value = if (target > 0uL && current > 0uL) {
                "${phase}:${current}:${target}"
            } else {
                phase
            }
            _syncProgress.value = if (target > 0uL) {
                current.toDouble() / target.toDouble()
            } else {
                0.0
            }
        }

        override fun onComplete(height: ULong) {
            if (BuildConfig.DEBUG) Log.i(TAG, "Sync complete at height $height")
            _isSyncing.value = false
            _syncPhase.value = "complete"
            _syncProgress.value = 1.0
            refreshBalance()
            refreshHistory()
            checkForTxConfirmation()

            // Continuous sync: re-sync after 60s to detect incoming transactions
            viewModelScope.launch {
                delay(60_000)
                if (!_isSyncing.value && walletLoaded) {
                    if (BuildConfig.DEBUG) Log.i(TAG, "Auto-resync: checking for new transactions...")
                    startSync()
                }
            }
        }

        override fun onError(message: String) {
            if (BuildConfig.DEBUG) Log.e(TAG, "Sync error: $message")
            _isSyncing.value = false
            _syncPhase.value = "failed"
            _errorMessage.value = message
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
            refreshBalance()
            refreshHistory()

            // Auto-sync periodically to catch block confirmation.
            // Zclassic block time is ~75s. Retry every 30s up to 6 times.
            viewModelScope.launch {
                repeat(6) {
                    delay(30_000)
                    if (_pendingConfirmationTxid.value != null && !_isSyncing.value) {
                        startSync()
                    }
                }
            }
        }

        override fun onError(message: String) {
            if (BuildConfig.DEBUG) Log.e(TAG, "Send error: $message")
            _isSending.value = false
            _sendPhase.value = null
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            _errorMessage.value = message
        }
    }

    // -----------------------------------------------------------------------
    // TX Lifecycle
    // -----------------------------------------------------------------------

    /**
     * Check if a pending TX just got confirmed (called after sync completes).
     */
    fun checkForTxConfirmation() {
        val pendingTxid = _pendingConfirmationTxid.value ?: return
        // Look for ANY entry (sent OR received) with this txid confirmed.
        // Self-sends produce both "sent" and "received" entries for the same txid.
        val confirmed = _transactions.value.any { it.txid == pendingTxid && it.confirmations > 0 }
        if (confirmed) {
            val elapsed = if (_sendTimestamp.value > 0) {
                (System.currentTimeMillis() - _sendTimestamp.value) / 1000
            } else {
                0L
            }
            val durationStr = if (elapsed > 0) formatDuration(elapsed) else null
            _confirmedTxid.value = pendingTxid
            _confirmationMessage.value = randomCypherpunkMessage() +
                (durationStr?.let { "\n\nBroadcast → Confirmed: $it" } ?: "")
            _pendingConfirmationTxid.value = null
            // Clear mempool status — TX is now confirmed in a block
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            // Post-settlement sync: new notes from the TX may lack witnesses.
            // One more sync rebuilds them so spendable count is correct.
            viewModelScope.launch {
                delay(5_000)
                if (!_isSyncing.value) startSync()
            }
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

            // Reset all StateFlows
            _balance.value = null
            _transactions.value = emptyList()
            _walletState.value = "uninitialized"
            _walletAddress.value = null
            _syncPhase.value = "idle"
            _syncProgress.value = 0.0
            _sendTxid.value = null
            _sendPhase.value = null
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            _confirmedTxid.value = null
            _confirmationMessage.value = null
            _incomingTxNotification.value = null
            _mnemonicWords.value = emptyList()
            _errorMessage.value = null

            // Delete database files
            withContext(Dispatchers.IO) {
                try {
                    uniffi.zipherx.deleteDatabase()
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.w(TAG, "deleteDatabase FFI call failed (may not exist): ${e.message}")
                }
            }

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
    }
}
