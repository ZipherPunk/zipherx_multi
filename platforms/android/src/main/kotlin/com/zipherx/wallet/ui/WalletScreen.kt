package com.zipherx.wallet.ui

import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
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
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.zipherx.wallet.Transaction
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import kotlinx.coroutines.delay

/**
 * Main wallet screen that combines the balance card, action buttons,
 * and a summary of recent transactions.
 *
 * TODO: KA-N11 — Migrate hardcoded string literals (button labels, error messages,
 *  section headers) to Android string resources (res/values/strings.xml) for
 *  localization support and centralized text management.
 */
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
    val walletState by viewModel.walletState.collectAsState()
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

    // KA-N3: These remember{} states survive configuration changes (rotation) but NOT
    // process death. Critical wallet state lives in WalletViewModel (ViewModel-scoped) which
    // survives config changes. For full process-death resilience, consider SavedStateHandle
    // for transient UI state like showConfirmationToast in the future.
    val snackbarHostState = remember { SnackbarHostState() }
    var showMempoolToast by remember { mutableStateOf(false) }
    var showConfirmationToast by remember { mutableStateOf(false) }
    var showIncomingToast by remember { mutableStateOf(false) }
    var showQuote by remember { mutableStateOf(false) }
    var showPendingWarning by remember { mutableStateOf(false) }
    var currentQuote by remember { mutableStateOf("") }
    var selectedTx by remember { mutableStateOf<Transaction?>(null) }
    val context = androidx.compose.ui.platform.LocalContext.current
    // KA-N2: disclaimer_accepted is intentionally stored in plain SharedPreferences.
    // It is non-sensitive (just tracks whether the legal disclaimer was shown) and does not
    // reveal wallet state or keys. Sensitive settings (auth_required, screenshot_protection)
    // are stored in EncryptedSharedPreferences via WalletViewModel.initPrefs().
    val prefs = remember { context.getSharedPreferences("zipherx_prefs", android.content.Context.MODE_PRIVATE) }
    var disclaimerAccepted by remember { mutableStateOf(prefs.getBoolean("disclaimer_accepted", false)) }

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
                        Icon(
                            imageVector = Icons.Filled.Lock,
                            contentDescription = "ZipherX lock shield",
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = if (isSyncing) {
                                Modifier.graphicsLayer {
                                    rotationY = shieldRotationY
                                    cameraDistance = 12f * density
                                }
                            } else {
                                Modifier
                            },
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
            // Show wallet view only when fully ready; otherwise show onboarding
            val walletReady = walletState == "ready" || walletState == "syncing" ||
                walletState == "synced" || walletState == "created"
            if (!walletReady) {
                if (!disclaimerAccepted) {
                    // Legal disclaimer screen
                    Spacer(modifier = Modifier.height(32.dp))

                    Icon(
                        imageVector = Icons.Filled.Lock,
                        contentDescription = "ZipherX logo",
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(64.dp).align(Alignment.CenterHorizontally),
                    )

                    Spacer(modifier = Modifier.height(16.dp))

                    Text(
                        text = "ZIPHERX",
                        style = MaterialTheme.typography.headlineLarge.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.fillMaxWidth(),
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    )

                    Spacer(modifier = Modifier.height(24.dp))

                    Text(
                        text = "LEGAL DISCLAIMER",
                        style = MaterialTheme.typography.titleMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            letterSpacing = 1.sp,
                        ),
                        color = Color(0xFFFFC107),
                    )

                    Spacer(modifier = Modifier.height(12.dp))

                    Text(
                        text = "This software is provided \"as is\", without warranty of any kind. " +
                            "ZipherX is an open-source, self-custodial cryptocurrency wallet. " +
                            "You are solely responsible for securing your private keys and seed phrase. " +
                            "Lost keys cannot be recovered by anyone.\n\n" +
                            "This software is not financial advice. Cryptocurrency transactions are irreversible. " +
                            "By using this application, you acknowledge that you understand the risks associated with " +
                            "managing your own cryptographic keys and transacting on a decentralized network.\n\n" +
                            "ZipherX does not collect, store, or transmit any personal data. " +
                            "All wallet data is stored locally on your device and encrypted using hardware-backed security.",
                        style = MaterialTheme.typography.bodySmall.copy(
                            fontFamily = FontFamily.Monospace,
                            lineHeight = 18.sp,
                        ),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )

                    Spacer(modifier = Modifier.height(16.dp))

                    Card(
                        modifier = Modifier.fillMaxWidth(),
                        colors = CardDefaults.cardColors(
                            containerColor = Color(0xFF1A1A0E),
                        ),
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = "IMPORTANT: SYNC NOTICE",
                                style = MaterialTheme.typography.labelMedium.copy(
                                    fontFamily = FontFamily.Monospace,
                                ),
                                color = Color(0xFFFFC107),
                            )
                            Spacer(modifier = Modifier.height(6.dp))
                            Text(
                                text = "During the initial blockchain sync (which may take 10-30 minutes), " +
                                    "the app must remain in the foreground and the screen must stay active. " +
                                    "Do not lock your device or switch to another app, otherwise the sync process " +
                                    "will be interrupted and will need to restart.",
                                style = MaterialTheme.typography.bodySmall.copy(
                                    fontFamily = FontFamily.Monospace,
                                    lineHeight = 16.sp,
                                ),
                                color = Color(0xFFFFC107).copy(alpha = 0.8f),
                            )
                        }
                    }

                    Spacer(modifier = Modifier.height(24.dp))

                    Button(
                        onClick = {
                            prefs.edit().putBoolean("disclaimer_accepted", true).apply()
                            disclaimerAccepted = true
                        },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("I Understand & Accept")
                    }

                    Spacer(modifier = Modifier.height(24.dp))
                } else {
                // Onboarding: no wallet yet
                var seedWords by remember { mutableStateOf(List(24) { "" }) }
                val filledCount = seedWords.count { it.isNotBlank() }
                var skInput by remember { mutableStateOf("") }

                Spacer(modifier = Modifier.height(48.dp))

                Text(
                    text = "Welcome to ZipherX",
                    style = MaterialTheme.typography.headlineMedium,
                )

                Spacer(modifier = Modifier.height(8.dp))

                Text(
                    text = "Privacy-first Zclassic wallet with Sapling shielded transactions.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                val isBusy = walletState == "creating" || walletState == "restoring" ||
                    walletState == "importing" || walletState == "loading"

                if (isBusy) {
                    Text(
                        text = when (walletState) {
                            "creating" -> "Creating wallet..."
                            "restoring" -> "Restoring wallet..."
                            "importing" -> "Importing key..."
                            else -> "Loading..."
                        },
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(modifier = Modifier.height(16.dp))
                }

                Spacer(modifier = Modifier.height(32.dp))

                // Option 1: Create new
                Button(
                    onClick = { viewModel.createNewWallet() },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !isBusy,
                ) {
                    Text("Create New Wallet")
                }

                Spacer(modifier = Modifier.height(24.dp))

                // Option 2: Restore from mnemonic
                Text(
                    text = "Restore from Mnemonic",
                    style = MaterialTheme.typography.titleSmall,
                )

                Spacer(modifier = Modifier.height(4.dp))

                Text(
                    text = "$filledCount/24 words filled",
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                    ),
                    color = if (filledCount == 24) Color(0xFF00E676)
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                )

                Spacer(modifier = Modifier.height(8.dp))

                // 24-word grid: 8 rows x 3 columns
                for (row in 0 until 8) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        for (col in 0 until 3) {
                            val index = row * 3 + col
                            OutlinedTextField(
                                value = seedWords[index],
                                onValueChange = { newValue ->
                                    // Detect multi-word paste (contains spaces)
                                    val trimmed = newValue.trim()
                                    val words = trimmed.split("\\s+".toRegex()).filter { it.isNotBlank() }
                                    if (words.size > 1) {
                                        // Paste detected: distribute words starting from this field
                                        val updated = seedWords.toMutableList()
                                        for (i in words.indices) {
                                            val targetIndex = index + i
                                            if (targetIndex < 24) {
                                                updated[targetIndex] = words[i].lowercase()
                                            }
                                        }
                                        seedWords = updated
                                    } else {
                                        // Single word edit: update only this field
                                        val updated = seedWords.toMutableList()
                                        updated[index] = newValue.lowercase().trim()
                                        seedWords = updated
                                    }
                                },
                                label = {
                                    Text(
                                        "#${index + 1}",
                                        style = TextStyle(fontSize = 10.sp),
                                    )
                                },
                                modifier = Modifier
                                    .weight(1f)
                                    .heightIn(min = 48.dp),
                                textStyle = TextStyle(
                                    fontSize = 12.sp,
                                    fontFamily = FontFamily.Monospace,
                                ),
                                singleLine = true,
                                keyboardOptions = KeyboardOptions(
                                    keyboardType = KeyboardType.Text,
                                    imeAction = if (index < 23) ImeAction.Next else ImeAction.Done,
                                    autoCorrectEnabled = false,
                                ),
                            )
                        }
                    }
                    if (row < 7) {
                        Spacer(modifier = Modifier.height(2.dp))
                    }
                }

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedButton(
                    onClick = {
                        val mnemonicWords = seedWords.map { it.trim().lowercase() }
                        viewModel.restoreFromMnemonic(mnemonicWords)
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !isBusy && filledCount == 24,
                ) {
                    Text("Restore Wallet")
                }

                Spacer(modifier = Modifier.height(24.dp))

                // Option 3: Import private key
                Text(
                    text = "Import Private Key",
                    style = MaterialTheme.typography.titleSmall,
                )

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedTextField(
                    value = skInput,
                    onValueChange = { skInput = it },
                    label = { Text("Spending key (hex or encoded)") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedButton(
                    onClick = { viewModel.importSpendingKey(skInput.trim()) },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !isBusy && skInput.trim().length >= 64,
                ) {
                    Text("Import Key")
                }

                Spacer(modifier = Modifier.height(24.dp))

                Text(
                    text = "Zclassic (ZCL) - Equihash(192,7) PoW\nSapling shielded transactions\nGroth16 zk-SNARKs\n100% P2P - no trusted servers",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                } // end disclaimerAccepted else
            } else {
                // Normal wallet view
                Spacer(modifier = Modifier.height(8.dp))

                BalanceCard(
                    balance = balance,
                    syncPhase = syncPhase,
                    syncProgress = syncProgress,
                    isSyncing = isSyncing,
                )

                // Last transaction activity below balance
                LastTransactionActivity(
                    transactions = transactions,
                    mempoolAccepted = mempoolAccepted,
                    mempoolPeerStatus = mempoolPeerStatus,
                    sendTxid = sendTxid,
                )

                // Pending confirmation banner
                if (pendingTxid != null) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Card(
                        modifier = Modifier.fillMaxWidth(),
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.3f),
                        ),
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                "AWAITING CONFIRMATION",
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.error,
                            )
                            Text(
                                if (mempoolPeerStatus != null) "Broadcast to $mempoolPeerStatus peers — waiting for block..."
                                else "Transaction broadcast — waiting for block confirmation...",
                                style = MaterialTheme.typography.bodySmall,
                            )
                            Text(
                                "tx: ${pendingTxid!!.take(16)}...",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
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
                    enabled = pendingTxid == null,
                ) {
                    Text(if (pendingTxid != null) "Send [Locked]" else "Send")
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
                    title = "ACCEPTED IN MEMPOOL",
                    message = "TX broadcast to ${mempoolPeerStatus ?: "?"} peers. Waiting for a miner to seal it into a block...",
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
                    message = confirmationMessage ?: "Transaction confirmed on-chain.",
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
                    CypherpunkToast(
                        icon = Icons.AutoMirrored.Filled.CallReceived,
                        iconColor = Color(0xFF00E676),
                        title = "INCOMING TRANSACTION",
                        message = "+${formatZclAmount(tx.amount)} ZCL received. " +
                            if (tx.confirmations > 0) "${tx.confirmations} confirmation(s)."
                            else "In mempool — waiting for miner.",
                        accentColor = Color(0xFF00E676),
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
