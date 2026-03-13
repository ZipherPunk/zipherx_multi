package com.zipherx.wallet

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.sp
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.viewmodel.compose.viewModel
import com.zipherx.wallet.ui.DisclaimerScreen
import com.zipherx.wallet.ui.ReceiveScreen
import com.zipherx.wallet.ui.SendScreen
import com.zipherx.wallet.ui.SetupScreen
import com.zipherx.wallet.ui.SettingsScreen
import com.zipherx.wallet.ui.StatusBar
import com.zipherx.wallet.ui.TransactionHistoryScreen
import com.zipherx.wallet.ui.WalletScreen

// TODO: KA-N8 — Handle `zclassic:` URI scheme for payment deep links.
//  Register an intent-filter in AndroidManifest.xml for the `zclassic:` scheme,
//  parse the URI in onCreate/onNewIntent, and pre-fill the SendScreen with
//  the destination address, amount, and memo from the URI.
class MainActivity : FragmentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // enableEdgeToEdge() — disabled: API 36 enforces edge-to-edge natively,
        // and the older activity library (1.8.2) conflicts with it.
        setContent {
            ZipherXTheme {
                val walletViewModel: WalletViewModel = viewModel()
                walletViewModel.setActivity(this@MainActivity)
                walletViewModel.initPrefs(this@MainActivity)

                // SECURITY NOTE: FLAG_SECURE is intentionally disabled during wallet setup
                // to allow users to photograph their mnemonic phrase. This creates a brief
                // window where screenshots are possible. Once setup completes, FLAG_SECURE
                // is re-enabled to block all screen capture.
                val isWalletActive by walletViewModel.isWalletActive.collectAsState()
                val screenshotProtection by walletViewModel.screenshotProtection.collectAsState()
                LaunchedEffect(isWalletActive, screenshotProtection) {
                    if (isWalletActive && screenshotProtection) {
                        window.setFlags(
                            WindowManager.LayoutParams.FLAG_SECURE,
                            WindowManager.LayoutParams.FLAG_SECURE,
                        )
                    } else {
                        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    }
                }

                Surface(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(Color(0xFF0A0A0A))
                ) {
                    ZipherXNavHost(viewModel = walletViewModel)
                }
            }
        }
    }
}

@Composable
fun ZipherXNavHost(viewModel: WalletViewModel) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val prefs = androidx.compose.runtime.remember {
        context.getSharedPreferences("zipherx_prefs", android.content.Context.MODE_PRIVATE)
    }
    val walletState by viewModel.walletState.collectAsState()

    // Determine starting screen based on disclaimer + wallet state
    val isLoading = walletState == "loading"
    val walletReady = walletState == "ready" || walletState == "syncing" ||
        walletState == "synced" || walletState == "created"
    val disclaimerAccepted = prefs.getBoolean("disclaimer_accepted", false)

    val startScreen = when {
        isLoading -> "loading"  // blank screen while checking wallet existence
        !disclaimerAccepted -> "disclaimer"
        !walletReady -> "setup"
        else -> "wallet"
    }

    var currentScreen by rememberSaveable { mutableStateOf(startScreen) }

    // Auto-advance when loading completes or wallet becomes ready
    LaunchedEffect(walletState, disclaimerAccepted) {
        when {
            currentScreen == "loading" && !isLoading -> {
                // Loading finished — decide where to go
                currentScreen = when {
                    !disclaimerAccepted -> "disclaimer"
                    walletReady -> "wallet"
                    else -> "setup"
                }
            }
            walletReady && currentScreen == "setup" -> {
                currentScreen = "wallet"
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        // Main content takes remaining space
        Box(modifier = Modifier.weight(1f)) {
            when (currentScreen) {
                "loading" -> {
                    // Blank screen while wallet state is loading — prevents setup flash
                    Box(modifier = Modifier.fillMaxSize().background(Color(0xFF0A0A0A)))
                }
                "disclaimer" -> DisclaimerScreen(
                    onAccept = {
                        prefs.edit().putBoolean("disclaimer_accepted", true).apply()
                        currentScreen = "setup"
                    },
                )
                "setup" -> SetupScreen(
                    viewModel = viewModel,
                    onWalletCreated = { currentScreen = "wallet" },
                )
                "wallet" -> WalletScreen(
                    viewModel = viewModel,
                    onNavigateToSend = { currentScreen = "send" },
                    onNavigateToReceive = { currentScreen = "receive" },
                    onNavigateToHistory = { currentScreen = "history" },
                    onNavigateToSettings = { currentScreen = "settings" },
                )
                "send" -> SendScreen(
                    viewModel = viewModel,
                    onNavigateBack = { currentScreen = "wallet" },
                )
                "receive" -> ReceiveScreen(
                    viewModel = viewModel,
                    onNavigateBack = { currentScreen = "wallet" },
                )
                "history" -> TransactionHistoryScreen(
                    viewModel = viewModel,
                    onNavigateBack = { currentScreen = "wallet" },
                )
                "settings" -> SettingsScreen(
                    viewModel = viewModel,
                    onNavigateBack = { currentScreen = "wallet" },
                )
            }
        }

        // Global status bar at bottom of every screen
        StatusBar(viewModel = viewModel)
    }
}

// Cypherpunk terminal color palette (matches iOS ZipherX)
object ZColors {
    val primary      = Color(0xFF00FF40)  // Neon green
    val primaryDark  = Color(0xFF00D940)
    val primaryDim   = Color(0xFF00B32E)
    val terminalBlack = Color(0xFF050505)
    val surface       = Color(0xFF1A1208)
    val progressBg    = Color(0xFF261905)
    val success       = Color(0xFF00FF40)
    val error         = Color(0xFFFF3131)
    val warning       = Color(0xFFFFC107)
    val glow          = Color(0x4D00FF40)  // 30% opacity
    val border        = Color(0x4D00FF40)
    val textDim       = Color(0xFF6B8F6B)
}

@Composable
fun ZipherXTheme(content: @Composable () -> Unit) {
    androidx.compose.material3.MaterialTheme(
        colorScheme = androidx.compose.material3.darkColorScheme(
            primary = ZColors.primary,
            onPrimary = ZColors.terminalBlack,
            surface = ZColors.surface,
            onSurface = ZColors.primary,
            surfaceVariant = ZColors.surface,
            onSurfaceVariant = ZColors.primaryDim,
            background = ZColors.terminalBlack,
            onBackground = ZColors.primary,
            error = ZColors.error,
            primaryContainer = ZColors.surface,
            onPrimaryContainer = ZColors.primary,
        ),
        typography = androidx.compose.material3.Typography(
            headlineLarge = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                fontSize = 24.sp,
            ),
            headlineMedium = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                fontSize = 20.sp,
            ),
            titleMedium = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
                fontSize = 14.sp,
            ),
            titleSmall = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
                fontSize = 12.sp,
            ),
            bodyLarge = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontSize = 13.sp,
            ),
            bodyMedium = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontSize = 12.sp,
            ),
            bodySmall = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontSize = 10.sp,
            ),
            labelLarge = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                fontSize = 12.sp,
            ),
            labelMedium = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontSize = 10.sp,
            ),
            labelSmall = androidx.compose.ui.text.TextStyle(
                fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                fontSize = 9.sp,
            ),
        ),
        content = content
    )
}
