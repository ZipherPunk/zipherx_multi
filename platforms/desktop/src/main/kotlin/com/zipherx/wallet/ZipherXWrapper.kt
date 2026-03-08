package com.zipherx.wallet

/**
 * ZipherX Rust FFI Wrapper for Desktop.
 *
 * Bridges UniFFI-generated Kotlin bindings to idiomatic Kotlin types.
 * The UniFFI bindings are the same on all JVM platforms.
 * Desktop version — no Android dependencies.
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

data class ConnectedPeerInfo(
    val address: String,
    val protocolVersion: Int,
    val userAgent: String,
    val startHeight: Int,
)

data class BannedPeerInfo(
    val host: String,
    val reason: String,
    val isPermanent: Boolean,
    val remainingSeconds: Long,
)

// ---------------------------------------------------------------------------
// Wrapper Singleton
// ---------------------------------------------------------------------------

object ZipherXWrapper {

    private const val TAG = "ZipherX.Wrapper"

    var platformStorage: uniffi.zipherx.PlatformStorageCallback? = null

    private fun persistSpendingKey(skBytes: List<UByte>) {
        val storage = platformStorage
        if (storage != null) {
            storage.storeKey("spending_key", skBytes)
        }
    }

    fun initialize() {
        uniffi.zipherx.initializeRuntime()
    }

    fun shutdown() {
        uniffi.zipherx.shutdownRuntime()
    }

    fun generateMnemonic(): String {
        return uniffi.zipherx.generateMnemonic()
    }

    fun validateMnemonic(phrase: String): Boolean {
        return uniffi.zipherx.validateMnemonic(phrase)
    }

    fun validateAddress(address: String): Boolean {
        return uniffi.zipherx.validateAddress(address)
    }

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

    fun createWallet(): Pair<List<String>, List<UByte>> {
        val words = uniffi.zipherx.createWalletNew()
        val phrase = words.joinToString(" ")
        val seed = uniffi.zipherx.mnemonicToSeed(phrase)
        val skBytes = uniffi.zipherx.deriveSpendingKey(seed, 0u)
        persistSpendingKey(skBytes)
        return Pair(words, skBytes)
    }

    fun restoreWallet(words: List<String>): List<UByte> {
        uniffi.zipherx.restoreWallet(words)
        val phrase = words.joinToString(" ")
        val seed = uniffi.zipherx.mnemonicToSeed(phrase)
        val skBytes = uniffi.zipherx.deriveSpendingKey(seed, 0u)
        persistSpendingKey(skBytes)
        return skBytes
    }

    fun importFromKey(skHex: String): List<UByte> {
        val bytes = skHex.chunked(2).map { it.toUByte(16) }
        uniffi.zipherx.importWalletFromKey(bytes)
        persistSpendingKey(bytes)
        return bytes
    }

    fun importFromEncodedKey(encoded: String): List<UByte> {
        val skBytes = uniffi.zipherx.decodeSpendingKey(encoded)
        uniffi.zipherx.importWalletFromKey(skBytes)
        persistSpendingKey(skBytes)
        return skBytes
    }

    fun deriveAddress(skBytes: List<UByte>): String {
        return uniffi.zipherx.deriveAddress(skBytes, 0uL)
    }

    fun getBalance(): Balance {
        val info = uniffi.zipherx.getBalance()
        return Balance(
            total = info.total.toLong(),
            spendable = info.spendable.toLong(),
            noteCount = info.noteCount.toInt(),
            spendableNoteCount = info.spendableNoteCount.toInt(),
        )
    }

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

    fun getTransactionCounts(): Pair<UInt, UInt> {
        val counts = uniffi.zipherx.getTransactionCounts()
        return Pair(counts.sentCount, counts.receivedCount)
    }

    fun getConnectedPeerCount(): UInt {
        return try {
            uniffi.zipherx.getConnectedPeerCount()
        } catch (e: Exception) {
            0u
        }
    }

    fun getOnionAddress(): String? {
        return uniffi.zipherx.getOnionAddress()
    }

    fun getTorState(): UByte {
        return uniffi.zipherx.getTorState()
    }

    fun setTorEnabled(enabled: Boolean) {
        uniffi.zipherx.setTorEnabled(enabled)
    }

    fun isTorEnabled(): Boolean {
        return uniffi.zipherx.isTorEnabled()
    }

    fun getVersion(): String {
        return uniffi.zipherx.getVersion()
    }

    fun getConnectedPeers(): List<ConnectedPeerInfo> {
        return try {
            uniffi.zipherx.getConnectedPeers().map { p ->
                ConnectedPeerInfo(
                    address = p.address,
                    protocolVersion = p.protocolVersion.toInt(),
                    userAgent = p.userAgent,
                    startHeight = p.startHeight.toInt(),
                )
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun getBannedPeers(): List<BannedPeerInfo> {
        return try {
            uniffi.zipherx.getBannedPeers().map { p ->
                BannedPeerInfo(
                    host = p.host,
                    reason = p.reason,
                    isPermanent = p.isPermanent,
                    remainingSeconds = p.remainingSeconds.toLong(),
                )
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun addCustomPeer(host: String, port: Int): Boolean {
        val trimmed = host.trim()
        if (trimmed.isEmpty() || trimmed.length > 253) return false
        return try {
            uniffi.zipherx.addCustomPeer(trimmed, port.toUShort())
        } catch (_: Exception) {
            false
        }
    }

    fun unbanPeer(host: String): Boolean {
        return try {
            uniffi.zipherx.unbanPeer(host)
        } catch (_: Exception) {
            false
        }
    }

    fun disconnectPeer(peerId: String): Boolean {
        return try {
            uniffi.zipherx.disconnectPeer(peerId)
        } catch (_: Exception) {
            false
        }
    }
}
