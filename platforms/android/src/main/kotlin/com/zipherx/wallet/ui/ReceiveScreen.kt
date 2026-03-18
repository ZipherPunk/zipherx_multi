package com.zipherx.wallet.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.zipherx.wallet.WalletViewModel
import com.zipherx.wallet.ZColors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Generate a QR code bitmap from a string.
 */
private fun generateQrBitmap(content: String, size: Int): Bitmap {
    val hints = mapOf(
        EncodeHintType.MARGIN to 1,
        EncodeHintType.CHARACTER_SET to "UTF-8",
    )
    val bitMatrix = QRCodeWriter().encode(content, BarcodeFormat.QR_CODE, size, size, hints)
    val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
    for (x in 0 until size) {
        for (y in 0 until size) {
            bitmap.setPixel(x, y, if (bitMatrix[x, y]) 0xFF000000.toInt() else 0xFFFFFFFF.toInt())
        }
    }
    return bitmap
}

/**
 * Receive screen displaying both shielded and transparent addresses
 * with QR codes and copy-to-clipboard buttons.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReceiveScreen(
    viewModel: WalletViewModel = viewModel(),
    onNavigateBack: () -> Unit = {},
) {
    val walletAddress by viewModel.walletAddress.collectAsState()
    val transparentAddress by viewModel.transparentAddress.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    // 0 = shielded, 1 = transparent
    var selectedTab by remember { mutableIntStateOf(0) }

    // If transparent address is null but imported keys exist, try loading from UTXOs
    LaunchedEffect(selectedTab, transparentAddress) {
        if (selectedTab == 1 && transparentAddress == null) {
            viewModel.loadImportedTransparentAddress()
        }
    }

    val displayAddress = if (selectedTab == 0) walletAddress else transparentAddress
    val addressLabel = if (selectedTab == 0) "SHIELDED ADDRESS" else "TRANSPARENT ADDRESS"

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "> RECEIVE ZCL",
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
        snackbarHost = { SnackbarHost(snackbarHostState) },
        containerColor = ZColors.terminalBlack,
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 16.dp)
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.height(16.dp))

            // Tab selector: Shielded / Transparent
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(1.dp, ZColors.primaryDim, RoundedCornerShape(0.dp)),
            ) {
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .background(if (selectedTab == 0) ZColors.primary else ZColors.surface)
                        .clickable { selectedTab = 0 }
                        .padding(12.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "SHIELDED (z)",
                        style = MaterialTheme.typography.labelMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = if (selectedTab == 0) ZColors.terminalBlack else ZColors.primaryDim,
                    )
                }
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .background(if (selectedTab == 1) ZColors.primary else ZColors.surface)
                        .clickable { selectedTab = 1 }
                        .padding(12.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "TRANSPARENT (t)",
                        style = MaterialTheme.typography.labelMedium.copy(
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.Bold,
                        ),
                        color = if (selectedTab == 1) ZColors.terminalBlack else ZColors.primaryDim,
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = addressLabel,
                style = MaterialTheme.typography.labelMedium.copy(
                    fontFamily = FontFamily.Monospace,
                    letterSpacing = 1.sp,
                ),
                color = ZColors.primaryDark,
            )

            Spacer(modifier = Modifier.height(16.dp))

            // QR Code
            displayAddress?.let { addr ->
                val qrBitmap = remember(addr) { generateQrBitmap(addr, 512) }
                Box(
                    modifier = Modifier
                        .size(240.dp)
                        .background(Color.White)
                        .padding(8.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Image(
                        bitmap = qrBitmap.asImageBitmap(),
                        contentDescription = "QR Code",
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Address text
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(ZColors.surface)
                    .border(1.dp, ZColors.primaryDim.copy(alpha = 0.3f), RoundedCornerShape(0.dp))
                    .padding(12.dp),
            ) {
                Text(
                    text = displayAddress ?: "No address available",
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 10.sp,
                    ),
                    textAlign = TextAlign.Center,
                    color = ZColors.primary,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = {
                    displayAddress?.let { addr ->
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("ZCL Address", addr))
                        scope.launch {
                            snackbarHostState.showSnackbar("Address copied (auto-clears in 30s)")
                        }
                        // Auto-clear clipboard after 30 seconds
                        scope.launch {
                            delay(30_000)
                            clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                        }
                    }
                },
                enabled = displayAddress != null,
                shape = RoundedCornerShape(0.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = ZColors.primary,
                    contentColor = ZColors.terminalBlack,
                    disabledContainerColor = ZColors.primaryDim.copy(alpha = 0.3f),
                    disabledContentColor = ZColors.primaryDim,
                ),
            ) {
                Icon(
                    Icons.Default.ContentCopy,
                    contentDescription = "Copy",
                    modifier = Modifier.padding(end = 8.dp),
                )
                Text(
                    text = "COPY ADDRESS",
                    style = MaterialTheme.typography.labelMedium.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                    ),
                )
            }

            if (selectedTab == 1) {
                Spacer(modifier = Modifier.height(12.dp))
                Text(
                    text = "Transparent addresses offer less privacy than shielded addresses. Use shielded when possible.",
                    style = MaterialTheme.typography.bodySmall,
                    color = ZColors.warning.copy(alpha = 0.7f),
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }

            Spacer(modifier = Modifier.height(24.dp))
        }
    }
}
