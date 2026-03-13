package com.zipherx.wallet.platform

import android.app.ActivityManager
import android.app.Application
import android.content.Context
import android.os.Build
import android.os.Process
import android.provider.Settings
import androidx.lifecycle.ProcessLifecycleOwner
import androidx.lifecycle.Lifecycle
import java.io.File

/**
 * Android platform information: filesystem paths, device identity, OS version,
 * and runtime state.
 */
class AndroidPlatformInfo(private val context: Context) {

    /**
     * Primary data directory for wallet databases, boost files, etc.
     * Backed by internal storage (encrypted at rest on FBE devices).
     */
    val dataDirectory: File
        get() = context.filesDir

    /**
     * Log output directory. Created on first access if it does not exist.
     */
    val logDirectory: File
        get() = File(context.filesDir, "logs").also { it.mkdirs() }

    /**
     * Cache directory for temporary files (headers, delta bundles in transit).
     * The system may delete these files when storage is low.
     */
    val cacheDirectory: File
        get() = context.cacheDir

    /**
     * Stable per-device identifier.
     *
     * [Settings.Secure.ANDROID_ID] is unique per app-signing-key per device
     * and persists across reinstalls on Android 8+. It is NOT a hardware ID.
     */
    val deviceId: String
        get() = Settings.Secure.getString(
            context.contentResolver,
            Settings.Secure.ANDROID_ID
        ) ?: "unknown"

    /**
     * Human-readable OS description including release version and API level.
     */
    val osDescription: String
        get() = "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})"

    /**
     * Whether the app is running on an emulator rather than a physical device.
     * Uses a combination of build fingerprint and product heuristics.
     */
    val isSimulator: Boolean
        get() = Build.FINGERPRINT.contains("generic") ||
                Build.FINGERPRINT.startsWith("unknown") ||
                Build.MODEL.contains("Emulator") ||
                Build.MODEL.contains("Android SDK built for") ||
                Build.MANUFACTURER.contains("Genymotion") ||
                Build.PRODUCT.contains("sdk") ||
                Build.PRODUCT.contains("google_sdk") ||
                Build.PRODUCT.contains("sdk_gphone") ||
                Build.HARDWARE.contains("goldfish") ||
                Build.HARDWARE.contains("ranchu")

    /**
     * Whether the application is currently in the foreground (visible to the user).
     *
     * Uses [ProcessLifecycleOwner] from the AndroidX Lifecycle library, which
     * tracks the aggregate lifecycle of all activities in the process.
     */
    val isForeground: Boolean
        get() = try {
            ProcessLifecycleOwner.get().lifecycle.currentState
                .isAtLeast(Lifecycle.State.STARTED)
        } catch (e: Exception) {
            // Fallback: check via ActivityManager importance
            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager
            val importance = am?.runningAppProcesses
                ?.firstOrNull { it.pid == Process.myPid() }
                ?.importance
            importance != null &&
                importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
        }
}
