/// SettingsView.swift
/// ZipherXSwift
///
/// Wallet settings with Cypherpunk terminal design.

import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

@available(iOS 17, macOS 14, *)
public struct SettingsView: View {

    var viewModel: WalletViewModel

    @State private var showRepairConfirm: Bool = false
    @State private var showRescanConfirm: Bool = false
    @State private var actionInProgress: Bool = false
    @State private var actionStatus: String?
    /// SA-5: Visual indicator that clipboard will auto-clear after copying onion address.
    @State private var onionCopied: Bool = false

    @Environment(\.dismiss) private var dismiss

    public init(viewModel: WalletViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(spacing: 0) {
                // Title bar
                HStack {
                    Button(action: { dismiss() }) {
                        Text("CLOSE")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDark)
                    }
                    .buttonStyle(.plain)
                    Spacer()
                    Text("SETTINGS")
                        .font(ZFonts.title)
                        .foregroundColor(ZColors.primary)
                    Spacer()
                    Text("CLOSE")
                        .font(ZFonts.caption)
                        .foregroundColor(.clear)
                }
                .padding(16)
                .background(ZColors.surface)
                .overlay(Rectangle().fill(ZColors.primaryDim).frame(height: 1), alignment: .bottom)

                ScrollView {
                    VStack(spacing: 16) {

                        // MARK: - Privacy Section
                        sectionHeader("PRIVACY")

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                HStack {
                                    Image(systemName: "lock.shield")
                                        .font(ZFonts.heading)
                                        .foregroundColor(ZColors.primary)
                                    Text("TOR NETWORK")
                                        .font(ZFonts.body)
                                        .foregroundColor(ZColors.primary)
                                    Spacer()
                                    Toggle("", isOn: Binding(
                                        get: { viewModel.torEnabled },
                                        set: { viewModel.setTorEnabled($0) }
                                    ))
                                    .labelsHidden()
                                    .tint(ZColors.primary)
                                }

                                Text(viewModel.torEnabled
                                    ? "All P2P traffic routed through Tor. Takes effect on next sync."
                                    : "Tor is disabled. P2P connections are direct.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)

                                if viewModel.torEnabled, let onion = viewModel.onionAddress {
                                    Rectangle()
                                        .fill(ZColors.primaryDim.opacity(0.3))
                                        .frame(height: 1)

                                    HStack {
                                        Image(systemName: "network")
                                            .font(ZFonts.small)
                                            .foregroundColor(ZColors.primary)
                                        Text(onion)
                                            .font(.system(size: 10, design: .monospaced))
                                            .foregroundColor(ZColors.primaryDim)
                                            .lineLimit(1)
                                            .minimumScaleFactor(0.5)
                                        Spacer()
                                        Button(action: { copyOnionAddress(onion) }) {
                                            HStack(spacing: 4) {
                                                Image(systemName: onionCopied ? "checkmark" : "doc.on.doc")
                                                    .font(ZFonts.small)
                                                // SA-5: Show auto-clear indicator after copy
                                                if onionCopied {
                                                    Text("Copied — clears 30s")
                                                        .font(ZFonts.small)
                                                }
                                            }
                                            .foregroundColor(onionCopied ? ZColors.success : ZColors.primaryDark)
                                        }
                                        .buttonStyle(.plain)
                                    }
                                }
                            }
                        }

                        // MARK: - Security Section
                        sectionHeader("SECURITY")

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                HStack {
                                    Image(systemName: "eye.slash.fill")
                                        .font(ZFonts.heading)
                                        .foregroundColor(ZColors.primary)
                                    Text("SCREENSHOT PROTECTION")
                                        .font(ZFonts.body)
                                        .foregroundColor(ZColors.primary)
                                    Spacer()
                                    Toggle("", isOn: Binding(
                                        get: { viewModel.screenshotProtectionEnabled },
                                        set: { viewModel.screenshotProtectionEnabled = $0 }
                                    ))
                                    .labelsHidden()
                                    .tint(ZColors.primary)
                                }

                                Text(viewModel.screenshotProtectionEnabled
                                    ? "App content is hidden in the app switcher and when backgrounded."
                                    : "Screenshot protection is disabled. App content is visible in the app switcher.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                            }
                        }

                        // MARK: - Maintenance Section
                        sectionHeader("MAINTENANCE")

                        ZCard {
                            VStack(spacing: 12) {
                                ZButton("Repair Database", icon: "wrench.and.screwdriver", style: .danger, action: {
                                    showRepairConfirm = true
                                })
                                .disabled(actionInProgress)

                                Text("Clears tree state, preserves notes and history.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                                    .frame(maxWidth: .infinity, alignment: .leading)

                                Rectangle()
                                    .fill(ZColors.primaryDim.opacity(0.3))
                                    .frame(height: 1)

                                ZButton("Full Rescan", icon: "arrow.counterclockwise.circle", style: .danger, action: {
                                    showRescanConfirm = true
                                })
                                .disabled(actionInProgress)

                                Text("Re-downloads everything from scratch. May take 5-15 minutes.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                                    .frame(maxWidth: .infinity, alignment: .leading)

                                if actionInProgress {
                                    HStack(spacing: 12) {
                                        ProgressView()
                                            .tint(ZColors.primary)
                                        Text((actionStatus ?? "Working...").uppercased())
                                            .font(ZFonts.caption)
                                            .foregroundColor(ZColors.primaryDark)
                                    }
                                    .padding(.top, 4)
                                }
                            }
                        }

                        // MARK: - About Section
                        sectionHeader("ABOUT")

                        ZCard {
                            VStack(spacing: 8) {
                                infoRow("VERSION", value: appVersion)
                                Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                                infoRow("WALLET STATE", value: viewModel.walletState.uppercased())
                                Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                                infoRow("SYNC PHASE", value: viewModel.syncPhase.replacingOccurrences(of: "_", with: " ").uppercased())
                            }
                        }

                        Spacer().frame(height: 16)
                    }
                    .padding(16)
                }
            }
        }
        .confirmationDialog(
            "Repair Database?",
            isPresented: $showRepairConfirm,
            titleVisibility: .visible
        ) {
            Button("Repair", role: .destructive) { runRepair() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Clears the commitment tree state and rebuilds it. Notes and transaction history are preserved.")
        }
        .confirmationDialog(
            "Full Rescan?",
            isPresented: $showRescanConfirm,
            titleVisibility: .visible
        ) {
            Button("Full Rescan", role: .destructive) { runFullRescan() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Clears all sync state and re-downloads everything from scratch.")
        }
    }

    // MARK: - Subviews

    private func sectionHeader(_ title: String) -> some View {
        Text(title)
            .font(ZFonts.caption)
            .foregroundColor(ZColors.primaryDark)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func infoRow(_ label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(ZFonts.caption)
                .foregroundColor(ZColors.primaryDim)
            Spacer()
            Text(value)
                .font(ZFonts.mono)
                .foregroundColor(ZColors.primary)
        }
    }

    // MARK: - Actions

    private func copyOnionAddress(_ address: String) {
        #if os(iOS)
        UIPasteboard.general.string = address
        // M-23: Clear clipboard after 30 seconds for privacy
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if UIPasteboard.general.string == address {
                UIPasteboard.general.string = ""
            }
        }
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(address, forType: .string)
        // M-23: Clear clipboard after 30 seconds for privacy
        let changeCount = NSPasteboard.general.changeCount
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if NSPasteboard.general.changeCount == changeCount {
                NSPasteboard.general.clearContents()
            }
        }
        #endif
        // SA-5: Visual feedback that clipboard will auto-clear
        onionCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            onionCopied = false
        }
    }

    private func runRepair() {
        actionInProgress = true
        actionStatus = "Repairing database..."
        DispatchQueue.global().async {
            #if canImport(ZipherXFFI)
            let result = Result { try repairDatabase() }
            #else
            let result: Result<Void, Error> = .failure(ZipherXError.ffiNotAvailable)
            #endif
            DispatchQueue.main.async {
                actionInProgress = false
                actionStatus = nil
                if case .failure(let e) = result {
                    viewModel.errorMessage = e.localizedDescription
                } else {
                    viewModel.refreshBalance()
                }
            }
        }
    }

    private func runFullRescan() {
        actionInProgress = true
        actionStatus = "Starting full rescan..."
        DispatchQueue.global().async {
            #if canImport(ZipherXFFI)
            let result = Result { try fullRescan() }
            #else
            let result: Result<Void, Error> = .failure(ZipherXError.ffiNotAvailable)
            #endif
            DispatchQueue.main.async {
                actionInProgress = false
                actionStatus = nil
                if case .failure(let e) = result {
                    viewModel.errorMessage = e.localizedDescription
                } else {
                    viewModel.refreshBalance()
                    viewModel.refreshHistory()
                }
            }
        }
    }

    // MARK: - Helpers

    private var appVersion: String {
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.1"
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "0"
        return "\(version) (\(build))"
    }
}
