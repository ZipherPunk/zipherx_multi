package com.zipherx.wallet.platform

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context

/**
 * System clipboard operations for copying addresses, txids, and other text.
 *
 * Uses the Android [ClipboardManager] system service. On Android 13+ (API 33),
 * the system automatically shows a visual confirmation when content is copied,
 * so no toast is needed.
 */
class AndroidClipboard(private val context: Context) {

    private val clipboardManager: ClipboardManager
        get() = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager

    /**
     * Copy text to the system clipboard.
     *
     * @param text The string to copy (e.g. a Zclassic shielded address).
     */
    fun copyText(text: String) {
        val clip = ClipData.newPlainText("ZipherX", text)
        clipboardManager.setPrimaryClip(clip)
    }

    /**
     * Read text from the system clipboard.
     *
     * @return The clipboard text, or `null` if the clipboard is empty
     *         or does not contain text.
     */
    fun pasteText(): String? {
        val clip = clipboardManager.primaryClip ?: return null
        if (clip.itemCount == 0) return null
        return clip.getItemAt(0)?.text?.toString()
    }
}
