package com.zipherx.wallet.platform

import android.util.Log

/**
 * Android Logcat logging with level routing.
 *
 * Maps string-based log levels from the Rust FFI layer to the
 * corresponding [android.util.Log] priority constants.
 *
 * @param tag The Logcat tag used for all messages from this logger.
 */
class AndroidLogger(private val tag: String = "ZipherX") {

    /**
     * Write a log message at the given level.
     *
     * @param level One of "debug", "info", "warning", "error". Unrecognized
     *              levels default to [Log.INFO].
     * @param message The log message body.
     */
    fun log(level: String, message: String) {
        when (level.lowercase()) {
            "debug", "verbose" -> Log.d(tag, message)
            "info" -> Log.i(tag, message)
            "warning", "warn" -> Log.w(tag, message)
            "error", "critical" -> Log.e(tag, message)
            else -> Log.i(tag, "[$level] $message")
        }
    }

    /** Convenience: log at debug level. */
    fun debug(message: String) = Log.d(tag, message)

    /** Convenience: log at info level. */
    fun info(message: String) = Log.i(tag, message)

    /** Convenience: log at warning level. */
    fun warn(message: String) = Log.w(tag, message)

    /** Convenience: log at error level. */
    fun error(message: String) = Log.e(tag, message)
}
