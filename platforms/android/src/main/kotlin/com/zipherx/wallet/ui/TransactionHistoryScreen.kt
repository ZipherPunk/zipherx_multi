package com.zipherx.wallet.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.CallMade
import androidx.compose.material.icons.automirrored.filled.CallReceived
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.SwapHoriz
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.zipherx.wallet.Transaction
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Full transaction history screen with a scrollable list of all transactions.
 *
 * KA-N7: Uses LazyColumn for efficient rendering. Currently loads all transactions
 * at once (up to 50 via getHistory). For wallets with very large transaction counts,
 * implement incremental pagination using the offset parameter in getHistory() and
 * LazyColumn's onReachEnd detection to load more items on scroll.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TransactionHistoryScreen(
    viewModel: WalletViewModel = viewModel(),
    onNavigateBack: () -> Unit = {},
) {
    val transactions by viewModel.transactions.collectAsState()
    val sentCount by viewModel.sentCount.collectAsState()
    val receivedCount by viewModel.receivedCount.collectAsState()
    var selectedTx by remember { mutableStateOf<Transaction?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Text(
                            text = "> HISTORY",
                            style = MaterialTheme.typography.titleMedium.copy(
                                fontFamily = FontFamily.Monospace,
                                fontWeight = FontWeight.Bold,
                                letterSpacing = 1.sp,
                            ),
                            color = ZColors.primary,
                        )
                        Text(
                            text = "IN:$receivedCount",
                            style = MaterialTheme.typography.labelSmall.copy(
                                fontFamily = FontFamily.Monospace,
                            ),
                            color = ZColors.primary,
                        )
                        Text(
                            text = "OUT:$sentCount",
                            style = MaterialTheme.typography.labelSmall.copy(
                                fontFamily = FontFamily.Monospace,
                            ),
                            color = ZColors.error,
                        )
                    }
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
        if (transactions.isEmpty()) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .background(ZColors.terminalBlack)
                    .padding(innerPadding),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = "> No transactions yet",
                    style = MaterialTheme.typography.bodyLarge.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                    color = ZColors.primaryDim,
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .background(ZColors.terminalBlack)
                    .padding(innerPadding)
                    .padding(horizontal = 16.dp),
            ) {
                items(transactions, key = { it.txid }) { tx ->
                    TransactionRow(
                        transaction = tx,
                        onClick = { selectedTx = tx },
                    )
                    Spacer(modifier = Modifier.height(6.dp))
                }
            }
        }

        selectedTx?.let { tx ->
            TransactionDetailDialog(
                transaction = tx,
                onDismiss = { selectedTx = null },
            )
        }
    }
}

/**
 * A single transaction row showing type icon, amount, and confirmation status.
 * Self-sends (txType "self") are displayed in yellow/amber.
 * Cypherpunk terminal styling: dark card, green border, monospace text.
 */
@Composable
fun TransactionRow(
    transaction: Transaction,
    modifier: Modifier = Modifier,
    onClick: () -> Unit = {},
) {
    val selfColor = ZColors.warning
    val icon: ImageVector
    val label: String
    val amountColor: Color

    val isSent = transaction.txType == "sent" || transaction.txType == "alpha"
    val isReceived = transaction.txType == "received" || transaction.txType == "beta"
    val isSelf = transaction.txType == "self"

    when {
        isSelf -> {
            icon = Icons.Default.SwapHoriz
            label = "SELF"
            amountColor = selfColor
        }
        isSent -> {
            icon = Icons.AutoMirrored.Filled.CallMade
            label = "SENT"
            amountColor = ZColors.error
        }
        isReceived -> {
            icon = Icons.AutoMirrored.Filled.CallReceived
            label = "RECEIVED"
            amountColor = ZColors.primary
        }
        else -> {
            icon = Icons.Default.SwapHoriz
            label = "CHANGE"
            amountColor = ZColors.primaryDim
        }
    }

    val confirmLabel = when (transaction.confirmations) {
        0L -> "Unconfirmed"
        1L -> "1 conf"
        else -> "${transaction.confirmations} conf"
    }
    val confirmColor = when {
        transaction.confirmations == 0L -> selfColor
        transaction.confirmations < 6 -> ZColors.primary.copy(alpha = 0.7f)
        else -> ZColors.primaryDim
    }

    Card(
        modifier = modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .border(1.dp, ZColors.primary.copy(alpha = 0.3f), RoundedCornerShape(2.dp)),
        shape = RoundedCornerShape(2.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (isSelf) Color(0xFF1A1A0E) else Color(0xFF0D0D0D),
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = label,
                tint = amountColor,
            )

            Spacer(modifier = Modifier.width(12.dp))

            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = label,
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        letterSpacing = 1.sp,
                    ),
                    color = if (isSelf) selfColor else ZColors.primary,
                )
                Text(
                    text = formatTimestamp(transaction.timestamp),
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                    color = ZColors.primaryDim,
                )
                // Truncated TXID
                Text(
                    text = transaction.txid.take(16) + "...",
                    style = MaterialTheme.typography.labelSmall.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 8.sp,
                    ),
                    color = ZColors.primaryDim.copy(alpha = 0.5f),
                )
            }

            Column(horizontalAlignment = Alignment.End) {
                val sign = if (isSent || isSelf) "-" else "+"
                Text(
                    text = "$sign${formatAmount(transaction.amount)}",
                    style = MaterialTheme.typography.bodyMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                    ),
                    color = amountColor,
                )
                Text(
                    text = confirmLabel,
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                    ),
                    color = confirmColor,
                )
            }
        }
    }
}

/**
 * Detail dialog for a transaction.
 * Cypherpunk terminal styling: dark background, green borders, monospace text.
 */
@Composable
fun TransactionDetailDialog(
    transaction: Transaction,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val isSent = transaction.txType == "sent" || transaction.txType == "alpha"
    val isReceived = transaction.txType == "received" || transaction.txType == "beta"
    val isSelf = transaction.txType == "self"
    val typeLabel = when {
        isSelf -> "SELF"
        isSent -> "SENT"
        isReceived -> "RECEIVED"
        else -> transaction.txType.uppercase()
    }
    val typeColor = when {
        isSelf -> ZColors.warning
        isSent -> ZColors.error
        isReceived -> ZColors.primary
        else -> ZColors.primaryDim
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = Color(0xFF0D0D0D),
        shape = RoundedCornerShape(2.dp),
        title = {
            Text(
                text = "> TRANSACTION DETAILS",
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
                // Type & Amount
                DetailRow("TYPE", typeLabel, typeColor)
                HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))
                val sign = if (isSent || isSelf) "-" else "+"
                DetailRow("AMOUNT", "$sign${formatAmount(transaction.amount)}", typeColor)
                if (transaction.fee > 0L) {
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))
                    DetailRow("FEE", formatAmount(transaction.fee))
                }
                HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))

                // Confirmations
                val confirmText = when (transaction.confirmations) {
                    0L -> "Unconfirmed (in mempool)"
                    1L -> "1 confirmation"
                    else -> "${transaction.confirmations} confirmations"
                }
                val confirmColor = when {
                    transaction.confirmations == 0L -> ZColors.warning
                    transaction.confirmations < 6 -> ZColors.primary
                    else -> ZColors.primary.copy(alpha = 0.7f)
                }
                DetailRow("CONFIRMATIONS", confirmText, confirmColor)
                HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))

                // Block height
                DetailRow(
                    "BLOCK HEIGHT",
                    if (transaction.height > 0) "%,d".format(transaction.height) else "Pending",
                )
                HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))

                // Date
                DetailRow("DATE", formatTimestampFull(transaction.timestamp))
                HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))

                // Memo
                if (!transaction.memo.isNullOrBlank()) {
                    DetailRow("MEMO", transaction.memo)
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.2f))
                }

                // TXID with copy button
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "TXID",
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                        letterSpacing = 1.sp,
                    ),
                    color = ZColors.primaryDim,
                )
                Spacer(modifier = Modifier.height(4.dp))
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(ZColors.terminalBlack)
                        .border(1.dp, ZColors.primaryDim.copy(alpha = 0.2f), RoundedCornerShape(0.dp))
                        .padding(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = transaction.txid,
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            fontSize = 9.sp,
                            lineHeight = 14.sp,
                        ),
                        color = ZColors.primaryDim,
                        modifier = Modifier.weight(1f),
                    )
                    IconButton(
                        onClick = {
                            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            clipboard.setPrimaryClip(ClipData.newPlainText("TXID", transaction.txid))
                            Toast.makeText(context, "TXID copied", Toast.LENGTH_SHORT).show()
                            // M-18: Auto-clear clipboard after 30 seconds
                            scope.launch {
                                delay(30_000)
                                clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                            }
                        },
                    ) {
                        Icon(
                            imageVector = Icons.Default.ContentCopy,
                            contentDescription = "Copy TXID",
                            tint = ZColors.primary,
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text(
                    text = "CLOSE",
                    color = ZColors.primaryDim,
                    style = MaterialTheme.typography.labelMedium.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                )
            }
        },
    )
}

@Composable
private fun DetailRow(label: String, value: String, valueColor: Color = ZColors.primary) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                letterSpacing = 1.sp,
            ),
            color = ZColors.primaryDim,
        )
        Spacer(modifier = Modifier.width(12.dp))
        Text(
            text = value,
            style = MaterialTheme.typography.bodySmall.copy(
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
            ),
            color = valueColor,
        )
    }
}

/**
 * Format a zatoshi amount as a short ZCL string.
 */
private fun formatAmount(zatoshis: Long): String {
    val whole = zatoshis / 100_000_000L
    val fraction = (zatoshis % 100_000_000L).let { if (it < 0) -it else it }
    return "%d.%08d ZCL".format(whole, fraction)
}

/**
 * Format a Unix timestamp to a human-readable date string.
 */
private fun formatTimestamp(timestamp: Long): String {
    if (timestamp == 0L) return "Pending"
    val sdf = SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.getDefault())
    return sdf.format(Date(timestamp * 1000))
}

/**
 * Format a Unix timestamp with full date, hour, minute, and seconds.
 */
private fun formatTimestampFull(timestamp: Long): String {
    if (timestamp == 0L) return "Pending"
    val sdf = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault())
    return sdf.format(Date(timestamp * 1000))
}
