package com.zipherx.wallet.platform

import java.io.File
import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.PBEKeySpec
import javax.crypto.spec.SecretKeySpec

/**
 * Desktop secure storage using password-derived encryption (PBKDF2 + AES-256-GCM).
 *
 * ## Security Model (KD-1)
 *
 * Desktop platforms lack hardware-backed secure enclaves (Android Keystore /
 * iOS Secure Enclave). All key protection relies on the user's password:
 *
 *  - **Key derivation**: PBKDF2-HMAC-SHA256, 600,000 iterations, 256-bit output.
 *    At ~200-500 ms per attempt on modern hardware, this makes brute-force of
 *    strong passwords impractical (an 8-character mixed-case+digit password has
 *    ~47 bits of entropy = ~2^47 attempts = thousands of CPU-years).
 *  - **Encryption**: AES-256-GCM with per-file random 96-bit nonces.
 *  - **Password requirements**: minimum 8 characters (enforced by PasswordScreen).
 *    Users should choose passwords with high entropy; the PBKDF2 cost only buys
 *    time against offline attacks on weak passwords.
 *  - **No hardware security**: The derived key exists in process memory while
 *    unlocked. A memory dump or debugger attachment can extract it. This is an
 *    inherent limitation of desktop JVM applications without a hardware security
 *    module.
 *
 * Keys are stored as encrypted files in the data directory.
 * The encryption key is derived from the user's password at session start.
 * For the DB encryption key, we use OS keyring when available, falling back
 * to file-based encrypted storage.
 *
 * File format: Base64(salt[16] + nonce[12] + ciphertext + tag[16])
 */
class DesktopSecureStorage(val dataDir: File) {

    private val keysDir = File(dataDir, "keys").also { it.mkdirs() }
    private var derivedKey: ByteArray? = null

    /**
     * Set the session password. Derives an AES-256 key using PBKDF2-HMAC-SHA256.
     * Must be called before any load/store operations on encrypted keys.
     */
    fun unlockWithPassword(password: String) {
        // Use a fixed salt file for key derivation (per-installation)
        // Salt file includes HMAC for integrity protection against tampering
        val saltFile = File(keysDir, ".salt")
        val salt: ByteArray
        if (saltFile.exists()) {
            val stored = saltFile.readBytes()
            if (stored.size < 48) {
                // Legacy salt without HMAC — migrate it
                salt = stored
                writeSaltWithHmac(saltFile, salt)
            } else {
                // First 16 bytes = salt, remaining 32 = HMAC-SHA256
                salt = stored.sliceArray(0 until 16)
                val storedHmac = stored.sliceArray(16 until 48)
                val expectedHmac = computeSaltHmac(salt)
                if (!storedHmac.contentEquals(expectedHmac)) {
                    // May be a legacy HMAC without the random secret — try migrating
                    val legacyHmac = computeLegacySaltHmac(salt)
                    if (storedHmac.contentEquals(legacyHmac)) {
                        // Legacy HMAC matches — migrate to new HMAC with random secret
                        writeSaltWithHmac(saltFile, salt)
                    } else {
                        throw SecurityException("Salt file integrity check failed — file may have been tampered with")
                    }
                }
            }
        } else {
            salt = ByteArray(16).also { SecureRandom().nextBytes(it) }
            writeSaltWithHmac(saltFile, salt)
        }
        val factory = SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256")
        val spec = PBEKeySpec(password.toCharArray(), salt, 600_000, 256)
        derivedKey = factory.generateSecret(spec).encoded
    }

    private fun writeSaltWithHmac(file: File, salt: ByteArray) {
        val hmac = computeSaltHmac(salt)
        file.writeBytes(salt + hmac)
    }

    private fun computeSaltHmac(salt: ByteArray): ByteArray {
        val mac = javax.crypto.Mac.getInstance("HmacSHA256")
        // Key the HMAC with machine identity + random secret so salt is bound to this installation
        val hmacSecret = getOrCreateHmacSecret()
        val hmacKey = ("ZipherX-salt-integrity:${System.getProperty("user.home")}:" +
            Base64.getEncoder().encodeToString(hmacSecret)).toByteArray()
        mac.init(SecretKeySpec(hmacKey, "HmacSHA256"))
        return mac.doFinal(salt)
    }

    /** Compute HMAC using the legacy key (without random secret) for migration. */
    private fun computeLegacySaltHmac(salt: ByteArray): ByteArray {
        val mac = javax.crypto.Mac.getInstance("HmacSHA256")
        val hmacKey = "ZipherX-salt-integrity:${System.getProperty("user.home")}".toByteArray()
        mac.init(SecretKeySpec(hmacKey, "HmacSHA256"))
        return mac.doFinal(salt)
    }

    /**
     * Get or create a random HMAC secret file.
     * This adds entropy to the HMAC key beyond predictable system properties.
     */
    private fun getOrCreateHmacSecret(): ByteArray {
        val secretFile = File(keysDir, ".hmac_secret")
        if (secretFile.exists()) {
            val data = secretFile.readBytes()
            if (data.size >= 32) return data
        }
        val secret = ByteArray(32).also { SecureRandom().nextBytes(it) }
        secretFile.writeBytes(secret)
        return secret
    }

    val isUnlocked: Boolean get() = derivedKey != null

    fun lock() {
        derivedKey?.fill(0)
        derivedKey = null
    }

    fun storeKey(identifier: String, data: ByteArray): Boolean {
        val key = derivedKey ?: return false
        return try {
            val nonce = ByteArray(12).also { SecureRandom().nextBytes(it) }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce))
            val ciphertext = cipher.doFinal(data)
            // Store: nonce + ciphertext (includes GCM tag)
            val combined = nonce + ciphertext
            val file = File(keysDir, sanitize(identifier))
            file.writeText(Base64.getEncoder().encodeToString(combined))
            true
        } catch (e: Exception) {
            false
        }
    }

    /**
     * Load and decrypt a stored key.
     * Returns null if the file does not exist (no key stored).
     * Throws on decryption failure (wrong password) — callers MUST handle this
     * to distinguish "no wallet" from "wrong password".
     */
    fun loadKey(identifier: String): ByteArray? {
        val key = derivedKey ?: return null
        val file = File(keysDir, sanitize(identifier))
        if (!file.exists()) return null
        val combined = Base64.getDecoder().decode(file.readText().trim())
        if (combined.size < 28) throw SecurityException("Corrupted key file: too short")
        val nonce = combined.sliceArray(0 until 12)
        val ciphertext = combined.sliceArray(12 until combined.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce))
        // This throws AEADBadTagException if password is wrong
        return cipher.doFinal(ciphertext)
    }

    fun deleteKey(identifier: String): Boolean {
        val file = File(keysDir, sanitize(identifier))
        return file.delete()
    }

    fun hasKey(identifier: String): Boolean {
        return File(keysDir, sanitize(identifier)).exists()
    }

    /**
     * Delete ALL data — keys, databases, delta files, everything in the data directory.
     * This is irreversible.
     */
    fun deleteAllData() {
        // Delete all key files
        keysDir.listFiles()?.forEach { it.delete() }
        // Delete all files in data dir (wallet.db, headers.db, delta/, params, etc.)
        dataDir.listFiles()?.forEach { file ->
            if (file.isDirectory) {
                file.deleteRecursively()
            } else {
                file.delete()
            }
        }
        // Also try to remove from OS keyring
        try {
            val keyring = com.github.javakeyring.Keyring.create()
            keyring.deletePassword("ZipherX", "db_encryption_key")
        } catch (_: Exception) { }
    }

    /**
     * Store a key without password encryption (for DB encryption key).
     * Uses OS keyring via java-keyring, falling back to machine-bound
     * encrypted file (AES-256-GCM keyed from machine identity).
     */
    fun storeUnlockedKey(identifier: String, data: ByteArray): Boolean {
        return try {
            // Try OS keyring first
            try {
                val keyring = com.github.javakeyring.Keyring.create()
                keyring.setPassword("ZipherX", identifier, Base64.getEncoder().encodeToString(data))
                return true
            } catch (_: Exception) { }
            // Fallback: encrypt with machine-derived key
            val machineKey = deriveMachineKey()
            val nonce = ByteArray(12).also { SecureRandom().nextBytes(it) }
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(machineKey, "AES"), GCMParameterSpec(128, nonce))
            val ciphertext = cipher.doFinal(data)
            machineKey.fill(0)
            val file = File(keysDir, ".${sanitize(identifier)}")
            file.writeText(Base64.getEncoder().encodeToString(nonce + ciphertext))
            true
        } catch (e: Exception) {
            false
        }
    }

    /**
     * Load a key stored without password encryption (for DB encryption key).
     */
    fun loadUnlockedKey(identifier: String): ByteArray? {
        return try {
            // Try OS keyring first
            try {
                val keyring = com.github.javakeyring.Keyring.create()
                val encoded = keyring.getPassword("ZipherX", identifier)
                if (encoded != null) return Base64.getDecoder().decode(encoded)
            } catch (_: Exception) { }
            // Fallback: decrypt with machine-derived key
            val file = File(keysDir, ".${sanitize(identifier)}")
            if (!file.exists()) return null
            val combined = Base64.getDecoder().decode(file.readText().trim())
            if (combined.size < 28) return null
            val nonce = combined.sliceArray(0 until 12)
            val ciphertext = combined.sliceArray(12 until combined.size)
            val machineKey = deriveMachineKey()
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(machineKey, "AES"), GCMParameterSpec(128, nonce))
            val result = cipher.doFinal(ciphertext)
            machineKey.fill(0)
            result
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Derive a 256-bit key from machine-specific identifiers + a persisted random secret.
     * This is NOT as secure as OS keyring, but prevents trivial
     * file-copy attacks. The key is bound to this machine's identity
     * and a random secret that makes it unpredictable even if system
     * properties are known.
     *
     * SECURITY LIMITATION (KD-2): This is a FALLBACK used only when the OS
     * keyring (macOS Keychain, GNOME Keyring, Windows Credential Manager) is
     * unavailable. The machine-derived key depends on:
     *   1. System properties (user.name, user.home, os.name, os.arch) — known
     *      to any process running as the same user.
     *   2. A random 32-byte secret persisted in `.machine_secret` — protects
     *      against remote attackers who know system properties but don't have
     *      filesystem access.
     *
     * An attacker with read access to the data directory can extract both the
     * encrypted key file AND the machine secret, then derive the same key.
     * The OS keyring path avoids this because the keyring itself is protected
     * by the OS login session. Prefer installing a supported keyring provider
     * (e.g., `gnome-keyring`, `kwallet`) on Linux desktops.
     */
    private fun deriveMachineKey(): ByteArray {
        val userName = System.getProperty("user.name") ?: "unknown"
        val userHome = System.getProperty("user.home") ?: "/tmp"
        val osName = System.getProperty("os.name") ?: "unknown"
        val osArch = System.getProperty("os.arch") ?: "unknown"
        // Include a persisted random secret to prevent predictability
        val machineSecret = getOrCreateMachineSecret()
        val secretHex = machineSecret.joinToString("") { "%02x".format(it) }
        val identity = "ZipherX-machine-key:$userName:$userHome:$osName:$osArch:$secretHex"
        // Fixed salt for machine key derivation (not secret — just ensures uniqueness)
        val salt = "ZipherX-fallback-storage-v1".toByteArray()
        val factory = SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256")
        val spec = PBEKeySpec(identity.toCharArray(), salt, 100_000, 256)
        return factory.generateSecret(spec).encoded
    }

    /**
     * Get or create a random machine secret file.
     * This adds entropy to the machine key derivation beyond predictable system properties.
     */
    private fun getOrCreateMachineSecret(): ByteArray {
        val secretFile = File(keysDir, ".machine_secret")
        if (secretFile.exists()) {
            val data = secretFile.readBytes()
            if (data.size >= 32) return data
        }
        val secret = ByteArray(32).also { SecureRandom().nextBytes(it) }
        secretFile.writeBytes(secret)
        return secret
    }

    private fun sanitize(name: String): String {
        return name.replace(Regex("[^a-zA-Z0-9_-]"), "_") + ".enc"
    }
}
