package com.zipherx.wallet.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import com.zipherx.wallet.ZipherXWrapper
import kotlinx.coroutines.launch

/**
 * Secured send screen with:
 * 1. Real-time address validation
 * 2. Confirmation dialog with transaction details
 * 3. Clipboard address-swap guard
 * 4. Biometric authentication before broadcast
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendScreen(
    viewModel: WalletViewModel = viewModel(),
    onNavigateBack: () -> Unit = {},
) {
    var address by remember { mutableStateOf("") }
    var amount by remember { mutableStateOf("") }
    val fee = "0.0001" // Fixed fee: 10,000 zatoshis
    var memo by remember { mutableStateOf("") }
    var addressValid by remember { mutableStateOf(false) }
    var addressWasPasted by remember { mutableStateOf(false) }
    var showConfirmDialog by remember { mutableStateOf(false) }
    var clipboardWarning by remember { mutableStateOf<String?>(null) }

    val isSending by viewModel.isSending.collectAsState()
    val sendPhase by viewModel.sendPhase.collectAsState()
    val sendTxid by viewModel.sendTxid.collectAsState()
    val balance by viewModel.balance.collectAsState()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    val terminalFieldColors = OutlinedTextFieldDefaults.colors(
        focusedTextColor = ZColors.primary,
        unfocusedTextColor = ZColors.primary,
        cursorColor = ZColors.primary,
        focusedBorderColor = ZColors.primary,
        unfocusedBorderColor = ZColors.primaryDim,
        disabledBorderColor = ZColors.primaryDim.copy(alpha = 0.4f),
        focusedLabelColor = ZColors.primaryDark,
        unfocusedLabelColor = ZColors.primaryDim,
        disabledLabelColor = ZColors.primaryDim.copy(alpha = 0.4f),
        focusedPlaceholderColor = ZColors.primaryDim.copy(alpha = 0.5f),
        unfocusedPlaceholderColor = ZColors.primaryDim.copy(alpha = 0.5f),
        focusedContainerColor = ZColors.terminalBlack,
        unfocusedContainerColor = ZColors.terminalBlack,
        disabledContainerColor = ZColors.terminalBlack,
        disabledTextColor = ZColors.primaryDim.copy(alpha = 0.5f),
        errorBorderColor = ZColors.error,
        errorLabelColor = ZColors.error,
        errorCursorColor = ZColors.error,
        errorTextColor = ZColors.primary,
        errorContainerColor = ZColors.terminalBlack,
    )

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "> SEND ZCL",
                        style = MaterialTheme.typography.titleMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp,
                        ),
                        color = ZColors.primary,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                            tint = ZColors.primary,
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = ZColors.terminalBlack,
                ),
            )
        },
        containerColor = ZColors.terminalBlack,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(ZColors.terminalBlack)
                .padding(innerPadding)
                .padding(horizontal = 16.dp)
                .verticalScroll(rememberScrollState()),
        ) {
            Spacer(modifier = Modifier.height(8.dp))

            // Spendable balance info
            balance?.let { bal ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(ZColors.surface)
                        .border(1.dp, ZColors.primaryDim.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                        .padding(12.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        text = "SPENDABLE:",
                        style = MaterialTheme.typography.labelMedium,
                        color = ZColors.primaryDim,
                    )
                    Text(
                        text = formatZatoshisAsZcl(bal.spendable),
                        style = MaterialTheme.typography.labelMedium.copy(
                            fontWeight = FontWeight.Bold,
                        ),
                        color = ZColors.primaryDark,
                    )
                }
                if (bal.total > 0 && bal.spendable == 0L) {
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "! Notes need witness rebuild -- re-sync to make funds spendable",
                        style = MaterialTheme.typography.bodySmall,
                        color = ZColors.error,
                    )
                }
                Spacer(modifier = Modifier.height(16.dp))
            }

            // Section label
            Text(
                text = "> TO ADDRESS",
                style = MaterialTheme.typography.labelMedium.copy(
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp,
                ),
                color = ZColors.primary,
            )
            Spacer(modifier = Modifier.height(6.dp))

            // Address field with validation
            OutlinedTextField(
                value = address,
                onValueChange = { newValue ->
                    // Detect paste: if new value is much longer than old, it was pasted
                    if (newValue.length > address.length + 5) {
                        addressWasPasted = true
                    }
                    address = newValue
                    addressValid = newValue.isNotBlank() && ZipherXWrapper.validateAddress(newValue)
                },
                label = { Text("Recipient Address") },
                placeholder = { Text("zs1...") },
                modifier = Modifier.fillMaxWidth().testTag("address_field"),
                singleLine = true,
                enabled = !isSending,
                colors = terminalFieldColors,
                shape = RoundedCornerShape(0.dp),
                textStyle = MaterialTheme.typography.bodySmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
                trailingIcon = {
                    if (address.isNotBlank()) {
                        if (addressValid) {
                            Icon(
                                Icons.Default.CheckCircle,
                                contentDescription = "Valid",
                                tint = ZColors.success,
                            )
                        } else {
                            Icon(
                                Icons.Default.Warning,
                                contentDescription = "Invalid",
                                tint = ZColors.error,
                            )
                        }
                    }
                },
                isError = address.isNotBlank() && !addressValid,
            )

            if (address.isNotBlank() && !addressValid) {
                Text(
                    text = "! Invalid shielded address",
                    style = MaterialTheme.typography.bodySmall,
                    color = ZColors.error,
                    modifier = Modifier.padding(start = 4.dp, top = 2.dp),
                )
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Section label
            Text(
                text = "> AMOUNT (ZCL)",
                style = MaterialTheme.typography.labelMedium.copy(
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp,
                ),
                color = ZColors.primary,
            )
            Spacer(modifier = Modifier.height(6.dp))

            // Amount field with Max button
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OutlinedTextField(
                    value = amount,
                    onValueChange = { newValue ->
                        val feeZatoshis = parseZclToZatoshis(fee)
                        val spendable = balance?.spendable ?: 0L
                        val maxZatoshis = (spendable - feeZatoshis).coerceAtLeast(0L)
                        val parsedZatoshis = parseZclToZatoshis(newValue)
                        amount = if (parsedZatoshis > maxZatoshis && maxZatoshis > 0L) {
                            formatZatoshisAsZclInput(maxZatoshis)
                        } else {
                            newValue
                        }
                    },
                    label = { Text("Amount") },
                    placeholder = { Text("0.00000000") },
                    modifier = Modifier.weight(1f).testTag("amount_field"),
                    singleLine = true,
                    enabled = !isSending,
                    colors = terminalFieldColors,
                    shape = RoundedCornerShape(0.dp),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                    ),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                )
                Spacer(modifier = Modifier.width(8.dp))
                OutlinedButton(
                    onClick = {
                        val feeZatoshis = parseZclToZatoshis(fee)
                        val spendable = balance?.spendable ?: 0L
                        val maxZatoshis = (spendable - feeZatoshis).coerceAtLeast(0L)
                        amount = formatZatoshisAsZclInput(maxZatoshis)
                    },
                    enabled = !isSending && balance != null,
                    shape = RoundedCornerShape(0.dp),
                    colors = ButtonDefaults.outlinedButtonColors(
                        contentColor = ZColors.primary,
                    ),
                    border = androidx.compose.foundation.BorderStroke(1.dp, ZColors.primaryDim),
                ) {
                    Text(
                        text = "MAX",
                        style = MaterialTheme.typography.labelMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        ),
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Fixed fee display (non-editable, matches Desktop)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(ZColors.surface)
                    .border(1.dp, ZColors.primaryDim.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                    .padding(12.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = "FEE:",
                    style = MaterialTheme.typography.labelMedium,
                    color = ZColors.primaryDim,
                )
                Text(
                    text = "0.0001 ZCL (10,000 zatoshis)",
                    style = MaterialTheme.typography.labelMedium,
                    color = ZColors.primaryDim,
                )
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Section label
            Text(
                text = "> MEMO",
                style = MaterialTheme.typography.labelMedium.copy(
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp,
                ),
                color = ZColors.primaryDim,
            )
            Spacer(modifier = Modifier.height(6.dp))

            // Memo field with Sapling 512-byte limit
            val memoBytes = memo.toByteArray(Charsets.UTF_8).size
            val memoOverLimit = memoBytes > 512

            OutlinedTextField(
                value = memo,
                onValueChange = { memo = it },
                label = { Text("Memo (optional)") },
                modifier = Modifier.fillMaxWidth(),
                maxLines = 3,
                enabled = !isSending,
                colors = terminalFieldColors,
                shape = RoundedCornerShape(0.dp),
                textStyle = MaterialTheme.typography.bodySmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
                supportingText = {
                    Text(
                        text = "$memoBytes / 512 bytes",
                        style = MaterialTheme.typography.bodySmall,
                        color = if (memoOverLimit) ZColors.error else ZColors.primaryDim,
                    )
                },
                isError = memoOverLimit,
            )
            if (memoOverLimit) {
                Text(
                    text = "! Memo exceeds 512-byte Sapling limit",
                    style = MaterialTheme.typography.bodySmall,
                    color = ZColors.error,
                    modifier = Modifier.padding(start = 4.dp, top = 2.dp),
                )
            }

            Spacer(modifier = Modifier.height(24.dp))

            // Send button - opens confirmation dialog
            Button(
                onClick = {
                    // Clipboard address-swap guard
                    clipboardWarning = null
                    if (addressWasPasted) {
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                        val clipText = clipboard?.primaryClip?.getItemAt(0)?.text?.toString()
                        if (clipText != null && clipText != address &&
                            clipText.startsWith("zs1") && ZipherXWrapper.validateAddress(clipText)
                        ) {
                            clipboardWarning = "Clipboard now contains a DIFFERENT valid address than what was pasted. " +
                                "This could indicate clipboard-hijacking malware. Verify the recipient address carefully."
                        }
                    }
                    showConfirmDialog = true
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.primary, RoundedCornerShape(0.dp))
                    .testTag("review_send_button"),
                enabled = !isSending && addressValid && parseZclToZatoshis(amount) > 0L && !memoOverLimit && (balance?.spendable ?: 0L) > 0L,
                shape = RoundedCornerShape(0.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = ZColors.primary,
                    contentColor = ZColors.terminalBlack,
                    disabledContainerColor = ZColors.primaryDim.copy(alpha = 0.3f),
                    disabledContentColor = ZColors.primaryDim,
                ),
            ) {
                Text(
                    text = if (isSending) "SENDING..." else "REVIEW & SEND",
                    style = MaterialTheme.typography.labelLarge.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        letterSpacing = 1.sp,
                    ),
                )
            }

            // Send phase indicator
            sendPhase?.let { phase ->
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "> ${formatSendPhase(phase)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = ZColors.primaryDark,
                )
            }

            // Success indicator
            sendTxid?.let { txid ->
                Spacer(modifier = Modifier.height(16.dp))
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(ZColors.surface)
                        .border(1.dp, ZColors.primary.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                        .padding(12.dp),
                ) {
                    Text(
                        text = "TRANSACTION SENT!",
                        style = MaterialTheme.typography.titleMedium.copy(
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp,
                        ),
                        color = ZColors.primary,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "TXID: $txid",
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            fontSize = 9.sp,
                        ),
                        color = ZColors.primaryDim,
                    )
                }
            }

            Spacer(modifier = Modifier.height(24.dp))
        }

        // Confirmation dialog
        if (showConfirmDialog) {
            val amountZatoshis = parseZclToZatoshis(amount)
            val feeZatoshis = parseZclToZatoshis(fee)
            val totalDeducted = amountZatoshis + feeZatoshis

            // Fee is fixed — no validation needed

            AlertDialog(
                onDismissRequest = { showConfirmDialog = false },
                containerColor = Color(0xFF0D0D0D),
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "> CONFIRM TRANSACTION",
                        style = MaterialTheme.typography.titleMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            letterSpacing = 1.sp,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = ZColors.primary,
                    )
                },
                text = {
                    Column(
                        modifier = Modifier
                            .border(1.dp, ZColors.primary.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                            .padding(8.dp),
                    ) {
                        // Clipboard warning
                        if (clipboardWarning != null) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(
                                        Color(0xFF1A0000),
                                        RoundedCornerShape(0.dp),
                                    )
                                    .border(
                                        1.dp,
                                        ZColors.error.copy(alpha = 0.6f),
                                        RoundedCornerShape(0.dp),
                                    )
                                    .padding(10.dp),
                                verticalAlignment = Alignment.Top,
                            ) {
                                Icon(
                                    Icons.Default.Warning,
                                    contentDescription = "Clipboard warning",
                                    tint = ZColors.error,
                                )
                                Spacer(modifier = Modifier.width(8.dp))
                                Text(
                                    text = clipboardWarning!!,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = ZColors.error,
                                )
                            }
                            Spacer(modifier = Modifier.height(12.dp))
                        }

                        ConfirmAddressRow(address)
                        ConfirmRow("Amount", formatZatoshisAsZcl(amountZatoshis))
                        ConfirmRow("Fee", formatZatoshisAsZcl(feeZatoshis))
                        ConfirmRow("Total deducted", formatZatoshisAsZcl(totalDeducted))
                        if (memo.isNotBlank()) {
                            ConfirmRow("Memo", memo)
                        }
                    }
                },
                confirmButton = {
                    Button(
                        onClick = {
                            showConfirmDialog = false
                            scope.launch {
                                // Biometric auth
                                val authed = viewModel.authenticateStrict(
                                    "Authorize sending ${formatZatoshisAsZcl(amountZatoshis)}"
                                )
                                if (authed) {
                                    val spendable = balance?.spendable ?: 0L
                                    val maxAmount = (spendable - feeZatoshis).coerceAtLeast(0L)
                                    val cappedAmount = amountZatoshis.coerceAtMost(maxAmount)
                                    val memoStr = memo.ifBlank { null }
                                    viewModel.send(address, cappedAmount, feeZatoshis, memoStr)
                                }
                            }
                        },
                        enabled = true,
                        shape = RoundedCornerShape(0.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.primary,
                            contentColor = ZColors.terminalBlack,
                        ),
                    ) {
                        Text(
                            text = "AUTHENTICATE & SEND",
                            style = MaterialTheme.typography.labelMedium.copy(
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.Bold,
                            ),
                        )
                    }
                },
                dismissButton = {
                    TextButton(onClick = { showConfirmDialog = false }) {
                        Text(
                            text = "CANCEL",
                            color = ZColors.primaryDim,
                            style = MaterialTheme.typography.labelMedium.copy(
                                fontFamily = FontFamily.Monospace,
                            ),
                        )
                    }
                },
            )
        }
    }
}

@Composable
private fun ConfirmAddressRow(addr: String) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
    ) {
        Text(
            text = "TO",
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                letterSpacing = 1.sp,
            ),
            color = ZColors.primaryDim,
        )
        Spacer(modifier = Modifier.height(4.dp))
        val highlightStyle = SpanStyle(
            color = ZColors.warning,
            fontWeight = FontWeight.Bold,
        )
        val normalStyle = SpanStyle(
            color = ZColors.primary,
        )
        val segLen = if (addr.length >= 30) 10 else addr.length / 3
        val midStart = addr.length / 2 - segLen / 2
        val midEnd = midStart + segLen
        val annotated = buildAnnotatedString {
            // Beginning - normal
            withStyle(normalStyle) { append(addr.substring(0, midStart)) }
            // Middle highlight only
            withStyle(highlightStyle) { append(addr.substring(midStart, midEnd)) }
            // End - normal
            withStyle(normalStyle) { append(addr.substring(midEnd)) }
        }
        Text(
            text = annotated,
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                lineHeight = 18.sp,
                fontSize = 9.sp,
            ),
            maxLines = 2,
            modifier = Modifier
                .fillMaxWidth()
                .background(ZColors.terminalBlack, RoundedCornerShape(0.dp))
                .border(1.dp, ZColors.primaryDim.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                .padding(8.dp),
        )
    }
}

@Composable
private fun ConfirmRow(label: String, value: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label.uppercase(),
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                letterSpacing = 1.sp,
            ),
            color = ZColors.primaryDim,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
            ),
            color = ZColors.primary,
        )
    }
}

/**
 * Parse a ZCL amount string to zatoshis without floating-point arithmetic.
 * Returns 0 if the input is not a valid number.
 */
private fun parseZclToZatoshis(zcl: String): Long {
    val trimmed = zcl.trim()
    if (trimmed.isEmpty()) return 0L
    val parts = trimmed.split(".")
    if (parts.size > 2) return 0L
    val whole = parts[0].toLongOrNull() ?: return 0L
    val frac = if (parts.size > 1) {
        parts[1].padEnd(8, '0').take(8).toLongOrNull() ?: return 0L
    } else {
        0L
    }
    return whole * 100_000_000L + frac
}

private fun formatZatoshisAsZclInput(zatoshis: Long): String {
    return "%.8f".format(zatoshis.toDouble() / 100_000_000.0)
}

private fun formatZatoshisAsZcl(zatoshis: Long): String {
    val whole = zatoshis / 100_000_000L
    val fraction = (zatoshis % 100_000_000L).let { if (it < 0) -it else it }
    return "%d.%08d ZCL".format(whole, fraction)
}

private fun formatSendPhase(phase: String): String {
    return when (phase) {
        "validating" -> "Validating inputs..."
        "note_selection" -> "Selecting notes..."
        "witness_validation" -> "Validating witnesses..."
        "building" -> "Building Groth16 proof..."
        "broadcasting" -> "Broadcasting to network..."
        "peer_response" -> "Waiting for peer acceptance..."
        "recording" -> "Recording transaction..."
        "complete" -> "Complete"
        "error" -> "Error"
        else -> phase.replaceFirstChar { it.uppercase() }
    }
}
