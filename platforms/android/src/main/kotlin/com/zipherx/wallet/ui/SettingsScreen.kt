package com.zipherx.wallet.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.PersistableBundle
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
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import com.zipherx.wallet.BannedPeerInfo
import com.zipherx.wallet.ConnectedPeerInfo
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
    val isRepairing by viewModel.isRepairing.collectAsState()
    val repairStatus by viewModel.repairStatus.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    var showDeleteConfirmDialog by remember { mutableStateOf(false) }
    var showExportKeyDialog by remember { mutableStateOf(false) }
    var exportedShieldedKey by remember { mutableStateOf<CharArray?>(null) }
    var exportedTransparentKey by remember { mutableStateOf<CharArray?>(null) }
    var showRecoveryPhraseDialog by remember { mutableStateOf(false) }
    var recoveryPhraseChars by remember { mutableStateOf<CharArray?>(null) }
    var showSecurityAuditDialog by remember { mutableStateOf(false) }
    var showRescanConfirmDialog by remember { mutableStateOf(false) }
    // WIF import state
    var showWifImportDialog by remember { mutableStateOf(false) }
    var wifImportText by remember { mutableStateOf("") }
    var wifImportResults by remember { mutableStateOf<List<Triple<Boolean, String, String>>?>(null) }

    // Peer management state
    var peerList by remember { mutableStateOf<List<ConnectedPeerInfo>>(emptyList()) }
    var bannedList by remember { mutableStateOf<List<BannedPeerInfo>>(emptyList()) }
    var customPeerHost by remember { mutableStateOf("") }
    var customPeerPort by remember { mutableStateOf("8033") }
    var peerActionResult by remember { mutableStateOf<String?>(null) }
    var peerSectionExpanded by remember { mutableStateOf(false) }

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

    // Periodic network info refresh (every 5s while screen is visible)
    LaunchedEffect(Unit) {
        while (true) {
            peerCount = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeerCount() }
            torState = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getTorState() } catch (_: Exception) { 0u.toUByte() }
            }
            torOnionAddr = withContext(Dispatchers.IO) {
                try { ZipherXWrapper.getOnionAddress() } catch (_: Exception) { null }
            }
            kotlinx.coroutines.delay(5_000)
        }
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
            // PEER MANAGEMENT SECTION (collapsible)
            // =================================================================
            SectionHeader("PEER MANAGEMENT")

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
                    // Clickable header row — toggles expand/collapse
                    Row(
                        modifier = Modifier.fillMaxWidth().clickable {
                            peerSectionExpanded = !peerSectionExpanded
                            if (peerSectionExpanded) {
                                scope.launch {
                                    peerCount = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeerCount() }
                                    peerList = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeers() }
                                    bannedList = withContext(Dispatchers.IO) { ZipherXWrapper.getBannedPeers() }
                                }
                            }
                        },
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "PEER DETAILS",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Bold,
                            color = ZColors.primary,
                            letterSpacing = 1.sp,
                        )
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(
                                text = "${peerList.size} connected, ${bannedList.size} banned",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 9.sp,
                                color = ZColors.primaryDim,
                            )
                            Text(
                                text = if (peerSectionExpanded) "[-]" else "[+]",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 12.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primary,
                            )
                        }
                    }

                    AnimatedVisibility(
                        visible = peerSectionExpanded,
                        enter = expandVertically(),
                        exit = shrinkVertically(),
                    ) {
                        Column {
                            Spacer(modifier = Modifier.height(8.dp))

                            // Refresh button
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.End,
                            ) {
                                OutlinedButton(
                                    onClick = {
                                        scope.launch {
                                            peerList = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeers() }
                                            bannedList = withContext(Dispatchers.IO) { ZipherXWrapper.getBannedPeers() }
                                            peerActionResult = null
                                        }
                                    },
                                    shape = RoundedCornerShape(2.dp),
                                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                                ) {
                                    Text("REFRESH", fontFamily = FontFamily.Monospace, fontSize = 10.sp, letterSpacing = 1.sp)
                                }
                            }

                            // Action result feedback
                            if (peerActionResult != null) {
                                Spacer(modifier = Modifier.height(8.dp))
                                Text(
                                    text = peerActionResult!!,
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 9.sp,
                                    color = if (peerActionResult!!.startsWith("Error")) ZColors.error else ZColors.primary,
                                )
                            }

                            Spacer(modifier = Modifier.height(12.dp))
                            HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                            Spacer(modifier = Modifier.height(12.dp))

                            // Connected peers list
                            Text(
                                text = "CONNECTED (${peerList.size})",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primary,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(6.dp))

                            if (peerList.isEmpty()) {
                                Text(
                                    text = "No peers loaded. Tap REFRESH.",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 9.sp,
                                    color = ZColors.primaryDim,
                                )
                            }
                            for (peer in peerList) {
                                Row(
                                    modifier = Modifier.fillMaxWidth().padding(vertical = 3.dp),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = peer.address,
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 9.sp,
                                            color = ZColors.primary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                        Text(
                                            text = "v${peer.protocolVersion} | ${peer.userAgent.take(24)} | h:${peer.startHeight}",
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 8.sp,
                                            color = ZColors.primaryDim,
                                        )
                                    }
                                    OutlinedButton(
                                        onClick = {
                                            scope.launch {
                                                val ok = withContext(Dispatchers.IO) { ZipherXWrapper.disconnectPeer(peer.address) }
                                                peerActionResult = if (ok) "Disconnected ${peer.address}" else "Error: disconnect failed"
                                                peerList = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeers() }
                                            }
                                        },
                                        shape = RoundedCornerShape(2.dp),
                                        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.error),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                                        modifier = Modifier.height(28.dp),
                                    ) {
                                        Text("DC", fontFamily = FontFamily.Monospace, fontSize = 8.sp)
                                    }
                                }
                            }

                            Spacer(modifier = Modifier.height(12.dp))
                            HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                            Spacer(modifier = Modifier.height(12.dp))

                            // Banned peers list
                            Text(
                                text = "BANNED (${bannedList.size})",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.error,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(6.dp))

                            if (bannedList.isEmpty()) {
                                Text(
                                    text = "No banned peers.",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 9.sp,
                                    color = ZColors.primaryDim,
                                )
                            }
                            for (peer in bannedList) {
                                Row(
                                    modifier = Modifier.fillMaxWidth().padding(vertical = 3.dp),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = peer.host,
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 9.sp,
                                            color = ZColors.error,
                                        )
                                        val timeStr = if (peer.isPermanent) "permanent" else "${peer.remainingSeconds}s left"
                                        Text(
                                            text = "${peer.reason.take(30)} | $timeStr",
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 8.sp,
                                            color = ZColors.primaryDim,
                                        )
                                    }
                                    OutlinedButton(
                                        onClick = {
                                            scope.launch {
                                                val ok = withContext(Dispatchers.IO) { ZipherXWrapper.unbanPeer(peer.host) }
                                                peerActionResult = if (ok) "Unbanned ${peer.host}" else "Error: unban failed"
                                                bannedList = withContext(Dispatchers.IO) { ZipherXWrapper.getBannedPeers() }
                                            }
                                        },
                                        shape = RoundedCornerShape(2.dp),
                                        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                                        contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                                        modifier = Modifier.height(28.dp),
                                    ) {
                                        Text("UNBAN", fontFamily = FontFamily.Monospace, fontSize = 8.sp)
                                    }
                                }
                            }

                            Spacer(modifier = Modifier.height(12.dp))
                            HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                            Spacer(modifier = Modifier.height(12.dp))

                            // Add custom peer
                            Text(
                                text = "ADD CUSTOM PEER",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.primaryDim,
                                letterSpacing = 1.sp,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = "IP address only (no hostnames — DNS leak prevention).",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 8.sp,
                                color = ZColors.primaryDim,
                            )
                            Spacer(modifier = Modifier.height(8.dp))

                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                OutlinedTextField(
                                    value = customPeerHost,
                                    onValueChange = { customPeerHost = it.filter { c -> c.isDigit() || c == '.' } },
                                    label = { Text("IP Address", fontFamily = FontFamily.Monospace, fontSize = 9.sp) },
                                    singleLine = true,
                                    modifier = Modifier.weight(1f),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        focusedBorderColor = ZColors.primary,
                                        unfocusedBorderColor = ZColors.primaryDim.copy(alpha = 0.4f),
                                        cursorColor = ZColors.primary,
                                        focusedTextColor = ZColors.primary,
                                        unfocusedTextColor = ZColors.primaryDim,
                                    ),
                                    shape = RoundedCornerShape(2.dp),
                                    textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 10.sp),
                                )
                                OutlinedTextField(
                                    value = customPeerPort,
                                    onValueChange = { customPeerPort = it.filter { c -> c.isDigit() }.take(5) },
                                    label = { Text("Port", fontFamily = FontFamily.Monospace, fontSize = 9.sp) },
                                    singleLine = true,
                                    modifier = Modifier.width(80.dp),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        focusedBorderColor = ZColors.primary,
                                        unfocusedBorderColor = ZColors.primaryDim.copy(alpha = 0.4f),
                                        cursorColor = ZColors.primary,
                                        focusedTextColor = ZColors.primary,
                                        unfocusedTextColor = ZColors.primaryDim,
                                    ),
                                    shape = RoundedCornerShape(2.dp),
                                    textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 10.sp),
                                )
                                OutlinedButton(
                                    onClick = {
                                        val port = customPeerPort.toIntOrNull() ?: 0
                                        if (customPeerHost.isBlank()) {
                                            peerActionResult = "Error: IP address required"
                                        } else if (port !in 1..65535) {
                                            peerActionResult = "Error: Invalid port (1-65535)"
                                        } else {
                                            scope.launch {
                                                val ok = withContext(Dispatchers.IO) { ZipherXWrapper.addCustomPeer(customPeerHost, port) }
                                                peerActionResult = if (ok) "Added ${customPeerHost}:$port" else "Error: Invalid IP or peer rejected"
                                                if (ok) {
                                                    customPeerHost = ""
                                                    peerList = withContext(Dispatchers.IO) { ZipherXWrapper.getConnectedPeers() }
                                                }
                                            }
                                        }
                                    },
                                    shape = RoundedCornerShape(2.dp),
                                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                                ) {
                                    Text("ADD", fontFamily = FontFamily.Monospace, fontSize = 10.sp)
                                }
                            }
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
                                    // Require authentication to toggle — falls back to
                                    // PIN/pattern if no biometrics enrolled
                                    val authed = viewModel.authenticateStrict(
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

                    // Export Private Keys
                    Text(
                        text = "EXPORT PRIVATE KEYS",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Anyone with these keys can spend your funds. Keep them safe.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))

                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                val authed = viewModel.authenticateStrict(
                                    "Authenticate to export private keys"
                                )
                                if (authed) {
                                    exportedShieldedKey = viewModel.getSpendingKeyHex()
                                    exportedTransparentKey = viewModel.getTransparentKeyWif()
                                    showExportKeyDialog = exportedShieldedKey != null
                                    if (exportedShieldedKey == null) {
                                        snackbarHostState.showSnackbar("No private key found")
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
                            text = "EXPORT PRIVATE KEYS",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            letterSpacing = 1.sp,
                        )
                    }

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(12.dp))

                    // Export Recovery Phrase
                    Text(
                        text = "RECOVERY PHRASE (24 WORDS)",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Export your 24-word mnemonic recovery phrase. Not available for seed/key imports.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))

                    OutlinedButton(
                        onClick = {
                            scope.launch {
                                val authed = viewModel.authenticateStrict(
                                    "Authenticate to export recovery phrase"
                                )
                                if (authed) {
                                    val phrase = viewModel.getRecoveryPhrase()
                                    if (phrase != null) {
                                        recoveryPhraseChars = phrase
                                        showRecoveryPhraseDialog = true
                                    } else {
                                        snackbarHostState.showSnackbar("No recovery phrase stored (wallet imported from key/seed)")
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
                            text = "EXPORT RECOVERY PHRASE",
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

                    Spacer(modifier = Modifier.height(12.dp))
                    HorizontalDivider(color = ZColors.primaryDim.copy(alpha = 0.3f))
                    Spacer(modifier = Modifier.height(12.dp))

                    // Import WIF Keys
                    Text(
                        text = "IMPORT TRANSPARENT KEYS",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = ZColors.primary,
                        letterSpacing = 1.sp,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Import transparent private keys in WIF format. These are NOT covered by your recovery phrase.",
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                        color = ZColors.primaryDim,
                    )
                    Spacer(modifier = Modifier.height(12.dp))

                    OutlinedButton(
                        onClick = { showWifImportDialog = true },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = ZColors.primary,
                        ),
                    ) {
                        Text(
                            text = "IMPORT WIF KEYS",
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
                    // Status indicator when a maintenance operation is in progress
                    if (isRepairing && repairStatus != null) {
                        LinearProgressIndicator(
                            modifier = Modifier.fillMaxWidth(),
                            color = ZColors.primary,
                            trackColor = ZColors.surface,
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = repairStatus ?: "",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primary,
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                    }

                    OutlinedButton(
                        onClick = { viewModel.repairDatabase() },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        enabled = !isRepairing && !isSyncing,
                        colors = ButtonDefaults.outlinedButtonColors(
                            contentColor = ZColors.primary,
                            disabledContentColor = ZColors.primaryDim,
                        ),
                    ) {
                        Text(
                            text = if (isRepairing) "REPAIRING..." else "REPAIR DATABASE",
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
                        onClick = { showRescanConfirmDialog = true },
                        modifier = Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(2.dp),
                        enabled = !isRepairing && !isSyncing,
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.error,
                            contentColor = Color.White,
                            disabledContainerColor = ZColors.error.copy(alpha = 0.3f),
                            disabledContentColor = Color.White.copy(alpha = 0.5f),
                        ),
                    ) {
                        Text(
                            text = if (isRepairing) "RESCANNING..." else "FULL RESCAN",
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

        // Export Key Dialog — matches egui design: both keys in one dialog
        if (showExportKeyDialog && exportedShieldedKey != null) {
            // KD-1: Auto-dismiss after 60 seconds to limit on-screen exposure
            LaunchedEffect(showExportKeyDialog) {
                kotlinx.coroutines.delay(60_000)
                exportedShieldedKey?.fill('\u0000')
                exportedShieldedKey = null
                exportedTransparentKey?.fill('\u0000')
                exportedTransparentKey = null
                showExportKeyDialog = false
            }
            AlertDialog(
                onDismissRequest = {
                    showExportKeyDialog = false
                    exportedShieldedKey?.fill('\u0000')
                    exportedShieldedKey = null
                    exportedTransparentKey?.fill('\u0000')
                    exportedTransparentKey = null
                },
                containerColor = Color(0xFF1A0808),
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "PRIVATE KEYS \u2014 KEEP SECRET",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 13.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.error,
                    )
                },
                text = {
                    Column {
                        // Shielded key section
                        Text(
                            text = "SHIELDED (z-address)",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                            color = ZColors.primary,
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = String(exportedShieldedKey!!),
                            fontFamily = FontFamily.Monospace,
                            fontSize = 8.sp,
                            lineHeight = 12.sp,
                            color = ZColors.warning,
                        )
                        Spacer(modifier = Modifier.height(6.dp))
                        OutlinedButton(
                            onClick = {
                                copyToClipboardSecure(context, viewModel, "shielded_key", String(exportedShieldedKey!!))
                                scope.launch { snackbarHostState.showSnackbar("Shielded key copied (auto-clears in 5s)") }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(0.dp),
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                        ) {
                            Text("[ COPY SHIELDED KEY ]", fontFamily = FontFamily.Monospace, fontSize = 9.sp)
                        }

                        // Transparent key section (if available)
                        if (exportedTransparentKey != null) {
                            Spacer(modifier = Modifier.height(12.dp))
                            Text(
                                text = "TRANSPARENT (t-address, WIF)",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                                color = ZColors.warning,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                text = String(exportedTransparentKey!!),
                                fontFamily = FontFamily.Monospace,
                                fontSize = 8.sp,
                                lineHeight = 12.sp,
                                color = ZColors.warning,
                            )
                            Spacer(modifier = Modifier.height(6.dp))
                            OutlinedButton(
                                onClick = {
                                    copyToClipboardSecure(context, viewModel, "transparent_key", String(exportedTransparentKey!!))
                                    scope.launch { snackbarHostState.showSnackbar("Transparent key copied (auto-clears in 5s)") }
                                },
                                modifier = Modifier.fillMaxWidth(),
                                shape = RoundedCornerShape(0.dp),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.warning),
                            ) {
                                Text("[ COPY TRANSPARENT KEY ]", fontFamily = FontFamily.Monospace, fontSize = 9.sp)
                            }
                        }
                    }
                },
                confirmButton = {},
                dismissButton = {
                    TextButton(onClick = {
                        showExportKeyDialog = false
                        exportedShieldedKey?.fill('\u0000')
                        exportedShieldedKey = null
                        exportedTransparentKey?.fill('\u0000')
                        exportedTransparentKey = null
                    }) {
                        Text(
                            text = "[ DISMISS ]",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                    }
                },
            )
        }

        // Recovery Phrase Dialog
        if (showRecoveryPhraseDialog && recoveryPhraseChars != null) {
            // Auto-dismiss after 60 seconds to limit on-screen exposure
            LaunchedEffect(showRecoveryPhraseDialog) {
                kotlinx.coroutines.delay(60_000)
                recoveryPhraseChars?.fill('\u0000')
                recoveryPhraseChars = null
                showRecoveryPhraseDialog = false
            }
            AlertDialog(
                onDismissRequest = {
                    showRecoveryPhraseDialog = false
                    recoveryPhraseChars?.fill('\u0000')
                    recoveryPhraseChars = null
                },
                containerColor = Color(0xFF1A0808),
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "RECOVERY PHRASE (24 WORDS)",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 13.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.warning,
                    )
                },
                text = {
                    Column {
                        Text(
                            text = "WRITE THESE DOWN AND KEEP THEM SAFE!",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                            color = ZColors.error,
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                        // Display words in a numbered grid
                        val words = String(recoveryPhraseChars!!).split(" ")
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .border(1.dp, ZColors.glow, RoundedCornerShape(2.dp))
                                .padding(12.dp),
                        ) {
                            words.chunked(3).forEachIndexed { rowIdx, row ->
                                Row(modifier = Modifier.fillMaxWidth()) {
                                    row.forEachIndexed { colIdx, word ->
                                        val num = rowIdx * 3 + colIdx + 1
                                        Text(
                                            text = "${num.toString().padStart(2)}. $word",
                                            fontSize = 11.sp,
                                            fontFamily = FontFamily.Monospace,
                                            color = ZColors.primary,
                                            modifier = Modifier.weight(1f),
                                        )
                                    }
                                }
                                Spacer(Modifier.height(4.dp))
                            }
                        }
                        Spacer(modifier = Modifier.height(12.dp))
                        OutlinedButton(
                            onClick = {
                                copyToClipboardSecure(context, viewModel, "recovery_phrase", String(recoveryPhraseChars!!))
                                scope.launch { snackbarHostState.showSnackbar("Recovery phrase copied (auto-clears in 5s)") }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(0.dp),
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                        ) {
                            Text("[ COPY PHRASE ]", fontFamily = FontFamily.Monospace, fontSize = 9.sp)
                        }
                    }
                },
                confirmButton = {},
                dismissButton = {
                    TextButton(onClick = {
                        showRecoveryPhraseDialog = false
                        recoveryPhraseChars?.fill('\u0000')
                        recoveryPhraseChars = null
                    }) {
                        Text(
                            text = "[ DISMISS ]",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                    }
                },
            )
        }

        // WIF Import Dialog
        if (showWifImportDialog) {
            AlertDialog(
                onDismissRequest = {
                    showWifImportDialog = false
                    wifImportText = ""
                    wifImportResults = null
                },
                containerColor = ZColors.surface,
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "IMPORT TRANSPARENT KEYS",
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        letterSpacing = 2.sp,
                        color = ZColors.primary,
                    )
                },
                text = {
                    Column {
                        Text(
                            text = "Paste transparent private keys (one per line):",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = "Accepted formats:\n\u2022 WIF compressed: L... or K... (standard)\n\u2022 Electrum-ZCL: p2pkh:L... or p2pkh:K...\n\u2022 Electrum CSV: t1addr,p2pkh:L... (paste full export)",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 9.sp,
                            color = ZColors.primaryDim,
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        OutlinedTextField(
                            value = wifImportText,
                            onValueChange = { wifImportText = it },
                            modifier = Modifier.fillMaxWidth().height(120.dp),
                            textStyle = androidx.compose.ui.text.TextStyle(
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = ZColors.primary,
                            ),
                            placeholder = {
                                Text(
                                    "L... or K... or p2pkh:L... (one per line)",
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 10.sp,
                                    color = ZColors.primaryDim.copy(alpha = 0.5f),
                                )
                            },
                        )
                        Spacer(modifier = Modifier.height(8.dp))

                        // Validate button
                        OutlinedButton(
                            onClick = {
                                val lines = wifImportText.lines().map { it.trim() }.filter { it.isNotEmpty() }
                                val results = mutableListOf<Triple<Boolean, String, String>>()
                                for (line in lines) {
                                    try {
                                        val validationResults = uniffi.zipherx.validateWifKeys(listOf(line))
                                        if (validationResults.isNotEmpty() && validationResults[0].valid) {
                                            val prefix = if (line.length > 8) "${line.substring(0, 8)}..." else line
                                            results.add(Triple(true, validationResults[0].address, prefix))
                                        } else {
                                            val prefix = if (line.length > 8) "${line.substring(0, 8)}..." else line
                                            val errMsg = validationResults.firstOrNull()?.errorMessage ?: "Invalid WIF"
                                            results.add(Triple(false, errMsg, prefix))
                                        }
                                    } catch (e: Exception) {
                                        val prefix = if (line.length > 8) "${line.substring(0, 8)}..." else line
                                        results.add(Triple(false, e.message ?: "Error", prefix))
                                    }
                                }
                                wifImportResults = results
                            },
                            modifier = Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(2.dp),
                        ) {
                            Text("VALIDATE", fontFamily = FontFamily.Monospace, fontSize = 10.sp, color = ZColors.primary)
                        }

                        // Results
                        val results = wifImportResults
                        if (results != null) {
                            Spacer(modifier = Modifier.height(8.dp))
                            val validCount = results.count { it.first }
                            val invalidCount = results.size - validCount
                            Text(
                                text = "Found $validCount valid, $invalidCount invalid key(s)",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 10.sp,
                                color = if (validCount > 0) ZColors.success else ZColors.error,
                            )
                            Spacer(modifier = Modifier.height(4.dp))
                            // Scrollable results list (for large imports)
                            Column(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(max = 200.dp)
                                    .verticalScroll(rememberScrollState())
                            ) {
                            for ((valid, addrOrErr, prefix) in results) {
                                Column(modifier = Modifier.padding(vertical = 2.dp)) {
                                    Row {
                                        Text(
                                            text = if (valid) "\u2713" else "\u2717",
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 10.sp,
                                            color = if (valid) ZColors.success else ZColors.error,
                                        )
                                        Spacer(modifier = Modifier.width(4.dp))
                                        Text(
                                            text = prefix,
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 9.sp,
                                            color = if (valid) ZColors.primary else ZColors.error,
                                        )
                                    }
                                    Text(
                                        text = if (valid) "  \u2192 $addrOrErr" else "  $addrOrErr",
                                        fontFamily = FontFamily.Monospace,
                                        fontSize = 9.sp,
                                        color = if (valid) ZColors.primaryDim else ZColors.error,
                                        maxLines = 2,
                                        modifier = Modifier.padding(start = 14.dp),
                                    )
                                }
                            }
                            } // end scrollable Column
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                text = "WARNING: Imported keys are NOT covered by your recovery phrase. Back up WIF keys separately.",
                                fontFamily = FontFamily.Monospace,
                                fontSize = 9.sp,
                                color = ZColors.warning,
                            )

                            if (validCount > 0) {
                                Spacer(modifier = Modifier.height(8.dp))
                                Button(
                                    onClick = {
                                        // Import valid keys via ViewModel (triggers rescan + balance refresh)
                                        scope.launch {
                                            try {
                                                val lines = wifImportText.lines().map { it.trim() }.filter { it.isNotEmpty() }
                                                val validResults = uniffi.zipherx.validateWifKeys(lines)
                                                val encKeys = mutableListOf<List<UByte>>()
                                                val addrs = mutableListOf<String>()
                                                for ((i, r) in validResults.withIndex()) {
                                                    if (r.valid) {
                                                        encKeys.add(lines[i].toByteArray().map { it.toUByte() })
                                                        addrs.add(r.address)
                                                    }
                                                }
                                                if (encKeys.isNotEmpty()) {
                                                    viewModel.importWifKeysAndRescan(encKeys, addrs)
                                                    snackbarHostState.showSnackbar("Imported ${encKeys.size} key(s) — scanning blockchain...")
                                                }
                                            } catch (e: Exception) {
                                                snackbarHostState.showSnackbar("Import error: ${e.message}")
                                            }
                                            wifImportText = ""
                                            wifImportResults = null
                                            showWifImportDialog = false
                                        }
                                    },
                                    modifier = Modifier.fillMaxWidth(),
                                    shape = RoundedCornerShape(2.dp),
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = ZColors.primary.copy(alpha = 0.2f),
                                        contentColor = ZColors.primary,
                                    ),
                                ) {
                                    Text(
                                        text = "IMPORT $validCount KEY(S)",
                                        fontFamily = FontFamily.Monospace,
                                        fontSize = 11.sp,
                                        letterSpacing = 1.sp,
                                    )
                                }
                            }
                        }
                    }
                },
                confirmButton = {},
                dismissButton = {
                    TextButton(onClick = {
                        showWifImportDialog = false
                        wifImportText = ""
                        wifImportResults = null
                    }) {
                        Text(
                            text = "[ CANCEL ]",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                    }
                },
            )
        }

        // Full Rescan Confirmation Dialog
        if (showRescanConfirmDialog) {
            AlertDialog(
                onDismissRequest = { showRescanConfirmDialog = false },
                containerColor = ZColors.surface,
                shape = RoundedCornerShape(2.dp),
                title = {
                    Text(
                        text = "FULL RESCAN?",
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
                            text = "This will reset all sync state and re-download everything from scratch.",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 11.sp,
                            color = ZColors.primary,
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "- Balance will show 0 until rescan completes\n- May take 5-15 minutes depending on connection\n- Your notes and spending key are preserved",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            color = ZColors.primaryDim,
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                        Text(
                            text = "Only use this if repair does not fix your issue.",
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
                            showRescanConfirmDialog = false
                            viewModel.fullRescan()
                        },
                        shape = RoundedCornerShape(2.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = ZColors.error,
                            contentColor = Color.White,
                        ),
                    ) {
                        Text(
                            text = "START FULL RESCAN",
                            fontFamily = FontFamily.Monospace,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                },
                dismissButton = {
                    TextButton(onClick = { showRescanConfirmDialog = false }) {
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
                                // Try biometric auth if available, but proceed if no biometrics enrolled
                                val authed = viewModel.authenticateBiometricOrSkip(
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
// Helper Functions
// ==========================================================================

/** Copy sensitive text to clipboard with auto-clear after 5 seconds. */
private fun copyToClipboardSecure(
    context: Context,
    viewModel: WalletViewModel,
    label: String,
    text: String,
) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    val clipData = ClipData.newPlainText(label, text)
    if (Build.VERSION.SDK_INT >= 33) {
        clipData.description.extras = PersistableBundle().apply {
            putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
        }
    }
    clipboard.setPrimaryClip(clipData)
    viewModel.viewModelScope.launch {
        kotlinx.coroutines.delay(5_000)
        clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
    }
}

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
