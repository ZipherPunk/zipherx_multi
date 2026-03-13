/// BalanceView.swift
/// ZipherXSwift
///
/// Cypherpunk-styled balance display with neon glow, dark terminal
/// background, and sync progress indicator.

import SwiftUI

@available(iOS 16, macOS 13, *)
public struct BalanceView: View {

    public let balance: Balance?
    public let syncPhase: String
    public let syncProgress: Double
    public let isSyncing: Bool
    public let currentHeight: UInt64
    public let targetHeight: UInt64
    public let syncSpeed: Double
    public let syncETA: TimeInterval?
    public let connectedPeers: UInt32
    public let pendingConfirmation: Bool

    @State private var blinkVisible = true

    public init(
        balance: Balance?,
        syncPhase: String,
        syncProgress: Double,
        isSyncing: Bool,
        currentHeight: UInt64 = 0,
        targetHeight: UInt64 = 0,
        syncSpeed: Double = 0,
        syncETA: TimeInterval? = nil,
        connectedPeers: UInt32 = 0,
        pendingConfirmation: Bool = false
    ) {
        self.balance = balance
        self.syncPhase = syncPhase
        self.syncProgress = syncProgress
        self.isSyncing = isSyncing
        self.currentHeight = currentHeight
        self.targetHeight = targetHeight
        self.syncSpeed = syncSpeed
        self.syncETA = syncETA
        self.connectedPeers = connectedPeers
        self.pendingConfirmation = pendingConfirmation
    }

    public var body: some View {
        ZCard {
            VStack(spacing: 12) {
                // Header
                HStack {
                    Text("SHIELDED BALANCE")
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.primaryDark)
                    Spacer()
                    if !isSyncing {
                        HStack(spacing: 4) {
                            Image(systemName: "checkmark")
                                .font(ZFonts.small)
                            Text("SYNCED")
                                .font(ZFonts.small)
                        }
                        .foregroundColor(ZColors.success)
                        .opacity(blinkVisible ? 1 : 0.3)
                        .onAppear {
                            withAnimation(.easeInOut(duration: 0.5).repeatForever(autoreverses: true)) {
                                blinkVisible = false
                            }
                        }
                    }
                }

                if pendingConfirmation {
                    // TX broadcast but not yet confirmed
                    Text("AWAITING CONFIRMATION")
                        .font(ZFonts.balance.weight(.bold))
                        .foregroundColor(ZColors.warning)
                        .shadow(color: ZColors.warning.opacity(0.3), radius: 3)
                        .minimumScaleFactor(0.6)
                        .lineLimit(1)
                    Text("TX broadcast — waiting for block...")
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.warning.opacity(0.7))
                } else {
                    // Balance amount
                    Text(zclFormatted(zatoshis: balance?.total ?? 0))
                        .font(ZFonts.balance)
                        .foregroundColor(ZColors.primary)
                        .shadow(color: ZColors.glow, radius: 3)
                        .minimumScaleFactor(0.6)
                        .lineLimit(1)

                    // Spendable
                    HStack(spacing: 4) {
                        Text("Spendable:")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDim)
                        Text(zclFormatted(zatoshis: balance?.spendable ?? 0))
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDark)
                    }
                }

                // Note count
                if let bal = balance {
                    Text("\(bal.spendableNoteCount)/\(bal.noteCount) notes spendable")
                        .font(ZFonts.small)
                        .foregroundColor(ZColors.primaryDim)
                }

                // Sync progress
                if isSyncing {
                    VStack(spacing: 6) {
                        ZProgressBar(progress: syncProgress)

                        // Heights: "123,456 / 3,015,509"
                        HStack {
                            Text(syncPhaseLabel(syncPhase))
                                .font(ZFonts.small)
                                .foregroundColor(ZColors.primaryDark)
                            Spacer()
                            if targetHeight > 0 {
                                Text("\(formatHeight(currentHeight)) / \(formatHeight(targetHeight))")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDark)
                            } else {
                                Text("\(Int(syncProgress * 100))%")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDark)
                            }
                        }

                        // Speed + ETA: "1,190 hdr/s — ETA 42m 15s"
                        if syncSpeed > 0 {
                            HStack {
                                Text("\(formatSpeed(syncSpeed)) hdr/s")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                                Spacer()
                                if let eta = syncETA, eta > 0, eta < 86400 {
                                    Text("ETA \(formatDuration(eta))")
                                        .font(ZFonts.small)
                                        .foregroundColor(ZColors.primaryDim)
                                }
                            }
                        }
                    }
                    .padding(.top, 4)
                }
            }
        }
    }

    private func zclFormatted(zatoshis: UInt64) -> String {
        let zcl = Double(zatoshis) / 1e8
        return String(format: "%.8f ZCL", zcl)
    }

    private func formatHeight(_ h: UInt64) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        return formatter.string(from: NSNumber(value: h)) ?? "\(h)"
    }

    private func formatSpeed(_ speed: Double) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.maximumFractionDigits = 0
        return formatter.string(from: NSNumber(value: speed)) ?? "\(Int(speed))"
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let s = Int(seconds)
        if s < 60 {
            return "\(s)s"
        } else if s < 3600 {
            return "\(s / 60)m \(s % 60)s"
        } else {
            let h = s / 3600
            let m = (s % 3600) / 60
            return "\(h)h \(m)m"
        }
    }

    private func syncPhaseLabel(_ phase: String) -> String {
        switch phase {
        case "boost_download": return "Downloading boost file..."
        case "boost_load":     return "Loading boost headers..."
        case "header_sync":    return "Syncing headers..."
        case "delta_sync":     return "Downloading outputs..."
        case "block_scan":     return "Scanning blocks..."
        case "witness_update":  return "Updating witnesses..."
        case "starting":       return "Starting sync..."
        case "idle":           return "Sync complete."
        default:               return phase.replacingOccurrences(of: "_", with: " ").uppercased()
        }
    }
}
