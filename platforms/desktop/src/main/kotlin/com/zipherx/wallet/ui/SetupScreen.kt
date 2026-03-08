package com.zipherx.wallet.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors

@Composable
fun SetupScreen(
    viewModel: WalletViewModel,
    onWalletCreated: () -> Unit,
) {
    var mode by remember { mutableStateOf<String?>(null) } // null, "create", "restore", "import"
    var mnemonicWords by remember { mutableStateOf<List<String>?>(null) }
    var restoreInput by remember { mutableStateOf("") }
    var importInput by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val vmError by viewModel.error.collectAsState()

    Column(
        modifier = Modifier
            .fillMaxSize()
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
                if (mnemonicWords == null) {
                    Text(
                        text = "Creating wallet...",
                        fontFamily = FontFamily.Monospace,
                        color = ZColors.primary,
                    )
                    LaunchedEffect(Unit) {
                        mnemonicWords = viewModel.createWallet()
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
                            .border(1.dp, ZColors.border, RoundedCornerShape(2.dp))
                            .padding(16.dp),
                    ) {
                        mnemonicWords!!.chunked(3).forEachIndexed { rowIdx, row ->
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
                    SetupButton("I HAVE SAVED MY PHRASE") { onWalletCreated() }
                }
            }
            "restore" -> {
                Text(
                    text = "> ENTER 24-WORD MNEMONIC",
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Monospace,
                    color = ZColors.primary,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = restoreInput,
                    onValueChange = { restoreInput = it; error = null },
                    label = { Text("Mnemonic words (space-separated)", fontFamily = FontFamily.Monospace, fontSize = 10.sp) },
                    modifier = Modifier.fillMaxWidth().height(120.dp),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = ZColors.primary,
                        unfocusedBorderColor = ZColors.border,
                        cursorColor = ZColors.primary,
                        focusedTextColor = ZColors.primary,
                        unfocusedTextColor = ZColors.primaryDim,
                    ),
                    shape = RoundedCornerShape(2.dp),
                )
                val displayError = error ?: vmError
                if (displayError != null) {
                    Spacer(Modifier.height(4.dp))
                    Text(displayError, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.error)
                }
                Spacer(Modifier.height(12.dp))
                SetupButton("RESTORE") {
                    val words = restoreInput.trim().split("\\s+".toRegex())
                    if (words.size != 24) {
                        error = "Expected 24 words, got ${words.size}"
                    } else if (viewModel.restoreWallet(words)) {
                        onWalletCreated()
                    } else {
                        error = viewModel.error.value ?: "Invalid mnemonic phrase"
                    }
                }
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = { mode = null }) {
                    Text("< BACK", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
                }
            }
            "import" -> {
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
                    color = ZColors.textDim,
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = importInput,
                    onValueChange = { importInput = it; error = null; viewModel.clearError() },
                    label = { Text("Private key", fontFamily = FontFamily.Monospace, fontSize = 10.sp) },
                    modifier = Modifier.fillMaxWidth().height(80.dp),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = ZColors.primary,
                        unfocusedBorderColor = ZColors.border,
                        cursorColor = ZColors.primary,
                        focusedTextColor = ZColors.primary,
                        unfocusedTextColor = ZColors.primaryDim,
                    ),
                    shape = RoundedCornerShape(2.dp),
                )
                val displayError = error ?: vmError
                if (displayError != null) {
                    Spacer(Modifier.height(4.dp))
                    Text(displayError, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = ZColors.error)
                }
                Spacer(Modifier.height(12.dp))
                SetupButton("IMPORT") {
                    val input = importInput.trim()
                    if (input.isEmpty()) {
                        error = "Enter a private key"
                    } else if (viewModel.importFromKey(input)) {
                        onWalletCreated()
                    } else {
                        error = viewModel.error.value ?: "Invalid private key"
                    }
                }
                Spacer(Modifier.height(8.dp))
                TextButton(onClick = { mode = null }) {
                    Text("< BACK", fontFamily = FontFamily.Monospace, color = ZColors.textDim)
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
        border = androidx.compose.foundation.BorderStroke(1.dp, ZColors.primary),
    ) {
        Text(text, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Bold, fontSize = 13.sp)
    }
}
