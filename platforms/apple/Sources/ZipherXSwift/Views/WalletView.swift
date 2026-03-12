/// WalletView.swift
/// ZipherXSwift
///
/// Root wallet screen with Cypherpunk terminal design.
///
/// SA-28: TODO — All user-facing strings should be wrapped in `String(localized:)` or
/// `NSLocalizedString` for localization support. Currently all strings are hardcoded in English.

import SwiftUI

// MARK: - Cypherpunk Quotes

private let pendingSettlementMessages: [String] = [
    "Your proof floats in the mempool.\nMiners compete to etch it into the next block.\nPatience \u{2014} privacy takes time.",
    "The zero-knowledge proof is verified.\nNow the chain must seal it.\nNo one knows what you sent. Not even the miners.",
    "Cypherpunks wait for blocks, not banks.\nYour shielded TX is queued.\nThe math is done. The mining continues.",
    "Your transaction is invisible to surveillance.\nA miner will lock it into stone shortly.\nTrust the protocol.",
    "Mempool accepted. Block pending.\nThe network validates without seeing.\nThis is what financial privacy looks like.",
    "Shielded and waiting.\nNo address. No amount. No trace.\nJust a proof waiting for its block.",
]

private let cypherpunkQuotes: [String] = [
    // Eric Hughes - A Cypherpunk's Manifesto (1993)
    "\"Privacy is necessary for an open society in the electronic age.\" \u{2014} Eric Hughes",
    "\"Privacy is not secrecy. A private matter is something one doesn't want the whole world to know, but a secret matter is something one doesn't want anybody to know.\" \u{2014} Eric Hughes",
    "\"Privacy is the power to selectively reveal oneself to the world.\" \u{2014} Eric Hughes",
    "\"We must defend our own privacy if we expect to have any.\" \u{2014} Eric Hughes",
    "\"Cypherpunks write code.\" \u{2014} Eric Hughes",
    "\"We know that software can't be destroyed and that a widely dispersed system can't be shut down.\" \u{2014} Eric Hughes",
    "\"We the Cypherpunks are dedicated to building anonymous systems.\" \u{2014} Eric Hughes",
    // Timothy C. May
    "\"Just as the technology of printing altered and reduced the power of medieval guilds, so too will cryptologic methods fundamentally alter the nature of corporations and of government interference in economic transactions.\" \u{2014} Timothy C. May",
    // Satoshi Nakamoto
    "\"The root problem with conventional currency is all the trust that's required to make it work.\" \u{2014} Satoshi Nakamoto",
    "\"What is needed is an electronic payment system based on cryptographic proof instead of trust.\" \u{2014} Satoshi Nakamoto",
    "\"If you don't believe it or don't get it, I don't have the time to try to convince you, sorry.\" \u{2014} Satoshi Nakamoto",
    "\"I've been working on a new electronic cash system that's fully peer-to-peer, with no trusted third party.\" \u{2014} Satoshi Nakamoto",
    // Phil Zimmermann
    "\"If privacy is outlawed, only outlaws will have privacy.\" \u{2014} Phil Zimmermann",
    "\"Privacy is an inherent human right, and a requirement for maintaining the human condition with dignity and respect.\" \u{2014} Phil Zimmermann",
    // Julian Assange
    "\"Privacy for the weak, transparency for the powerful.\" \u{2014} Julian Assange",
    "\"Cryptography is the ultimate form of non-violent direct action.\" \u{2014} Julian Assange",
    // John Perry Barlow
    "\"Relying on the government to protect your privacy is like asking a peeping tom to install your window blinds.\" \u{2014} John Perry Barlow",
    // Bruce Schneier
    "\"Privacy is not something that I'm merely entitled to, it's an absolute prerequisite.\" \u{2014} Bruce Schneier",
    "\"Security is a process, not a product.\" \u{2014} Bruce Schneier",
    // Edward Snowden
    "\"Arguing that you don't care about the right to privacy because you have nothing to hide is no different than saying you don't care about free speech because you have nothing to say.\" \u{2014} Edward Snowden",
    "\"Privacy isn't about something to hide. Privacy is about something to protect.\" \u{2014} Edward Snowden",
    // Hal Finney
    "\"Running bitcoin.\" \u{2014} Hal Finney",
    // Nick Szabo
    "\"Trusted third parties are security holes.\" \u{2014} Nick Szabo",
    // Others
    "\"In a time of deceit, telling the truth is a revolutionary act.\" \u{2014} George Orwell",
    "\"Those who would give up essential Liberty, to purchase a little temporary Safety, deserve neither Liberty nor Safety.\" \u{2014} Benjamin Franklin",
    "\"The only way to deal with an unfree world is to become so absolutely free that your very existence is an act of rebellion.\" \u{2014} Albert Camus",
    // ZipherX
    "\"Zero-knowledge. Zero trust. Zero compromise.\" \u{2014} ZipherX",
    "\"Your keys, your coins. Your privacy, your right.\" \u{2014} ZipherX",
    "\"In math we trust.\" \u{2014} ZipherX",
]

@available(iOS 17, macOS 14, *)
public struct WalletView: View {

    @State private var viewModel = WalletViewModel()
    @State private var activeSheet: WalletSheet?
    @State private var peerTimer: Timer?
    @State private var showPendingWarning = false
    /// H-15: Obscure the view when app enters background/inactive (app switcher protection).
    @State private var isObscured = false
    @State private var currentQuote: String?
    @State private var showQuote = false
    @Environment(\.scenePhase) private var scenePhase

    public init() {}

    public var body: some View {
        ZStack(alignment: .top) {
            ZColors.terminalBlack.ignoresSafeArea()

            ScrollView {
                VStack(spacing: 16) {
                    // Menu bar
                    HStack {
                        HStack(spacing: 6) {
                            Image(systemName: "lock.shield.fill")
                                .font(ZFonts.heading)
                                .foregroundColor(ZColors.primary)
                                .shadow(color: ZColors.glow, radius: 3)
                            Text("ZIPHERX")
                                .font(ZFonts.title)
                                .foregroundColor(ZColors.primary)
                        }
                        .contentShape(Rectangle())
                        .onTapGesture {
                            currentQuote = cypherpunkQuotes.randomElement()
                            withAnimation(.easeIn(duration: 0.2)) {
                                showQuote = true
                            }
                        }
                        Spacer()
                        if viewModel.isSyncing {
                            Button(action: { viewModel.stopSync() }) {
                                HStack(spacing: 4) {
                                    Image(systemName: "stop.circle")
                                    Text("STOP")
                                }
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.warning)
                            }
                            .buttonStyle(.plain)
                        } else {
                            Button(action: { viewModel.startSync() }) {
                                HStack(spacing: 4) {
                                    Image(systemName: "arrow.clockwise")
                                    Text("SYNC")
                                }
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.primaryDark)
                            }
                            .buttonStyle(.plain)
                        }
                        Button(action: { activeSheet = .settings }) {
                            Image(systemName: "gear")
                                .font(ZFonts.heading)
                                .foregroundColor(ZColors.primaryDark)
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)

                    // Peer counter
                    HStack(spacing: 6) {
                        Image(systemName: "antenna.radiowaves.left.and.right")
                            .font(ZFonts.small)
                        Text("\(viewModel.connectedPeers) PEERS CONNECTED")
                            .font(ZFonts.small)
                        Spacer()
                    }
                    .foregroundColor(viewModel.connectedPeers > 0 ? ZColors.success : ZColors.error)
                    .padding(.horizontal, 16)

                    // Cypherpunk quote banner
                    if showQuote, let quote = currentQuote {
                        Text(quote)
                            .font(ZFonts.small)
                            .foregroundColor(ZColors.primaryDim)
                            .multilineTextAlignment(.center)
                            .padding(8)
                            .frame(maxWidth: .infinity)
                            .background(ZColors.terminalBlack)
                            .overlay(Rectangle().stroke(ZColors.primaryDim.opacity(0.4), lineWidth: 1))
                            .padding(.horizontal, 16)
                            .transition(.opacity.combined(with: .move(edge: .top)))
                    }

                    // Balance
                    BalanceView(
                        balance: viewModel.balance,
                        syncPhase: viewModel.syncPhase,
                        syncProgress: viewModel.syncProgress,
                        isSyncing: viewModel.isSyncing,
                        currentHeight: viewModel.syncCurrentHeight,
                        targetHeight: viewModel.syncTargetHeight,
                        syncSpeed: viewModel.syncSpeed,
                        syncETA: viewModel.syncETA,
                        connectedPeers: viewModel.connectedPeers,
                        pendingConfirmation: viewModel.pendingConfirmationTxid != nil
                    )
                    // SA-26: VoiceOver accessibility label for balance display
                    .accessibilityLabel("Shielded balance: \(String(format: "%.8f ZCL", Double(viewModel.balance?.total ?? 0) / 1e8))")
                    .padding(.horizontal, 16)

                    // Last transaction activity below balance
                    if let lastTx = viewModel.transactions.first {
                        LastTransactionActivityView(
                            transaction: lastTx,
                            mempoolAccepted: viewModel.mempoolAccepted,
                            mempoolPeerStatus: viewModel.mempoolPeerStatus
                        )
                        .padding(.horizontal, 16)
                    }

                    // Pending settlement banner with cypherpunk message
                    if viewModel.pendingConfirmationTxid != nil {
                        PendingSettlementBanner(
                            txid: viewModel.pendingConfirmationTxid ?? "",
                            peerStatus: viewModel.mempoolPeerStatus
                        )
                        .padding(.horizontal, 16)
                    }

                    // Action buttons
                    HStack(spacing: 12) {
                        ZButton(
                            viewModel.pendingConfirmationTxid != nil ? "Send [Locked]" : "Send",
                            icon: "paperplane.fill",
                            action: {
                                if viewModel.pendingConfirmationTxid != nil {
                                    showPendingWarning = true
                                } else {
                                    activeSheet = .send
                                }
                            }
                        )
                        .disabled(viewModel.isSending || viewModel.isSyncing || viewModel.pendingConfirmationTxid != nil)
                        // SA-26: VoiceOver accessibility label for send button
                        .accessibilityLabel(viewModel.pendingConfirmationTxid != nil ? "Send locked, awaiting confirmation" : "Send ZCL")
                        ZButton("Receive", icon: "qrcode", style: .secondary, action: { activeSheet = .receive })
                            // SA-26: VoiceOver accessibility label for receive button
                            .accessibilityLabel("Receive ZCL, show shielded address")
                    }
                    .padding(.horizontal, 16)

                    // Transaction history
                    TransactionHistoryView(
                        transactions: viewModel.transactions,
                        sentCount: viewModel.sentCount,
                        receivedCount: viewModel.receivedCount
                    )
                    .padding(.horizontal, 16)
                }
                .padding(.bottom, 16)
            }

            // TX confirmation toast — slides down when a pending TX gets first confirmation
            if viewModel.confirmationMessage != nil, viewModel.confirmedTxid != nil {
                ConfirmationToast(
                    icon: "cube.fill",
                    iconColor: ZColors.success,
                    title: "BLOCK CONFIRMED",
                    message: WalletViewModel.randomSettlementMessage()
                ) {
                    viewModel.dismissConfirmation()
                }
                .transition(.move(edge: .top).combined(with: .opacity))
                .padding(.top, 60)
                .zIndex(100)
            }

            // Mempool acceptance toast
            if viewModel.mempoolAccepted, viewModel.lastSentTxid != nil, activeSheet != .send {
                ConfirmationToast(
                    icon: "hourglass",
                    iconColor: ZColors.warning,
                    title: "MEMPOOL CLEARED",
                    message: WalletViewModel.randomClearingMessage()
                ) {
                    viewModel.clearSendStatus()
                }
                .transition(.move(edge: .top).combined(with: .opacity))
                .padding(.top, 60)
                .zIndex(99)
            }

            // Incoming TX toast
            if let incomingTx = viewModel.incomingTxNotification {
                let amount = Double(incomingTx.amount) / 1e8
                let incomingMessage = incomingTx.confirmations > 0
                    ? String(format: "[ +%.8f ZCL ]\n%@", amount, WalletViewModel.randomSettlementMessage())
                    : String(format: "[ +%.8f ZCL ]\n%@", amount, WalletViewModel.randomClearingMessage())
                ConfirmationToast(
                    icon: incomingTx.confirmations > 0 ? "lock.fill" : "arrow.down.left",
                    iconColor: incomingTx.confirmations > 0 ? ZColors.success : Color(hex: 0x00BCD4),
                    title: incomingTx.confirmations > 0 ? "BLOCK CONFIRMED" : "INCOMING TX",
                    message: incomingMessage
                ) {
                    viewModel.dismissIncomingNotification()
                }
                .transition(.move(edge: .top).combined(with: .opacity))
                .padding(.top, 60)
                .zIndex(98)
            }
        }
        // H-15: Privacy overlay when app enters background/inactive
        .overlay {
            if isObscured {
                ZStack {
                    ZColors.terminalBlack.ignoresSafeArea()
                    VStack(spacing: 12) {
                        Image(systemName: "lock.shield.fill")
                            .font(.system(size: 48))
                            .foregroundColor(ZColors.primary)
                            .shadow(color: ZColors.glow, radius: 6)
                        Text("ZIPHERX")
                            .font(.system(size: 24, weight: .bold, design: .monospaced))
                            .foregroundColor(ZColors.primary)
                        Text("WALLET LOCKED")
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundColor(ZColors.primaryDim)
                    }
                }
                .transition(.opacity)
            }
        }
        .onChange(of: scenePhase) { _, newPhase in
            // Only obscure when screenshot protection is enabled
            guard viewModel.screenshotProtectionEnabled else {
                isObscured = false
                return
            }
            switch newPhase {
            case .inactive, .background:
                withAnimation(.easeIn(duration: 0.1)) {
                    isObscured = true
                }
            case .active:
                withAnimation(.easeOut(duration: 0.2)) {
                    isObscured = false
                }
            @unknown default:
                break
            }
        }
        .animation(.spring(response: 0.4, dampingFraction: 0.8), value: viewModel.confirmedTxid != nil)
        .animation(.spring(response: 0.4, dampingFraction: 0.8), value: viewModel.mempoolAccepted)
        .animation(.spring(response: 0.4, dampingFraction: 0.8), value: viewModel.incomingTxNotification?.txid)
        .animation(.easeInOut(duration: 0.3), value: showQuote)
        .onChange(of: showQuote) { _, isShowing in
            if isShowing {
                // Auto-dismiss quote after 5 seconds
                DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                    withAnimation(.easeOut(duration: 0.3)) {
                        showQuote = false
                    }
                }
            }
        }
        .sheet(item: $activeSheet, onDismiss: {
            // Clear lastSentTxid so celebration doesn't replay on next send
            viewModel.lastSentTxid = nil
            // Refresh balance and history after returning from any sheet
            viewModel.refreshBalance()
            viewModel.refreshHistory()
        }) { sheet in
            sheetContent(for: sheet)
        }
        .alert("Error", isPresented: errorAlertBinding) {
            Button("OK", role: .cancel) {
                viewModel.errorMessage = nil
            }
        } message: {
            Text(viewModel.errorMessage ?? "An unknown error occurred.")
        }
        .alert("Send Locked", isPresented: $showPendingWarning) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("You have an unconfirmed transaction waiting for block confirmation. Sending is disabled until the previous transaction confirms.")
        }
        .task {
            viewModel.loadWallet()
            if viewModel.errorMessage == nil {
                viewModel.startSync()
            }
        }
        .onAppear {
            // Refresh peer count every 3 seconds
            peerTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { _ in
                DispatchQueue.main.async {
                    viewModel.connectedPeers = ZipherXWrapper.getConnectedPeerCount()
                }
            }
        }
        .onDisappear {
            peerTimer?.invalidate()
            peerTimer = nil
        }
    }

    @ViewBuilder
    private func sheetContent(for sheet: WalletSheet) -> some View {
        switch sheet {
        case .send:
            SendView(viewModel: viewModel)
        case .receive:
            ReceiveView(address: viewModel.walletAddress)
        case .settings:
            SettingsView(viewModel: viewModel)
        }
    }

    private var errorAlertBinding: Binding<Bool> {
        Binding(
            get: { viewModel.errorMessage != nil },
            set: { if !$0 { viewModel.errorMessage = nil } }
        )
    }
}

private enum WalletSheet: String, Identifiable {
    case send, receive, settings
    var id: String { rawValue }
}

// MARK: - Confirmation Toast

/// Slide-down notification shown for mempool acceptance, block confirmations, and incoming TXs.
/// Displays a cypherpunk message and auto-dismisses after 8 seconds.
@available(iOS 17, macOS 14, *)
struct ConfirmationToast: View {

    var icon: String = "cube.fill"
    var iconColor: Color = ZColors.success
    var title: String = "BLOCK CONFIRMED"
    let message: String
    let onDismiss: () -> Void

    @State private var showContent = false

    var body: some View {
        VStack(spacing: 8) {
            HStack(spacing: 10) {
                Image(systemName: icon)
                    .font(.system(size: 20))
                    .foregroundColor(iconColor)
                Text(title)
                    .font(ZFonts.caption)
                    .foregroundColor(iconColor)
                Spacer()
                Button(action: onDismiss) {
                    Image(systemName: "xmark")
                        .font(ZFonts.small)
                        .foregroundColor(ZColors.primaryDim)
                }
                .buttonStyle(.plain)
            }
            Text(message)
                .font(ZFonts.small)
                .foregroundColor(ZColors.primary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(ZColors.surface)
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(iconColor.opacity(0.4), lineWidth: 1)
                )
                .shadow(color: iconColor.opacity(0.2), radius: 8)
        )
        .padding(.horizontal, 16)
        .onAppear {
            withAnimation(.spring(response: 0.4, dampingFraction: 0.8)) {
                showContent = true
            }
            // Auto-dismiss after 8 seconds
            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                withAnimation(.easeOut(duration: 0.3)) {
                    onDismiss()
                }
            }
        }
    }
}

// MARK: - Pending Settlement Banner

/// Cypherpunk-themed banner shown while waiting for block confirmation after mempool acceptance.
@available(iOS 17, macOS 14, *)
struct PendingSettlementBanner: View {
    let txid: String
    let peerStatus: String?

    // Pick one random message and keep it stable for this view's lifetime
    @State private var message: String = pendingSettlementMessages.randomElement() ?? ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("[~]")
                    .font(.system(size: 12, weight: .bold, design: .monospaced))
                    .foregroundColor(ZColors.warning)
                Text("AWAITING SETTLEMENT")
                    .font(.system(size: 10, weight: .bold, design: .monospaced))
                    .foregroundColor(ZColors.warning)
            }
            Text(message)
                .font(.system(size: 10, design: .monospaced))
                .foregroundColor(ZColors.warning.opacity(0.8))
                .fixedSize(horizontal: false, vertical: true)
            if !txid.isEmpty {
                Text("tx: \(txid.prefix(16))...")
                    .font(.system(size: 8, design: .monospaced))
                    .foregroundColor(ZColors.primaryDim)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(RoundedRectangle(cornerRadius: 4).stroke(ZColors.warning, lineWidth: 1))
        .background(ZColors.warning.opacity(0.08))
    }
}

// MARK: - Last Transaction Activity

/// Shows the most recent transaction below the balance with live status.
@available(iOS 17, macOS 14, *)
struct LastTransactionActivityView: View {

    let transaction: WalletTransaction
    let mempoolAccepted: Bool
    let mempoolPeerStatus: String?

    private var isSent: Bool {
        transaction.txType == "sent" || transaction.txType.hasPrefix("alpha")
    }
    private var isReceived: Bool {
        transaction.txType == "received" || transaction.txType.hasPrefix("beta")
    }
    private var isSelf: Bool {
        transaction.txType == "self"
    }

    private var typeIcon: String {
        if isSelf { return "arrow.2.squarepath" }
        if isSent { return "arrow.up.right" }
        if isReceived { return "arrow.down.left" }
        return "questionmark.circle"
    }

    private var typeColor: Color {
        if isSelf { return ZColors.warning }
        if isSent { return ZColors.error }
        if isReceived { return ZColors.success }
        return ZColors.primaryDim
    }

    private var typeLabel: String {
        if isSelf { return "SELF" }
        if isSent { return "SENT" }
        if isReceived { return "RECEIVED" }
        return transaction.txType.uppercased()
    }

    private var statusText: String {
        if transaction.confirmations == 0 && mempoolAccepted {
            return "In mempool (\(mempoolPeerStatus ?? "?") peers) — waiting for miner"
        }
        if transaction.confirmations == 0 {
            return "Unconfirmed — broadcasting..."
        }
        if transaction.confirmations == 1 {
            return "1 confirmation"
        }
        return "\(transaction.confirmations) confirmations"
    }

    private var statusColor: Color {
        transaction.confirmations == 0 ? ZColors.warning : ZColors.success
    }

    var body: some View {
        guard isSent || isReceived || isSelf else { return AnyView(EmptyView()) }
        return AnyView(
            ZCard {
                HStack(spacing: 10) {
                    Image(systemName: typeIcon)
                        .font(ZFonts.heading)
                        .foregroundColor(typeColor)

                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 6) {
                            let prefix = isSent || isSelf ? "-" : "+"
                            let zcl = Double(transaction.amount) / 1e8
                            Text(String(format: "%@%.8f ZCL", prefix, zcl))
                                .font(ZFonts.mono)
                                .foregroundColor(typeColor)
                            Text(typeLabel)
                                .font(ZFonts.small)
                                .foregroundColor(typeColor.opacity(0.7))
                        }
                        HStack(spacing: 4) {
                            if transaction.confirmations == 0 {
                                Image(systemName: "hourglass")
                                    .font(.system(size: 10))
                                    .foregroundColor(statusColor)
                            }
                            Text(statusText)
                                .font(ZFonts.small)
                                .foregroundColor(statusColor)
                        }
                    }

                    Spacer()
                }
            }
        )
    }
}
