package com.zipherx.wallet.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import com.zipherx.wallet.ZipherXWrapper
import androidx.compose.ui.text.input.PasswordVisualTransformation
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun SendScreen(
    viewModel: WalletViewModel,
    onBack: () -> Unit,
) {
    var toAddress by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    var memo by remember { mutableStateOf("") }
    var showConfirm by remember { mutableStateOf(false) }
    var showReAuth by remember { mutableStateOf(false) }
    var reAuthPassword by remember { mutableStateOf("") }
    var reAuthError by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var isSending by remember { mutableStateOf(false) }
    var sendPhase by remember { mutableStateOf("") }
    var sendSuccess by remember { mutableStateOf<String?>(null) }
    val balance by viewModel.balance.collectAsState()
    val peerCount by viewModel.peerCount.collectAsState()
    val mempoolPeerStatus by viewModel.mempoolPeerStatus.collectAsState()
    val pendingTxid by viewModel.pendingConfirmationTxid.collectAsState()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        // Header
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) {
                Icon(Icons.Filled.ArrowBack, "Back", tint = ZColors.primary)
            }
            Text(
                "> SEND ZCL",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
                color = ZColors.primary,
            )
        }
        Spacer(Modifier.height(16.dp))

        // Available balance
        val spendableZcl = balance.spendable / 100_000_000.0
        Text(
            "Available: %.8f ZCL".format(spendableZcl),
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primaryDim,
        )
        Spacer(Modifier.height(16.dp))

        // To Address (KD-8: real-time validation)
        val addressValid = toAddress.isNotBlank() && ZipherXWrapper.validateAddress(toAddress)
        Text("> TO ADDRESS", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        OutlinedTextField(
            value = toAddress,
            onValueChange = { toAddress = it; error = null },
            placeholder = { Text("zs1...", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
            modifier = Modifier.fillMaxWidth(),
            colors = terminalFieldColors(),
            shape = RoundedCornerShape(2.dp),
            singleLine = false,
            maxLines = 3,
            trailingIcon = {
                if (toAddress.isNotBlank()) {
                    Text(
                        if (addressValid) "[OK]" else "[!!]",
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        color = if (addressValid) ZColors.success else ZColors.error,
                        modifier = Modifier.padding(end = 8.dp),
                    )
                }
            },
        )
        Spacer(Modifier.height(12.dp))

        // Amount
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("> AMOUNT (ZCL)", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, fontWeight = FontWeight.Bold)
            OutlinedButton(
                onClick = {
                    val fee = 10_000L
                    val maxZatoshis = (balance.spendable - fee).coerceAtLeast(0)
                    amount = formatZatoshisToZcl(maxZatoshis)
                    error = null
                },
                shape = RoundedCornerShape(2.dp),
                border = BorderStroke(1.dp, ZColors.primaryDim),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primaryDim),
                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                modifier = Modifier.height(22.dp),
                enabled = balance.spendable > 10_000L,
            ) {
                Text("MAX", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 9.sp)
            }
        }
        Spacer(Modifier.height(4.dp))
        OutlinedTextField(
            value = amount,
            onValueChange = { amount = it; error = null },
            placeholder = { Text("0.00000000", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
            modifier = Modifier.fillMaxWidth(),
            colors = terminalFieldColors(),
            shape = RoundedCornerShape(2.dp),
            singleLine = true,
        )
        Spacer(Modifier.height(12.dp))

        // Memo (KD-7: byte-length validation, Sapling memo field limit is 512 bytes)
        val memoByteCount = memo.toByteArray(Charsets.UTF_8).size
        Text("> MEMO (optional)", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        OutlinedTextField(
            value = memo,
            onValueChange = { memo = it; error = null },
            placeholder = { Text("Encrypted memo", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
            modifier = Modifier.fillMaxWidth().height(80.dp),
            colors = terminalFieldColors(),
            shape = RoundedCornerShape(2.dp),
        )
        Spacer(Modifier.height(2.dp))
        Text(
            "$memoByteCount / 512 bytes",
            fontSize = 9.sp,
            fontFamily = FontFamily.Monospace,
            color = if (memoByteCount > 512) ZColors.error else ZColors.textDim,
        )
        Spacer(Modifier.height(4.dp))

        // Fee info (KD-9: Hardcoded fee is intentional for simplicity — Zclassic uses
        // a fixed 10,000 zatoshi fee for all shielded transactions per protocol convention.)
        Text(
            "Fee: 0.00010000 ZCL (10,000 zatoshis)",
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
        )

        // Warning if there's a pending unconfirmed TX
        if (pendingTxid != null) {
            Spacer(Modifier.height(8.dp))
            Text(
                "[!] Previous TX awaiting confirmation (${pendingTxid!!.take(12)}...)",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.warning,
            )
        }

        if (error != null) {
            Spacer(Modifier.height(8.dp))
            Text(error!!, fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
        }

        if (isSending && sendPhase.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            Text(sendPhase, fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
            Spacer(Modifier.height(4.dp))
            LinearProgressIndicator(
                modifier = Modifier.fillMaxWidth().height(4.dp),
                color = ZColors.primary,
                trackColor = ZColors.progressBg,
            )
        }

        Spacer(Modifier.weight(1f))

        // Send button
        OutlinedButton(
            onClick = {
                val parsedZatoshis = parseZclToZatoshis(amount)
                val fee = 10_000L
                if (peerCount == 0) {
                    error = "No peers connected. Start sync first to connect to the network."
                } else if (toAddress.isBlank()) {
                    error = "Address is required"
                } else if (!ZipherXWrapper.validateAddress(toAddress)) {
                    error = "Invalid shielded address"
                } else if (parsedZatoshis == null || parsedZatoshis <= 0L) {
                    error = "Invalid amount"
                } else if (parsedZatoshis + fee > balance.spendable) {
                    error = "Amount + fee exceeds spendable balance"
                } else if (memo.toByteArray(Charsets.UTF_8).size > 512) {
                    error = "Memo exceeds 512 bytes (${memo.toByteArray(Charsets.UTF_8).size} bytes)"
                } else {
                    showConfirm = true
                }
            },
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(2.dp),
            border = BorderStroke(1.dp, ZColors.primary),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
            enabled = !isSending && pendingTxid == null,
        ) {
            Text(
                if (isSending) "SENDING..." else "REVIEW & SEND",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 14.sp,
            )
        }
    }

    // Success celebration dialog
    if (sendSuccess != null) {
        var txidCopied by remember { mutableStateOf(false) }

        // Pulsing glow animation
        val infiniteTransition = rememberInfiniteTransition(label = "success_pulse")
        val glowAlpha by infiniteTransition.animateFloat(
            initialValue = 0.3f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(
                animation = tween(1000, easing = FastOutSlowInEasing),
                repeatMode = RepeatMode.Reverse,
            ),
            label = "glow_alpha",
        )

        AlertDialog(
            onDismissRequest = { sendSuccess = null; onBack() },
            containerColor = Color(0xFF0A0A0A),
            shape = RoundedCornerShape(2.dp),
            title = {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    // Checkmark with glow
                    Text(
                        "[+]",
                        fontSize = 36.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.success,
                        style = LocalTextStyle.current.copy(
                            shadow = Shadow(
                                ZColors.success.copy(alpha = glowAlpha),
                                Offset(0f, 0f),
                                16f,
                            )
                        ),
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "TRANSACTION BROADCAST",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        color = ZColors.success,
                        letterSpacing = 2.sp,
                    )
                }
            },
            text = {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    // Amount sent
                    Text(
                        "$amount ZCL",
                        fontSize = 20.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        style = LocalTextStyle.current.copy(
                            shadow = Shadow(ZColors.glow, Offset(0f, 0f), 8f)
                        ),
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        if (mempoolPeerStatus != null) "Accepted by $mempoolPeerStatus peers — awaiting miners"
                        else "Accepted by peers — awaiting miners",
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primaryDim,
                    )

                    Spacer(Modifier.height(16.dp))
                    HorizontalDivider(color = ZColors.border)
                    Spacer(Modifier.height(12.dp))

                    // TXID section
                    Text(
                        "TXID",
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primaryDim,
                        letterSpacing = 1.sp,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        sendSuccess!!,
                        fontSize = 8.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primary,
                        textAlign = TextAlign.Center,
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                            .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
                            .padding(8.dp),
                    )
                    Spacer(Modifier.height(8.dp))

                    // Auto-clear clipboard after 30 seconds
                    val txidScope = rememberCoroutineScope()
                    // Copy TXID button
                    OutlinedButton(
                        onClick = {
                            val clipboard = Toolkit.getDefaultToolkit().systemClipboard
                            clipboard.setContents(StringSelection(sendSuccess!!), null)
                            txidCopied = true
                            txidScope.launch {
                                delay(30_000)
                                clipboard.setContents(StringSelection(""), null)
                            }
                        },
                        shape = RoundedCornerShape(2.dp),
                        border = BorderStroke(1.dp, ZColors.primary),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
                    ) {
                        Icon(Icons.Filled.ContentCopy, null, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(4.dp))
                        Text(
                            if (txidCopied) "COPIED!" else "COPY TXID",
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                            fontSize = 10.sp,
                        )
                    }

                    Spacer(Modifier.height(12.dp))

                    // Cypherpunk quote
                    Text(
                        "\"In math we trust.\"",
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.textDim,
                        textAlign = TextAlign.Center,
                    )
                    Text(
                        "— ZipherX",
                        fontSize = 8.sp,
                        fontFamily = FontFamily.Monospace,
                        color = Color(0xFF3A5F3A),
                    )
                }
            },
            confirmButton = {
                OutlinedButton(
                    onClick = { sendSuccess = null; onBack() },
                    shape = RoundedCornerShape(2.dp),
                    border = BorderStroke(1.dp, ZColors.success),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.success),
                ) {
                    Text("DONE", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold)
                }
            },
        )
    }

    // Confirmation dialog
    if (showConfirm) {
        AlertDialog(
            onDismissRequest = { showConfirm = false },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = {
                Text("> CONFIRM SEND", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, color = ZColors.primary)
            },
            text = {
                Column {
                    Text("TO:", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, letterSpacing = 1.sp)
                    Spacer(Modifier.height(2.dp))
                    // Full address: prefix green, middle bold yellow, suffix green
                    val prefixLen = 12
                    val suffixLen = 12
                    val prefix = toAddress.take(prefixLen)
                    val suffix = toAddress.takeLast(suffixLen)
                    val middle = if (toAddress.length > prefixLen + suffixLen) {
                        toAddress.substring(prefixLen, toAddress.length - suffixLen)
                    } else ""
                    Text(
                        buildAnnotatedString {
                            withStyle(SpanStyle(color = ZColors.primary, fontSize = 9.sp)) { append(prefix) }
                            withStyle(SpanStyle(color = ZColors.warning, fontWeight = FontWeight.Bold, fontSize = 9.sp)) { append(middle) }
                            withStyle(SpanStyle(color = ZColors.primary, fontSize = 9.sp)) { append(suffix) }
                        },
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                            .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
                            .padding(6.dp),
                    )
                    Spacer(Modifier.height(8.dp))
                    Text("Amount: $amount ZCL", fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.primary)
                    Text("Fee: 0.00010000 ZCL", fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.textDim)
                    if (memo.isNotBlank()) {
                        Text("Memo: $memo", fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.textDim)
                    }
                    Spacer(Modifier.height(8.dp))
                    Text("This action is irreversible.", fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.warning)
                }
            },
            confirmButton = {
                OutlinedButton(
                    onClick = {
                        showConfirm = false
                        showReAuth = true
                    },
                    shape = RoundedCornerShape(2.dp),
                    border = BorderStroke(1.dp, ZColors.primary),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                ) {
                    Text("CONFIRM SEND", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showConfirm = false }) {
                    Text("CANCEL", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                }
            },
        )
    }

    // Re-authentication dialog — verify password before broadcast
    if (showReAuth) {
        AlertDialog(
            onDismissRequest = { showReAuth = false; reAuthPassword = ""; reAuthError = null },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = {
                Text(
                    "> VERIFY PASSWORD",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = ZColors.warning,
                )
            },
            text = {
                Column {
                    Text(
                        "Re-enter your password to authorize this transaction.",
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primaryDim,
                    )
                    Spacer(Modifier.height(12.dp))
                    OutlinedTextField(
                        value = reAuthPassword,
                        onValueChange = { reAuthPassword = it; reAuthError = null },
                        label = { Text("Password", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
                        visualTransformation = PasswordVisualTransformation(),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        colors = OutlinedTextFieldDefaults.colors(
                            focusedBorderColor = ZColors.primary,
                            unfocusedBorderColor = ZColors.border,
                            cursorColor = ZColors.primary,
                            focusedTextColor = ZColors.primary,
                            unfocusedTextColor = ZColors.primaryDim,
                        ),
                        shape = RoundedCornerShape(2.dp),
                    )
                    if (reAuthError != null) {
                        Spacer(Modifier.height(8.dp))
                        Text(reAuthError!!, fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    }
                }
            },
            confirmButton = {
                OutlinedButton(
                    onClick = {
                        if (viewModel.verifyPassword(reAuthPassword)) {
                            showReAuth = false
                            reAuthPassword = ""
                            reAuthError = null
                            // Password verified — proceed with send
                            isSending = true
                            sendPhase = "Preparing..."
                            val amountZatoshis = parseZclToZatoshis(amount) ?: 0L
                            val fee = 10_000L
                            viewModel.send(
                                toAddress = toAddress,
                                amountZatoshis = amountZatoshis,
                                fee = fee,
                                memo = memo.ifBlank { null },
                                onPhase = { phase, current, total ->
                                    sendPhase = when (phase) {
                                        "validating" -> "Validating..."
                                        "note_selection" -> "Selecting notes..."
                                        "witness_validation" -> "Validating witnesses ($current/$total)..."
                                        "building" -> "Building transaction (proof $current/$total)..."
                                        "broadcasting" -> "Broadcasting to peers..."
                                        "peer_response" -> "Peers: $current/$total accepted"
                                        "recording" -> "Recording in database..."
                                        "complete" -> "Complete!"
                                        else -> phase
                                    }
                                },
                                onComplete = { txid, _, _ ->
                                    isSending = false
                                    sendSuccess = txid
                                },
                                onError = { msg ->
                                    isSending = false
                                    error = msg
                                },
                            )
                        } else {
                            reAuthError = "Wrong password"
                        }
                    },
                    shape = RoundedCornerShape(2.dp),
                    border = BorderStroke(1.dp, ZColors.primary),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                ) {
                    Text("VERIFY & SEND", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showReAuth = false; reAuthPassword = ""; reAuthError = null }) {
                    Text("CANCEL", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                }
            },
        )
    }
}

/** Format zatoshis to ZCL string without floating-point arithmetic. */
private fun formatZatoshisToZcl(zatoshis: Long): String {
    val whole = zatoshis / 100_000_000L
    val frac = zatoshis % 100_000_000L
    return "%d.%08d".format(whole, frac)
}

/**
 * Parse a ZCL amount string to zatoshis without floating-point arithmetic.
 * Returns null if the input is not a valid number.
 */
private fun parseZclToZatoshis(text: String): Long? {
    val trimmed = text.trim()
    if (trimmed.isEmpty()) return null
    val parts = trimmed.split(".")
    if (parts.size > 2) return null
    val whole = parts[0].toLongOrNull() ?: return null
    val frac = if (parts.size > 1) {
        parts[1].padEnd(8, '0').take(8).toLongOrNull() ?: return null
    } else {
        0L
    }
    return whole * 100_000_000L + frac
}

@Composable
private fun terminalFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = ZColors.primary,
    unfocusedBorderColor = ZColors.border,
    cursorColor = ZColors.primary,
    focusedTextColor = ZColors.primary,
    unfocusedTextColor = ZColors.primaryDim,
    focusedPlaceholderColor = ZColors.textDim,
    unfocusedPlaceholderColor = ZColors.textDim,
)
