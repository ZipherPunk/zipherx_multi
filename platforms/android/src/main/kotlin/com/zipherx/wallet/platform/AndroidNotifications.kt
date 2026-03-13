package com.zipherx.wallet.platform

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicInteger

/**
 * Local notification support via [NotificationManager].
 *
 * Creates a single notification channel on initialization and provides
 * methods to post and manage notifications for wallet events (TX received,
 * sync complete, etc.).
 */
class AndroidNotifications(private val context: Context) {

    companion object {
        private const val TAG = "ZipherX.Notifications"
        const val CHANNEL_ID = "zipherx_wallet"
        const val CHANNEL_NAME = "ZipherX Wallet"
        const val CHANNEL_DESCRIPTION = "Wallet transaction and sync notifications"
    }

    /** Auto-incrementing notification ID to avoid collisions. */
    private val notificationIdCounter = AtomicInteger(1000)

    init {
        createChannel()
    }

    /**
     * Create the notification channel (required on Android 8.0+ / API 26+).
     * Safe to call multiple times; the system ignores duplicate channels.
     */
    private fun createChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                CHANNEL_NAME,
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = CHANNEL_DESCRIPTION
                enableVibration(true)
                setShowBadge(true)
            }

            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE)
                    as NotificationManager
            manager.createNotificationChannel(channel)
            Log.d(TAG, "Notification channel created: $CHANNEL_ID")
        }
    }

    /**
     * Post a local notification.
     *
     * On Android 13+ (API 33), this requires the `POST_NOTIFICATIONS` runtime
     * permission. If the permission has not been granted, the notification is
     * silently dropped and the method returns `false`.
     *
     * @param title The notification title.
     * @param body The notification body text.
     * @return `true` if the notification was successfully posted.
     */
    fun sendNotification(title: String, body: String): Boolean {
        if (!hasNotificationPermission()) {
            Log.w(TAG, "POST_NOTIFICATIONS permission not granted; skipping notification")
            return false
        }

        return try {
            val notification = NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentTitle(title)
                .setContentText(body)
                .setPriority(NotificationCompat.PRIORITY_DEFAULT)
                .setAutoCancel(true)
                .build()

            val id = notificationIdCounter.getAndIncrement()
            NotificationManagerCompat.from(context).notify(id, notification)
            Log.d(TAG, "Notification posted (id=$id): $title")
            true
        } catch (e: SecurityException) {
            Log.e(TAG, "SecurityException posting notification: ${e.message}")
            false
        } catch (e: Exception) {
            Log.e(TAG, "Failed to post notification: ${e.message}")
            false
        }
    }

    /**
     * Check whether the app holds the notification permission.
     *
     * On API < 33, notifications are allowed by default. On API 33+,
     * the `POST_NOTIFICATIONS` runtime permission must be granted.
     *
     * @return `true` if notifications can be posted.
     */
    fun hasNotificationPermission(): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.POST_NOTIFICATIONS
            ) == PackageManager.PERMISSION_GRANTED
        } else {
            true
        }
    }
}
