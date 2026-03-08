package com.zipherx.wallet.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.ZColors

@Composable
fun PasswordScreen(
    hasWallet: Boolean,
    onUnlock: (String) -> Unit,
    onSkip: () -> Unit,
    passwordError: String? = null,
    onClearError: () -> Unit = {},
) {
    var password by remember { mutableStateOf("") }
    var confirmPassword by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var passwordVisible by remember { mutableStateOf(false) }
    val isNewSetup = !hasWallet
    val focusRequester = remember { FocusRequester() }

    // Rate-limiting state (client-side only).
    // SECURITY NOTE (KD-3): This rate limit uses Compose `remember` state, which
    // resets if the user restarts the application. It is NOT a substitute for
    // server-side throttling. The real brute-force protection is the PBKDF2
    // iteration count (600,000 rounds of HMAC-SHA256) in DesktopSecureStorage,
    // which makes each password attempt take ~200-500 ms on typical hardware.
    // Persisting the counter to disk was considered but provides limited benefit:
    // an attacker with disk access can simply delete the counter file.
    var failedAttempts by remember { mutableStateOf(0) }
    var cooldownUntil by remember { mutableStateOf(0L) }
    var cooldownRemaining by remember { mutableStateOf(0L) }

    // Cooldown timer
    LaunchedEffect(cooldownUntil) {
        if (cooldownUntil > 0) {
            while (System.currentTimeMillis() < cooldownUntil) {
                cooldownRemaining = (cooldownUntil - System.currentTimeMillis()) / 1000
                kotlinx.coroutines.delay(1000)
            }
            cooldownRemaining = 0
        }
    }

    LaunchedEffect(Unit) { focusRequester.requestFocus() }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        // ASCII art logo
        Text(
            text = "[ ZIPHERX ]",
            fontSize = 28.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primary,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            text = "Privacy-first Zclassic wallet",
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primaryDim,
        )
        Spacer(Modifier.height(32.dp))

        // Lock icon
        Text(
            text = if (isNewSetup) "> SET PASSWORD" else "> UNLOCK WALLET",
            fontSize = 14.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = FontFamily.Monospace,
            color = ZColors.primary,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            text = if (isNewSetup)
                "Choose a password to encrypt your wallet keys.\nThis protects your private key on disk."
            else
                "Enter your password to decrypt and access your wallet.",
            fontSize = 11.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.height(24.dp))

        // Password field
        OutlinedTextField(
            value = password,
            onValueChange = { password = it; error = null; onClearError() },
            label = { Text("Password", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
            visualTransformation = if (passwordVisible) VisualTransformation.None else PasswordVisualTransformation(),
            singleLine = true,
            modifier = Modifier
                .fillMaxWidth(0.8f)
                .focusRequester(focusRequester),
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = ZColors.primary,
                unfocusedBorderColor = ZColors.border,
                cursorColor = ZColors.primary,
                focusedTextColor = ZColors.primary,
                unfocusedTextColor = ZColors.primaryDim,
                focusedLabelColor = ZColors.primary,
                unfocusedLabelColor = ZColors.textDim,
            ),
            shape = RoundedCornerShape(2.dp),
            trailingIcon = {
                IconButton(onClick = { passwordVisible = !passwordVisible }) {
                    Icon(
                        if (passwordVisible) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                        contentDescription = if (passwordVisible) "Hide password" else "Show password",
                        tint = ZColors.primaryDim,
                        modifier = Modifier.size(20.dp),
                    )
                }
            },
        )
        Spacer(Modifier.height(12.dp))

        // Confirm password (new setup only)
        if (isNewSetup) {
            OutlinedTextField(
                value = confirmPassword,
                onValueChange = { confirmPassword = it; error = null },
                label = { Text("Confirm Password", fontFamily = FontFamily.Monospace, fontSize = 11.sp) },
                visualTransformation = if (passwordVisible) VisualTransformation.None else PasswordVisualTransformation(),
                singleLine = true,
                modifier = Modifier.fillMaxWidth(0.8f),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = ZColors.primary,
                    unfocusedBorderColor = ZColors.border,
                    cursorColor = ZColors.primary,
                    focusedTextColor = ZColors.primary,
                    unfocusedTextColor = ZColors.primaryDim,
                    focusedLabelColor = ZColors.primary,
                    unfocusedLabelColor = ZColors.textDim,
                ),
                shape = RoundedCornerShape(2.dp),
                trailingIcon = {
                    IconButton(onClick = { passwordVisible = !passwordVisible }) {
                        Icon(
                            if (passwordVisible) Icons.Filled.VisibilityOff else Icons.Filled.Visibility,
                            contentDescription = if (passwordVisible) "Hide password" else "Show password",
                            tint = ZColors.primaryDim,
                            modifier = Modifier.size(20.dp),
                        )
                    }
                },
            )
            Spacer(Modifier.height(12.dp))
        }

        // Error (local or from ViewModel)
        val displayError = error ?: passwordError
        if (displayError != null) {
            Text(
                text = displayError,
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.error,
            )
            Spacer(Modifier.height(8.dp))
        }

        // Cooldown message
        if (cooldownRemaining > 0) {
            Text(
                text = "Too many failed attempts. Try again in ${cooldownRemaining}s",
                fontSize = 11.sp,
                fontFamily = FontFamily.Monospace,
                color = ZColors.warning,
            )
            Spacer(Modifier.height(8.dp))
        }

        // Unlock / Set Password button
        OutlinedButton(
            onClick = {
                // Check cooldown
                if (System.currentTimeMillis() < cooldownUntil) {
                    error = "Please wait for cooldown"
                    return@OutlinedButton
                }
                if (password.length < 8) {
                    error = "Password must be at least 8 characters"
                    return@OutlinedButton
                }
                if (isNewSetup && password != confirmPassword) {
                    error = "Passwords do not match"
                    return@OutlinedButton
                }
                onUnlock(password)
                // If we're still on this screen after onUnlock, password was wrong
                // The caller should handle the failure; we track attempts here
                if (!isNewSetup) {
                    failedAttempts++
                    if (failedAttempts >= 5) {
                        val cooldownSeconds = 30L
                        cooldownUntil = System.currentTimeMillis() + (cooldownSeconds * 1000)
                        error = "Too many failed attempts. Locked for ${cooldownSeconds}s."
                        failedAttempts = 0  // Reset counter after applying cooldown
                    }
                }
            },
            modifier = Modifier.fillMaxWidth(0.8f),
            enabled = cooldownRemaining <= 0,
            shape = RoundedCornerShape(2.dp),
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = ZColors.primary,
            ),
            border = androidx.compose.foundation.BorderStroke(1.dp, ZColors.primary),
        ) {
            Text(
                text = if (isNewSetup) "SET PASSWORD" else "UNLOCK",
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                fontSize = 14.sp,
            )
        }

        Spacer(Modifier.height(32.dp))
        Text(
            text = "\"Privacy is the power to selectively\nreveal oneself to the world.\"",
            fontSize = 10.sp,
            fontFamily = FontFamily.Monospace,
            color = ZColors.textDim,
            textAlign = TextAlign.Center,
        )
        Text(
            text = "— Eric Hughes, A Cypherpunk's Manifesto",
            fontSize = 9.sp,
            fontFamily = FontFamily.Monospace,
            color = Color(0xFF3A5F3A),
            textAlign = TextAlign.Center,
        )
    }
}
