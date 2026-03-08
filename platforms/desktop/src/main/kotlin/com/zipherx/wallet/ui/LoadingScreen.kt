package com.zipherx.wallet.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.ZColors

@Composable
fun LoadingScreen(
    phase: String,
    error: String?,
) {
    // Spinning shield animation
    val infiniteTransition = rememberInfiniteTransition(label = "shield_spin")
    val rotationY by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 3000, easing = LinearEasing),
            repeatMode = RepeatMode.Restart,
        ),
        label = "shield_rotation",
    )

    // Glowing pulse
    val glowAlpha by infiniteTransition.animateFloat(
        initialValue = 0.3f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1500, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "glow_pulse",
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        // Spinning shield icon
        Icon(
            imageVector = Icons.Filled.Lock,
            contentDescription = "ZipherX",
            tint = ZColors.primary.copy(alpha = glowAlpha),
            modifier = Modifier
                .size(80.dp)
                .graphicsLayer {
                    this.rotationY = rotationY
                    cameraDistance = 12f * density
                },
        )

        Spacer(Modifier.height(24.dp))

        Text(
            "ZIPHERX",
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            fontSize = 28.sp,
            color = ZColors.primary,
            letterSpacing = 4.sp,
        )

        Spacer(Modifier.height(8.dp))

        Text(
            "Privacy-First Cryptocurrency Wallet",
            fontFamily = FontFamily.Monospace,
            fontSize = 11.sp,
            color = ZColors.primaryDim,
        )

        Spacer(Modifier.height(32.dp))

        if (error != null) {
            Text(
                "ERROR: $error",
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
                color = ZColors.error,
                textAlign = TextAlign.Center,
            )
        } else {
            // Init phase text
            Text(
                phase,
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
                color = ZColors.primaryDim,
            )

            Spacer(Modifier.height(12.dp))

            LinearProgressIndicator(
                modifier = Modifier
                    .width(200.dp)
                    .height(3.dp),
                color = ZColors.primary,
                trackColor = ZColors.progressBg,
            )
        }

        Spacer(Modifier.height(48.dp))

        Text(
            "\"Cypherpunks write code.\"",
            fontFamily = FontFamily.Monospace,
            fontSize = 10.sp,
            color = ZColors.textDim,
        )
        Text(
            "— Eric Hughes, A Cypherpunk's Manifesto",
            fontFamily = FontFamily.Monospace,
            fontSize = 9.sp,
            color = ZColors.textDim,
        )
    }
}
