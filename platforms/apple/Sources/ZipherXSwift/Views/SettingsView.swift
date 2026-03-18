/// SettingsView.swift
/// ZipherXSwift
///
/// Wallet settings with Cypherpunk terminal design.
/// Feature parity with Desktop (Compose Desktop) and Android.

import SwiftUI
import LocalAuthentication
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

    // Peer management state
    @State private var connectedPeers: [ConnectedPeerInfo] = []
    @State private var bannedPeers: [BannedPeerInfo] = []
    @State private var customPeerHost: String = ""
    @State private var customPeerPort: String = "8033"
    @State private var peerActionResult: String?
    @State private var peerSectionExpanded: Bool = false

    // Export private key state
    @State private var showExportKey: Bool = false
    @State private var exportedKeyDisplay: String?
    @State private var exportedKeyFull: String?
    @State private var keyCopied: Bool = false

    // Security audit state
    @State private var showAuditReport: Bool = false

    // WIF import state
    @State private var showWifImport: Bool = false
    @State private var wifImportText: String = ""
    @State private var wifImportResults: [(valid: Bool, address: String, prefix: String)]? = nil

    // Delete all data state
    @State private var showDeleteConfirm: Bool = false

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

                        // MARK: - Sync Section
                        sectionHeader("SYNC")

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                HStack {
                                    Image(systemName: "arrow.triangle.2.circlepath")
                                        .font(ZFonts.heading)
                                        .foregroundColor(ZColors.primary)
                                    Text(syncPhaseLabel(viewModel.syncPhase))
                                        .font(ZFonts.body)
                                        .foregroundColor(ZColors.primary)
                                    Spacer()
                                }

                                Button(action: {
                                    if viewModel.isSyncing {
                                        viewModel.stopSync()
                                    } else {
                                        viewModel.startSync()
                                    }
                                }) {
                                    HStack(spacing: 6) {
                                        Image(systemName: viewModel.isSyncing ? "stop.fill" : "play.fill")
                                            .font(ZFonts.small)
                                        Text(viewModel.isSyncing ? "STOP SYNC" : "START SYNC")
                                            .font(ZFonts.caption)
                                    }
                                    .foregroundColor(viewModel.isSyncing ? ZColors.error : ZColors.primary)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 8)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(viewModel.isSyncing ? ZColors.error : ZColors.primaryDim, lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)

                                Text("First sync: 10-30 min. Subsequent: <1 min.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                            }
                        }

                        // MARK: - Network Section
                        sectionHeader("NETWORK")

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                // Connected peers count
                                HStack {
                                    Circle()
                                        .fill(viewModel.connectedPeers > 0 ? ZColors.success : ZColors.error)
                                        .frame(width: 8, height: 8)
                                    Text("CONNECTED PEERS")
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.primaryDim)
                                    Spacer()
                                    Text("\(viewModel.connectedPeers)")
                                        .font(ZFonts.mono)
                                        .foregroundColor(ZColors.primary)
                                }

                                Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)

                                // Tor toggle
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

                                // Tor state
                                if viewModel.torEnabled {
                                    HStack {
                                        Text("TOR STATE")
                                            .font(ZFonts.caption)
                                            .foregroundColor(ZColors.primaryDim)
                                        Spacer()
                                        Text(torStateLabel)
                                            .font(ZFonts.mono)
                                            .foregroundColor(torStateColor)
                                    }
                                }

                                // Onion address
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

                        // MARK: - Peer Management Section (collapsible)
                        sectionHeader("PEER MANAGEMENT")

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                // Clickable header — toggles expand/collapse
                                Button(action: {
                                    withAnimation(.easeInOut(duration: 0.25)) {
                                        peerSectionExpanded.toggle()
                                    }
                                    if peerSectionExpanded && connectedPeers.isEmpty {
                                        connectedPeers = ZipherXWrapper.getConnectedPeers()
                                        bannedPeers = ZipherXWrapper.getBannedPeers()
                                    }
                                }) {
                                    HStack {
                                        Text("PEER DETAILS")
                                            .font(ZFonts.body)
                                            .foregroundColor(ZColors.primary)
                                        Spacer()
                                        Text("\(connectedPeers.count) connected, \(bannedPeers.count) banned")
                                            .font(ZFonts.small)
                                            .foregroundColor(ZColors.primaryDim)
                                        Text(peerSectionExpanded ? "[-]" : "[+]")
                                            .font(ZFonts.body)
                                            .foregroundColor(ZColors.primary)
                                    }
                                }
                                .buttonStyle(.plain)

                                if peerSectionExpanded {
                                    // Refresh button
                                    HStack {
                                        Spacer()
                                        Button(action: {
                                            connectedPeers = ZipherXWrapper.getConnectedPeers()
                                            bannedPeers = ZipherXWrapper.getBannedPeers()
                                            peerActionResult = nil
                                        }) {
                                            Text("REFRESH")
                                                .font(ZFonts.caption)
                                                .foregroundColor(ZColors.primary)
                                        }
                                        .buttonStyle(.plain)
                                    }

                                    // Action result
                                    if let result = peerActionResult {
                                        Text(result)
                                            .font(ZFonts.small)
                                            .foregroundColor(result.hasPrefix("Error") ? ZColors.error : ZColors.primary)
                                    }

                                    Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)

                                    // Connected peers
                                    Text("CONNECTED (\(connectedPeers.count))")
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.primary)

                                    if connectedPeers.isEmpty {
                                        Text("No peers loaded. Tap REFRESH.")
                                            .font(ZFonts.small)
                                            .foregroundColor(ZColors.primaryDim)
                                    }

                                    ForEach(connectedPeers) { peer in
                                        HStack {
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(peer.address)
                                                    .font(ZFonts.mono)
                                                    .foregroundColor(ZColors.primary)
                                                    .lineLimit(1)
                                                Text("v\(peer.protocolVersion) | \(String(peer.userAgent.prefix(24))) | h:\(peer.startHeight)")
                                                    .font(.system(size: 9, design: .monospaced))
                                                    .foregroundColor(ZColors.primaryDim)
                                            }
                                            Spacer()
                                            Button(action: {
                                                let ok = ZipherXWrapper.disconnectPeer(peerId: peer.address)
                                                peerActionResult = ok ? "Disconnected \(peer.address)" : "Error: disconnect failed"
                                                connectedPeers = ZipherXWrapper.getConnectedPeers()
                                            }) {
                                                Text("DC")
                                                    .font(.system(size: 9, design: .monospaced))
                                                    .foregroundColor(ZColors.error)
                                            }
                                            .buttonStyle(.plain)
                                        }
                                    }

                                    Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)

                                    // Banned peers
                                    Text("BANNED (\(bannedPeers.count))")
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.error)

                                    if bannedPeers.isEmpty {
                                        Text("No banned peers.")
                                            .font(ZFonts.small)
                                            .foregroundColor(ZColors.primaryDim)
                                    }

                                    ForEach(bannedPeers) { peer in
                                        HStack {
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(peer.host)
                                                    .font(ZFonts.mono)
                                                    .foregroundColor(ZColors.error)
                                                    .lineLimit(1)
                                                let timeStr = peer.isPermanent ? "permanent" : "\(peer.remainingSeconds)s left"
                                                Text("\(String(peer.reason.prefix(30))) | \(timeStr)")
                                                    .font(.system(size: 9, design: .monospaced))
                                                    .foregroundColor(ZColors.primaryDim)
                                            }
                                            Spacer()
                                            Button(action: {
                                                let ok = ZipherXWrapper.unbanPeer(host: peer.host)
                                                peerActionResult = ok ? "Unbanned \(peer.host)" : "Error: unban failed"
                                                bannedPeers = ZipherXWrapper.getBannedPeers()
                                            }) {
                                                Text("UNBAN")
                                                    .font(.system(size: 9, design: .monospaced))
                                                    .foregroundColor(ZColors.primary)
                                            }
                                            .buttonStyle(.plain)
                                        }
                                    }

                                    Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)

                                    // Add custom peer
                                    Text("ADD CUSTOM PEER")
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.primaryDim)

                                    Text("IP address only (no hostnames — DNS leak prevention).")
                                        .font(.system(size: 9, design: .monospaced))
                                        .foregroundColor(ZColors.primaryDim)

                                    HStack(spacing: 8) {
                                        TextField("IP Address", text: $customPeerHost)
                                            .font(.system(size: 11, design: .monospaced))
                                            .foregroundColor(ZColors.primary)
                                            .textFieldStyle(.roundedBorder)
                                            #if os(iOS)
                                            .keyboardType(.decimalPad)
                                            #endif
                                            .frame(maxWidth: .infinity)
                                            .onChange(of: customPeerHost) { _, newValue in
                                                customPeerHost = newValue.filter { $0.isNumber || $0 == "." }
                                            }

                                        TextField("Port", text: $customPeerPort)
                                            .font(.system(size: 11, design: .monospaced))
                                            .foregroundColor(ZColors.primary)
                                            .textFieldStyle(.roundedBorder)
                                            #if os(iOS)
                                            .keyboardType(.numberPad)
                                            #endif
                                            .frame(width: 70)
                                            .onChange(of: customPeerPort) { _, newValue in
                                                customPeerPort = String(newValue.filter { $0.isNumber }.prefix(5))
                                            }

                                        Button(action: {
                                            let port = UInt16(customPeerPort) ?? 0
                                            if customPeerHost.trimmingCharacters(in: .whitespaces).isEmpty {
                                                peerActionResult = "Error: IP address required"
                                            } else if port == 0 {
                                                peerActionResult = "Error: Invalid port (1-65535)"
                                            } else {
                                                let ok = ZipherXWrapper.addCustomPeer(host: customPeerHost, port: port)
                                                peerActionResult = ok ? "Added \(customPeerHost):\(port)" : "Error: Invalid IP or peer rejected"
                                                if ok {
                                                    customPeerHost = ""
                                                    connectedPeers = ZipherXWrapper.getConnectedPeers()
                                                }
                                            }
                                        }) {
                                            Text("ADD")
                                                .font(ZFonts.caption)
                                                .foregroundColor(ZColors.primary)
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
                                // Screenshot protection
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

                                Rectangle()
                                    .fill(ZColors.primaryDim.opacity(0.3))
                                    .frame(height: 1)

                                // Export private key
                                Button(action: authenticateForExport) {
                                    HStack(spacing: 6) {
                                        Image(systemName: "key.fill")
                                            .font(ZFonts.small)
                                        Text("EXPORT PRIVATE KEY")
                                            .font(ZFonts.caption)
                                    }
                                    .foregroundColor(ZColors.warning)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 8)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ZColors.warning.opacity(0.5), lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)

                                Text("Requires authentication. Never share your private key.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)

                                Rectangle()
                                    .fill(ZColors.primaryDim.opacity(0.3))
                                    .frame(height: 1)

                                // Import WIF keys
                                Button(action: authenticateForImport) {
                                    HStack(spacing: 6) {
                                        Image(systemName: "square.and.arrow.down")
                                            .font(ZFonts.small)
                                        Text("IMPORT WIF KEYS")
                                            .font(ZFonts.caption)
                                    }
                                    .foregroundColor(ZColors.primary)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 8)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ZColors.primaryDim, lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)

                                Text("Import transparent private keys (WIF format). Not covered by recovery phrase.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)

                                Rectangle()
                                    .fill(ZColors.primaryDim.opacity(0.3))
                                    .frame(height: 1)

                                // Security audit report
                                Button(action: { showAuditReport = true }) {
                                    HStack(spacing: 6) {
                                        Image(systemName: "shield.lefthalf.filled")
                                            .font(ZFonts.small)
                                        Text("SECURITY AUDIT REPORT")
                                            .font(ZFonts.caption)
                                    }
                                    .foregroundColor(ZColors.primary)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 8)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ZColors.primaryDim, lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)
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

                                Text("Re-downloads everything from scratch.")
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
                                infoRow("PLATFORM", value: platformInfo)
                                Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                                infoRow("WALLET STATE", value: viewModel.walletState.uppercased())
                                Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                                infoRow("SYNC PHASE", value: viewModel.syncPhase.replacingOccurrences(of: "_", with: " ").uppercased())
                            }
                        }

                        // MARK: - Danger Zone
                        Text("DANGER ZONE")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.error)
                            .frame(maxWidth: .infinity, alignment: .leading)

                        ZCard {
                            VStack(alignment: .leading, spacing: 12) {
                                Button(action: authenticateForDelete) {
                                    HStack(spacing: 6) {
                                        Image(systemName: "trash.fill")
                                            .font(ZFonts.small)
                                        Text("DELETE ALL DATA")
                                            .font(ZFonts.caption)
                                    }
                                    .foregroundColor(ZColors.error)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 8)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ZColors.error.opacity(0.5), lineWidth: 1)
                                    )
                                }
                                .buttonStyle(.plain)

                                Text("Permanently deletes wallet, private key, and all data. YOUR FUNDS WILL BE LOST if you don't have your mnemonic phrase.")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.error.opacity(0.7))
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
        .confirmationDialog(
            "Delete All Data?",
            isPresented: $showDeleteConfirm,
            titleVisibility: .visible
        ) {
            Button("DELETE EVERYTHING", role: .destructive) { deleteAllWalletData() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This will permanently delete your private key, wallet database, transaction history, and all synced data. YOUR FUNDS WILL BE LOST if you don't have your mnemonic phrase.")
        }
        .sheet(isPresented: $showExportKey, onDismiss: clearExportedKey) {
            exportKeySheet
        }
        .sheet(isPresented: $showAuditReport) {
            securityAuditSheet
        }
        .sheet(isPresented: $showWifImport) {
            wifImportSheet
        }
    }

    // MARK: - WIF Import Sheet

    private var wifImportSheet: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HStack {
                        Text("IMPORT TRANSPARENT KEYS")
                            .font(ZFonts.title)
                            .foregroundColor(ZColors.primary)
                        Spacer()
                    }
                    .padding(.top, 24)

                    Text("Paste WIF private keys (one per line):")
                        .font(ZFonts.small)
                        .foregroundColor(ZColors.primaryDim)

                    TextEditor(text: $wifImportText)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(ZColors.primary)
                        .frame(minHeight: 100, maxHeight: 150)
                        .padding(4)
                        .overlay(
                            RoundedRectangle(cornerRadius: 4)
                                .stroke(ZColors.primaryDim.opacity(0.5), lineWidth: 1)
                        )
                        .scrollContentBackground(.hidden)

                    // Validate button
                    Button(action: validateWifKeys) {
                        Text("VALIDATE")
                            .font(ZFonts.heading)
                            .foregroundColor(ZColors.primary)
                            .padding(.horizontal, 24)
                            .padding(.vertical, 8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 4)
                                    .stroke(ZColors.primaryDim, lineWidth: 1)
                            )
                    }
                    .buttonStyle(.plain)

                    // Results
                    if let results = wifImportResults {
                        let validCount = results.filter { $0.valid }.count
                        ForEach(Array(results.enumerated()), id: \.offset) { _, item in
                            HStack(spacing: 4) {
                                Text(item.valid ? "[OK]" : "[X]")
                                    .font(.system(size: 10, design: .monospaced))
                                    .foregroundColor(item.valid ? ZColors.success : ZColors.error)
                                Text(item.prefix)
                                    .font(.system(size: 10, design: .monospaced))
                                    .foregroundColor(ZColors.primaryDim)
                                Text("-> \(item.address)")
                                    .font(.system(size: 10, design: .monospaced))
                                    .foregroundColor(item.valid ? ZColors.primary : ZColors.error)
                                    .lineLimit(1)
                            }
                        }

                        Text("WARNING: Imported keys are NOT covered by your recovery phrase. Back up WIF keys separately.")
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundColor(ZColors.warning)

                        if validCount > 0 {
                            Button(action: importValidWifKeys) {
                                Text("IMPORT \(validCount) KEY(S)")
                                    .font(ZFonts.heading)
                                    .foregroundColor(ZColors.primary)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 10)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 4)
                                            .stroke(ZColors.primary, lineWidth: 1)
                                    )
                            }
                            .buttonStyle(.plain)
                        }
                    }

                    Button(action: {
                        wifImportText = ""
                        wifImportResults = nil
                        showWifImport = false
                    }) {
                        Text("CLOSE")
                            .font(ZFonts.heading)
                            .foregroundColor(ZColors.primaryDim)
                            .padding(.horizontal, 48)
                            .padding(.vertical, 12)
                            .overlay(
                                RoundedRectangle(cornerRadius: 4)
                                    .stroke(ZColors.primaryDim.opacity(0.5), lineWidth: 1)
                            )
                    }
                    .buttonStyle(.plain)
                    .frame(maxWidth: .infinity, alignment: .center)
                }
                .padding(.horizontal, 24)
            }
        }
    }

    private func validateWifKeys() {
        let lines = wifImportText.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        let validationResults = ZipherXWrapper.validateWifKeys(lines)
        var results: [(valid: Bool, address: String, prefix: String)] = []
        for (i, r) in validationResults.enumerated() {
            let line = i < lines.count ? lines[i] : ""
            let prefix = line.count > 8 ? "\(String(line.prefix(8)))..." : line
            if r.valid {
                results.append((valid: true, address: r.address, prefix: prefix))
            } else {
                results.append((valid: false, address: r.errorMessage, prefix: prefix))
            }
        }
        wifImportResults = results
    }

    private func importValidWifKeys() {
        let lines = wifImportText.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
        let validationResults = ZipherXWrapper.validateWifKeys(lines)
        var encKeys: [[UInt8]] = []
        var addresses: [String] = []
        for (i, r) in validationResults.enumerated() {
            if r.valid && i < lines.count {
                // Store WIF bytes as the encrypted key placeholder
                encKeys.append(Array(lines[i].utf8))
                addresses.append(r.address)
            }
        }
        if !encKeys.isEmpty {
            let _ = ZipherXWrapper.importWifKeys(encryptedKeys: encKeys, addresses: addresses)
        }
        wifImportText = ""
        wifImportResults = nil
        showWifImport = false
    }

    // MARK: - Export Key Sheet

    @ViewBuilder
    private var exportKeySheet: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(spacing: 20) {
                // Header
                HStack {
                    Spacer()
                    Text("EXPORT PRIVATE KEY")
                        .font(ZFonts.title)
                        .foregroundColor(ZColors.warning)
                    Spacer()
                }
                .padding(.top, 24)

                VStack(alignment: .leading, spacing: 12) {
                    // Warning
                    HStack(spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundColor(ZColors.error)
                        Text("NEVER SHARE THIS KEY!")
                            .font(ZFonts.body)
                            .foregroundColor(ZColors.error)
                    }

                    Text("Anyone with this key can spend your funds.")
                        .font(ZFonts.small)
                        .foregroundColor(ZColors.error)

                    Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)

                    // Key display (truncated)
                    if let display = exportedKeyDisplay {
                        Text(display)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundColor(ZColors.primary)
                            .lineLimit(3)
                    } else {
                        Text("No private key loaded.")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDim)
                    }

                    // Copy button
                    if exportedKeyFull != nil {
                        Button(action: copyExportedKey) {
                            HStack(spacing: 6) {
                                Image(systemName: keyCopied ? "checkmark" : "doc.on.doc")
                                    .font(ZFonts.small)
                                Text(keyCopied ? "COPIED — auto-clears 30s" : "COPY TO CLIPBOARD")
                                    .font(ZFonts.caption)
                            }
                            .foregroundColor(keyCopied ? ZColors.success : ZColors.warning)
                            .padding(.horizontal, 16)
                            .padding(.vertical, 8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 4)
                                    .stroke((keyCopied ? ZColors.success : ZColors.warning).opacity(0.5), lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)

                        if keyCopied {
                            Text("Key copied to clipboard. Auto-clears after 30 seconds for security.")
                                .font(ZFonts.small)
                                .foregroundColor(ZColors.warning)
                        }
                    }
                }
                .padding(.horizontal, 24)

                Spacer()

                // Close button
                Button(action: { showExportKey = false }) {
                    Text("CLOSE")
                        .font(ZFonts.heading)
                        .foregroundColor(ZColors.primary)
                        .padding(.horizontal, 48)
                        .padding(.vertical, 12)
                        .overlay(
                            RoundedRectangle(cornerRadius: 4)
                                .stroke(ZColors.primaryDim, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .padding(.bottom, 40)
            }
        }
        .task {
            // KD-1: Auto-dismiss after 60 seconds to limit on-screen exposure
            try? await Task.sleep(nanoseconds: 60_000_000_000)
            showExportKey = false
        }
    }

    // MARK: - Security Audit Sheet

    @ViewBuilder
    private var securityAuditSheet: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(spacing: 20) {
                HStack {
                    Spacer()
                    Text("SECURITY AUDIT")
                        .font(ZFonts.title)
                        .foregroundColor(ZColors.primary)
                    Spacer()
                }
                .padding(.top, 24)

                ZCard {
                    VStack(spacing: 8) {
                        auditRow("Database encrypted", ok: AppleSecureStorage().hasKey(identifier: "db_encryption_key"))
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Private key secured", ok: AppleSecureStorage().hasKey(identifier: "spending_key"))
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Biometric available", ok: LAContext().canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: nil))
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Tor enabled", ok: viewModel.torEnabled)
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Peers connected", ok: viewModel.connectedPeers > 0)
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Screenshot protection", ok: viewModel.screenshotProtectionEnabled)
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        auditRow("Secure Enclave", ok: AppleSecureStorage().isHardwareBacked)
                    }
                }
                .padding(.horizontal, 16)

                ZCard {
                    VStack(spacing: 4) {
                        infoRow("PLATFORM", value: platformInfo)
                        Rectangle().fill(ZColors.primaryDim.opacity(0.3)).frame(height: 1)
                        infoRow("VERSION", value: appVersion)
                    }
                }
                .padding(.horizontal, 16)

                Spacer()

                Button(action: { showAuditReport = false }) {
                    Text("CLOSE")
                        .font(ZFonts.heading)
                        .foregroundColor(ZColors.primary)
                        .padding(.horizontal, 48)
                        .padding(.vertical, 12)
                        .overlay(
                            RoundedRectangle(cornerRadius: 4)
                                .stroke(ZColors.primaryDim, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .padding(.bottom, 40)
            }
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

    private func auditRow(_ label: String, ok: Bool) -> some View {
        HStack {
            Text(label)
                .font(ZFonts.caption)
                .foregroundColor(ZColors.primaryDim)
            Spacer()
            Text(ok ? "[OK]" : "[!!]")
                .font(ZFonts.mono)
                .foregroundColor(ok ? ZColors.success : ZColors.error)
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

    // MARK: - Export Private Key

    private func authenticateForExport() {
        // SA-22: Fresh LAContext per authentication request
        let ctx = LAContext()
        var error: NSError?
        if ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) {
            ctx.evaluatePolicy(
                .deviceOwnerAuthentication,
                localizedReason: "Authenticate to export your private key"
            ) { success, authError in
                DispatchQueue.main.async {
                    if success {
                        loadAndDisplayKey()
                    } else if let authError = authError {
                        let laErr = authError as? LAError
                        if laErr?.code != .userCancel && laErr?.code != .appCancel {
                            viewModel.errorMessage = "Authentication failed: \(authError.localizedDescription)"
                        }
                    }
                }
            }
        } else {
            viewModel.errorMessage = "No authentication method available. Please enable a device passcode or biometrics in Settings."
        }
    }

    // MARK: - Import WIF Keys

    private func authenticateForImport() {
        let ctx = LAContext()
        var error: NSError?
        if ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) {
            ctx.evaluatePolicy(
                .deviceOwnerAuthentication,
                localizedReason: "Authenticate to import keys"
            ) { success, authError in
                DispatchQueue.main.async {
                    if success {
                        showWifImport = true
                    } else if let authError = authError {
                        let laErr = authError as? LAError
                        if laErr?.code != .userCancel && laErr?.code != .appCancel {
                            viewModel.errorMessage = "Authentication failed: \(authError.localizedDescription)"
                        }
                    }
                }
            }
        } else {
            // Fallback: no biometric/passcode available — allow import
            showWifImport = true
        }
    }

    private func loadAndDisplayKey() {
        // SA-AUDIT: Zero spending key data after encoding
        var skData = ZipherXWrapper.loadSpendingKey()
        defer {
            let count = skData?.count ?? 0
            skData?.resetBytes(in: 0..<count)
        }
        guard let sk = skData, !sk.isEmpty else {
            viewModel.errorMessage = "No private key found. Please restore or import your wallet."
            return
        }

        do {
            let encoded = try ZipherXWrapper.encodeSpendingKey(Array(sk))
            exportedKeyFull = encoded
            // Truncated display for on-screen safety
            if encoded.count > 24 {
                let prefix = String(encoded.prefix(16))
                let suffix = String(encoded.suffix(8))
                exportedKeyDisplay = "\(prefix)...\(suffix)"
            } else {
                exportedKeyDisplay = encoded
            }
            showExportKey = true
        } catch {
            viewModel.errorMessage = "Failed to encode private key: \(error.localizedDescription)"
        }
    }

    private func copyExportedKey() {
        guard let key = exportedKeyFull else { return }
        #if os(iOS)
        UIPasteboard.general.string = key
        // M-23: Clear clipboard after 30 seconds
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if UIPasteboard.general.string == key {
                UIPasteboard.general.string = ""
            }
        }
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(key, forType: .string)
        let changeCount = NSPasteboard.general.changeCount
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if NSPasteboard.general.changeCount == changeCount {
                NSPasteboard.general.clearContents()
            }
        }
        #endif
        keyCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            keyCopied = false
        }
    }

    private func clearExportedKey() {
        exportedKeyFull = nil
        exportedKeyDisplay = nil
        keyCopied = false
    }

    // MARK: - Delete All Data

    private func authenticateForDelete() {
        // SA-22: Fresh LAContext per authentication request
        let ctx = LAContext()
        var error: NSError?
        if ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) {
            ctx.evaluatePolicy(
                .deviceOwnerAuthentication,
                localizedReason: "Authenticate to delete all wallet data"
            ) { success, authError in
                DispatchQueue.main.async {
                    if success {
                        showDeleteConfirm = true
                    } else if let authError = authError {
                        let laErr = authError as? LAError
                        if laErr?.code != .userCancel && laErr?.code != .appCancel {
                            viewModel.errorMessage = "Authentication failed: \(authError.localizedDescription)"
                        }
                    }
                }
            }
        } else {
            viewModel.errorMessage = "No authentication method available."
        }
    }

    private func deleteAllWalletData() {
        let storage = AppleSecureStorage()
        // Delete Keychain items
        _ = try? storage.deleteKey(identifier: "spending_key")
        _ = try? storage.deleteKey(identifier: "wallet_seed")
        _ = try? storage.deleteKey(identifier: "wallet_mnemonic")
        _ = try? storage.deleteKey(identifier: "db_encryption_key")
        _ = try? storage.deleteKey(identifier: "screenshot_protection")

        // Delete Application Support directory
        let appSupport: String
        #if os(macOS)
        appSupport = (NSSearchPathForDirectoriesInDomains(.applicationSupportDirectory, .userDomainMask, true).first ?? "~/Library/Application Support") + "/ZipherX_Multi"
        #else
        appSupport = (NSSearchPathForDirectoriesInDomains(.applicationSupportDirectory, .userDomainMask, true).first ?? NSHomeDirectory() + "/Library/Application Support") + "/ZipherX_Multi"
        #endif
        try? FileManager.default.removeItem(atPath: appSupport)

        // Exit app (same pattern as Desktop/Android)
        exit(0)
    }

    // MARK: - Maintenance Actions

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

    private var platformInfo: String {
        #if os(iOS)
        return "iOS \(UIDevice.current.systemVersion)"
        #elseif os(macOS)
        let v = ProcessInfo.processInfo.operatingSystemVersion
        return "macOS \(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
        #else
        return "Unknown"
        #endif
    }

    private var torStateLabel: String {
        let state = ZipherXWrapper.getTorState()
        switch state {
        case 0: return "DISCONNECTED"
        case 1: return "CONNECTING"
        case 2: return "BOOTSTRAPPING"
        case 3: return "CONNECTED"
        case 4: return "ERROR"
        default: return "UNKNOWN"
        }
    }

    private var torStateColor: Color {
        let state = ZipherXWrapper.getTorState()
        switch state {
        case 0: return ZColors.primaryDim
        case 1, 2: return ZColors.warning
        case 3: return ZColors.success
        case 4: return ZColors.error
        default: return ZColors.primaryDim
        }
    }

    private func syncPhaseLabel(_ phase: String) -> String {
        switch phase {
        case "boost_download": return "Downloading boost file..."
        case "boost_load":     return "Loading boost headers..."
        case "header_sync":    return "Syncing headers..."
        case "delta_sync":     return "Downloading outputs..."
        case "block_scan":     return "Scanning blocks..."
        case "witness_update": return "Updating witnesses..."
        case "starting":       return "Starting sync..."
        case "idle":           return "Sync idle."
        default:               return phase.replacingOccurrences(of: "_", with: " ").uppercased()
        }
    }
}
