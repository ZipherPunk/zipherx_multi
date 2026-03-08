package com.zipherx.wallet

import android.util.Log
import com.zipherx.wallet.BuildConfig

/**
 * ZipherX Rust FFI Wrapper for Android.
 *
 * Bridges UniFFI-generated Kotlin bindings to idiomatic Kotlin types.
 * After running `build-android.sh`, UniFFI generates `uniffi.zipherx.*`
 * which this wrapper delegates to.
 *
 * TODO: KA-N1 — Certificate pinning should be implemented for any future
 *  HTTPS endpoints (e.g., boost file server). Use OkHttp CertificatePinner
 *  with SHA-256 pin(s) for known hosts once an HTTP client dependency is added.
 *  All current network traffic goes through the Rust P2P layer (not HTTP).
 */

// ---------------------------------------------------------------------------
// Data Classes (mirror the UDL dictionary types)
// ---------------------------------------------------------------------------

data class WalletConfig(
    val dbPath: String,
    val headerStorePath: String,
    val deltaStoreDir: String,
    val spendParamsPath: String,
    val outputParamsPath: String,
    val accountIndex: UInt = 0u,
    val dbEncryptionKey: List<UByte>? = null,
)

data class Balance(
    val total: Long,
    val spendable: Long,
    val noteCount: Int,
    val spendableNoteCount: Int,
)

data class WalletSummary(
    val state: String,
    val address: String?,
    val totalBalance: Long,
    val spendableBalance: Long,
    val noteCount: Int,
    val lastSyncedHeight: Long,
    val chainTip: Long,
    val startupMode: String?,
    val syncPhase: String,
)

data class Transaction(
    val txid: String,
    val txType: String,
    val amount: Long,
    val fee: Long,
    val address: String?,
    val memo: String?,
    val confirmations: Long,
    val height: Long,
    val timestamp: Long,
)

// ---------------------------------------------------------------------------
// Wrapper Singleton
// ---------------------------------------------------------------------------

object ZipherXWrapper {

    private const val TAG = "ZipherX.Wrapper"

    /** Reference to platform storage for persisting SK after create/import. */
    var platformStorage: uniffi.zipherx.PlatformStorageCallback? = null

    /**
     * Store the spending key in platform secure storage so the Rust
     * sync engine can load it via loadKey("spending_key").
     */
    private fun persistSpendingKey(skBytes: List<UByte>) {
        val storage = platformStorage
        if (storage != null) {
            val stored = storage.storeKey("spending_key", skBytes)
            if (BuildConfig.DEBUG) Log.i(TAG, "persistSpendingKey: stored=$stored")
        } else {
            if (BuildConfig.DEBUG) Log.w(TAG, "persistSpendingKey: no platform storage registered!")
        }
    }

    /**
     * Initialize the Tokio async runtime on the Rust side.
     * Must be called once before any other wallet operations.
     */
    fun initialize() {
        try {
            uniffi.zipherx.initializeRuntime()
            if (BuildConfig.DEBUG) Log.i(TAG, "Runtime initialized")
        } catch (e: Exception) {
            if (BuildConfig.DEBUG) Log.e(TAG, "initialize() failed: ${e.message}")
            throw e
        }
    }

    /**
     * Shut down the Rust runtime.
     */
    fun shutdown() {
        uniffi.zipherx.shutdownRuntime()
    }

    /**
     * Generate a new 24-word BIP39 mnemonic phrase.
     */
    fun generateMnemonic(): String {
        return uniffi.zipherx.generateMnemonic()
    }

    /**
     * Validate a BIP39 mnemonic phrase.
     */
    fun validateMnemonic(phrase: String): Boolean {
        return uniffi.zipherx.validateMnemonic(phrase)
    }

    /**
     * Validate a Zclassic shielded address.
     */
    fun validateAddress(address: String): Boolean {
        return uniffi.zipherx.validateAddress(address)
    }

    /**
     * Initialize the wallet with paths and configuration.
     */
    fun initializeWallet(config: WalletConfig) {
        val ffiConfig = uniffi.zipherx.WalletConfigFfi(
            dbPath = config.dbPath,
            headerStorePath = config.headerStorePath,
            deltaStoreDir = config.deltaStoreDir,
            spendParamsPath = config.spendParamsPath,
            outputParamsPath = config.outputParamsPath,
            accountIndex = config.accountIndex,
            dbEncryptionKey = config.dbEncryptionKey,
        )
        uniffi.zipherx.initializeWallet(ffiConfig)
    }

    /**
     * Create a brand new wallet. Returns pair of (mnemonic words, SK bytes).
     */
    fun createWallet(): Pair<List<String>, List<UByte>> {
        val words = uniffi.zipherx.createWalletNew()
        val phrase = words.joinToString(" ")
        val seed = uniffi.zipherx.mnemonicToSeed(phrase)
        val skBytes = uniffi.zipherx.deriveSpendingKey(seed, 0u)
        persistSpendingKey(skBytes)
        return Pair(words, skBytes)
    }

    /**
     * Restore a wallet from an existing mnemonic phrase. Returns SK bytes.
     */
    fun restoreWallet(words: List<String>): List<UByte> {
        uniffi.zipherx.restoreWallet(words)
        val phrase = words.joinToString(" ")
        val seed = uniffi.zipherx.mnemonicToSeed(phrase)
        val skBytes = uniffi.zipherx.deriveSpendingKey(seed, 0u)
        persistSpendingKey(skBytes)
        return skBytes
    }

    /**
     * Import a wallet from a raw spending key (hex). Returns SK bytes.
     */
    fun importFromKey(skHex: String): List<UByte> {
        val bytes = skHex.chunked(2).map { it.toUByte(16) }
        uniffi.zipherx.importWalletFromKey(bytes)
        persistSpendingKey(bytes)
        return bytes
    }

    /**
     * Import a wallet from an encoded spending key. Returns SK bytes.
     */
    fun importFromEncodedKey(encoded: String): List<UByte> {
        val skBytes = uniffi.zipherx.decodeSpendingKey(encoded)
        uniffi.zipherx.importWalletFromKey(skBytes)
        persistSpendingKey(skBytes)
        return skBytes
    }

    /**
     * Derive the shielded address from spending key bytes.
     */
    fun deriveAddress(skBytes: List<UByte>): String {
        return uniffi.zipherx.deriveAddress(skBytes, 0uL)
    }

    /**
     * Get the current wallet balance.
     */
    fun getBalance(): Balance {
        val info = uniffi.zipherx.getBalance()
        return Balance(
            total = info.total.toLong(),
            spendable = info.spendable.toLong(),
            noteCount = info.noteCount.toInt(),
            spendableNoteCount = info.spendableNoteCount.toInt(),
        )
    }

    /**
     * Get a full summary of the wallet state.
     */
    fun getSummary(): WalletSummary {
        val s = uniffi.zipherx.getWalletSummary()
        return WalletSummary(
            state = s.state,
            address = s.address,
            totalBalance = s.totalBalance.toLong(),
            spendableBalance = s.spendableBalance.toLong(),
            noteCount = s.noteCount.toInt(),
            lastSyncedHeight = s.lastSyncedHeight.toLong(),
            chainTip = s.chainTip.toLong(),
            startupMode = s.startupMode,
            syncPhase = s.syncPhase,
        )
    }

    /**
     * Get paginated transaction history.
     */
    fun getHistory(limit: Int, offset: Int): List<Transaction> {
        val records = uniffi.zipherx.getTransactionHistory(limit.toUInt(), offset.toUInt())
        return records.map { tx ->
            Transaction(
                txid = tx.txid,
                txType = tx.txType,
                amount = tx.amount.toLong(),
                fee = tx.fee.toLong(),
                address = tx.address,
                memo = tx.memo,
                confirmations = tx.confirmations.toLong(),
                height = tx.height.toLong(),
                timestamp = tx.timestamp.toLong(),
            )
        }
    }

    /**
     * Get sent/received transaction counts.
     */
    fun getTransactionCounts(): Pair<UInt, UInt> {
        val counts = uniffi.zipherx.getTransactionCounts()
        return Pair(counts.sentCount, counts.receivedCount)
    }

    /**
     * Get the number of connected P2P peers.
     */
    fun getConnectedPeerCount(): UInt {
        return try {
            uniffi.zipherx.getConnectedPeerCount()
        } catch (e: Exception) {
            0u
        }
    }

    /**
     * Get the .onion address if Tor hidden service is initialized.
     */
    fun getOnionAddress(): String? {
        return uniffi.zipherx.getOnionAddress()
    }

    /**
     * Get the Tor connection state (0=disconnected, 3=connected).
     */
    fun getTorState(): UByte {
        return uniffi.zipherx.getTorState()
    }

    /**
     * Enable or disable Tor for P2P connections.
     * Tor is disabled by default. Takes effect on next sync.
     */
    fun setTorEnabled(enabled: Boolean) {
        uniffi.zipherx.setTorEnabled(enabled)
    }

    /**
     * Check whether Tor is currently enabled.
     */
    fun isTorEnabled(): Boolean {
        return uniffi.zipherx.isTorEnabled()
    }
}
