package com.zipherx.wallet.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors

@Composable
fun SetupScreen(
    viewModel: WalletViewModel,
    onWalletCreated: () -> Unit,
) {
    val focusManager = LocalFocusManager.current
    var mode by remember { mutableStateOf<String?>(null) } // null, "create", "restore", "import"
    var seedWords by remember { mutableStateOf(List(24) { "" }) }
    var importInput by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val mnemonicWords by viewModel.mnemonicWords.collectAsState()
    val walletState by viewModel.walletState.collectAsState()
    val errorMessage by viewModel.errorMessage.collectAsState()

    // Detect wallet creation completed — mnemonic words are available
    val walletCreated = walletState == "created" && mnemonicWords.isNotEmpty()

    // Detect restore/import completed
    LaunchedEffect(walletState) {
        if (walletState == "ready") {
            onWalletCreated()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ZColors.terminalBlack)
            .padding(24.dp)
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(32.dp))
        Text(
            text = "[ ZIPHERX ]",
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primary,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = "> WALLET SETUP",
            fontSize = 14.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primaryDark,
        )
        Spacer(Modifier.height(24.dp))

        when (mode) {
            null -> {
                // Choice buttons
                SetupButton("CREATE NEW WALLET") { mode = "create" }
                Spacer(Modifier.height(12.dp))
                SetupButton("RESTORE FROM MNEMONIC") { mode = "restore" }
                Spacer(Modifier.height(12.dp))
                SetupButton("IMPORT PRIVATE KEY") { mode = "import" }
            }
            "create" -> {
                if (!walletCreated) {
                    val isBusy = walletState == "creating"
                    Text(
                        text = if (isBusy) "Creating wallet..." else "Ready to create",
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primary,
                    )
                    if (!isBusy) {
                        LaunchedEffect(Unit) {
                            viewModel.createNewWallet()
                        }
                    }
                } else {
                    Text(
                        text = "> YOUR 24-WORD RECOVERY PHRASE",
                        fontSize = 13.sp,
                        fontWeight = FontWeight.Bold,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.warning,
                    )
                    Spacer(Modifier.height(4.dp))
                    Text(
                        text = "WRITE THESE DOWN AND KEEP THEM SAFE!",
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.error,
                    )
                    Spacer(Modifier.height(16.dp))

                    // Word grid
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .border(1.dp, ZColors.glow, RoundedCornerShape(2.dp))
                            .padding(16.dp),
                    ) {
                        mnemonicWords.chunked(3).forEachIndexed { rowIdx, row ->
                            Row(modifier = Modifier.fillMaxWidth()) {
                                row.forEachIndexed { colIdx, word ->
                                    val num = rowIdx * 3 + colIdx + 1
                                    Text(
                                        text = "${num.toString().padStart(2)}. $word",
                                        fontSize = 12.sp,
                                        fontFamily = FontFamily.Monospace,
                                        color = ZColors.primary,
                                        modifier = Modifier.weight(1f),
                                    )
                                }
                            }
                            Spacer(Modifier.height(4.dp))
                        }
                    }

                    Spacer(Modifier.height(24.dp))
                    SetupButton("I HAVE SAVED MY PHRASE") {
                        viewModel.clearMnemonic()
                        onWalletCreated()
                    }
                }
            }
            "restore" -> {
                val focusRequesters = remember { List(24) { FocusRequester() } }
                val isBusy = walletState == "restoring"

                Text(
                    text = "> ENTER 24-WORD MNEMONIC",
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primary,
                )
                Spacer(Modifier.height(8.dp))

                val filledCount = seedWords.count { it.isNotBlank() }
                Text(
                    text = "$filledCount/24 words filled",
                    fontSize = 11.sp,
                    fontFamily = FontFamily.Monospace,
                    color = if (filledCount == 24) ZColors.primary else ZColors.primaryDim,
                )
                Spacer(Modifier.height(12.dp))

                // 24-field grid: 8 rows x 3 columns
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    for (rowIdx in 0 until 8) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            for (colIdx in 0 until 3) {
                                val idx = rowIdx * 3 + colIdx
                                val wordNum = idx + 1
                                OutlinedTextField(
                                    value = seedWords[idx],
                                    onValueChange = { newValue ->
                                        error = null
                                        val trimmed = newValue.trim()
                                        if (trimmed.contains(" ") || trimmed.contains("\t") || trimmed.contains("\n")) {
                                            val pastedWords = trimmed.split("\\s+".toRegex()).filter { it.isNotBlank() }
                                            val updated = seedWords.toMutableList()
                                            for (i in pastedWords.indices) {
                                                val targetIdx = idx + i
                                                if (targetIdx < 24) {
                                                    updated[targetIdx] = pastedWords[i].lowercase()
                                                }
                                            }
                                            seedWords = updated
                                            val nextFocus = minOf(idx + pastedWords.size, 23)
                                            try { focusRequesters[nextFocus].requestFocus() } catch (_: Exception) {}
                                        } else {
                                            val updated = seedWords.toMutableList()
                                            updated[idx] = newValue.lowercase().replace(" ", "")
                                            seedWords = updated
                                            if (newValue.endsWith(" ") && trimmed.isNotBlank() && idx < 23) {
                                                updated[idx] = trimmed.lowercase()
                                                seedWords = updated
                                                try { focusRequesters[idx + 1].requestFocus() } catch (_: Exception) {}
                                            }
                                        }
                                    },
                                    modifier = Modifier
                                        .weight(1f)
                                        .height(52.dp)
                                        .focusRequester(focusRequesters[idx]),
                                    textStyle = TextStyle(
                                        fontFamily = FontFamily.Monospace,
                                        fontSize = 11.sp,
                                        color = ZColors.primary,
                                    ),
                                    label = {
                                        Text(
                                            "#$wordNum",
                                            fontFamily = FontFamily.Monospace,
                                            fontSize = 9.sp,
                                            color = ZColors.primaryDim,
                                        )
                                    },
                                    singleLine = true,
                                    enabled = !isBusy,
                                    keyboardOptions = KeyboardOptions(
                                        keyboardType = KeyboardType.Text,
                                        imeAction = if (idx < 23) ImeAction.Next else ImeAction.Done,
                                        autoCorrectEnabled = false,
                                    ),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        focusedBorderColor = ZColors.primary,
                                        unfocusedBorderColor = ZColors.glow,
                                        cursorColor = ZColors.primary,
                                        focusedTextColor = ZColors.primary,
                                        unfocusedTextColor = ZColors.primaryDim,
                                    ),
                                    shape = RoundedCornerShape(2.dp),
                                )
                            }
                        }
                    }
                }

                val displayError = error ?: errorMessage
                if (displayError != null) {
                    Spacer(Modifier.height(4.dp))
                    Text(displayError, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.error)
                }
                Spacer(Modifier.height(12.dp))
                SetupButton(if (isBusy) "RESTORING..." else "RESTORE") {
                    focusManager.clearFocus() // Dismiss keyboard on first tap
                    if (!isBusy) {
                        val words = seedWords.map { it.trim().lowercase() }
                        val emptyCount = words.count { it.isBlank() }
                        if (emptyCount > 0) {
                            error = "Missing $emptyCount word${if (emptyCount > 1) "s" else ""} — fill all 24 fields"
                        } else {
                            viewModel.restoreFromMnemonic(words)
                        }
                    }
                }
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = { mode = null; error = null; viewModel.clearError() }) {
                    Text("< BACK", fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                }
            }
            "import" -> {
                val isBusy = walletState == "importing"

                Text(
                    text = "> IMPORT PRIVATE KEY",
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primary,
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "Hex (338 chars) or Bech32 (secret-extended-key-main1...)",
                    fontSize = 10.sp,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primaryDim,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = importInput,
                    onValueChange = { importInput = it; error = null; viewModel.clearError() },
                    label = { Text("Private key", fontFamily = FontFamily.Monospace, fontSize = 10.sp) },
                    modifier = Modifier.fillMaxWidth().height(80.dp),
                    enabled = !isBusy,
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = ZColors.primary,
                        unfocusedBorderColor = ZColors.glow,
                        cursorColor = ZColors.primary,
                        focusedTextColor = ZColors.primary,
                        unfocusedTextColor = ZColors.primaryDim,
                    ),
                    shape = RoundedCornerShape(2.dp),
                )
                val displayError = error ?: errorMessage
                if (displayError != null) {
                    Spacer(Modifier.height(4.dp))
                    Text(displayError, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.error)
                }
                Spacer(Modifier.height(12.dp))
                SetupButton(if (isBusy) "IMPORTING..." else "IMPORT") {
                    focusManager.clearFocus() // Dismiss keyboard on first tap
                    if (!isBusy) {
                        val input = importInput.trim()
                        if (input.isEmpty()) {
                            error = "Enter a private key"
                        } else {
                            viewModel.importSpendingKey(input)
                        }
                    }
                }
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = { mode = null; error = null; viewModel.clearError() }) {
                    Text("< BACK", fontFamily = FontFamily.Monospace, color = ZColors.primaryDim)
                }
            }
        }
    }
}

@Composable
private fun SetupButton(text: String, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(0.8f),
        shape = RoundedCornerShape(2.dp),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = ZColors.primary),
        border = BorderStroke(1.dp, ZColors.primary),
    ) {
        Text(
            text,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            fontSize = 13.sp,
            modifier = Modifier.padding(vertical = 4.dp),
        )
    }
}
