package com.zipherx.wallet.platform

import android.content.Context
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import java.util.Base64

/**
 * Android Keystore-backed secure storage.
 *
 * Uses [EncryptedSharedPreferences] with a [MasterKey] backed by
 * Android Keystore (StrongBox when available). All byte array values
 * are stored as Base64-encoded strings.
 */
class AndroidSecureStorage(private val context: Context) {

    companion object {
        private const val TAG = "ZipherX.SecureStorage"
        private const val PREFS_FILE = "zipherx_secure_prefs"
    }

    private val masterKey: MasterKey by lazy {
        val spec = KeyGenParameterSpec.Builder(
            MasterKey.DEFAULT_MASTER_KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        ).apply {
            setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            setKeySize(256)
            if (isHardwareBacked) {
                setIsStrongBoxBacked(true)
            }
        }.build()

        MasterKey.Builder(context)
            .setKeyGenParameterSpec(spec)
            .build()
    }

    private val prefs: SharedPreferences by lazy {
        EncryptedSharedPreferences.create(
            context,
            PREFS_FILE,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }

    /**
     * Store a key-value pair in encrypted storage.
     *
     * @param identifier The key name.
     * @param data The raw bytes to store (Base64-encoded internally).
     * @return `true` if the write committed successfully.
     */
    fun storeKey(identifier: String, data: ByteArray): Boolean {
        return try {
            val encoded = Base64.getEncoder().encodeToString(data)
            prefs.edit().putString(identifier, encoded).commit()
        } catch (e: Exception) {
            Log.e(TAG, "storeKey failed for '$identifier': ${e.message}")
            false
        }
    }

    /**
     * Load a value from encrypted storage.
     *
     * @param identifier The key name.
     * @return The raw bytes, or `null` if the key does not exist or decryption fails.
     */
    fun loadKey(identifier: String): ByteArray? {
        return try {
            val encoded = prefs.getString(identifier, null) ?: return null
            Base64.getDecoder().decode(encoded)
        } catch (e: Exception) {
            Log.e(TAG, "loadKey failed for '$identifier': ${e.message}")
            null
        }
    }

    /**
     * Delete a key from encrypted storage.
     *
     * @param identifier The key name.
     * @return `true` if the removal committed successfully.
     */
    fun deleteKey(identifier: String): Boolean {
        return try {
            prefs.edit().remove(identifier).commit()
        } catch (e: Exception) {
            Log.e(TAG, "deleteKey failed for '$identifier': ${e.message}")
            false
        }
    }

    /**
     * Check whether a key exists in encrypted storage.
     *
     * @param identifier The key name.
     * @return `true` if the key is present.
     */
    fun hasKey(identifier: String): Boolean {
        return try {
            prefs.contains(identifier)
        } catch (e: Exception) {
            Log.e(TAG, "hasKey failed for '$identifier': ${e.message}")
            false
        }
    }

    /**
     * Whether the keystore is backed by dedicated hardware (StrongBox).
     *
     * StrongBox provides tamper-resistant key storage on devices that
     * ship with a secure element (Pixel 3+, Samsung Galaxy S9+, etc.).
     */
    val isHardwareBacked: Boolean
        get() = context.packageManager
            .hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)
}
