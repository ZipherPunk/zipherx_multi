package com.zipherx.wallet

import android.app.Application
import android.util.Log
import com.zipherx.wallet.platform.AndroidSecureStorage
import com.zipherx.wallet.platform.registerPlatformServices
import java.security.SecureRandom

class ZipherXApplication : Application() {

    companion object {
        private const val TAG = "ZipherX.App"
    }

    var isNativeLoaded = false
        private set

    /**
     * Get or create a 32-byte AES-256 encryption key for the SQLCipher database.
     * Stored in Android Keystore-backed EncryptedSharedPreferences, separate
     * from the spending key. Generated once on first launch.
     */
    private fun getOrCreateDbEncryptionKey(): List<UByte> {
        val storage = AndroidSecureStorage(this)
        val existing = storage.loadKey("db_encryption_key")
        if (existing != null && existing.size == 32) {
            return existing.map { it.toUByte() }
        }
        // Generate a fresh 32-byte key using SecureRandom
        val key = ByteArray(32)
        SecureRandom().nextBytes(key)
        storage.storeKey("db_encryption_key", key)
        return key.map { it.toUByte() }
    }

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "ZipherX Application starting...")

        // Load the Rust FFI native library
        try {
            System.loadLibrary("zipherx_ffi")
            Log.i(TAG, "Native library loaded successfully")
            isNativeLoaded = true
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "Failed to load native library: ${e.message}")
            Log.w(TAG, "Running in UI-only mode (no Rust backend)")
            return
        }

        // Register platform services (Keystore, biometric, etc.)
        try {
            registerPlatformServices(this)
        } catch (e: Exception) {
            Log.e(TAG, "Platform services registration failed: ${e.message}")
        }

        // Initialize the Rust async runtime
        try {
            ZipherXWrapper.initialize()
            Log.i(TAG, "Rust runtime initialized")
        } catch (e: Exception) {
            Log.e(TAG, "Runtime init failed: ${e.message}")
        }

        // Initialize wallet storage paths with encrypted database
        try {
            val dataDir = filesDir.absolutePath
            val dbKey = getOrCreateDbEncryptionKey()

            // Use external storage for boost cache (large 2-4 GB files)
            // to avoid filling up limited internal storage.
            // Falls back to internal storage if external is unavailable.
            val boostCacheDir = (getExternalFilesDir(null) ?: filesDir).let {
                "$it/BoostCache"
            }
            Log.i(TAG, "Boost cache dir: $boostCacheDir")

            val config = WalletConfig(
                dbPath = "$dataDir/wallet.db",
                headerStorePath = "$dataDir/headers.bin",
                deltaStoreDir = "$dataDir/delta",
                spendParamsPath = "$dataDir/sapling-spend.params",
                outputParamsPath = "$dataDir/sapling-output.params",
                dbEncryptionKey = dbKey,
                boostCacheDir = boostCacheDir,
            )
            ZipherXWrapper.initializeWallet(config)
            Log.i(TAG, "Wallet storage initialized (encrypted)")
        } catch (e: Exception) {
            Log.w(TAG, "Wallet init: ${e.message}")
        }
    }
}
