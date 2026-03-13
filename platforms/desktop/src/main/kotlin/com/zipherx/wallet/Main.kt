package com.zipherx.wallet

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import com.zipherx.wallet.platform.*
import com.zipherx.wallet.ui.*
import java.io.File

// =============================================================================
// Cypherpunk Terminal Theme Colors
// =============================================================================
object ZColors {
    val primary       = Color(0xFF00FF40)
    val primaryDark   = Color(0xFF00D940)
    val primaryDim    = Color(0xFF00B32E)
    val terminalBlack = Color(0xFF050505)
    val surface       = Color(0xFF1A1208)
    val surfaceDark   = Color(0xFF0A0A0A)
    val progressBg    = Color(0xFF261905)
    val success       = Color(0xFF00FF40)
    val error         = Color(0xFFFF3131)
    val warning       = Color(0xFFFFC107)
    val glow          = Color(0x4D00FF40)
    val border        = Color(0x4D00FF40)
    val textDim       = Color(0xFF6B8F6B)
}

// =============================================================================
// Navigation
// =============================================================================
enum class Screen {
    LOADING,
    DISCLAIMER,
    PASSWORD,
    SETUP,
    WALLET,
    SEND,
    SETTINGS,
}

// =============================================================================
// Main Entry Point
// =============================================================================
fun main() {
    // Native library is loaded automatically by JNA via UniFFI bindings.
    // Set jna.library.path in build.gradle.kts to point to resources dir.
    val dataDir = getDataDirectory()

    val storage = DesktopSecureStorage(dataDir)

    application {
        val windowState = rememberWindowState(width = 420.dp, height = 780.dp)

        Window(
            onCloseRequest = ::exitApplication,
            state = windowState,
            title = "ZipherX",
            resizable = true,
        ) {
            ZipherXDesktopApp(storage, dataDir, onExitApp = ::exitApplication)
        }
    }
}

@Composable
fun ZipherXDesktopApp(storage: DesktopSecureStorage, dataDir: File, onExitApp: () -> Unit = {}) {
    val viewModel = remember { WalletViewModel(storage) }
    var currentScreen by remember { mutableStateOf(Screen.LOADING) }
    var initPhase by remember { mutableStateOf("Initializing runtime...") }
    var initError by remember { mutableStateOf<String?>(null) }

    // Initialize runtime with progress
    LaunchedEffect(Unit) {
        try {
            initPhase = "Initializing runtime..."
            ZipherXWrapper.initialize()

            initPhase = "Setting up secure storage..."
            val storageCallback = DesktopPlatformStorageCallback(storage)
            uniffi.zipherx.setPlatformStorage(storageCallback)
            ZipherXWrapper.platformStorage = storageCallback

            initPhase = "Generating encryption key..."
            val dbKey = getOrCreateDbEncryptionKey(storage)

            initPhase = "Opening wallet database..."
            val config = WalletConfig(
                dbPath = File(dataDir, "wallet.db").absolutePath,
                headerStorePath = File(dataDir, "headers.db").absolutePath,
                deltaStoreDir = File(dataDir, "delta").absolutePath,
                spendParamsPath = File(dataDir, "sapling-spend.params").absolutePath,
                outputParamsPath = File(dataDir, "sapling-output.params").absolutePath,
                dbEncryptionKey = dbKey,
            )
            ZipherXWrapper.initializeWallet(config)

            initPhase = "Ready."
            kotlinx.coroutines.delay(400)

            // Skip disclaimer if user has already accepted it before
            val disclaimerFile = java.io.File(dataDir, "disclaimer_accepted")
            if (disclaimerFile.exists() && storage.hasKey("spending_key")) {
                // Wallet exists + disclaimer already accepted → go straight to password
                currentScreen = Screen.PASSWORD
            } else {
                currentScreen = Screen.DISCLAIMER
            }
        } catch (e: Exception) {
            initError = e.message ?: "Unknown initialization error"
            System.err.println("Init error: ${e.message}")
        }
    }

    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = ZColors.primary,
            onPrimary = ZColors.terminalBlack,
            surface = ZColors.surfaceDark,
            onSurface = ZColors.primary,
            background = ZColors.terminalBlack,
            onBackground = ZColors.primary,
            error = ZColors.error,
        ),
        typography = Typography().let { base ->
            base.copy(
                displayLarge = base.displayLarge.copy(fontFamily = FontFamily.Monospace),
                displayMedium = base.displayMedium.copy(fontFamily = FontFamily.Monospace),
                displaySmall = base.displaySmall.copy(fontFamily = FontFamily.Monospace),
                headlineLarge = base.headlineLarge.copy(fontFamily = FontFamily.Monospace),
                headlineMedium = base.headlineMedium.copy(fontFamily = FontFamily.Monospace),
                headlineSmall = base.headlineSmall.copy(fontFamily = FontFamily.Monospace),
                titleLarge = base.titleLarge.copy(fontFamily = FontFamily.Monospace),
                titleMedium = base.titleMedium.copy(fontFamily = FontFamily.Monospace),
                titleSmall = base.titleSmall.copy(fontFamily = FontFamily.Monospace),
                bodyLarge = base.bodyLarge.copy(fontFamily = FontFamily.Monospace),
                bodyMedium = base.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                bodySmall = base.bodySmall.copy(fontFamily = FontFamily.Monospace),
                labelLarge = base.labelLarge.copy(fontFamily = FontFamily.Monospace),
                labelMedium = base.labelMedium.copy(fontFamily = FontFamily.Monospace),
                labelSmall = base.labelSmall.copy(fontFamily = FontFamily.Monospace),
            )
        },
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(ZColors.terminalBlack)
        ) {
            when (currentScreen) {
                Screen.LOADING -> LoadingScreen(
                    phase = initPhase,
                    error = initError,
                )
                Screen.DISCLAIMER -> DisclaimerScreen(
                    onAccept = {
                        // Persist acceptance so we skip disclaimer on next launch
                        java.io.File(dataDir, "disclaimer_accepted").writeText("1")
                        currentScreen = Screen.PASSWORD
                    },
                )
                Screen.PASSWORD -> {
                    val passwordError by viewModel.passwordError.collectAsState()
                    PasswordScreen(
                        hasWallet = storage.hasKey("spending_key"),
                        onUnlock = { password ->
                            val success = viewModel.unlockWithPassword(password)
                            if (success) {
                                currentScreen = if (viewModel.hasExistingWallet()) Screen.WALLET else Screen.SETUP
                            }
                            // If !success, passwordError is set — PasswordScreen stays visible
                        },
                        onSkip = {
                            currentScreen = Screen.SETUP
                        },
                        passwordError = passwordError,
                        onClearError = { viewModel.clearPasswordError() },
                    )
                }
                Screen.SETUP -> SetupScreen(
                    viewModel = viewModel,
                    onWalletCreated = { currentScreen = Screen.WALLET },
                )
                Screen.WALLET -> WalletScreen(
                    viewModel = viewModel,
                    onNavigateToSend = { currentScreen = Screen.SEND },
                    onNavigateToSettings = { currentScreen = Screen.SETTINGS },
                )
                Screen.SEND -> SendScreen(
                    viewModel = viewModel,
                    onBack = { currentScreen = Screen.WALLET },
                )
                Screen.SETTINGS -> SettingsScreen(
                    viewModel = viewModel,
                    onBack = { currentScreen = Screen.WALLET },
                    onDeleteWallet = {
                        viewModel.deleteAllData()
                        viewModel.shutdown()
                        onExitApp()
                    },
                )
            }
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            viewModel.shutdown()
        }
    }
}
