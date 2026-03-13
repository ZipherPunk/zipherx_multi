package com.zipherx.wallet.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.zipherx.wallet.Balance
import com.zipherx.wallet.ZColors

/**
 * Balance card composable — Cypherpunk terminal design.
 *
 * Displays the total and spendable balance in ZCL along with
 * the current sync phase and progress indicator.
 */
@Composable
fun BalanceCard(
    balance: Balance?,
    syncPhase: String,
    syncProgress: Double,
    isSyncing: Boolean,
    isPendingConfirmation: Boolean = false,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(ZColors.surface)
            .border(1.dp, ZColors.primaryDim, RoundedCornerShape(0.dp))
            .shadow(4.dp, ambientColor = ZColors.glow, spotColor = ZColors.glow)
            .padding(16.dp),
    ) {
        // Header
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "SHIELDED BALANCE",
                style = MaterialTheme.typography.labelMedium,
                color = ZColors.primaryDark,
            )
            if (!isSyncing) {
                val blink = rememberInfiniteTransition(label = "synced_blink")
                val alpha by blink.animateFloat(
                    initialValue = 1f,
                    targetValue = 0.3f,
                    animationSpec = infiniteRepeatable(
                        animation = tween(500),
                        repeatMode = RepeatMode.Reverse,
                    ),
                    label = "blink_alpha",
                )
                Text(
                    text = "✓ SYNCED",
                    style = MaterialTheme.typography.labelSmall,
                    color = ZColors.success.copy(alpha = alpha),
                )
            }
        }

        Spacer(modifier = Modifier.height(8.dp))

        if (isPendingConfirmation) {
            // TX broadcast but change note not yet mined — hide balance
            // to avoid showing misleading intermediate value.
            val blink = rememberInfiniteTransition(label = "pending_blink")
            val alpha by blink.animateFloat(
                initialValue = 1f,
                targetValue = 0.4f,
                animationSpec = infiniteRepeatable(
                    animation = tween(800),
                    repeatMode = RepeatMode.Reverse,
                ),
                label = "pending_alpha",
            )
            Text(
                text = "AWAITING CONFIRMATION",
                style = MaterialTheme.typography.headlineLarge.copy(
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 20.sp,
                ),
                color = ZColors.warning.copy(alpha = alpha),
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = "TX broadcast — waiting for block...",
                style = MaterialTheme.typography.labelSmall.copy(
                    fontFamily = FontFamily.Monospace,
                ),
                color = ZColors.warning.copy(alpha = 0.7f),
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        } else {
            // Balance amount with glow
            Text(
                text = formatZcl(balance?.total ?: 0L),
                style = MaterialTheme.typography.headlineLarge.copy(
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 24.sp,
                ),
                color = ZColors.primary,
                modifier = Modifier
                    .testTag("balance_text")
                    .align(Alignment.CenterHorizontally),
            )

            Spacer(modifier = Modifier.height(8.dp))

            // Spendable
            Row(
                modifier = Modifier.align(Alignment.CenterHorizontally),
            ) {
                Text(
                    text = "Spendable: ",
                    style = MaterialTheme.typography.labelMedium,
                    color = ZColors.primaryDim,
                )
                Text(
                    text = formatZcl(balance?.spendable ?: 0L),
                    style = MaterialTheme.typography.labelMedium,
                    color = ZColors.primaryDark,
                )
            }
        }

        // Note count
        if (balance != null && balance.noteCount > 0) {
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = "${balance.spendableNoteCount}/${balance.noteCount} notes spendable",
                style = MaterialTheme.typography.labelSmall,
                color = ZColors.primaryDim,
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        }

        // Sync progress
        if (isSyncing) {
            Spacer(modifier = Modifier.height(12.dp))

            // Custom progress bar
            if (syncProgress > 0.0) {
                ZProgressBar(progress = syncProgress)
                Spacer(modifier = Modifier.height(6.dp))
            }

            // Phase + heights
            val formatted = formatSyncPhase(syncPhase)
            Text(
                text = formatted,
                style = MaterialTheme.typography.labelSmall,
                color = ZColors.primaryDark,
            )
        }
    }
}

/**
 * Cypherpunk-styled progress bar with gradient fill and glow.
 */
@Composable
private fun ZProgressBar(progress: Double) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(12.dp)
            .background(ZColors.progressBg)
            .border(1.dp, ZColors.primaryDim),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth(fraction = progress.toFloat().coerceIn(0f, 1f))
                .height(10.dp)
                .padding(1.dp)
                .background(
                    Brush.horizontalGradient(
                        colors = listOf(ZColors.primaryDark, ZColors.primary),
                    )
                )
                .shadow(2.dp, ambientColor = ZColors.glow, spotColor = ZColors.glow),
        )
    }
}

/**
 * Format a zatoshi value as a human-readable ZCL string.
 * 1 ZCL = 100,000,000 zatoshis.
 */
private fun formatZcl(zatoshis: Long): String {
    val whole = zatoshis / 100_000_000L
    val fraction = (zatoshis % 100_000_000L).let {
        if (it < 0) -it else it
    }
    return "%d.%08d ZCL".format(whole, fraction)
}

/**
 * Convert a sync phase identifier to a user-friendly label.
 */
private fun formatSyncPhase(phase: String): String {
    // Phase may include height info as "phase:current:target"
    // Repair phases come as "repair:phase:current:target"
    val parts = phase.split(":")
    val base = parts[0]

    // Handle repair phases: "repair:boost_download:current:target" etc.
    if (base == "repair" && parts.size >= 2) {
        val repairPhase = parts[1]
        val repairLabel = when (repairPhase) {
            "boost_download" -> "Repairing: downloading boost"
            "boost_load" -> "Repairing: loading boost"
            "header_sync" -> "Repairing: syncing headers"
            "delta_sync" -> "Repairing: syncing blocks"
            "block_scan" -> "Repairing: scanning blocks"
            "witness_update" -> "Repairing: updating witnesses"
            "complete" -> "Repair complete"
            else -> "Repairing: $repairPhase"
        }
        return if (parts.size == 4) {
            val current = parts[2].toLongOrNull() ?: 0L
            val target = parts[3].toLongOrNull() ?: 0L
            if (repairPhase == "boost_download") {
                val currentMb = current / (1024 * 1024)
                val totalMb = target / (1024 * 1024)
                val pct = if (target > 0) (current * 100 / target) else 0
                "$repairLabel: %,d / %,d MB (%d%%)".format(currentMb, totalMb, pct)
            } else {
                val pct = if (target > 0) (current * 100 / target) else 0
                "$repairLabel: %,d / %,d (%d%%)".format(current, target, pct)
            }
        } else {
            "$repairLabel..."
        }
    }

    // Handle repairing_witnesses initial notification
    if (base == "repairing_witnesses") {
        return if (parts.size == 3) {
            val missing = parts[2].toLongOrNull() ?: 0L
            "Repairing $missing witnesses..."
        } else {
            "Repairing witnesses..."
        }
    }

    val label = when (base) {
        "idle" -> "Idle"
        "starting" -> "Starting..."
        "boost_download" -> "Downloading boost"
        "boost_load" -> "Loading boost headers"
        "header_sync" -> "Syncing headers"
        "delta_sync" -> "Syncing blocks"
        "block_scan" -> "Scanning blocks"
        "gap_fill" -> "Filling gaps"
        "witness_update" -> "Updating witnesses"
        "boost_failed" -> "Boost download failed"
        "complete" -> "Sync complete"
        "failed" -> "Sync failed"
        else -> base.replaceFirstChar { it.uppercase() }
    }
    return if (parts.size == 3) {
        val current = parts[1].toLongOrNull() ?: 0L
        val target = parts[2].toLongOrNull() ?: 0L
        if (base == "boost_download") {
            val currentMb = current / (1024 * 1024)
            val totalMb = target / (1024 * 1024)
            val pct = if (target > 0) (current * 100 / target) else 0
            "$label: %,d / %,d MB (%d%%)".format(currentMb, totalMb, pct)
        } else {
            val pct = if (target > 0) (current * 100 / target) else 0
            "$label: %,d / %,d (%d%%)".format(current, target, pct)
        }
    } else {
        "$label..."
    }
}
