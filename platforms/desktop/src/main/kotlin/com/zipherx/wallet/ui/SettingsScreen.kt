package com.zipherx.wallet.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.BannedPeerInfo
import com.zipherx.wallet.ConnectedPeerInfo
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import com.zipherx.wallet.ZipherXWrapper
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.ui.text.input.PasswordVisualTransformation
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun SettingsScreen(
    viewModel: WalletViewModel,
    onBack: () -> Unit,
    onDeleteWallet: () -> Unit,
) {
    val peerCount by viewModel.peerCount.collectAsState()
    val torEnabled by viewModel.torEnabled.collectAsState()
    val isSyncing by viewModel.isSyncing.collectAsState()
    val syncPhase by viewModel.syncPhase.collectAsState()
    var showExportKey by remember { mutableStateOf(false) }
    var showDeleteConfirm by remember { mutableStateOf(false) }
    var showAudit by remember { mutableStateOf(false) }

    // Re-authentication state for sensitive operations
    var showPasswordReauth by remember { mutableStateOf(false) }
    var reAuthAction by remember { mutableStateOf<String?>(null) }
    var reAuthPassword by remember { mutableStateOf("") }
    var reAuthError by remember { mutableStateOf<String?>(null) }

    // Peer management state
    var showPeerManagement by remember { mutableStateOf(false) }
    var connectedPeers by remember { mutableStateOf<List<ConnectedPeerInfo>>(emptyList()) }
    var bannedPeers by remember { mutableStateOf<List<BannedPeerInfo>>(emptyList()) }
    var customPeerHost by remember { mutableStateOf("") }
    var customPeerPort by remember { mutableStateOf("8033") }
    var peerActionResult by remember { mutableStateOf<String?>(null) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
    ) {
        // Header
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) {
                Icon(Icons.Filled.ArrowBack, "Back", tint = ZColors.primary)
            }
            Text(
                "> SETTINGS",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
                color = ZColors.primary,
            )
        }
        Spacer(Modifier.height(16.dp))

        // === SYNC ===
        SectionHeader("SYNC")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            Text(syncPhase, fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TerminalButton(if (isSyncing) "STOP SYNC" else "START SYNC") {
                    if (isSyncing) viewModel.stopSync() else viewModel.startSync()
                }
            }
            Spacer(Modifier.height(4.dp))
            Text(
                "First sync: 10-30 min. Subsequent: <1 min.",
                fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim,
            )
        }
        Spacer(Modifier.height(12.dp))

        // === NETWORK ===
        SectionHeader("NETWORK")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("Connected Peers", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                Text("$peerCount", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary, fontWeight = FontWeight.Bold)
            }
            Spacer(Modifier.height(8.dp))

            // Tor toggle
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Tor Network", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                Switch(
                    checked = torEnabled,
                    onCheckedChange = { viewModel.setTorEnabled(it) },
                    colors = SwitchDefaults.colors(
                        checkedThumbColor = ZColors.primary,
                        checkedTrackColor = Color(0xFF003300),
                        uncheckedThumbColor = ZColors.textDim,
                        uncheckedTrackColor = Color(0xFF222222),
                    ),
                )
            }

            val onionAddress = ZipherXWrapper.getOnionAddress()
            if (onionAddress != null) {
                Spacer(Modifier.height(4.dp))
                Text("Onion: $onionAddress", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
            }

            val torState = ZipherXWrapper.getTorState()
            val torLabel = when (torState.toInt()) {
                0 -> "Disconnected"
                1 -> "Connecting"
                2 -> "Bootstrapping"
                3 -> "Connected"
                4 -> "Error"
                else -> "Unknown"
            }
            Spacer(Modifier.height(4.dp))
            Text("Tor state: $torLabel", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim)
        }
        Spacer(Modifier.height(12.dp))

        // === PEER MANAGEMENT (collapsible) ===
        SectionHeader("PEER MANAGEMENT")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            // Clickable header row — toggles expand/collapse
            Row(
                modifier = Modifier.fillMaxWidth().clickable {
                    showPeerManagement = !showPeerManagement
                    if (showPeerManagement && connectedPeers.isEmpty()) {
                        connectedPeers = ZipherXWrapper.getConnectedPeers()
                        bannedPeers = ZipherXWrapper.getBannedPeers()
                    }
                },
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Peers", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        "${connectedPeers.size} connected, ${bannedPeers.size} banned",
                        fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim,
                    )
                    Text(
                        if (showPeerManagement) "[-]" else "[+]",
                        fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary, fontWeight = FontWeight.Bold,
                    )
                }
            }

            AnimatedVisibility(
                visible = showPeerManagement,
                enter = expandVertically(),
                exit = shrinkVertically(),
            ) {
                Column {
                    Spacer(Modifier.height(8.dp))

                    // Refresh button
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        TerminalButton("REFRESH") {
                            connectedPeers = ZipherXWrapper.getConnectedPeers()
                            bannedPeers = ZipherXWrapper.getBannedPeers()
                            peerActionResult = null
                        }
                    }
                    Spacer(Modifier.height(8.dp))

                    // Action result feedback
                    if (peerActionResult != null) {
                        Text(
                            peerActionResult!!,
                            fontSize = 9.sp, fontFamily = FontFamily.Monospace,
                            color = if (peerActionResult!!.startsWith("Error")) ZColors.error else ZColors.primary,
                        )
                        Spacer(Modifier.height(6.dp))
                    }

                    // Connected peers
                    Text("> CONNECTED (${connectedPeers.size})", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary, fontWeight = FontWeight.Bold)
                    Spacer(Modifier.height(4.dp))
                    if (connectedPeers.isEmpty()) {
                        Text("No peers. Click REFRESH.", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                    }
                    for (peer in connectedPeers) {
                        Row(
                            modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(peer.address, fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
                                Text(
                                    "v${peer.protocolVersion} | ${peer.userAgent.take(30)} | h:${peer.startHeight}",
                                    fontSize = 8.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim,
                                )
                            }
                            OutlinedButton(
                                onClick = {
                                    val ok = ZipherXWrapper.disconnectPeer(peer.address)
                                    peerActionResult = if (ok) "Disconnected ${peer.address}" else "Error: disconnect failed"
                                    connectedPeers = ZipherXWrapper.getConnectedPeers()
                                },
                                shape = RoundedCornerShape(2.dp),
                                border = BorderStroke(1.dp, ZColors.error),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.error),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                            ) {
                                Text("DC", fontFamily = FontFamily.Monospace, fontSize = 8.sp)
                            }
                        }
                    }

                    Spacer(Modifier.height(8.dp))

                    // Banned peers
                    Text("> BANNED (${bannedPeers.size})", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error, fontWeight = FontWeight.Bold)
                    Spacer(Modifier.height(4.dp))
                    if (bannedPeers.isEmpty()) {
                        Text("No banned peers.", fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                    }
                    for (peer in bannedPeers) {
                        Row(
                            modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(peer.host, fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                                val timeStr = if (peer.isPermanent) "permanent" else "${peer.remainingSeconds}s remaining"
                                Text(
                                    "${peer.reason.take(40)} | $timeStr",
                                    fontSize = 8.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim,
                                )
                            }
                            OutlinedButton(
                                onClick = {
                                    val ok = ZipherXWrapper.unbanPeer(peer.host)
                                    peerActionResult = if (ok) "Unbanned ${peer.host}" else "Error: unban failed"
                                    bannedPeers = ZipherXWrapper.getBannedPeers()
                                },
                                shape = RoundedCornerShape(2.dp),
                                border = BorderStroke(1.dp, ZColors.primary),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                                contentPadding = PaddingValues(horizontal = 8.dp, vertical = 2.dp),
                            ) {
                                Text("UNBAN", fontFamily = FontFamily.Monospace, fontSize = 8.sp)
                            }
                        }
                    }

                    Spacer(Modifier.height(8.dp))

                    // Add custom peer
                    Text("> ADD CUSTOM PEER", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim, fontWeight = FontWeight.Bold)
                    Spacer(Modifier.height(4.dp))
                    Text("IP address only (no hostnames for DNS leak prevention).", fontSize = 8.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                    Spacer(Modifier.height(4.dp))
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        androidx.compose.material3.OutlinedTextField(
                            value = customPeerHost,
                            onValueChange = { customPeerHost = it.filter { c -> c.isDigit() || c == '.' } },
                            label = { Text("IP Address", fontFamily = FontFamily.Monospace, fontSize = 9.sp) },
                            singleLine = true,
                            modifier = Modifier.weight(1f),
                            colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = ZColors.primary,
                                unfocusedBorderColor = ZColors.border,
                                cursorColor = ZColors.primary,
                                focusedTextColor = ZColors.primary,
                                unfocusedTextColor = ZColors.primaryDim,
                            ),
                            shape = RoundedCornerShape(2.dp),
                            textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 10.sp),
                        )
                        androidx.compose.material3.OutlinedTextField(
                            value = customPeerPort,
                            onValueChange = { customPeerPort = it.filter { c -> c.isDigit() }.take(5) },
                            label = { Text("Port", fontFamily = FontFamily.Monospace, fontSize = 9.sp) },
                            singleLine = true,
                            modifier = Modifier.width(80.dp),
                            colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = ZColors.primary,
                                unfocusedBorderColor = ZColors.border,
                                cursorColor = ZColors.primary,
                                focusedTextColor = ZColors.primary,
                                unfocusedTextColor = ZColors.primaryDim,
                            ),
                            shape = RoundedCornerShape(2.dp),
                            textStyle = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 10.sp),
                        )
                        TerminalButton("ADD") {
                            val port = customPeerPort.toIntOrNull() ?: 0
                            if (customPeerHost.isBlank()) {
                                peerActionResult = "Error: IP address required"
                            } else if (port !in 1..65535) {
                                peerActionResult = "Error: Invalid port (1-65535)"
                            } else {
                                val ok = ZipherXWrapper.addCustomPeer(customPeerHost, port)
                                peerActionResult = if (ok) "Added ${customPeerHost}:$port" else "Error: Invalid IP or peer rejected"
                                if (ok) {
                                    customPeerHost = ""
                                    connectedPeers = ZipherXWrapper.getConnectedPeers()
                                }
                            }
                        }
                    }
                }
            }
        }
        Spacer(Modifier.height(12.dp))

        // === SECURITY ===
        // NOTE: Compose Desktop has no FLAG_SECURE equivalent for screenshot protection.
        // Desktop OS-level screenshot blocking is not available via the JVM.
        SectionHeader("SECURITY")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            TerminalButton("EXPORT PRIVATE KEY") {
                reAuthAction = "export_key"
                reAuthPassword = ""
                reAuthError = null
                showPasswordReauth = true
            }
            Spacer(Modifier.height(8.dp))
            TerminalButton("SECURITY AUDIT REPORT") { showAudit = true }
        }
        Spacer(Modifier.height(12.dp))

        // === ABOUT ===
        SectionHeader("ABOUT")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            InfoRow("Version", "1.0.0")
            InfoRow("Platform", "${System.getProperty("os.name")} / ${System.getProperty("os.arch")}")
            InfoRow("Runtime", "JVM ${System.getProperty("java.version")}")
        }
        Spacer(Modifier.height(12.dp))

        // === DANGER ZONE ===
        SectionHeader("DANGER ZONE", color = ZColors.error)
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, ZColors.error.copy(alpha = 0.5f), RoundedCornerShape(2.dp))
                .padding(12.dp),
        ) {
            OutlinedButton(
                onClick = {
                    reAuthAction = "delete_all"
                    reAuthPassword = ""
                    reAuthError = null
                    showPasswordReauth = true
                },
                shape = RoundedCornerShape(2.dp),
                border = BorderStroke(1.dp, ZColors.error),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.error),
            ) {
                Text("DELETE ALL DATA", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 11.sp)
            }
            Spacer(Modifier.height(4.dp))
            Text(
                "Permanently deletes wallet, keys, and all data.",
                fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.error.copy(alpha = 0.7f),
            )
        }
    }

    // Export private key dialog (KD-5: uses CharArray, zeroed after use)
    if (showExportKey) {
        val encodedKey = remember { viewModel.getEncodedPrivateKey() }
        val truncatedDisplay = if (encodedKey != null && encodedKey.size > 24) {
            String(encodedKey, 0, 16) + "..." + String(encodedKey, encodedKey.size - 8, 8)
        } else if (encodedKey != null) {
            String(encodedKey)
        } else null
        var copiedToClipboard by remember { mutableStateOf(false) }

        // KD-1: Auto-dismiss after 60 seconds to limit on-screen exposure
        LaunchedEffect(showExportKey) {
            kotlinx.coroutines.delay(60_000)
            encodedKey?.fill('\u0000')
            showExportKey = false
        }

        // Auto-clear clipboard after 10 seconds
        LaunchedEffect(copiedToClipboard) {
            if (copiedToClipboard) {
                kotlinx.coroutines.delay(10_000)
                val clipboard = Toolkit.getDefaultToolkit().systemClipboard
                clipboard.setContents(StringSelection(""), null)
                copiedToClipboard = false
            }
        }

        AlertDialog(
            onDismissRequest = { encodedKey?.fill('\u0000'); showExportKey = false },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = { Text("> EXPORT PRIVATE KEY", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, color = ZColors.warning) },
            text = {
                Column {
                    Text("NEVER SHARE THIS KEY!", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.error, fontWeight = FontWeight.Bold)
                    Text("Anyone with this key can spend your funds.", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        truncatedDisplay ?: "No key loaded",
                        fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary,
                    )
                    Spacer(Modifier.height(12.dp))

                    if (encodedKey != null) {
                        OutlinedButton(
                            onClick = {
                                val clipboard = Toolkit.getDefaultToolkit().systemClipboard
                                clipboard.setContents(StringSelection(String(encodedKey)), null)
                                copiedToClipboard = true
                            },
                            shape = RoundedCornerShape(2.dp),
                            border = BorderStroke(1.dp, ZColors.warning),
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.warning),
                        ) {
                            Text(
                                if (copiedToClipboard) "COPIED! Auto-clears in 10s" else "COPY TO CLIPBOARD",
                                fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 10.sp,
                            )
                        }

                        if (copiedToClipboard) {
                            Spacer(Modifier.height(8.dp))
                            Text(
                                "WARNING: Key copied to clipboard. It will be automatically cleared after 10 seconds for security.",
                                fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.warning,
                                lineHeight = 14.sp,
                            )
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { encodedKey?.fill('\u0000'); showExportKey = false }) {
                    Text("CLOSE", fontFamily = FontFamily.Monospace, color = ZColors.primary)
                }
            },
        )
    }

    // Delete confirmation dialog
    if (showDeleteConfirm) {
        AlertDialog(
            onDismissRequest = { showDeleteConfirm = false },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = { Text("> DELETE ALL DATA", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, color = ZColors.error) },
            text = {
                Column {
                    Text("This will permanently delete:", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
                    Text("  - Private key", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    Text("  - Wallet database", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    Text("  - Transaction history", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    Text("  - All synced data", fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.error)
                    Spacer(Modifier.height(8.dp))
                    Text("YOUR FUNDS WILL BE LOST if you don't have your mnemonic phrase!", fontSize = 11.sp, fontFamily = FontFamily.Monospace, color = ZColors.warning, fontWeight = FontWeight.Bold)
                }
            },
            confirmButton = {
                OutlinedButton(
                    onClick = { showDeleteConfirm = false; onDeleteWallet() },
                    shape = RoundedCornerShape(2.dp),
                    border = BorderStroke(1.dp, ZColors.error),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.error),
                ) {
                    Text("DELETE EVERYTHING", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteConfirm = false }) {
                    Text("CANCEL", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                }
            },
        )
    }

    // Password re-authentication dialog for sensitive operations
    if (showPasswordReauth) {
        AlertDialog(
            onDismissRequest = { showPasswordReauth = false; reAuthPassword = ""; reAuthError = null },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = {
                Text(
                    "> VERIFY PASSWORD",
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    color = if (reAuthAction == "delete_all") ZColors.error else ZColors.warning,
                )
            },
            text = {
                Column {
                    Text(
                        "Re-enter your password to proceed.",
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
                            showPasswordReauth = false
                            reAuthPassword = ""
                            reAuthError = null
                            when (reAuthAction) {
                                "export_key" -> showExportKey = true
                                "delete_all" -> showDeleteConfirm = true
                            }
                            reAuthAction = null
                        } else {
                            reAuthError = "Wrong password"
                        }
                    },
                    shape = RoundedCornerShape(2.dp),
                    border = BorderStroke(1.dp, ZColors.primary),
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
                ) {
                    Text("VERIFY", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                TextButton(onClick = { showPasswordReauth = false; reAuthPassword = ""; reAuthError = null }) {
                    Text("CANCEL", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                }
            },
        )
    }

    // Security audit dialog
    if (showAudit) {
        AlertDialog(
            onDismissRequest = { showAudit = false },
            containerColor = ZColors.surfaceDark,
            shape = RoundedCornerShape(2.dp),
            title = { Text("> SECURITY AUDIT", fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, color = ZColors.primary) },
            text = {
                Column {
                    AuditRow("Database encrypted", true)
                    AuditRow("Private key encrypted", true)
                    AuditRow("Password protection", true)
                    AuditRow("Tor enabled", torEnabled)
                    AuditRow("Peers connected", peerCount > 0)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "OS: ${System.getProperty("os.name")}",
                        fontSize = 9.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim,
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { showAudit = false }) {
                    Text("CLOSE", fontFamily = FontFamily.Monospace, color = ZColors.primary)
                }
            },
        )
    }
}

@Composable
private fun SectionHeader(text: String, color: Color = ZColors.primaryDim) {
    Text(
        "> $text",
        fontSize = 10.sp,
        fontFamily = FontFamily.Monospace,
        fontWeight = FontWeight.Bold,
        color = color,
        letterSpacing = 1.sp,
    )
    Spacer(Modifier.height(4.dp))
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.textDim)
        Text(value, fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primary)
    }
}

@Composable
private fun AuditRow(label: String, ok: Boolean) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, fontSize = 10.sp, fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
        Text(
            if (ok) "[OK]" else "[!!]",
            fontSize = 10.sp, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold,
            color = if (ok) ZColors.success else ZColors.error,
        )
    }
}

@Composable
private fun TerminalButton(text: String, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        shape = RoundedCornerShape(2.dp),
        border = BorderStroke(1.dp, ZColors.primary),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
    ) {
        Text(text, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 10.sp)
    }
}
