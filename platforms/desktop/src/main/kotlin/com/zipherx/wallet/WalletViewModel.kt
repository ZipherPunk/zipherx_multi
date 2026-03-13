package com.zipherx.wallet

import com.zipherx.wallet.platform.DesktopSecureStorage
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// Sync task status
enum class SyncTaskStatus { PENDING, IN_PROGRESS, COMPLETED, FAILED }

data class SyncTask(
    val id: String,
    val title: String,
    val status: SyncTaskStatus = SyncTaskStatus.PENDING,
    val detail: String? = null,
    val progress: Float? = null,        // 0.0 to 1.0
    val startTimeMs: Long? = null,      // System.currentTimeMillis when started
    val endTimeMs: Long? = null,        // System.currentTimeMillis when completed
)

/**
 * Desktop WalletViewModel — manages wallet state using Kotlin coroutines.
 *
 * No Android ViewModel dependency. Uses a CoroutineScope tied to the
 * application lifecycle (cancelled on shutdown).
 */
class WalletViewModel(
    private val storage: DesktopSecureStorage,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    // --- State flows ---
    private val _walletState = MutableStateFlow("uninitialized")
    val walletState: StateFlow<String> = _walletState.asStateFlow()

    private val _address = MutableStateFlow<String?>(null)
    val address: StateFlow<String?> = _address.asStateFlow()

    private val _balance = MutableStateFlow(Balance(0, 0, 0, 0))
    val balance: StateFlow<Balance> = _balance.asStateFlow()

    private val _transactions = MutableStateFlow<List<Transaction>>(emptyList())
    val transactions: StateFlow<List<Transaction>> = _transactions.asStateFlow()

    private val _syncPhase = MutableStateFlow("Idle")
    val syncPhase: StateFlow<String> = _syncPhase.asStateFlow()

    private val _syncProgress = MutableStateFlow(0f)
    val syncProgress: StateFlow<Float> = _syncProgress.asStateFlow()

    private val _isSyncing = MutableStateFlow(false)
    val isSyncing: StateFlow<Boolean> = _isSyncing.asStateFlow()

    private val _peerCount = MutableStateFlow(0)
    val peerCount: StateFlow<Int> = _peerCount.asStateFlow()

    private val _torEnabled = MutableStateFlow(false)
    val torEnabled: StateFlow<Boolean> = _torEnabled.asStateFlow()

    private val _blockHeight = MutableStateFlow(0L)
    val blockHeight: StateFlow<Long> = _blockHeight.asStateFlow()

    private val _version = MutableStateFlow("")
    val version: StateFlow<String> = _version.asStateFlow()

    private val _syncTasks = MutableStateFlow<List<SyncTask>>(emptyList())
    val syncTasks: StateFlow<List<SyncTask>> = _syncTasks.asStateFlow()

    private val _overallProgress = MutableStateFlow(0f)
    val overallProgress: StateFlow<Float> = _overallProgress.asStateFlow()

    private val _syncStartTimeMs = MutableStateFlow(0L)
    val syncStartTimeMs: StateFlow<Long> = _syncStartTimeMs.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    private val _isLocked = MutableStateFlow(true)
    val isLocked: StateFlow<Boolean> = _isLocked.asStateFlow()

    // Send lifecycle: clearing (mempool) → settlement (block confirmation)
    private val _mempoolAccepted = MutableStateFlow(false)
    val mempoolAccepted: StateFlow<Boolean> = _mempoolAccepted.asStateFlow()

    private val _mempoolPeerStatus = MutableStateFlow<String?>(null)
    val mempoolPeerStatus: StateFlow<String?> = _mempoolPeerStatus.asStateFlow()

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

    private var sendTimestamp: Long? = null
    private var mempoolTimestamp: Long? = null
    private var confirmedSentCountAtSend: Int = 0

    private val _passwordError = MutableStateFlow<String?>(null)
    val passwordError: StateFlow<String?> = _passwordError.asStateFlow()

    private var skBytes: ByteArray? = null

    fun unlockWithPassword(password: String): Boolean {
        try {
            storage.unlockWithPassword(password)
        } catch (e: Exception) {
            _passwordError.value = "Unlock failed: ${e.message}"
            return false
        }
        // Try to load spending key — loadKey returns null if file not found,
        // throws on decryption failure (wrong password)
        try {
            val sk = storage.loadKey("spending_key")
            if (sk != null) {
                skBytes = sk
                _isLocked.value = false
                _passwordError.value = null
                loadWalletState()
                return true
            }
        } catch (e: javax.crypto.AEADBadTagException) {
            _passwordError.value = "Wrong password"
            storage.lock()
            return false
        } catch (e: Exception) {
            _passwordError.value = "Decryption failed: ${e.message}"
            storage.lock()
            return false
        }
        // No wallet yet — still unlocked for create/restore
        _isLocked.value = false
        _passwordError.value = null
        return true
    }

    fun clearPasswordError() {
        _passwordError.value = null
    }

    fun hasExistingWallet(): Boolean {
        return storage.hasKey("spending_key")
    }

    fun createWallet(): List<String>? {
        return try {
            val (words, sk) = ZipherXWrapper.createWallet()
            skBytes = sk.toSecureByteArray()
            _walletState.value = "locked"
            deriveAndSetAddress()
            startSync()
            words
        } catch (e: Exception) {
            _error.value = "Create wallet failed: ${e.message}"
            null
        }
    }

    fun restoreWallet(words: List<String>): Boolean {
        return try {
            val sk = ZipherXWrapper.restoreWallet(words)
            skBytes = sk.toSecureByteArray()
            _walletState.value = "locked"
            deriveAndSetAddress()
            startSync()
            true
        } catch (e: Exception) {
            _error.value = "Restore failed: ${e.message}"
            false
        }
    }

    fun importFromKey(keyString: String): Boolean {
        return try {
            val sk = if (keyString.startsWith("secret-extended-key")) {
                ZipherXWrapper.importFromEncodedKey(keyString)
            } else {
                ZipherXWrapper.importFromKey(keyString)
            }
            skBytes = sk.toSecureByteArray()
            _walletState.value = "locked"
            deriveAndSetAddress()
            startSync()
            true
        } catch (e: Exception) {
            _error.value = "Import failed: ${e.message}"
            false
        }
    }

    /** Convert List<UByte> to ByteArray for secure storage. */
    private fun List<UByte>.toSecureByteArray(): ByteArray = ByteArray(size) { this[it].toByte() }

    /** Convert ByteArray to List<UByte> for FFI calls. */
    private fun ByteArray.toUByteList(): List<UByte> = map { it.toUByte() }

    /**
     * Check if the device has enough free disk space for sync operations.
     * First sync requires ~4 GB (boost download + header DB), subsequent syncs ~1 GB.
     * Returns null if space is sufficient, or an error message string if not.
     */
    private fun checkDiskSpace(): String? {
        return try {
            val dataDir = storage.dataDir.toPath()
            val store = if (java.nio.file.Files.exists(dataDir)) {
                java.nio.file.Files.getFileStore(dataDir)
            } else {
                java.nio.file.Files.getFileStore(java.nio.file.Paths.get(System.getProperty("user.home")))
            }
            val availableBytes = store.usableSpace
            val availableGB = availableBytes / (1024.0 * 1024.0 * 1024.0)
            val headerDb = dataDir.resolve("headers.db")
            val isFirstSync = !java.nio.file.Files.exists(headerDb)
            // First sync: 2.1 GB boost file + 1.5 GB header DB + 0.5 GB delta + 1 GB working = ~6 GB
            val requiredGB = if (isFirstSync) 6.0 else 1.0
            val requiredLabel = if (isFirstSync) "6 GB" else "1 GB"
            if (availableGB < requiredGB) {
                "Insufficient storage: %.1f GB available, %s required for %s. Free up space and try again.".format(
                    availableGB,
                    requiredLabel,
                    if (isFirstSync) "initial sync" else "sync",
                )
            } else {
                null
            }
        } catch (_: Exception) {
            null // Can't check — proceed and let sync fail with its own error if needed
        }
    }

    fun startSync() {
        if (_isSyncing.value) return
        // Check available disk space before starting sync
        val storageError = checkDiskSpace()
        if (storageError != null) {
            _error.value = storageError
            return
        }
        scope.launch {
            _isSyncing.value = true
            _syncPhase.value = "Starting..."
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

            try {
                val sk = skBytes?.toUByteList() ?: emptyList()
                uniffi.zipherx.startSync(object : uniffi.zipherx.SyncProgressCallback {
                    override fun onProgress(phase: String, current: ULong, target: ULong) {
                        _syncPhase.value = phase
                        val phaseProgress = if (target > 0uL) current.toFloat() / target.toFloat() else 0f
                        _syncProgress.value = phaseProgress
                        _peerCount.value = ZipherXWrapper.getConnectedPeerCount().toInt()

                        // Update task list
                        if (phase != lastPhase) {
                            // Mark previous phase as completed, new phase as in-progress
                            markPhaseTransition(lastPhase, phase)
                            lastPhase = phase
                        }
                        updateTaskProgress(phase, current.toLong(), target.toLong(), phaseProgress)
                    }
                    override fun onComplete(height: ULong) {
                        _syncPhase.value = "Synced to $height"
                        _syncProgress.value = 1f
                        _overallProgress.value = 1f
                        _isSyncing.value = false
                        _blockHeight.value = height.toLong()
                        markAllTasksCompleted()
                        // Sequential: confirmation check before balance update
                        // so banner clears in same UI frame as balance change
                        scope.launch {
                            refreshHistoryAndCheckConfirmation()
                            try { _balance.value = ZipherXWrapper.getBalance() } catch (_: Exception) { }
                        }
                    }
                    override fun onError(message: String) {
                        _error.value = "Sync error: $message"
                        _isSyncing.value = false
                        markCurrentTaskFailed(lastPhase, message)
                    }
                })
            } catch (e: Exception) {
                _error.value = "Sync failed: ${e.message}"
                _isSyncing.value = false
            }
        }
    }

    /** Silent sync — updates DB confirmations without showing sync task bar UI. */
    private var isSyncingSilent = false
    private fun syncSilent() {
        if (_isSyncing.value || isSyncingSilent) return
        scope.launch {
            isSyncingSilent = true
            try {
                val sk = skBytes?.toUByteList() ?: emptyList()
                uniffi.zipherx.startSync(object : uniffi.zipherx.SyncProgressCallback {
                    override fun onProgress(phase: String, current: ULong, target: ULong) {
                        _peerCount.value = ZipherXWrapper.getConnectedPeerCount().toInt()
                    }
                    override fun onComplete(height: ULong) {
                        isSyncingSilent = false
                        _blockHeight.value = height.toLong()
                        scope.launch {
                            refreshHistoryAndCheckConfirmation()
                            try { _balance.value = ZipherXWrapper.getBalance() } catch (_: Exception) { }
                        }
                    }
                    override fun onError(message: String) {
                        isSyncingSilent = false
                    }
                })
            } catch (_: Exception) {
                isSyncingSilent = false
            }
        }
    }

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

    fun stopSync() {
        uniffi.zipherx.stopSync()
        _isSyncing.value = false
        _syncPhase.value = "Stopped"
    }

    fun refreshBalance() {
        scope.launch {
            try {
                _balance.value = ZipherXWrapper.getBalance()
            } catch (_: Exception) { }
        }
    }

    private suspend fun refreshHistoryAndCheckConfirmation() {
        try {
            val rawRecords = ZipherXWrapper.getHistory(50, 0)

            // Detect self-sends: txids that appear as both sent AND received
            val grouped = rawRecords.groupBy { it.txid }
            val records = mutableListOf<Transaction>()
            val processedTxids = mutableSetOf<String>()

            for (tx in rawRecords) {
                if (tx.txid in processedTxids) continue
                val group = grouped[tx.txid] ?: listOf(tx)
                val hasSent = group.any { it.txType == "sent" }
                val hasReceived = group.any { it.txType == "received" }

                if (hasSent && hasReceived) {
                    val sentTx = group.first { it.txType == "sent" }
                    records.add(sentTx.copy(txType = "self", amount = sentTx.fee))
                    processedTxids.add(tx.txid)
                } else {
                    records.add(tx)
                    processedTxids.add(tx.txid)
                }
            }

            _transactions.value = records
            checkPendingConfirmation()
        } catch (_: Exception) { }
    }

    fun refreshHistory() {
        scope.launch { refreshHistoryAndCheckConfirmation() }
    }

    /** Check if the pending send TX has been confirmed in a block.
     *  Uses TWO strategies: exact txid match OR detecting a new confirmed sent/self TX
     *  that appeared since the send. The second strategy handles txid mismatches. */
    private fun checkPendingConfirmation() {
        if (_pendingConfirmationTxid.value == null) return
        val txList = _transactions.value
        val pendingTxid = _pendingConfirmationTxid.value!!

        // Strategy 1: exact txid match
        val matchedByTxid = txList.any { it.txid == pendingTxid && it.confirmations > 0 }
        // Strategy 2: count confirmed sent/self TXs — if more than at send time, our TX confirmed
        val currentConfirmedCount = txList.count {
            it.confirmations > 0 && (it.txType == "sent" || it.txType == "self")
        }
        val matchedByCount = currentConfirmedCount > confirmedSentCountAtSend

        if (matchedByTxid || matchedByCount) {
            // Settlement detected — show celebration
            val elapsed = sendTimestamp?.let { (System.currentTimeMillis() - it) / 1000 }
            val durationStr = if (elapsed != null && elapsed > 0) "${elapsed}s" else ""
            // Find the confirmed TX (prefer exact match, fallback to newest)
            val confirmedTx = txList.firstOrNull { it.txid == pendingTxid && it.confirmations > 0 }
                ?: txList.firstOrNull { it.confirmations > 0 && (it.txType == "sent" || it.txType == "self") }
            _settlementTxid.value = confirmedTx?.txid ?: pendingTxid
            _settlementCelebration.value = randomSettlementMessage()
            _settlementDuration.value = durationStr
            _pendingConfirmationTxid.value = null
            _mempoolAccepted.value = false
            _mempoolPeerStatus.value = null
            // Post-settlement sync: new notes from the TX may lack witnesses.
            // One more sync rebuilds them so spendable count is correct.
            scope.launch {
                kotlinx.coroutines.delay(5_000)
                syncSilent()
            }
        }
    }

    /** Dismiss the clearing (mempool) celebration. */
    fun dismissClearing() {
        _clearingCelebration.value = null
        _clearingDuration.value = null
    }

    /** Dismiss the settlement (confirmation) celebration. */
    fun dismissSettlement() {
        _settlementCelebration.value = null
        _settlementDuration.value = null
        _settlementTxid.value = null
    }

    companion object {
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

    fun send(
        toAddress: String,
        amountZatoshis: Long,
        fee: Long,
        memo: String?,
        onPhase: (String, Int, Int) -> Unit,
        onComplete: (String, Long, Long) -> Unit,
        onError: (String) -> Unit,
    ) {
        val skArray = skBytes
        if (skArray == null) {
            onError("Wallet is locked")
            return
        }
        scope.launch {
            try {
                uniffi.zipherx.sendWithProgress(
                    toAddress,
                    amountZatoshis.toULong(),
                    fee.toULong(),
                    memo,
                    skArray.toUByteList(),
                    object : uniffi.zipherx.SendProgressCallback {
                        override fun onPhase(phase: String, current: UInt, total: UInt) {
                            // Detect mempool acceptance from peer_response phase
                            if (phase == "peer_response" && current.toInt() > 0) {
                                _mempoolAccepted.value = true
                                _mempoolPeerStatus.value = "$current/$total"
                                mempoolTimestamp = System.currentTimeMillis()
                            }
                            onPhase(phase, current.toInt(), total.toInt())
                        }
                        override fun onComplete(txid: String, amount: ULong, fee: ULong) {
                            sendTimestamp = System.currentTimeMillis()
                            _pendingConfirmationTxid.value = txid
                            // Snapshot confirmed sent/self count for fallback detection
                            confirmedSentCountAtSend = _transactions.value.count {
                                it.confirmations > 0 && (it.txType == "sent" || it.txType == "self")
                            }
                            // Show clearing (mempool) celebration
                            val clearingElapsed = mempoolTimestamp?.let {
                                (System.currentTimeMillis() - it) / 1000
                            }
                            _clearingCelebration.value = randomClearingMessage()
                            _clearingDuration.value = if (clearingElapsed != null) "${clearingElapsed}s" else null
                            _mempoolAccepted.value = true
                            // Don't refreshBalance() here — spent notes are already marked but change
                            // note won't appear until the TX is mined, so balance would show 0.
                            // Balance will update naturally when the first auto-sync completes.
                            refreshHistory()
                            onComplete(txid, amount.toLong(), fee.toLong())
                            // Background poll for confirmation — silent sync
                            // (no task bar UI) to update confirmations in DB
                            scope.launch {
                                repeat(12) {
                                    kotlinx.coroutines.delay(30_000)
                                    if (_pendingConfirmationTxid.value != null) {
                                        syncSilent()
                                    }
                                }
                                // Safety: auto-clear after all retries if still pending
                                if (_pendingConfirmationTxid.value != null) {
                                    _pendingConfirmationTxid.value = null
                                    _mempoolAccepted.value = false
                                    _mempoolPeerStatus.value = null
                                }
                            }
                        }
                        override fun onError(message: String) {
                            _mempoolAccepted.value = false
                            _mempoolPeerStatus.value = null
                            onError(message)
                        }
                    }
                )
            } catch (e: Exception) {
                onError(e.message ?: "Send failed")
            }
        }
    }

    /**
     * Return the encoded private key as a CharArray for export.
     * Caller MUST zero the CharArray after use to minimize key exposure in memory. (KD-5)
     */
    fun getEncodedPrivateKey(): CharArray? {
        val sk = skBytes ?: return null
        return try {
            val encoded = uniffi.zipherx.encodeSpendingKey(sk.toUByteList())
            encoded.toCharArray()
        } catch (_: Exception) {
            null
        }
    }

    fun setTorEnabled(enabled: Boolean) {
        ZipherXWrapper.setTorEnabled(enabled)
        _torEnabled.value = enabled
    }

    fun deleteAllData() {
        // Stop sync if running
        if (_isSyncing.value) {
            stopSync()
        }
        // Securely zero spending key bytes before releasing
        skBytes?.fill(0)
        skBytes = null
        _walletState.value = "uninitialized"
        _address.value = null
        _balance.value = Balance(0, 0, 0, 0)
        _transactions.value = emptyList()
        _syncPhase.value = "Idle"
        _syncProgress.value = 0f
        _overallProgress.value = 0f
        _syncTasks.value = emptyList()
        _peerCount.value = 0
        _blockHeight.value = 0L
        _error.value = null
        _mempoolAccepted.value = false
        _mempoolPeerStatus.value = null
        _pendingConfirmationTxid.value = null
        _clearingCelebration.value = null
        _clearingDuration.value = null
        _settlementCelebration.value = null
        _settlementDuration.value = null
        _settlementTxid.value = null
        // Delete ALL files on disk — keys, databases, delta, headers, everything
        storage.deleteAllData()
    }

    fun clearError() {
        _error.value = null
    }

    fun shutdown() {
        skBytes?.fill(0)
        skBytes = null
        scope.cancel()
        storage.lock()
    }

    private fun loadWalletState() {
        scope.launch {
            try {
                val summary = ZipherXWrapper.getSummary()
                _walletState.value = summary.state
                _blockHeight.value = summary.lastSyncedHeight
                deriveAndSetAddress()
                refreshBalance()
                refreshHistory()
                _peerCount.value = ZipherXWrapper.getConnectedPeerCount().toInt()
                _torEnabled.value = ZipherXWrapper.isTorEnabled()
                _version.value = ZipherXWrapper.getVersion()
                // Auto-start sync to reconnect peers (incremental — fast if already synced)
                startSync()
            } catch (_: Exception) { }
        }
    }

    private fun deriveAndSetAddress() {
        val sk = skBytes ?: return
        try {
            _address.value = ZipherXWrapper.deriveAddress(sk.toUByteList())
        } catch (_: Exception) { }
    }

    /** Verify a password against the current storage (for re-authentication). */
    fun verifyPassword(password: String): Boolean {
        return try {
            // Create a temporary storage to test decryption without affecting the current session
            val testStorage = com.zipherx.wallet.platform.DesktopSecureStorage(storage.dataDir)
            testStorage.unlockWithPassword(password)
            val result = testStorage.loadKey("spending_key")
            testStorage.lock()
            result != null
        } catch (e: Exception) {
            false
        }
    }
}
