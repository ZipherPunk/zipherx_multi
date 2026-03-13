package com.zipherx.wallet.platform

import android.content.Context
import android.util.Log
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * AndroidX Biometric authentication (fingerprint / face / iris).
 *
 * Uses [BiometricPrompt] with [BIOMETRIC_STRONG] authenticator class,
 * requiring Class 3 biometrics as defined by Android CDD.
 */
class AndroidBiometricAuth(private val context: Context) {

    companion object {
        private const val TAG = "ZipherX.BiometricAuth"
        private const val AUTH_TIMEOUT_SECONDS = 60L
    }

    private val biometricManager: BiometricManager
        get() = BiometricManager.from(context)

    /**
     * Whether the device has biometric hardware capable of Class 3 (strong) authentication.
     */
    val isAvailable: Boolean
        get() {
            val result = biometricManager.canAuthenticate(BIOMETRIC_STRONG)
            return result != BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE &&
                   result != BiometricManager.BIOMETRIC_ERROR_HW_UNAVAILABLE
        }

    /**
     * Whether the user has enrolled at least one strong biometric credential.
     */
    val isEnrolled: Boolean
        get() = biometricManager.canAuthenticate(BIOMETRIC_STRONG) ==
                BiometricManager.BIOMETRIC_SUCCESS

    /**
     * Human-readable biometric type name.
     * Android does not expose the specific biometric modality to apps,
     * so this returns a generic label.
     */
    val biometricType: String
        get() = "Biometric"

    /**
     * Whether the device has any credential (biometric OR PIN/pattern/password).
     */
    val hasDeviceCredential: Boolean
        get() = biometricManager.canAuthenticate(BIOMETRIC_STRONG or DEVICE_CREDENTIAL) ==
                BiometricManager.BIOMETRIC_SUCCESS

    /**
     * Show a biometric prompt and block until the user authenticates or cancels.
     *
     * This bridges the asynchronous [BiometricPrompt] callback into a synchronous
     * result using a [CountDownLatch]. Must be called when [activity] is in the
     * resumed state.
     *
     * @param reason The message displayed on the biometric prompt.
     * @param activity The [FragmentActivity] hosting the prompt UI.
     * @return `true` if authentication succeeded, `false` on failure or cancellation.
     */
    fun authenticate(reason: String, activity: FragmentActivity): Boolean {
        return authenticateWith(reason, activity, BIOMETRIC_STRONG)
    }

    /**
     * Show a biometric-or-device-credential prompt. Falls back to PIN/pattern/password
     * when no biometrics are enrolled. Use for mandatory auth on security-sensitive
     * actions (e.g. sending funds).
     */
    fun authenticateStrict(reason: String, activity: FragmentActivity): Boolean {
        return authenticateWith(reason, activity, BIOMETRIC_STRONG or DEVICE_CREDENTIAL)
    }

    private fun authenticateWith(
        reason: String,
        activity: FragmentActivity,
        authenticators: Int,
    ): Boolean {
        val latch = CountDownLatch(1)
        val success = AtomicBoolean(false)

        val executor = ContextCompat.getMainExecutor(activity)

        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                Log.i(TAG, "Authentication succeeded")
                success.set(true)
                latch.countDown()
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                Log.w(TAG, "Authentication error ($errorCode): $errString")
                success.set(false)
                latch.countDown()
            }

            override fun onAuthenticationFailed() {
                Log.w(TAG, "Authentication failed (bad credential)")
                // Don't count down here; the system will let the user retry
                // or eventually call onAuthenticationError.
            }
        }

        val prompt = BiometricPrompt(activity, executor, callback)

        val builder = BiometricPrompt.PromptInfo.Builder()
            .setTitle("ZipherX Authentication")
            .setSubtitle(reason)
            .setAllowedAuthenticators(authenticators)

        // setNegativeButtonText is not allowed when DEVICE_CREDENTIAL is set
        if (authenticators and DEVICE_CREDENTIAL == 0) {
            builder.setNegativeButtonText("Cancel")
        }

        val promptInfo = builder.build()

        activity.runOnUiThread {
            prompt.authenticate(promptInfo)
        }

        latch.await(AUTH_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        return success.get()
    }
}
