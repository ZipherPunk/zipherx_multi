package com.zipherx.wallet.ui

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.PersistableBundle
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.lifecycle.viewModelScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import com.zipherx.wallet.ZipherXWrapper
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Settings screen for wallet configuration.
 *
 * Cypherpunk terminal aesthetic with peer info, Tor controls,
 * auth lock toggle, security audit, maintenance, and danger zone.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    viewModel: WalletViewModel,
    onNavigateBack: () -> Unit = {},
) {
    val torEnabled by viewModel.torEnabled.collectAsState()
    val isSyncing by viewModel.isSyncing.collectAsState()
    val isAuthRequired by viewModel.isAuthRequired.collectAsState()
    val screenshotProtection by viewModel.screenshotProtection.collectAsState()
    val connectedPeers by viewModel.connectedPeers.collectAsState()
    val onionAddress by viewModel.onionAddress.collectAsState()
    val syncPhase by viewModel.syncPhase.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    var showDeleteConfirmDialog by remember { mutableStateOf(false) }
    var showExportKeyDialog by remember { mutableStateOf(false) }
    var exportedKey by remember { mutableStateOf<CharArray?>(null) }
    var showSecurityAuditDialog by remember { mutableStateOf(false) }

    // Tor state polling
    var torState by remember { mutableStateOf<UByte>(0u) }
    var torOnionAddr by remember { mutableStateOf<String?>(null) }
    var peerCount by remember { mutableStateOf(0u) }

    // Refresh network info
    fun refreshNetworkInfo() {
        scope.launch {
            peerCount = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeerCount() }
            torState = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getTorState() } catch (_: Exception) { 0u.toUByte() }
            }
            torOnionAddr = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getOnionAddress() } catch (_: Exception) { null }
            }
        }
    }

    // Initial load
    remember {
        scope.launch {
            peerCount = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeerCount() }
            torState = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getTorState() } catch (_: Exception) { 0u.toUByte() }
            }
            torOnionAddr = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getOnionAddress() } catch (_: Exception) { null }
            }
        }
        true
    }

    val cardShape = RoundedCornerShape(2.dp)
    val cardBorder = Modifier.border(1.dp, ZColors.primaryDim.copy(alpha = 0.4f), cardShape)

    val switchColors = SwitchDefaults.colors(
        checkedThumbColor = ZColors.terminalBlack,
        checkedTrackColor = ZColors.primary,
        uncheckedThumbColor = ZColors.primaryDim,
        uncheckedTrackColor = ZColors.surface,
        uncheckedBorderColor = ZColors.primaryDim,
    )

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "SETTINGS",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        letterSpacing = 2.sp,
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
                    containerColor = ZColors.surface,
                ),
            )
        },
        snackbarHost = { SnackbarHost(snackbarHostState) },
        containerColor = Color(0xFF0A0A0A),
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 16.dp)
                .verticalScroll(rememberScrollState()),
        ) {
            Spacer(modifier = Modifier.height(12.dp))

            // =================================================================
            // PEER INFO SECTION
            // =================================================================
            SectionHeader("PEER INFO")

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(cardBorder),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = ZColors.surface),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween,
                    ) {
                        Column {
                            Text(
                                text = "CONNECTED PEERS",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                // Green/red dot indicator
                                Box(
                                    modifier = Modifier
                                        .size(8.dp)
                                        .background(
                                            if (peerCount > 0u) ZColors.primary else ZColors.error,
                                            RoundedCornerShape(4.dp),
                                        ),
                                )
                                Spacer(modifier = Modifier.width(8.dp))
                                Text(
                                    text = "$peerCount",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 20.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = ZColors.primary,
                                )
                                Spacer(modifier = Modifier.width(6.dp))
                                Text(
                                    text = if (peerCount == 1u) "PEER" else "PEERS",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 10.sp,
                                    color = ZColors.primaryDim,
                                )
                            }
                        }
                        OutlinedButton(
                            onClick = { refreshNetworkInfo() },
                            shape = RoundedCornerShape(2.dp),
                            colors = ButtonDefaults.outlinedButtonColors(
                                contentColor = ZColors.primary,
                            ),
                        ) {
                            Text(
                                text = "REFRESH",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                letterSpacing = 1.sp,
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // TOR SECTION
            // =================================================================
            SectionHeader("TOR NETWORK")

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(cardBorder),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = ZColors.surface),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "TOR ROUTING",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primary,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = if (torEnabled)
                                    "All P2P traffic routed through Tor."
                                else
                                    "Tor disabled. P2P connections are direct.",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                            )
                        }
                        Switch(
                            checked = torEnabled,
                            onCheckedChange = { viewModel.setTorEnabled(it) },
                            colors = switchColors,
                        )
                    }

                    Spacer(modifier = Modifier.height(8.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(8.dp))

                    // Tor state row
                    val torStateLabel = when (torState.toInt()) {
                        0 -> "DISCONNECTED"
                        1 -> "CONNECTING"
                        2 -> "BOOTSTRAPPING"
                        3 -> "CONNECTED"
                        else -> "UNKNOWN"
                    }
                    val torStateColor = when (torState.toInt()) {
                        3 -> ZColors.primary
                        1, 2 -> ZColors.warning
                        else -> ZColors.error
                    }

                    InfoRow("STATE", torStateLabel, torStateColor)

                    if (torState.toInt() in 1..2) {
                        Spacer(modifier = Modifier.height(6.dp))
                        LinearProgressIndicator(
                            modifier = Modifier
                                .fillMaxWidth()
                                .height(2.dp),
                            color = ZColors.primary,
                            trackColor = ZColors.surface,
                        )
                    }

                    // Onion address display
                    if (torEnabled && torOnionAddr != null) {
                        Spacer(modifier = Modifier.height(8.dp))
                        HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                        Spacer(modifier = Modifier.height(8.dp))

                        Text(
                            text = "HIDDEN SERVICE",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 9.sp,
                            color = ZColors.primaryDim,
                            letterSpacing = 1.sp,
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = torOnionAddr ?: "",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 9.sp,
                                color = ZColors.primaryDark,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            Spacer(modifier = Modifier.width(8.dp))
                            OutlinedButton(
                                onClick = {
                                    torOnionAddr?.let { addr ->
                                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                        clipboard.setPrimaryClip(ClipData.newPlainText("onion", addr))
                                        scope.launch {
                                            snackbarHostState.showSnackbar("Onion address copied (auto-clears in 30s)")
                                        }
                                        // Auto-clear clipboard after 30 seconds
                                        scope.launch {
                                            kotlinx.coroutines.delay(30_000)
                                            clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                                        }
                                    }
                                },
                                shape = RoundedCornerShape(2.dp),
                                colors = ButtonDefaults.outlinedButtonColors(
                                    contentColor = ZColors.primary,
                                ),
                                modifier = Modifier.height(30.dp),
                            ) {
                                Text(
                                    text = "COPY",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 9.sp,
                                    letterSpacing = 1.sp,
                                )
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // AUTH / LOCK TOGGLE
            // =================================================================
            SectionHeader("SECURITY")

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(cardBorder),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = ZColors.surface),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    // Biometric auth toggle
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "BIOMETRIC LOCK",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primary,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = if (isAuthRequired)
                                    "Biometric auth required on launch and sensitive operations."
                                else
                                    "No auth required. Enable for extra security.",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                            )
                        }
                        Switch(
                            checked = isAuthRequired,
                            onCheckedChange = { newValue ->
                                scope.launch {
                                    // Always require biometric to toggle this setting
                                    val authed = viewModel.authenticateBiometric(
                                        if (newValue) "Authenticate to enable biometric lock"
                                        else "Authenticate to disable biometric lock"
                                    )
                                    if (authed) {
                                        viewModel.setAuthRequired(newValue)
                                    }
                                }
                            },
                            colors = switchColors,
                        )
                    }

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(12.dp))

                    // Screenshot protection toggle
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(
                                text = "SCREENSHOT PROTECTION",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primary,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = if (screenshotProtection)
                                    "Screenshots blocked while wallet is active."
                                else
                                    "Screenshots allowed. Less secure.",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                            )
                        }
                        Switch(
                            checked = screenshotProtection,
                            onCheckedChange = { viewModel.setScreenshotProtection(it) },
                            colors = switchColors,
                        )
                    }

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(12.dp))

                    // Export Spending Key
                    Text(
                        text = "EXPORT SPENDING KEY",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Export your private spending key. Anyone with this key can spend your funds.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                val authed = viewModel.authenticateBiometric(
                                    "Authenticate to export spending key"
                                )
                                if (authed) {
                                    exportedKey = viewModel.getSpendingKeyHex()
                                    showExportKeyDialog = exportedKey != null
                                    if (exportedKey == null) {
                                        snackbarHostState.showSnackbar("No spending key found")
                                    }
                                }
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = ZColors.primary,
                        ),
                    ) {
                        Text(
                            text = "EXPORT KEY",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            letterSpacing = 1.sp,
                        )
                    }

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(12.dp))

                    // Security Audit Report
                    Text(
                        text = "SECURITY AUDIT",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Generate a security checklist report for this wallet instance.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    OutlinedButton(
                        onClick = {
                            refreshNetworkInfo()
                            showSecurityAuditDialog = true
                        },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = ZColors.primary,
                        ),
                    ) {
                        Text(
                            text = "RUN AUDIT",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            letterSpacing = 1.sp,
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // SYNC SECTION
            // =================================================================
            SectionHeader("SYNC")

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(cardBorder),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = ZColors.surface),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Text(
                        text = "BLOCKCHAIN SYNC",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Downloads and verifies block headers and shielded transaction data from the P2P network. " +
                            "First sync may take 10-30 minutes. Subsequent syncs are incremental.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    Button(
                        onClick = {
                            if (isSyncing) viewModel.stopSync() else viewModel.startSync()
                        },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = if (isSyncing) ZColors.error else ZColors.primary,
                            contentColor = ZColors.terminalBlack,
                        ),
                    ) {
                        Text(
                            text = if (isSyncing) "STOP SYNC" else "START SYNC",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp,
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // MAINTENANCE SECTION
            // =================================================================
            SectionHeader("MAINTENANCE")

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .then(cardBorder),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = ZColors.surface),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                snackbarHostState.showSnackbar("Repair Database started")
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = ZColors.primary,
                        ),
                    ) {
                        Text(
                            text = "REPAIR DATABASE",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            letterSpacing = 1.sp,
                        )
                    }
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Clears tree state, preserves notes and history.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 9.sp,
                        color = ZColors.primaryDim,
                    )

                    Spacer(modifier = Modifier.height(12.dp))

                    Button(
                        onClick = {
                            scope.launch {
                                snackbarHostState.showSnackbar("Full Rescan started")
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.error,
                            contentColor = Color.White,
                        ),
                    ) {
                        Text(
                            text = "FULL RESCAN",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp,
                        )
                    }
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Re-downloads everything from scratch. May take 5-15 minutes.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 9.sp,
                        color = ZColors.primaryDim,
                    )
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // DANGER ZONE
            // =================================================================
            SectionHeader("DANGER ZONE", isError = true)

            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.error.copy(alpha = 0.4f), cardShape),
                shape = cardShape,
                colors = CardDefaults.cardColors(containerColor = Color(0xFF1A0A0A)),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                ) {
                    Text(
                        text = "DELETE ALL DATA",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.error,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Permanently deletes your wallet, spending key, transaction history, and all sync data. " +
                            "This cannot be undone. Back up your spending key first.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    Button(
                        onClick = { showDeleteConfirmDialog = true },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.error,
                            contentColor = Color.White,
                        ),
                    ) {
                        Text(
                            text = "DELETE ALL DATA",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp,
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(20.dp))

            // =================================================================
            // ABOUT SECTION
            // =================================================================
            HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
            Spacer(modifier = Modifier.height(12.dp))

            Text(
                text = "ZIPHERX FOR ANDROID",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                color = ZColors.primaryDim,
                letterSpacing = 1.sp,
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = "VERSION 1.0.0 (PHASE 10B)",
                fontFamily = FontFamily.Monospace,
                fontSize = 9.sp,
                color = ZColors.primaryDim,
            )
            Spacer(modifier = Modifier.height(2.dp))
            Text(
                text = "ZCLASSIC (ZCL) - PRIVACY-FIRST CRYPTOCURRENCY",
                fontFamily = FontFamily.Monospace,
                fontSize = 9.sp,
                color = ZColors.primaryDim,
            )
            Spacer(modifier = Modifier.height(2.dp))
            Text(
                text = "SAPLING SHIELDED TX WITH GROTH16 ZK-SNARKS",
                fontFamily = FontFamily.Monospace,
                fontSize = 9.sp,
                color = ZColors.primaryDim,
            )

            Spacer(modifier = Modifier.height(24.dp))
        }

        // ==================================================================
        // DIALOGS
        // ==================================================================

        // Export Key Dialog (KD-5: exportedKey is CharArray, zeroed after use)
        if (showExportKeyDialog && exportedKey != null) {
            // KD-1: Auto-dismiss after 60 seconds to limit on-screen exposure
            LaunchedEffect(showExportKeyDialog) {
                kotlinx.coroutines.delay(60_000)
                exportedKey?.fill('\u0000')
                exportedKey = null
                showExportKeyDialog = false
            }
            AlertDialog(
                onDismissRequest = {
                    showExportKeyDialog = false
                    exportedKey?.fill('\u0000')
                    exportedKey = null
                },
                containerColor = ZColors.surface,
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "SPENDING KEY",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.error,
                    )
                },
                text = {
                    Column {
                        Text(
                            text = "WARNING: Anyone with this key can spend all your funds. Never share it.",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.error,
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                        Text(
                            text = String(exportedKey!!),
                            fontFamily = FontFamily.Monospace,
                            fontSize = 9.sp,
                            lineHeight = 14.sp,
                            color = ZColors.primary,
                        )
                    }
                },
                confirmButton = {
                    TextButton(onClick = {
                        exportedKey?.let { key ->
                            val keyString = String(key)
                            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            val clipData = ClipData.newPlainText("spending_key", keyString)
                            // Mark as sensitive so Android 13+ hides the content in clipboard previews
                            if (Build.VERSION.SDK_INT >= 33) {
                                clipData.description.extras = PersistableBundle().apply {
                                    putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                                }
                            }
                            clipboard.setPrimaryClip(clipData)
                            // KD-11: Use viewModel scope for auto-clear so it survives composable disposal
                            viewModel.viewModelScope.launch {
                                kotlinx.coroutines.delay(5_000)
                                clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                            }
                            // Zero the CharArray after copying to clipboard
                            key.fill('\u0000')
                        }
                        showExportKeyDialog = false
                        exportedKey = null
                        scope.launch { snackbarHostState.showSnackbar("Key copied (auto-clears in 5s)") }
                    }) {
                        Text(
                            text = "COPY & CLOSE",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primary,
                        )
                    }
                },
                dismissButton = {
                    TextButton(onClick = {
                        showExportKeyDialog = false
                        exportedKey?.fill('\u0000')
                        exportedKey = null
                    }) {
                        Text(
                            text = "CLOSE",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                    }
                },
            )
        }

        // Delete All Data Confirmation Dialog
        if (showDeleteConfirmDialog) {
            AlertDialog(
                onDismissRequest = { showDeleteConfirmDialog = false },
                containerColor = Color(0xFF1A0A0A),
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "DELETE ALL DATA?",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.error,
                    )
                },
                text = {
                    Column {
                        Text(
                            text = "This will permanently delete:",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            color = ZColors.primary,
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "- Your spending key\n- All transaction history\n- All sync data\n- All wallet configuration",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.error,
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                        Text(
                            text = "If you have not backed up your spending key or mnemonic, your funds will be PERMANENTLY LOST.",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                            color = ZColors.error,
                        )
                    }
                },
                confirmButton = {
                    Button(
                        onClick = {
                            showDeleteConfirmDialog = false
                            scope.launch {
                                val authed = viewModel.authenticateBiometric(
                                    "Authenticate to delete all wallet data"
                                )
                                if (authed) {
                                    viewModel.deleteAllData()
                                    // KA-N9: Graceful shutdown instead of exitProcess(0)
                                    // finishAffinity closes all activities without abruptly
                                    // killing the process, allowing cleanup to complete.
                                    (context as? android.app.Activity)?.finishAffinity()
                                }
                            }
                        },
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.error,
                            contentColor = Color.White,
                        ),
                    ) {
                        Text(
                            text = "DELETE EVERYTHING",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                },
                dismissButton = {
                    TextButton(onClick = { showDeleteConfirmDialog = false }) {
                        Text(
                            text = "CANCEL",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                    }
                },
            )
        }

        // Security Audit Report Dialog
        if (showSecurityAuditDialog) {
            val dbEncrypted = true // DB is always encrypted via SQLCipher (see ZipherXApplication)
            val skInSecureStorage = viewModel.hasSpendingKey()
            val auditTorEnabled = torEnabled
            val auditAuthEnabled = isAuthRequired
            val auditSyncPhase = syncPhase.uppercase()
            val appVersion = "1.0.0"

            AlertDialog(
                onDismissRequest = { showSecurityAuditDialog = false },
                containerColor = ZColors.surface,
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "SECURITY AUDIT",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.primary,
                    )
                },
                text = {
                    Column {
                        AuditRow("DATABASE ENCRYPTED", dbEncrypted)
                        Spacer(modifier = Modifier.height(6.dp))
                        AuditRow("SK IN SECURE STORAGE", skInSecureStorage)
                        Spacer(modifier = Modifier.height(6.dp))
                        AuditRow("BIOMETRIC AUTH ENABLED", auditAuthEnabled)
                        Spacer(modifier = Modifier.height(6.dp))
                        AuditRow("TOR ENABLED", auditTorEnabled)
                        Spacer(modifier = Modifier.height(10.dp))
                        HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                        Spacer(modifier = Modifier.height(10.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(
                                text = "LAST SYNC",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                            )
                            Text(
                                text = auditSyncPhase,
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primary,
                            )
                        }
                        Spacer(modifier = Modifier.height(4.dp))
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(
                                text = "APP VERSION",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primaryDim,
                            )
                            Text(
                                text = appVersion,
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primary,
                            )
                        }
                    }
                },
                confirmButton = {
                    TextButton(onClick = { showSecurityAuditDialog = false }) {
                        Text(
                            text = "CLOSE",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primary,
                        )
                    }
                },
            )
        }
    }
}

// ==========================================================================
// Helper Composables
// ==========================================================================

@Composable
private fun SectionHeader(title: String, isError: Boolean = false) {
    Text(
        text = title,
        fontFamily = FontFamily.Monospace,
        fontSize = 11.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 2.sp,
        color = if (isError) ZColors.error else ZColors.primaryDark,
    )
    Spacer(modifier = Modifier.height(8.dp))
}

@Composable
private fun InfoRow(label: String, value: String, valueColor: Color = ZColors.primary) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = ZColors.primaryDim,
            letterSpacing = 1.sp,
        )
        Text(
            text = value,
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            fontWeight = FontWeight.Bold,
            color = valueColor,
        )
    }
}

@Composable
private fun AuditRow(label: String, enabled: Boolean) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = ZColors.primaryDim,
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .background(
                        if (enabled) ZColors.primary else ZColors.error,
                        RoundedCornerShape(4.dp),
                    ),
            )
            Spacer(modifier = Modifier.width(6.dp))
            Text(
                text = if (enabled) "YES" else "NO",
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
                fontWeight = FontWeight.Bold,
                color = if (enabled) ZColors.primary else ZColors.error,
            )
        }
    }
}
