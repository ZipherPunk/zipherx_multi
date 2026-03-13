package com.zipherx.wallet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.CallMade
import androidx.compose.material.icons.automirrored.filled.CallReceived
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.HourglassBottom
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.foundation.Image
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import com.zipherx.wallet.R
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.zipherx.wallet.SyncTask
import com.zipherx.wallet.SyncTaskStatus
import com.zipherx.wallet.Transaction
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.Canvas
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import kotlinx.coroutines.delay

/**
 * Main wallet screen that combines the balance card, action buttons,
 * and a summary of recent transactions.
 *
 * TODO: KA-N11 — Migrate hardcoded string literals (button labels, error messages,
 *  section headers) to Android string resources (res/values/strings.xml) for
 *  localization support and centralized text management.
 */
// ---------------------------------------------------------------------------
// Cypherpunk notification messages (matching egui macOS app)
// ---------------------------------------------------------------------------

private val clearingMessages = listOf(
    "Transaction accepted by the network mempool.\nYour zero-knowledge proof passed validation.",
    "Peers accepted your shielded transaction.\nWaiting for a miner to seal it into a block.",
    "Mempool cleared. Your TX is queued for the next block.\nThe network validates. Trust the math.",
    "Proof verified by peers. Transaction is in the mempool.\nNo identity revealed. Awaiting block inclusion.",
    "Network nodes accepted your transaction.\nShielded, validated, waiting for settlement.",
)

private val settlementMessages = listOf(
    "Your transaction is now etched into the chain.\nPrivacy preserved. No trace left behind.",
    "The miners have spoken.\nYour shielded TX is sealed in cryptographic stone forever.",
    "Zero-knowledge proof verified.\nAnother private transaction joins the immutable ledger.",
    "Confirmation received.\nYour funds moved without leaving a trace.\nThe chain remembers. The world does not.",
    "Block mined. Cypherpunks write code.\nMiners write history.\nYour privacy is now permanent.",
    "Trust math, not middlemen.\nYour transaction is confirmed and irreversible.",
    "The proof is in the block.\nShielded, verified, sealed.\nThis is financial sovereignty.",
    "Another block, another victory for privacy.\nNo KYC. No surveillance. Just math.",
    "Your transaction joined the longest chain.\nCensorship-resistant. Permissionless. Private.",
    "Confirmed. The network accepted your proof.\nNo identity revealed. No trail to follow.",
)

private val pendingSettlementMessages = listOf(
    "Your proof floats in the mempool.\nMiners compete to etch it into the next block.\nPatience — privacy takes time.",
    "The zero-knowledge proof is verified.\nNow the chain must seal it.\nNo one knows what you sent. Not even the miners.",
    "Cypherpunks wait for blocks, not banks.\nYour shielded TX is queued.\nThe math is done. The mining continues.",
    "Your transaction is invisible to surveillance.\nA miner will lock it into stone shortly.\nTrust the protocol.",
    "Mempool accepted. Block pending.\nThe network validates without seeing.\nThis is what financial privacy looks like.",
    "Shielded and waiting.\nNo address. No amount. No trace.\nJust a proof waiting for its block.",
)

private val pendingIncomingMessages = listOf(
    "An incoming shielded transfer detected in the mempool.\nWaiting for a miner to seal it into a block.",
    "Someone sent you ZCL through a zero-knowledge proof.\nThe network is processing it. Patience.",
    "Incoming funds detected. The proof is valid.\nA miner will etch it into the chain shortly.",
    "Shielded transfer inbound.\nNo sender identity revealed. No amount exposed.\nJust math, waiting for its block.",
    "The mempool holds your incoming ZCL.\nSoon a miner will confirm it forever.\nPrivacy works both ways.",
)

private fun randomClearingMessage(): String = clearingMessages.random()
private fun randomSettlementMessage(): String = settlementMessages.random()
private fun randomPendingSettlementMessage(): String = pendingSettlementMessages.random()
private fun randomPendingIncomingMessage(): String = pendingIncomingMessages.random()

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WalletScreen(
    viewModel: WalletViewModel = viewModel(),
    onNavigateToSend: () -> Unit = {},
    onNavigateToReceive: () -> Unit = {},
    onNavigateToHistory: () -> Unit = {},
    onNavigateToSettings: () -> Unit = {},
) {
    val balance by viewModel.balance.collectAsState()
    val errorMessage by viewModel.errorMessage.collectAsState()
    val isSyncing by viewModel.isSyncing.collectAsState()
    val syncPhase by viewModel.syncPhase.collectAsState()
    val syncProgress by viewModel.syncProgress.collectAsState()
    val transactions by viewModel.transactions.collectAsState()
    val mempoolAccepted by viewModel.mempoolAccepted.collectAsState()
    val mempoolPeerStatus by viewModel.mempoolPeerStatus.collectAsState()
    val confirmedTxid by viewModel.confirmedTxid.collectAsState()
    val confirmationMessage by viewModel.confirmationMessage.collectAsState()
    val sendTxid by viewModel.sendTxid.collectAsState()
    val sendAmount by viewModel.sendAmount.collectAsState()
    val incomingTx by viewModel.incomingTxNotification.collectAsState()
    val pendingTxid by viewModel.pendingConfirmationTxid.collectAsState()
    val syncTasks by viewModel.syncTasks.collectAsState()
    val overallProgress by viewModel.overallProgress.collectAsState()
    val syncStartTimeMs by viewModel.syncStartTimeMs.collectAsState()
    val isInitialSync by viewModel.isInitialSync.collectAsState()
    val clearingCelebration by viewModel.clearingCelebration.collectAsState()
    val clearingDuration by viewModel.clearingDuration.collectAsState()
    val settlementCelebration by viewModel.settlementCelebration.collectAsState()
    val settlementDuration by viewModel.settlementDuration.collectAsState()
    val settlementTxid by viewModel.settlementTxid.collectAsState()
    val pendingIncomingTxid by viewModel.pendingIncomingTxid.collectAsState()
    val pendingIncomingAmount by viewModel.pendingIncomingAmount.collectAsState()
    val incomingSettlementCelebration by viewModel.incomingSettlementCelebration.collectAsState()
    val incomingSettlementTxid by viewModel.incomingSettlementTxid.collectAsState()
    val boostFailed by viewModel.boostFailed.collectAsState()

    // KA-N3: These remember{} states survive configuration changes (rotation) but NOT
    // process death. Critical wallet state lives in WalletViewModel (ViewModel-scoped) which
    // survives config changes. For full process-death resilience, consider SavedStateHandle
    // for transient UI state like showConfirmationToast in the future.
    val context = LocalContext.current
    val snackbarHostState = remember { SnackbarHostState() }
    var showMempoolToast by remember { mutableStateOf(false) }
    var showConfirmationToast by remember { mutableStateOf(false) }
    var showIncomingToast by remember { mutableStateOf(false) }
    var showQuote by remember { mutableStateOf(false) }
    var showPendingWarning by remember { mutableStateOf(false) }
    var currentQuote by remember { mutableStateOf("") }
    var selectedTx by remember { mutableStateOf<Transaction?>(null) }

    // KA-N5: LaunchedEffect(Unit) is intentional — runs once on first composition to
    // trigger initial wallet load. Re-triggering is not desired; subsequent refreshes
    // happen via sync callbacks and explicit user actions.
    LaunchedEffect(Unit) {
        viewModel.loadWallet()
    }

    LaunchedEffect(errorMessage) {
        errorMessage?.let {
            snackbarHostState.showSnackbar(it)
            viewModel.clearError()
        }
    }

    // Show mempool toast when TX is accepted
    LaunchedEffect(mempoolAccepted, sendTxid) {
        if (mempoolAccepted && sendTxid != null) {
            showMempoolToast = true
            delay(8000)
            showMempoolToast = false
        }
    }

    // Show confirmation toast when TX is confirmed
    LaunchedEffect(confirmedTxid) {
        if (confirmedTxid != null) {
            showConfirmationToast = true
            delay(10000)
            showConfirmationToast = false
            viewModel.dismissConfirmation()
        }
    }

    // Show incoming TX toast
    LaunchedEffect(incomingTx) {
        if (incomingTx != null) {
            showIncomingToast = true
            delay(8000)
            showIncomingToast = false
            viewModel.dismissIncomingNotification()
        }
    }

    // Auto-dismiss quote after 5 seconds
    LaunchedEffect(showQuote, currentQuote) {
        if (showQuote) {
            delay(5000)
            showQuote = false
        }
    }

    val infiniteTransition = rememberInfiniteTransition(label = "shield_spin")
    val shieldRotationY by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 3000, easing = LinearEasing),
            repeatMode = RepeatMode.Restart,
        ),
        label = "shield_rotation_y",
    )

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                currentQuote = cypherpunkQuotes.random()
                                showQuote = true
                            },
                        horizontalArrangement = Arrangement.Center,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Image(
                            painter = painterResource(id = R.drawable.zipherx_logo),
                            contentDescription = "ZipherX logo",
                            modifier = (if (isSyncing) {
                                Modifier.graphicsLayer {
                                    rotationY = shieldRotationY
                                    cameraDistance = 12f * density
                                }
                            } else {
                                Modifier
                            }).size(32.dp),
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        Text(
                            text = "ZIPHERX",
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) { innerPadding ->
        Box(modifier = Modifier.fillMaxSize().padding(innerPadding)) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 16.dp)
                .verticalScroll(rememberScrollState()),
        ) {
                // Normal wallet view
                Spacer(modifier = Modifier.height(8.dp))

                BalanceCard(
                    balance = balance,
                    syncPhase = syncPhase,
                    syncProgress = syncProgress,
                    isSyncing = isSyncing,
                    isPendingConfirmation = pendingTxid != null,
                )

                // Detailed sync task UI — only shown during initial sync
                if (isInitialSync && isSyncing && syncTasks.isNotEmpty()) {
                    Spacer(modifier = Modifier.height(8.dp))
                    SyncTaskSection(
                        syncTasks = syncTasks,
                        overallProgress = overallProgress,
                        syncStartTimeMs = syncStartTimeMs,
                        isSyncing = isSyncing,
                    )
                }

                // Boost download failure dialog
                if (boostFailed != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    BoostFailedCard(
                        reason = boostFailed!!.first,
                        attempts = boostFailed!!.second,
                        onContinue = { viewModel.onBoostFailedContinue() },
                        onQuit = {
                            viewModel.onBoostFailedQuit()
                            (context as? android.app.Activity)?.finishAffinity()
                        },
                    )
                }

                // Clearing celebration (mempool accepted)
                if (clearingCelebration != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    CelebrationCard(
                        title = "MEMPOOL CLEARED",
                        subtitle = randomClearingMessage().split("\n").first() ?: "",
                        message = clearingCelebration!!,
                        duration = clearingDuration,
                        txid = pendingTxid,
                        color = ZColors.warning,
                        onAcknowledge = { viewModel.dismissClearing() },
                    )
                }

                // Pending settlement indicator (after clearing acknowledged, waiting for block)
                if (pendingTxid != null && clearingCelebration == null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    PendingSettlementBanner(pendingTxid = pendingTxid!!)
                }

                // Settlement celebration (block confirmed)
                if (settlementCelebration != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    CelebrationCard(
                        title = "BLOCK CONFIRMED",
                        subtitle = randomSettlementMessage().split("\n").first() ?: "",
                        message = settlementCelebration!!,
                        duration = settlementDuration,
                        txid = settlementTxid,
                        color = ZColors.success,
                        onAcknowledge = { viewModel.dismissSettlement() },
                    )
                }

                // Pending incoming TX banner (mempool detected, awaiting block)
                if (pendingIncomingTxid != null && incomingSettlementCelebration == null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    PendingIncomingBanner(
                        pendingTxid = pendingIncomingTxid!!,
                        amount = pendingIncomingAmount,
                    )
                }

                // Incoming TX settlement celebration (block confirmed)
                if (incomingSettlementCelebration != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    CelebrationCard(
                        title = "INCOMING CONFIRMED",
                        subtitle = randomSettlementMessage().split("\n").first() ?: "",
                        message = incomingSettlementCelebration!!,
                        duration = null,
                        txid = incomingSettlementTxid,
                        color = Color(0xFF00E676),
                        onAcknowledge = { viewModel.dismissIncomingSettlement() },
                    )
                }

                Spacer(modifier = Modifier.height(16.dp))

                Button(
                    onClick = {
                        if (pendingTxid != null) {
                            showPendingWarning = true
                        } else {
                            onNavigateToSend()
                        }
                    },
                    modifier = Modifier.fillMaxWidth().testTag("send_button"),
                    enabled = pendingTxid == null && (balance?.spendable ?: 0L) > 0L,
                ) {
                    val label = when {
                        pendingTxid != null -> "Send [Locked]"
                        (balance?.spendable ?: 0L) == 0L -> "Send [Syncing...]"
                        else -> "Send"
                    }
                    Text(label)
                }

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedButton(
                    onClick = onNavigateToReceive,
                    modifier = Modifier.fillMaxWidth().testTag("receive_button"),
                ) {
                    Text("Receive")
                }

                Spacer(modifier = Modifier.height(24.dp))

                Text(
                    text = "Recent Transactions",
                    style = MaterialTheme.typography.titleMedium,
                )

                Spacer(modifier = Modifier.height(8.dp))

                if (transactions.isEmpty()) {
                    Text(
                        text = "No transactions yet",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    transactions.take(5).forEach { tx ->
                        TransactionRow(
                            transaction = tx,
                            onClick = { selectedTx = tx },
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                    }

                    if (transactions.size > 5) {
                        OutlinedButton(
                            onClick = onNavigateToHistory,
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("View All Transactions")
                        }
                    }
                }

                Spacer(modifier = Modifier.height(16.dp))

                OutlinedButton(
                    onClick = onNavigateToSettings,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Settings")
                }

                Spacer(modifier = Modifier.height(24.dp))
        }

            // --- Notification toasts overlay ---

            // Mempool acceptance toast
            AnimatedVisibility(
                visible = showMempoolToast,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter).padding(top = 8.dp),
            ) {
                CypherpunkToast(
                    icon = Icons.Default.HourglassBottom,
                    iconColor = Color(0xFFFFC107),
                    title = "MEMPOOL CLEARED",
                    message = randomClearingMessage(),
                    accentColor = Color(0xFFFFC107),
                    onDismiss = {
                        showMempoolToast = false
                        viewModel.clearSendStatus()
                    },
                )
            }

            // Block confirmation toast
            AnimatedVisibility(
                visible = showConfirmationToast,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter).padding(top = 8.dp),
            ) {
                CypherpunkToast(
                    icon = Icons.Default.CheckCircle,
                    iconColor = Color(0xFF00E676),
                    title = "BLOCK CONFIRMED",
                    message = confirmationMessage ?: randomSettlementMessage(),
                    accentColor = Color(0xFF00E676),
                    onDismiss = {
                        showConfirmationToast = false
                        viewModel.dismissConfirmation()
                    },
                )
            }

            // Incoming TX toast
            AnimatedVisibility(
                visible = showIncomingToast,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter).padding(top = 8.dp),
            ) {
                incomingTx?.let { tx ->
                    val (title, message, color) = if (tx.confirmations > 0) {
                        Triple(
                            "BLOCK CONFIRMED",
                            "[ +${formatZclAmount(tx.amount)} ZCL ]\n${randomSettlementMessage()}",
                            Color(0xFF00E676),
                        )
                    } else {
                        Triple(
                            "INCOMING TX",
                            "[ +${formatZclAmount(tx.amount)} ZCL ]\n${randomClearingMessage()}",
                            Color(0xFF00BCD4),
                        )
                    }
                    CypherpunkToast(
                        icon = if (tx.confirmations > 0) Icons.Default.Lock else Icons.AutoMirrored.Filled.CallReceived,
                        iconColor = color,
                        title = title,
                        message = message,
                        accentColor = color,
                        onDismiss = {
                            showIncomingToast = false
                            viewModel.dismissIncomingNotification()
                        },
                    )
                }
            }

            // Cypherpunk quote toast
            AnimatedVisibility(
                visible = showQuote,
                enter = slideInVertically(initialOffsetY = { -it }) + fadeIn(),
                exit = slideOutVertically(targetOffsetY = { -it }) + fadeOut(),
                modifier = Modifier.align(Alignment.TopCenter).padding(top = 8.dp),
            ) {
                Card(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp)
                        .clickable { showQuote = false }
                        .border(1.dp, Color(0xFF00FF41).copy(alpha = 0.3f), RoundedCornerShape(12.dp)),
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(
                        containerColor = Color(0xFF0D1117),
                    ),
                    elevation = CardDefaults.cardElevation(defaultElevation = 8.dp),
                ) {
                    Text(
                        text = currentQuote,
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            lineHeight = 18.sp,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = Color(0xFF00FF41),
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
        } // Box

        // Transaction detail dialog
        selectedTx?.let { tx ->
            TransactionDetailDialog(
                transaction = tx,
                onDismiss = { selectedTx = null },
            )
        }

        // Pending TX warning dialog — sending is blocked
        if (showPendingWarning) {
            androidx.compose.material3.AlertDialog(
                onDismissRequest = { showPendingWarning = false },
                title = { Text("Send Locked") },
                text = {
                    Column {
                        Text("You have an unconfirmed transaction waiting for block confirmation.")
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            "Sending is disabled until the previous transaction confirms. This prevents double-spend risk.",
                            color = MaterialTheme.colorScheme.error,
                        )
                        if (pendingTxid != null) {
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                "tx: ${pendingTxid!!.take(16)}...",
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                    }
                },
                confirmButton = {
                    androidx.compose.material3.TextButton(onClick = { showPendingWarning = false }) {
                        Text("OK")
                    }
                },
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Last Transaction Activity (below balance)
// ---------------------------------------------------------------------------

@Composable
private fun LastTransactionActivity(
    transactions: List<Transaction>,
    mempoolAccepted: Boolean,
    mempoolPeerStatus: String?,
    sendTxid: String?,
) {
    val lastTx = transactions.firstOrNull() ?: return
    val isSelf = lastTx.txType == "self"
    val isSent = lastTx.txType == "sent" || lastTx.txType == "alpha"
    val isReceived = lastTx.txType == "received" || lastTx.txType == "beta"
    if (!isSent && !isReceived && !isSelf) return

    val (icon, label, color) = when {
        isSelf -> Triple(Icons.AutoMirrored.Filled.CallMade, "Self", Color(0xFFFFC107))
        isSent -> Triple(Icons.AutoMirrored.Filled.CallMade, "Sent", Color(0xFFFF5252))
        else -> Triple(Icons.AutoMirrored.Filled.CallReceived, "Received", Color(0xFF00E676))
    }

    val statusText = when {
        lastTx.confirmations == 0L && mempoolAccepted -> "In mempool (${mempoolPeerStatus ?: "?"} peers) — waiting for miner"
        lastTx.confirmations == 0L -> "Unconfirmed — broadcasting..."
        lastTx.confirmations == 1L -> "1 confirmation"
        else -> "${lastTx.confirmations} confirmations"
    }

    val statusColor = when {
        lastTx.confirmations == 0L -> Color(0xFFFFC107)
        lastTx.confirmations < 6 -> Color(0xFF00E676)
        else -> Color(0xFF00E676).copy(alpha = 0.7f)
    }

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 8.dp),
        colors = CardDefaults.cardColors(
            containerColor = Color(0xFF1A1A2E),
        ),
        shape = RoundedCornerShape(8.dp),
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
                tint = color,
                modifier = Modifier.size(20.dp),
            )
            Spacer(modifier = Modifier.width(8.dp))
            Column(modifier = Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val sign = if (isSent) "-" else "+"
                    Text(
                        text = "$sign${formatZclAmount(lastTx.amount)} ZCL",
                        style = MaterialTheme.typography.bodyMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = color,
                    )
                    Spacer(modifier = Modifier.width(6.dp))
                    Text(
                        text = label.uppercase(),
                        style = MaterialTheme.typography.labelSmall,
                        color = color.copy(alpha = 0.7f),
                    )
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (lastTx.confirmations == 0L) {
                        Icon(
                            imageVector = Icons.Default.HourglassBottom,
                            contentDescription = "Pending confirmation",
                            tint = statusColor,
                            modifier = Modifier.size(12.dp),
                        )
                        Spacer(modifier = Modifier.width(4.dp))
                    }
                    Text(
                        text = statusText,
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                        ),
                        color = statusColor,
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cypherpunk Toast Notification
// ---------------------------------------------------------------------------

@Composable
private fun CypherpunkToast(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    iconColor: Color,
    title: String,
    message: String,
    accentColor: Color,
    onDismiss: () -> Unit,
) {
    // Confetti particle animation
    val particleProgress = remember { Animatable(0f) }
    LaunchedEffect(Unit) {
        particleProgress.animateTo(
            targetValue = 1f,
            animationSpec = tween(durationMillis = 2500, easing = LinearEasing),
        )
    }
    val confettiColors = remember {
        listOf(
            Color(0xFF00E676), Color(0xFF00BCD4), Color(0xFFFFC107),
            Color(0xFF00FFA3), Color(0xFF76FF03), Color(0xFF18FFFF),
        )
    }
    data class Particle(val x: Float, val startY: Float, val speed: Float, val color: Color, val size: Float)
    val particles = remember {
        List(40) {
            Particle(
                x = Math.random().toFloat(),
                startY = -Math.random().toFloat() * 0.3f,
                speed = 0.3f + Math.random().toFloat() * 0.7f,
                color = confettiColors[(Math.random() * confettiColors.size).toInt()],
                size = 3f + Math.random().toFloat() * 5f,
            )
        }
    }

    Box {
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .border(1.dp, accentColor.copy(alpha = 0.4f), RoundedCornerShape(12.dp)),
            shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(
                containerColor = Color(0xFF0D1117),
            ),
            elevation = CardDefaults.cardElevation(defaultElevation = 8.dp),
        ) {
            Box {
                // Confetti canvas overlay
                Canvas(
                    modifier = Modifier
                        .matchParentSize()
                        .graphicsLayer { clip = true },
                ) {
                    val t = particleProgress.value
                    val alpha = (1f - t).coerceIn(0f, 1f)
                    for (p in particles) {
                        val px = p.x * size.width
                        val py = (p.startY + t * p.speed) * size.height * 3f
                        if (py in 0f..size.height) {
                            drawRect(
                                color = p.color.copy(alpha = alpha * 0.8f),
                                topLeft = Offset(px, py),
                                size = Size(p.size, p.size * 1.5f),
                            )
                        }
                    }
                }

                Column(modifier = Modifier.padding(16.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            imageVector = icon,
                            contentDescription = title,
                            tint = iconColor,
                            modifier = Modifier.size(24.dp),
                        )
                        Spacer(modifier = Modifier.width(10.dp))
                        Text(
                            text = title,
                            style = MaterialTheme.typography.labelLarge.copy(
                                fontFamily = FontFamily.Monospace,
                                letterSpacing = 1.sp,
                            ),
                            color = accentColor,
                            modifier = Modifier.weight(1f),
                        )
                        IconButton(
                            onClick = onDismiss,
                            modifier = Modifier.size(24.dp),
                        ) {
                            Icon(
                                imageVector = Icons.Default.Close,
                                contentDescription = "Dismiss",
                                tint = Color(0xFF4A6A5A),
                                modifier = Modifier.size(16.dp),
                            )
                        }
                    }
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = message,
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            lineHeight = 18.sp,
                        ),
                        color = Color(0xFF00FFA3),
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sync Task Section
// ---------------------------------------------------------------------------

@Composable
private fun SyncTaskSection(
    syncTasks: List<SyncTask>,
    overallProgress: Float,
    syncStartTimeMs: Long,
    isSyncing: Boolean,
) {
    // Timer tick for elapsed/ETA updates
    var tick by remember { mutableStateOf(0L) }
    LaunchedEffect(isSyncing) {
        while (isSyncing) {
            delay(1000)
            tick = System.currentTimeMillis()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, ZColors.border, RoundedCornerShape(4.dp))
            .background(Color(0xFF0D0D0D), RoundedCornerShape(4.dp))
            .padding(12.dp),
    ) {
        // Overall progress header
        val elapsedMs = if (syncStartTimeMs > 0 && tick > 0) tick - syncStartTimeMs else 0L
        val elapsedStr = formatDuration(elapsedMs)
        val etaStr = if (overallProgress > 0.05f && overallProgress < 1f) {
            val totalEstMs = (elapsedMs / overallProgress).toLong()
            val remainMs = totalEstMs - elapsedMs
            "ETA ${formatDuration(remainMs)}"
        } else if (overallProgress >= 1f) "Done" else "Calculating..."

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "> SYNC PROGRESS",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = ZColors.primaryDim,
                letterSpacing = 1.sp,
            )
            Text(
                "${(overallProgress * 100).toInt()}%",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = ZColors.primary,
            )
        }
        Spacer(modifier = Modifier.height(4.dp))
        LinearProgressIndicator(
            progress = { overallProgress },
            modifier = Modifier.fillMaxWidth().height(4.dp),
            color = ZColors.primary,
            trackColor = ZColors.progressBg,
        )
        Spacer(modifier = Modifier.height(4.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "Elapsed: $elapsedStr",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
            )
            Text(
                etaStr,
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
            )
        }

        Spacer(modifier = Modifier.height(8.dp))
        HorizontalDivider(color = ZColors.border)
        Spacer(modifier = Modifier.height(8.dp))

        // Individual task rows
        syncTasks.forEach { task ->
            SyncTaskRow(task = task, tick = tick)
            Spacer(modifier = Modifier.height(4.dp))
        }
    }
}

@Composable
private fun SyncTaskRow(task: SyncTask, tick: Long) {
    val statusIcon = when (task.status) {
        SyncTaskStatus.PENDING -> "[ ]"
        SyncTaskStatus.IN_PROGRESS -> "[>]"
        SyncTaskStatus.COMPLETED -> "[+]"
        SyncTaskStatus.FAILED -> "[!]"
    }
    val statusColor = when (task.status) {
        SyncTaskStatus.PENDING -> ZColors.textDim
        SyncTaskStatus.IN_PROGRESS -> ZColors.primary
        SyncTaskStatus.COMPLETED -> ZColors.success
        SyncTaskStatus.FAILED -> ZColors.error
    }

    Column(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    statusIcon,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = statusColor,
                )
                Spacer(modifier = Modifier.width(6.dp))
                Text(
                    task.title,
                    fontSize = 11.sp,
                    fontFamily = FontFamily.Monospace,
                    color = if (task.status == SyncTaskStatus.PENDING) ZColors.textDim else ZColors.primary,
                )
            }
            // Duration for completed or in-progress tasks
            val durationStr = when {
                task.status == SyncTaskStatus.COMPLETED && task.startTimeMs != null && task.endTimeMs != null ->
                    formatDuration(task.endTimeMs - task.startTimeMs)
                task.status == SyncTaskStatus.IN_PROGRESS && task.startTimeMs != null && tick > 0 ->
                    formatDuration(tick - task.startTimeMs)
                else -> ""
            }
            if (durationStr.isNotEmpty()) {
                Text(
                    durationStr,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
            }
        }

        // Detail text and per-task progress bar
        if (task.status == SyncTaskStatus.IN_PROGRESS) {
            if (task.detail != null) {
                Text(
                    task.detail,
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                    modifier = Modifier.padding(start = 24.dp),
                )
            }
            if (task.progress != null && task.progress > 0f) {
                Spacer(modifier = Modifier.height(2.dp))
                Row(
                    modifier = Modifier.fillMaxWidth().padding(start = 24.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    LinearProgressIndicator(
                        progress = { task.progress },
                        modifier = Modifier.weight(1f).height(3.dp),
                        color = ZColors.primary,
                        trackColor = ZColors.progressBg,
                    )
                    Text(
                        "${(task.progress * 100).toInt()}%",
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primaryDim,
                    )
                    // Per-task ETA
                    if (task.startTimeMs != null && tick > 0 && task.progress > 0.02f && task.progress < 1f) {
                        val taskElapsed = tick - task.startTimeMs
                        val taskTotal = (taskElapsed / task.progress).toLong()
                        val taskRemain = taskTotal - taskElapsed
                        Text(
                            "~${formatDuration(taskRemain)}",
                            fontSize = 9.sp,
                            fontFamily = FontFamily.Monospace,
                            color = ZColors.textDim,
                        )
                    }
                }
            }
        } else if (task.status == SyncTaskStatus.FAILED && task.detail != null) {
            Text(
                task.detail,
                fontSize = 9.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.error,
                modifier = Modifier.padding(start = 24.dp),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Celebration Card (Clearing / Settlement)
// ---------------------------------------------------------------------------

@Composable
private fun CelebrationCard(
    title: String,
    subtitle: String,
    message: String,
    duration: String?,
    txid: String?,
    color: Color,
    onAcknowledge: () -> Unit,
) {
    val pulse = rememberInfiniteTransition(label = "${title}_pulse")
    val glowAlpha by pulse.animateFloat(
        initialValue = 0.05f, targetValue = 0.20f,
        animationSpec = infiniteRepeatable(
            animation = tween(1500, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ), label = "${title}_glow",
    )
    val borderAlpha by pulse.animateFloat(
        initialValue = 0.6f, targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ), label = "${title}_border",
    )

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(2.dp, color.copy(alpha = borderAlpha), RoundedCornerShape(4.dp))
            .background(color.copy(alpha = glowAlpha), RoundedCornerShape(4.dp))
            .padding(20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            imageVector = Icons.Filled.Lock,
            contentDescription = title,
            tint = color,
            modifier = Modifier.size(36.dp),
        )
        Spacer(modifier = Modifier.height(8.dp))

        Text(
            title,
            fontSize = 18.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = color,
            letterSpacing = 4.sp,
        )
        Spacer(modifier = Modifier.height(2.dp))
        Text(
            subtitle,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = color.copy(alpha = 0.7f),
        )
        Spacer(modifier = Modifier.height(4.dp))

        HorizontalDivider(
            modifier = Modifier.fillMaxWidth(0.6f),
            color = color.copy(alpha = 0.3f),
            thickness = 1.dp,
        )

        Spacer(modifier = Modifier.height(10.dp))
        Text(
            message,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = color.copy(alpha = 0.9f),
            textAlign = TextAlign.Center,
            lineHeight = 18.sp,
        )

        if (duration != null) {
            Spacer(modifier = Modifier.height(6.dp))
            Text(
                "Duration: $duration",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = color.copy(alpha = 0.8f),
            )
        }

        if (txid != null) {
            Spacer(modifier = Modifier.height(6.dp))
            Text(
                "tx: ${txid.take(24)}...",
                fontSize = 9.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
            )
        }

        Spacer(modifier = Modifier.height(12.dp))

        OutlinedButton(
            onClick = onAcknowledge,
            shape = RoundedCornerShape(4.dp),
        ) {
            Text(
                "OK",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 12.sp,
                letterSpacing = 2.sp,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Boost Failed Card
// ---------------------------------------------------------------------------

@Composable
private fun BoostFailedCard(
    reason: String,
    attempts: Int,
    onContinue: () -> Unit,
    onQuit: () -> Unit,
) {
    val pulse = rememberInfiniteTransition(label = "boost_fail_pulse")
    val borderAlpha by pulse.animateFloat(
        initialValue = 0.5f, targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ), label = "boost_fail_border",
    )

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(2.dp, ZColors.error.copy(alpha = borderAlpha), RoundedCornerShape(4.dp))
            .background(ZColors.error.copy(alpha = 0.08f), RoundedCornerShape(4.dp))
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            "[!] BOOST DOWNLOAD FAILED",
            fontSize = 14.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = ZColors.error,
            letterSpacing = 2.sp,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            "Failed after $attempts attempts.\n$reason",
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.error.copy(alpha = 0.9f),
            textAlign = TextAlign.Center,
            lineHeight = 18.sp,
        )
        Spacer(modifier = Modifier.height(6.dp))
        Text(
            "You can continue with slow P2P header sync\nor quit and try again later.",
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
            textAlign = TextAlign.Center,
            lineHeight = 16.sp,
        )
        Spacer(modifier = Modifier.height(16.dp))

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Button(
                onClick = onContinue,
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(2.dp),
                colors = androidx.compose.material3.ButtonDefaults.buttonColors(
                    containerColor = ZColors.primary,
                    contentColor = Color.Black,
                ),
            ) {
                Text(
                    "CONTINUE",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 11.sp,
                    letterSpacing = 1.sp,
                )
            }
            Button(
                onClick = onQuit,
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(2.dp),
                colors = androidx.compose.material3.ButtonDefaults.buttonColors(
                    containerColor = ZColors.error,
                    contentColor = Color.White,
                ),
            ) {
                Text(
                    "QUIT",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 11.sp,
                    letterSpacing = 1.sp,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pending Settlement Banner
// ---------------------------------------------------------------------------

@Composable
private fun PendingSettlementBanner(pendingTxid: String) {
    val pulseTransition = rememberInfiniteTransition(label = "pending_pulse")
    val pulseAlpha by pulseTransition.animateFloat(
        initialValue = 0.5f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1200, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "pulse_alpha",
    )
    // Pick a random cypherpunk message once and remember it for this composition
    val message = remember { randomPendingSettlementMessage() }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, ZColors.warning.copy(alpha = 0.6f), RoundedCornerShape(4.dp))
            .background(ZColors.warning.copy(alpha = 0.05f), RoundedCornerShape(4.dp))
            .padding(12.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "[~]",
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = ZColors.warning.copy(alpha = pulseAlpha),
            )
            Text(
                "AWAITING SETTLEMENT",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = ZColors.warning,
                letterSpacing = 1.sp,
            )
        }
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            message,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.warning.copy(alpha = 0.8f),
            lineHeight = 16.sp,
        )
        Spacer(modifier = Modifier.height(6.dp))
        Text(
            "tx: ${pendingTxid.take(16)}...",
            fontSize = 9.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
        )
    }
}

// ---------------------------------------------------------------------------
// Pending Incoming TX Banner
// ---------------------------------------------------------------------------

@Composable
private fun PendingIncomingBanner(pendingTxid: String, amount: Long) {
    val incomingColor = Color(0xFF00BCD4)
    val pulseTransition = rememberInfiniteTransition(label = "incoming_pulse")
    val pulseAlpha by pulseTransition.animateFloat(
        initialValue = 0.5f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1200, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "incoming_pulse_alpha",
    )
    val message = remember { randomPendingIncomingMessage() }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(1.dp, incomingColor.copy(alpha = 0.6f), RoundedCornerShape(4.dp))
            .background(incomingColor.copy(alpha = 0.05f), RoundedCornerShape(4.dp))
            .padding(12.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                "[<]",
                fontSize = 12.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = incomingColor.copy(alpha = pulseAlpha),
            )
            Text(
                "INCOMING TX PENDING",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = incomingColor,
                letterSpacing = 1.sp,
            )
        }
        if (amount > 0) {
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                "[ +${formatZclAmount(amount)} ZCL ]",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = incomingColor,
            )
        }
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            message,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = incomingColor.copy(alpha = 0.8f),
            lineHeight = 16.sp,
        )
        Spacer(modifier = Modifier.height(6.dp))
        Text(
            "tx: ${pendingTxid.take(16)}...",
            fontSize = 9.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
        )
    }
}

// ---------------------------------------------------------------------------
// Duration Formatter
// ---------------------------------------------------------------------------

private fun formatDuration(ms: Long): String {
    if (ms < 0) return "0s"
    val totalSec = ms / 1000
    return when {
        totalSec < 60 -> "${totalSec}s"
        totalSec < 3600 -> "${totalSec / 60}m ${totalSec % 60}s"
        else -> "${totalSec / 3600}h ${(totalSec % 3600) / 60}m"
    }
}

/**
 * Format a zatoshi amount as a ZCL string.
 */
private fun formatZclAmount(zatoshis: Long): String {
    val whole = zatoshis / 100_000_000L
    val fraction = (zatoshis % 100_000_000L).let { if (it < 0) -it else it }
    return "%d.%08d".format(whole, fraction)
}

private val cypherpunkQuotes = listOf(
    "\"Privacy is necessary for an open society in the electronic age.\" — Eric Hughes",
    "\"Cypherpunks write code.\" — Eric Hughes, A Cypherpunk's Manifesto",
    "\"We must defend our own privacy if we expect to have any.\" — Eric Hughes",
    "\"Privacy is not secrecy. A private matter is something one doesn't want the whole world to know.\" — Eric Hughes",
    "\"If you want privacy, you must create it for yourself.\" — Eric Hughes",
    "\"The computer can be used as a tool to liberate and protect people, rather than to control them.\" — Hal Finney",
    "\"Strong cryptography can resist an unlimited application of violence.\" — Jacob Appelbaum",
    "\"There is no justice in following unjust laws.\" — Aaron Swartz",
    "\"Encryption works. Properly implemented strong crypto systems are one of the few things that you can rely on.\" — Edward Snowden",
    "\"We are creating a world where anyone, anywhere may express their beliefs without fear.\" — John Perry Barlow",
    "\"A society that trades freedom for security deserves neither and will lose both.\" — Benjamin Franklin",
    "\"Information wants to be free.\" — Stewart Brand",
    "\"The Net treats censorship as damage and routes around it.\" — John Gilmore",
    "\"Zero-knowledge. Zero trust. Zero compromise.\" — ZipherX",
    "\"Your keys, your coins. Your privacy, your right.\" — ZipherX",
    "\"In math we trust.\" — ZipherX",
)
