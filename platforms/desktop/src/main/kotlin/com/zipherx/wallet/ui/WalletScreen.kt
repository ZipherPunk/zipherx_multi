package com.zipherx.wallet.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.Transaction
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import java.text.SimpleDateFormat
import java.util.*
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private val pendingSettlementMessages = listOf(
    "Your proof floats in the mempool.\nMiners compete to etch it into the next block.\nPatience — privacy takes time.",
    "The zero-knowledge proof is verified.\nNow the chain must seal it.\nNo one knows what you sent. Not even the miners.",
    "Cypherpunks wait for blocks, not banks.\nYour shielded TX is queued.\nThe math is done. The mining continues.",
    "Your transaction is invisible to surveillance.\nA miner will lock it into stone shortly.\nTrust the protocol.",
    "Mempool accepted. Block pending.\nThe network validates without seeing.\nThis is what financial privacy looks like.",
    "Shielded and waiting.\nNo address. No amount. No trace.\nJust a proof waiting for its block.",
)

private val cypherpunkQuotes = listOf(
    // Eric Hughes - A Cypherpunk's Manifesto (1993)
    "\"Privacy is necessary for an open society in the electronic age.\" — Eric Hughes",
    "\"Privacy is not secrecy. A private matter is something one doesn't want the whole world to know, but a secret matter is something one doesn't want anybody to know.\" — Eric Hughes",
    "\"Privacy is the power to selectively reveal oneself to the world.\" — Eric Hughes",
    "\"We must defend our own privacy if we expect to have any.\" — Eric Hughes",
    "\"Cypherpunks write code.\" — Eric Hughes",
    "\"We know that software can't be destroyed and that a widely dispersed system can't be shut down.\" — Eric Hughes",
    "\"We the Cypherpunks are dedicated to building anonymous systems.\" — Eric Hughes",
    // Timothy C. May
    "\"Just as the technology of printing altered and reduced the power of medieval guilds, so too will cryptologic methods fundamentally alter the nature of corporations and of government interference in economic transactions.\" — Timothy C. May",
    // Satoshi Nakamoto
    "\"The root problem with conventional currency is all the trust that's required to make it work.\" — Satoshi Nakamoto",
    "\"What is needed is an electronic payment system based on cryptographic proof instead of trust.\" — Satoshi Nakamoto",
    "\"If you don't believe it or don't get it, I don't have the time to try to convince you, sorry.\" — Satoshi Nakamoto",
    "\"I've been working on a new electronic cash system that's fully peer-to-peer, with no trusted third party.\" — Satoshi Nakamoto",
    // Phil Zimmermann
    "\"If privacy is outlawed, only outlaws will have privacy.\" — Phil Zimmermann",
    "\"Privacy is an inherent human right, and a requirement for maintaining the human condition with dignity and respect.\" — Phil Zimmermann",
    // Julian Assange
    "\"Privacy for the weak, transparency for the powerful.\" — Julian Assange",
    "\"Cryptography is the ultimate form of non-violent direct action.\" — Julian Assange",
    // John Perry Barlow
    "\"Relying on the government to protect your privacy is like asking a peeping tom to install your window blinds.\" — John Perry Barlow",
    // Bruce Schneier
    "\"Privacy is not something that I'm merely entitled to, it's an absolute prerequisite.\" — Bruce Schneier",
    "\"Security is a process, not a product.\" — Bruce Schneier",
    // Edward Snowden
    "\"Arguing that you don't care about the right to privacy because you have nothing to hide is no different than saying you don't care about free speech because you have nothing to say.\" — Edward Snowden",
    "\"Privacy isn't about something to hide. Privacy is about something to protect.\" — Edward Snowden",
    // Hal Finney
    "\"Running bitcoin.\" — Hal Finney",
    // Nick Szabo
    "\"Trusted third parties are security holes.\" — Nick Szabo",
    // Others
    "\"In a time of deceit, telling the truth is a revolutionary act.\" — George Orwell",
    "\"Those who would give up essential Liberty, to purchase a little temporary Safety, deserve neither Liberty nor Safety.\" — Benjamin Franklin",
    "\"The only way to deal with an unfree world is to become so absolutely free that your very existence is an act of rebellion.\" — Albert Camus",
    // ZipherX
    "\"Zero-knowledge. Zero trust. Zero compromise.\" — ZipherX",
    "\"Your keys, your coins. Your privacy, your right.\" — ZipherX",
    "\"In math we trust.\" — ZipherX",
)

@Composable
fun WalletScreen(
    viewModel: WalletViewModel,
    onNavigateToSend: () -> Unit,
    onNavigateToSettings: () -> Unit,
) {
    val balance by viewModel.balance.collectAsState()
    val address by viewModel.address.collectAsState()
    val transactions by viewModel.transactions.collectAsState()
    val syncPhase by viewModel.syncPhase.collectAsState()
    val syncProgress by viewModel.syncProgress.collectAsState()
    val isSyncing by viewModel.isSyncing.collectAsState()
    val peerCount by viewModel.peerCount.collectAsState()
    val blockHeight by viewModel.blockHeight.collectAsState()
    val version by viewModel.version.collectAsState()
    val syncTasks by viewModel.syncTasks.collectAsState()
    val overallProgress by viewModel.overallProgress.collectAsState()
    val syncStartTimeMs by viewModel.syncStartTimeMs.collectAsState()
    val pendingTxid by viewModel.pendingConfirmationTxid.collectAsState()
    val mempoolAccepted by viewModel.mempoolAccepted.collectAsState()
    val mempoolPeerStatus by viewModel.mempoolPeerStatus.collectAsState()
    val clearingCelebration by viewModel.clearingCelebration.collectAsState()
    val clearingDuration by viewModel.clearingDuration.collectAsState()
    val settlementCelebration by viewModel.settlementCelebration.collectAsState()
    val settlementDuration by viewModel.settlementDuration.collectAsState()
    val settlementTxid by viewModel.settlementTxid.collectAsState()
    var showReceive by remember { mutableStateOf(false) }
    var selectedTx by remember { mutableStateOf<Transaction?>(null) }
    val blockHeightVal by viewModel.blockHeight.collectAsState()
    var currentQuote by remember { mutableStateOf<String?>(null) }
    var showQuote by remember { mutableStateOf(false) }
    var showPendingWarning by remember { mutableStateOf(false) }
    val clipboardScope = rememberCoroutineScope()

    // Spinning shield during sync
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

    // Auto-dismiss quote after 5 seconds
    LaunchedEffect(showQuote) {
        if (showQuote) {
            kotlinx.coroutines.delay(5000)
            showQuote = false
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Header with clickable spinning logo
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                modifier = Modifier.clickable {
                    currentQuote = cypherpunkQuotes.random()
                    showQuote = true
                },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    Icons.Filled.Lock,
                    contentDescription = "ZipherX lock shield",
                    tint = ZColors.primary,
                    modifier = Modifier
                        .size(24.dp)
                        .then(
                            if (isSyncing) {
                                Modifier.graphicsLayer {
                                    rotationY = shieldRotationY
                                    cameraDistance = 12f * density
                                }
                            } else Modifier
                        ),
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    "ZIPHERX",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 18.sp,
                    color = ZColors.primary,
                )
            }
            IconButton(onClick = onNavigateToSettings) {
                Icon(Icons.Filled.Settings, "Settings", tint = ZColors.primaryDim)
            }
        }

        // Cypherpunk quote toast
        if (showQuote && currentQuote != null) {
            Spacer(Modifier.height(4.dp))
            Text(
                currentQuote!!,
                fontSize = 9.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                    .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
                    .padding(8.dp),
            )
        }

        Spacer(Modifier.height(16.dp))

        // Balance card
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
                .padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                "> SHIELDED BALANCE",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = ZColors.primaryDim,
                letterSpacing = 2.sp,
            )
            Spacer(Modifier.height(12.dp))

            if (pendingTxid != null) {
                // TX broadcast but not yet confirmed — show "Await conf" instead of balance
                Text(
                    text = "AWAITING CONFIRMATION",
                    fontSize = 22.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = ZColors.warning,
                    style = LocalTextStyle.current.copy(
                        shadow = Shadow(ZColors.warning.copy(alpha = 0.3f), Offset(0f, 0f), 12f)
                    ),
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    text = "TX broadcast — waiting for block...",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.warning.copy(alpha = 0.7f),
                )
            } else {
                val totalZcl = balance.total / 100_000_000.0
                Text(
                    text = "%.8f ZCL".format(totalZcl),
                    fontSize = 28.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = ZColors.primary,
                    style = LocalTextStyle.current.copy(
                        shadow = Shadow(ZColors.glow, Offset(0f, 0f), 12f)
                    ),
                )
                Spacer(Modifier.height(8.dp))
                val spendableZcl = balance.spendable / 100_000_000.0
                Text(
                    text = "Spendable: %.8f ZCL".format(spendableZcl),
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primaryDim,
                )
                Text(
                    text = "${balance.spendableNoteCount}/${balance.noteCount} notes spendable",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
            }
        }

        Spacer(Modifier.height(12.dp))

        // =====================================================================
        // CLEARING celebration (mempool accepted) — user must acknowledge
        // =====================================================================
        if (clearingCelebration != null) {
            CelebrationCard(
                title = "CLEARING",
                subtitle = "Transaction accepted by mempool",
                message = clearingCelebration!!,
                duration = clearingDuration,
                txid = pendingTxid,
                color = ZColors.warning,
                onAcknowledge = { viewModel.dismissClearing() },
            )
            Spacer(Modifier.height(12.dp))
        }

        // Pending settlement indicator (after clearing acknowledged, waiting for block)
        if (pendingTxid != null && clearingCelebration == null) {
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
            val message = remember { pendingSettlementMessages.random() }
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.warning.copy(alpha = 0.6f), RoundedCornerShape(2.dp))
                    .background(ZColors.warning.copy(alpha = 0.05f), RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("[~]", fontSize = 12.sp, fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold, color = ZColors.warning.copy(alpha = pulseAlpha))
                    Text("AWAITING SETTLEMENT", fontSize = 10.sp, fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold, color = ZColors.warning, letterSpacing = 1.sp)
                }
                Spacer(Modifier.height(8.dp))
                Text(message, fontSize = 10.sp, fontFamily = FontFamily.Monospace,
                    color = ZColors.warning.copy(alpha = 0.8f), lineHeight = 16.sp)
                Spacer(Modifier.height(4.dp))
                Text("tx: ${pendingTxid?.take(16)}...", fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace, color = ZColors.textDim)
            }
            Spacer(Modifier.height(12.dp))
        }

        // =====================================================================
        // SETTLEMENT celebration (block confirmed) — user must acknowledge
        // =====================================================================
        if (settlementCelebration != null) {
            CelebrationCard(
                title = "SETTLEMENT",
                subtitle = "Transaction confirmed in block",
                message = settlementCelebration!!,
                duration = settlementDuration,
                txid = settlementTxid,
                color = ZColors.success,
                onAcknowledge = { viewModel.dismissSettlement() },
            )
            Spacer(Modifier.height(12.dp))
        }

        // Sync status — detailed task view
        if (isSyncing && syncTasks.isNotEmpty()) {
            // Timer tick for elapsed/ETA updates
            var tick by remember { mutableStateOf(0L) }
            LaunchedEffect(isSyncing) {
                while (isSyncing) {
                    kotlinx.coroutines.delay(1000)
                    tick = System.currentTimeMillis()
                }
            }

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                    .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                // Overall progress header
                val elapsedMs = if (syncStartTimeMs > 0) tick - syncStartTimeMs else 0L
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
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primaryDim,
                        letterSpacing = 1.sp,
                    )
                    Text(
                        "${(overallProgress * 100).toInt()}%",
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                    )
                }
                Spacer(Modifier.height(4.dp))
                LinearProgressIndicator(
                    progress = { overallProgress },
                    modifier = Modifier.fillMaxWidth().height(4.dp),
                    color = ZColors.primary,
                    trackColor = ZColors.progressBg,
                )
                Spacer(Modifier.height(4.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        "Elapsed: $elapsedStr",
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.textDim,
                    )
                    Text(
                        etaStr,
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.textDim,
                    )
                }

                Spacer(Modifier.height(8.dp))
                HorizontalDivider(color = ZColors.border)
                Spacer(Modifier.height(8.dp))

                // Individual task rows
                syncTasks.forEach { task ->
                    SyncTaskRow(task, tick)
                    Spacer(Modifier.height(4.dp))
                }
            }
            Spacer(Modifier.height(12.dp))
        }

        // Address
        if (address != null) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                Text(
                    "> ADDRESS",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = ZColors.primaryDim,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    address!!,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primary,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Spacer(Modifier.height(12.dp))
        }

        // Action buttons
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedButton(
                onClick = {
                    if (pendingTxid != null) {
                        showPendingWarning = true
                    } else {
                        onNavigateToSend()
                    }
                },
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(2.dp),
                border = BorderStroke(1.dp, if (pendingTxid != null) ZColors.textDim else ZColors.primary),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = if (pendingTxid != null) ZColors.textDim else ZColors.primary,
                ),
            ) {
                Icon(Icons.Filled.Send, null, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(4.dp))
                Text(
                    if (pendingTxid != null) "SEND [LOCKED]" else "SEND",
                    fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 12.sp,
                )
            }
            OutlinedButton(
                onClick = { showReceive = true },
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(2.dp),
                border = BorderStroke(1.dp, ZColors.primary),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
            ) {
                Text("RECEIVE", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 12.sp)
            }
        }

        Spacer(Modifier.height(16.dp))

        // Recent transactions
        Text(
            "> RECENT TRANSACTIONS",
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = ZColors.primaryDim,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))

        if (transactions.isEmpty()) {
            Text(
                "No transactions yet",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(24.dp),
            )
        } else {
            transactions.take(10).forEach { tx ->
                TransactionRow(tx, blockHeightVal, onClick = { selectedTx = tx })
                Spacer(Modifier.height(4.dp))
            }
        }

        Spacer(Modifier.height(16.dp))

        // Status bar
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(8.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    "Peers: $peerCount",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = if (peerCount > 0) ZColors.primary else ZColors.textDim,
                )
                Text(
                    "Block: ${if (blockHeight > 0) blockHeight.toString() else "—"}",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
            }
            Spacer(Modifier.height(2.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    syncPhaseLabel(syncPhase),
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
                if (version.isNotEmpty()) {
                    Text(
                        "v$version",
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.textDim,
                    )
                }
            }
        }
    }

    // Receive dialog
    if (showReceive && address != null) {
        AlertDialog(
            onDismissRequest = { showReceive = false },
            containerColor = Color(0xFF0D0D0D),
            shape = RoundedCornerShape(2.dp),
            title = {
                Text("> RECEIVE ZCL", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, color = ZColors.primary)
            },
            text = {
                Column {
                    Text(
                        "Your shielded address:",
                        fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim,
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        address!!,
                        fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary,
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                            .padding(8.dp),
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = {
                            val clipboard = Toolkit.getDefaultToolkit().systemClipboard
                            clipboard.setContents(StringSelection(address!!), null)
                            // Auto-clear clipboard after 30 seconds
                            clipboardScope.launch {
                                delay(30_000)
                                clipboard.setContents(StringSelection(""), null)
                            }
                        },
                        shape = RoundedCornerShape(2.dp),
                        border = BorderStroke(1.dp, ZColors.primary),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                    ) {
                        Icon(Icons.Filled.ContentCopy, null, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("COPY ADDRESS", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 10.sp)
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showReceive = false }) {
                    Text("CLOSE", fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                }
            },
        )
    }

    // Transaction detail dialog
    if (selectedTx != null) {
        TransactionDetailDialog(tx = selectedTx!!, onDismiss = { selectedTx = null })
    }

    // Pending TX warning dialog — sending is blocked until confirmation
    if (showPendingWarning) {
        AlertDialog(
            onDismissRequest = { showPendingWarning = false },
            containerColor = Color(0xFF0A0A0A),
            shape = RoundedCornerShape(2.dp),
            title = {
                Text(
                    "> SEND LOCKED",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = ZColors.warning,
                )
            },
            text = {
                Column {
                    Text(
                        "You have an unconfirmed transaction waiting for block confirmation.",
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primaryDim,
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Sending is disabled until the previous transaction confirms. This prevents double-spend risk with the same notes.",
                        fontSize = 10.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.warning,
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "tx: ${pendingTxid?.take(24) ?: ""}...",
                        fontSize = 9.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.textDim,
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { showPendingWarning = false }) {
                    Text("OK", fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                }
            },
        )
    }
}

@Composable
private fun TransactionRow(tx: Transaction, currentBlockHeight: Long, onClick: () -> Unit) {
    val isSelf = tx.txType.lowercase() == "self"
    val isReceived = tx.txType.lowercase().contains("receive")
    val typeLabel = when {
        isSelf -> "SELF"
        isReceived -> "RECEIVED"
        else -> "SENT"
    }
    val sign = when {
        isSelf -> "" // self-send, net effect is only fee
        isReceived -> "+"
        else -> "-"
    }
    val color = when {
        isSelf -> ZColors.warning       // yellow
        isReceived -> ZColors.primary    // green
        else -> ZColors.error            // red
    }
    val zcl = tx.amount / 100_000_000.0

    // Date/time
    val dateStr = if (tx.timestamp > 0) {
        SimpleDateFormat("MM/dd HH:mm", Locale.getDefault()).format(Date(tx.timestamp * 1000))
    } else "Pending"

    // Confirmations
    val confs = if (currentBlockHeight > 0 && tx.height > 0) {
        (currentBlockHeight - tx.height + 1).coerceAtLeast(0)
    } else tx.confirmations
    val confLabel = when {
        confs <= 0L -> "unconfirmed"
        confs == 1L -> "1 conf"
        confs < 100L -> "$confs confs"
        else -> "${confs}+ confs"
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onClick() }
            .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
            .background(Color(0xFF0D0D0D), RoundedCornerShape(2.dp))
            .padding(10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    typeLabel,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = color,
                )
                Text(
                    dateStr,
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
            }
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    tx.txid.take(12) + "...",
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
                Text(
                    confLabel,
                    fontSize = 8.sp,
                    fontFamily = FontFamily.Monospace,
                    color = if (confs > 0) ZColors.success else ZColors.warning,
                )
            }
        }
        Text(
            text = "$sign%.8f ZCL".format(zcl),
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = color,
        )
    }
}

@Composable
private fun TransactionDetailDialog(tx: Transaction, onDismiss: () -> Unit) {
    val isSelf = tx.txType.lowercase() == "self"
    val isReceived = tx.txType.lowercase().contains("receive")
    val typeLabel = when {
        isSelf -> "SELF TRANSFER"
        isReceived -> "RECEIVED"
        else -> "SENT"
    }
    val typeColor = when {
        isSelf -> ZColors.warning       // yellow
        isReceived -> ZColors.primary    // green
        else -> ZColors.error            // red
    }
    val zcl = tx.amount / 100_000_000.0
    val feeZcl = tx.fee / 100_000_000.0
    val dateStr = if (tx.timestamp > 0) {
        SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault()).format(Date(tx.timestamp * 1000))
    } else "Unknown"

    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = Color(0xFF0D0D0D),
        shape = RoundedCornerShape(2.dp),
        title = {
            Text(
                "> TRANSACTION DETAILS",
                fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold,
                color = typeColor, fontSize = 13.sp,
            )
        },
        text = {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                    .padding(12.dp),
            ) {
                // Type in its color
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("TYPE", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, letterSpacing = 1.sp)
                    Text(typeLabel, fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = typeColor, fontWeight = FontWeight.Bold)
                }
                DetailRow("AMOUNT", "%.8f ZCL".format(zcl))
                if (tx.fee > 0) DetailRow("FEE", "%.8f ZCL".format(feeZcl))
                DetailRow("CONFIRMATIONS", "${tx.confirmations}")
                DetailRow("BLOCK HEIGHT", "${tx.height}")
                DetailRow("DATE", dateStr)
                if (!tx.memo.isNullOrBlank()) DetailRow("MEMO", tx.memo)

                Spacer(Modifier.height(8.dp))
                HorizontalDivider(color = ZColors.border)
                Spacer(Modifier.height(8.dp))

                Text("TXID", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, letterSpacing = 1.sp)
                Spacer(Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        tx.txid,
                        fontSize = 8.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary,
                        modifier = Modifier.weight(1f),
                    )
                    val txidScope = rememberCoroutineScope()
                    IconButton(
                        onClick = {
                            val clipboard = Toolkit.getDefaultToolkit().systemClipboard
                            clipboard.setContents(StringSelection(tx.txid), null)
                            // Auto-clear clipboard after 30 seconds
                            txidScope.launch {
                                delay(30_000)
                                clipboard.setContents(StringSelection(""), null)
                            }
                        },
                        modifier = Modifier.size(24.dp),
                    ) {
                        Icon(Icons.Filled.ContentCopy, "Copy TXID", tint = ZColors.primary, modifier = Modifier.size(14.dp))
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("CLOSE", fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
            }
        },
    )
}

@Composable
private fun SyncTaskRow(task: com.zipherx.wallet.SyncTask, tick: Long) {
    val statusIcon = when (task.status) {
        com.zipherx.wallet.SyncTaskStatus.PENDING -> "[ ]"
        com.zipherx.wallet.SyncTaskStatus.IN_PROGRESS -> "[>]"
        com.zipherx.wallet.SyncTaskStatus.COMPLETED -> "[+]"
        com.zipherx.wallet.SyncTaskStatus.FAILED -> "[!]"
    }
    val statusColor = when (task.status) {
        com.zipherx.wallet.SyncTaskStatus.PENDING -> ZColors.textDim
        com.zipherx.wallet.SyncTaskStatus.IN_PROGRESS -> ZColors.primary
        com.zipherx.wallet.SyncTaskStatus.COMPLETED -> ZColors.success
        com.zipherx.wallet.SyncTaskStatus.FAILED -> ZColors.error
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
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = statusColor,
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    task.title,
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = if (task.status == com.zipherx.wallet.SyncTaskStatus.PENDING) ZColors.textDim else ZColors.primary,
                )
            }
            // Duration for completed or in-progress tasks
            val durationStr = when {
                task.status == com.zipherx.wallet.SyncTaskStatus.COMPLETED && task.startTimeMs != null && task.endTimeMs != null ->
                    formatDuration(task.endTimeMs - task.startTimeMs)
                task.status == com.zipherx.wallet.SyncTaskStatus.IN_PROGRESS && task.startTimeMs != null && tick > 0 ->
                    formatDuration(tick - task.startTimeMs)
                else -> ""
            }
            if (durationStr.isNotEmpty()) {
                Text(
                    durationStr,
                    fontSize = 9.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                )
            }
        }

        // Detail text and per-task progress bar
        if (task.status == com.zipherx.wallet.SyncTaskStatus.IN_PROGRESS) {
            if (task.detail != null) {
                Text(
                    task.detail,
                    fontSize = 8.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.textDim,
                    modifier = Modifier.padding(start = 24.dp),
                )
            }
            if (task.progress != null && task.progress > 0f) {
                Spacer(Modifier.height(2.dp))
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
                        fontSize = 8.sp,
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
                            fontSize = 8.sp,
                            fontFamily = FontFamily.Monospace,
                            color = ZColors.textDim,
                        )
                    }
                }
            }
        } else if (task.status == com.zipherx.wallet.SyncTaskStatus.FAILED && task.detail != null) {
            Text(
                task.detail,
                fontSize = 8.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.error,
                modifier = Modifier.padding(start = 24.dp),
            )
        }
    }
}

private fun formatDuration(ms: Long): String {
    if (ms < 0) return "0s"
    val totalSec = ms / 1000
    return when {
        totalSec < 60 -> "${totalSec}s"
        totalSec < 3600 -> "${totalSec / 60}m ${totalSec % 60}s"
        else -> "${totalSec / 3600}h ${(totalSec % 3600) / 60}m"
    }
}

private fun syncPhaseLabel(phase: String): String = when (phase.lowercase()) {
    "boost_download" -> "Downloading boost file..."
    "boost_load" -> "Loading boost headers..."
    "header_sync" -> "Syncing headers..."
    "delta_sync" -> "Downloading outputs..."
    "block_scan" -> "Scanning blocks..."
    "witness_update" -> "Updating witnesses..."
    "starting", "starting..." -> "Starting sync..."
    "idle" -> "Sync complete."
    else -> if (phase.startsWith("Synced to")) phase else phase.replace("_", " ").uppercase()
}

@Composable
private fun DetailRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, letterSpacing = 1.sp)
        Text(value, fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
    }
}

/**
 * Reusable celebration card for Clearing (mempool) and Settlement (block confirmation).
 * Pulsing glow, shield icon, message, duration, txid, and ACKNOWLEDGE button.
 */
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
            Icons.Filled.Lock,
            contentDescription = title,
            tint = color,
            modifier = Modifier.size(36.dp),
        )
        Spacer(Modifier.height(8.dp))

        Text(
            title,
            fontSize = 18.sp,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = color,
            letterSpacing = 4.sp,
            style = LocalTextStyle.current.copy(
                shadow = Shadow(color.copy(alpha = 0.4f), Offset(0f, 0f), 16f)
            ),
        )
        Spacer(Modifier.height(2.dp))
        Text(
            subtitle,
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = color.copy(alpha = 0.7f),
        )
        Spacer(Modifier.height(4.dp))

        HorizontalDivider(
            modifier = Modifier.fillMaxWidth(0.6f),
            color = color.copy(alpha = 0.3f),
            thickness = 1.dp,
        )

        Spacer(Modifier.height(10.dp))
        Text(
            message,
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = color.copy(alpha = 0.9f),
            textAlign = TextAlign.Center,
            lineHeight = 18.sp,
        )

        if (duration != null) {
            Spacer(Modifier.height(6.dp))
            Text(
                "Duration: $duration",
                fontSize = 10.sp,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = color.copy(alpha = 0.8f),
            )
        }

        if (txid != null) {
            Spacer(Modifier.height(6.dp))
            Text(
                "tx: ${txid.take(24)}...",
                fontSize = 9.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.textDim,
            )
        }

        Spacer(Modifier.height(12.dp))

        OutlinedButton(
            onClick = onAcknowledge,
            shape = RoundedCornerShape(2.dp),
            border = BorderStroke(1.dp, color),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = color),
        ) {
            Text(
                "ACKNOWLEDGE",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 12.sp,
                letterSpacing = 2.sp,
            )
        }
    }
}
