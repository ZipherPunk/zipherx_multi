package com.zipherx.wallet.platform

import android.content.Context
import android.util.Log

/**
 * Bridges [AndroidSecureStorage] to the Rust FFI `PlatformStorageCallback`.
 *
 * Implements the UniFFI-generated `PlatformStorageCallback` trait interface
 * to provide Android Keystore-backed secure storage to the Rust core.
 */
class AndroidPlatformStorageCallback(context: Context)
    : uniffi.zipherx.PlatformStorageCallback {

    companion object {
        private const val TAG = "ZipherX.PlatformBridge"
    }

    private val storage = AndroidSecureStorage(context)

    override fun loadKey(key: String): List<UByte>? {
        return storage.loadKey(key)?.map { it.toUByte() }
    }

    override fun storeKey(key: String, value: List<UByte>): Boolean {
        return storage.storeKey(key, value.map { it.toByte() }.toByteArray())
    }

    override fun deleteKey(key: String): Boolean {
        return storage.deleteKey(key)
    }

    override fun hasKey(key: String): Boolean {
        return storage.hasKey(key)
    }
}

/**
 * Aggregate holder for all platform service implementations.
 */
class PlatformServices(context: Context) {
    val secureStorage = AndroidSecureStorage(context)
    val biometricAuth = AndroidBiometricAuth(context)
    val platformInfo = AndroidPlatformInfo(context)
    val notifications = AndroidNotifications(context)
    val clipboard = AndroidClipboard(context)
    val logger = AndroidLogger()
}

/**
 * Register platform services with the Rust FFI layer.
 *
 * Call this once during `Application.onCreate()` before any Rust FFI calls
 * that require platform storage.
 *
 * @param context Application context (not Activity) to avoid leaks.
 */
fun registerPlatformServices(context: Context) {
    val appContext = context.applicationContext
    Log.i("ZipherX.PlatformBridge", "Registering platform services")

    // Ensure log directory exists
    val platformInfo = AndroidPlatformInfo(appContext)
    platformInfo.logDirectory // triggers mkdirs via lazy getter

    // Register secure storage callback with Rust and the Wrapper
    val storageCallback = AndroidPlatformStorageCallback(appContext)
    uniffi.zipherx.setPlatformStorage(storageCallback)
    com.zipherx.wallet.ZipherXWrapper.platformStorage = storageCallback

    Log.i("ZipherX.PlatformBridge", "Platform services registered (${platformInfo.osDescription})")
}
