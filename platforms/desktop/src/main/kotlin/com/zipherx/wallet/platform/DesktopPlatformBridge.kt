package com.zipherx.wallet.platform

import java.io.File
import java.security.SecureRandom

/**
 * Desktop platform bridge — implements the UniFFI PlatformStorageCallback
 * for Rust FFI integration on desktop (Windows/Linux/macOS).
 */
class DesktopPlatformStorageCallback(
    private val storage: DesktopSecureStorage,
) : uniffi.zipherx.PlatformStorageCallback {

    override fun loadKey(key: String): List<UByte>? {
        // For spending key, use password-encrypted storage
        // For db_encryption_key, use OS keyring
        val data = if (key == "db_encryption_key") {
            storage.loadUnlockedKey(key)
        } else {
            storage.loadKey(key)
        }
        return data?.map { it.toUByte() }
    }

    override fun storeKey(key: String, value: List<UByte>): Boolean {
        val bytes = value.map { it.toByte() }.toByteArray()
        return if (key == "db_encryption_key") {
            storage.storeUnlockedKey(key, bytes)
        } else {
            storage.storeKey(key, bytes)
        }
    }

    override fun deleteKey(key: String): Boolean {
        return storage.deleteKey(key)
    }

    override fun hasKey(key: String): Boolean {
        return storage.hasKey(key)
    }
}

/**
 * Get or create a 32-byte DB encryption key.
 * Stored in OS keyring (not password-protected — available at app start).
 */
fun getOrCreateDbEncryptionKey(storage: DesktopSecureStorage): List<UByte> {
    val existing = storage.loadUnlockedKey("db_encryption_key")
    if (existing != null && existing.size == 32) {
        return existing.map { it.toUByte() }
    }
    val key = ByteArray(32)
    SecureRandom().nextBytes(key)
    storage.storeUnlockedKey("db_encryption_key", key)
    return key.map { it.toUByte() }
}

/**
 * Get the platform-specific data directory.
 */
fun getDataDirectory(): File {
    val os = System.getProperty("os.name").lowercase()
    val dir = when {
        os.contains("win") -> {
            val appData = System.getenv("LOCALAPPDATA") ?: System.getProperty("user.home")
            File(appData, "ZipherX")
        }
        os.contains("mac") -> {
            File(System.getProperty("user.home"), "Library/Application Support/ZipherX")
        }
        else -> {
            // Linux: XDG_DATA_HOME or ~/.local/share
            val xdg = System.getenv("XDG_DATA_HOME")
            if (xdg != null) File(xdg, "zipherx")
            else File(System.getProperty("user.home"), ".local/share/zipherx")
        }
    }
    dir.mkdirs()
    return dir
}
