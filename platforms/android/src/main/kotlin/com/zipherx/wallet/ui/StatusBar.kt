package com.zipherx.wallet.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.WalletViewModel

/**
 * Global status bar displayed at the bottom of every screen.
 * Shows: version | peers | block height | sync status
 */
@Composable
fun StatusBar(viewModel: WalletViewModel) {
    val peers by viewModel.connectedPeers.collectAsState()
    val height by viewModel.blockHeight.collectAsState()
    val status by viewModel.networkStatus.collectAsState()

    val statusColor = when (status) {
        "Synced" -> Color(0xFF00E676)
        "Syncing" -> Color(0xFFFFC107)
        "Connected" -> Color(0xFF00E676)
        "Disconnected" -> Color(0xFFFF5252)
        "Error" -> Color(0xFFFF5252)
        else -> Color(0xFF888888)
    }

    val dotColor = when (status) {
        "Synced", "Connected" -> Color(0xFF00E676)
        "Syncing" -> Color(0xFFFFC107)
        else -> Color(0xFFFF5252)
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color(0xFF080808))
            .windowInsetsPadding(WindowInsets.navigationBars)
            .padding(horizontal = 12.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Version
        Text(
            text = "v1.0.0",
            style = MaterialTheme.typography.labelSmall.copy(
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
            ),
            color = Color(0xFF555555),
        )

        // Peers
        Text(
            text = "${peers} peers",
            style = MaterialTheme.typography.labelSmall.copy(
                fontFamily = FontFamily.Monospace,
                fontSize = 10.sp,
            ),
            color = if (peers > 0u) Color(0xFF888888) else Color(0xFFFF5252),
        )

        // Block height + status
        Row(verticalAlignment = Alignment.CenterVertically) {
            Spacer(
                modifier = Modifier
                    .size(6.dp)
                    .clip(CircleShape)
                    .background(dotColor)
            )
            Spacer(modifier = Modifier.width(4.dp))
            Text(
                text = if (height > 0) "%,d".format(height) else "---",
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                ),
                color = Color(0xFF888888),
            )
            Spacer(modifier = Modifier.width(6.dp))
            Text(
                text = status,
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                    fontSize = 10.sp,
                ),
                color = statusColor,
            )
        }
    }
}
