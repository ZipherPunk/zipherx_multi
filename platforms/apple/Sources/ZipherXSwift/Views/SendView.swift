/// SendView.swift
/// ZipherXSwift
///
/// Send-transaction form with Cypherpunk terminal design.
/// Shows a full-screen celebration overlay on successful broadcast.

import SwiftUI
import LocalAuthentication
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

@available(iOS 17, macOS 14, *)
public struct SendView: View {

    var viewModel: WalletViewModel

    @State private var recipientAddress: String = ""
    @State private var amountText: String = ""
    private let fixedFee: UInt64 = 10_000 // 0.0001 ZCL — fixed, not editable
    @State private var memo: String = ""
    @State private var addressValid: Bool = false
    @State private var showCelebration: Bool = false
    @State private var celebrationTxid: String = ""
    @State private var celebrationAmount: UInt64 = 0

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
                        Text("CANCEL")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDark)
                    }
                    .buttonStyle(.plain)
                    Spacer()
                    Text("SEND ZCL")
                        .font(ZFonts.title)
                        .foregroundColor(ZColors.primary)
                    Spacer()
                    Button(action: confirmSend) {
                        Text("SEND")
                            .font(ZFonts.caption)
                            .foregroundColor(canSend ? ZColors.primary : ZColors.primaryDim)
                    }
                    .buttonStyle(.plain)
                    .disabled(!canSend)
                    // SA-26: VoiceOver accessibility label for send button
                    .accessibilityLabel("Send ZCL transaction")
                }
                .padding(16)
                .background(ZColors.surface)
                .overlay(Rectangle().fill(ZColors.primaryDim).frame(height: 1), alignment: .bottom)

                ScrollView {
                    VStack(spacing: 16) {
                        // Recipient
                        VStack(alignment: .leading, spacing: 6) {
                            Text("RECIPIENT")
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.primaryDark)
                            ZTextField("Shielded address (zs1...)", text: $recipientAddress)
                                // SA-26: VoiceOver accessibility label for address field
                                .accessibilityLabel("Recipient shielded address")
                                .onChange(of: recipientAddress) { _, newValue in
                                    addressValid = ZipherXWrapper.validateAddress(newValue)
                                }
                            if !recipientAddress.isEmpty && !addressValid {
                                HStack(spacing: 4) {
                                    Image(systemName: "xmark.circle")
                                    Text("Invalid address")
                                }
                                .font(ZFonts.small)
                                .foregroundColor(ZColors.error)
                            }
                        }

                        // Amount
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Text("AMOUNT")
                                    .font(ZFonts.caption)
                                    .foregroundColor(ZColors.primaryDark)
                                Spacer()
                                Button(action: fillMaxAmount) {
                                    Text("MAX")
                                        .font(ZFonts.small)
                                        .foregroundColor(ZColors.primary)
                                        .padding(.horizontal, 8)
                                        .padding(.vertical, 2)
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 4)
                                                .stroke(ZColors.primaryDim, lineWidth: 1)
                                        )
                                }
                                .buttonStyle(.plain)
                                .disabled(viewModel.isSending)
                            }
                            ZTextField("ZCL (e.g. 1.5)", text: $amountText)
                                .onChange(of: amountText) { _, newValue in
                                    capAmountIfNeeded()
                                }
                        }

                        // Fee (fixed)
                        VStack(alignment: .leading, spacing: 6) {
                            Text("FEE")
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.primaryDark)
                            Text("0.0001 ZCL (10,000 zatoshis)")
                                .font(ZFonts.mono)
                                .foregroundColor(ZColors.primaryDim)
                                .padding(10)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(ZColors.terminalBlack)
                                .overlay(Rectangle().stroke(ZColors.primaryDim, lineWidth: 1))
                        }

                        // Memo
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Text("MEMO (OPTIONAL)")
                                    .font(ZFonts.caption)
                                    .foregroundColor(ZColors.primaryDark)
                                Spacer()
                                Text("\(memo.utf8.count)/512 bytes")
                                    .font(ZFonts.small)
                                    .foregroundColor(memo.utf8.count > 512 ? ZColors.error : ZColors.primaryDim)
                            }
                            ZTextField("Up to 512 bytes UTF-8", text: $memo)
                                // M-22: Truncate memo if it exceeds 512 bytes UTF-8
                                .onChange(of: memo) { _, newValue in
                                    if newValue.utf8.count > 512 {
                                        var truncated = newValue
                                        while truncated.utf8.count > 512 {
                                            truncated.removeLast()
                                        }
                                        memo = truncated
                                    }
                                }
                        }

                        // Progress
                        if viewModel.isSending {
                            ZCard {
                                HStack(spacing: 12) {
                                    ProgressView()
                                        .tint(ZColors.primary)
                                    Text(viewModel.syncPhase.replacingOccurrences(of: "_", with: " ").uppercased())
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.primaryDark)
                                }
                            }
                        }
                    }
                    .padding(16)
                }
            }

            // Full-screen celebration overlay
            if showCelebration {
                CelebrationOverlay(
                    txid: celebrationTxid,
                    amount: celebrationAmount,
                    mempoolAccepted: viewModel.mempoolAccepted,
                    peerStatus: viewModel.mempoolPeerStatus
                ) {
                    viewModel.clearSendStatus()
                    dismiss()
                }
                .transition(.opacity)
            }
        }
        .onChange(of: viewModel.lastSentTxid) { _, newTxid in
            guard let txid = newTxid, !txid.isEmpty else { return }
            celebrationTxid = txid
            celebrationAmount = parsedAmount
            withAnimation(.easeIn(duration: 0.3)) {
                showCelebration = true
            }
        }
    }

    private var canSend: Bool {
        addressValid && parsedAmount > 0 &&
        parsedAmount + fixedFee <= (viewModel.balance?.spendable ?? 0) &&
        !viewModel.isSending && !showCelebration
    }

    private var maxSpendable: UInt64 {
        let spendable = viewModel.balance?.spendable ?? 0
        return spendable > fixedFee ? spendable - fixedFee : 0
    }

    /// Parse ZCL amount string to zatoshis using integer-only arithmetic
    /// to avoid IEEE 754 floating-point precision loss (e.g., 0.29 * 1e8 != 29000000).
    private var parsedAmount: UInt64 {
        parseZclToZatoshis(amountText) ?? 0
    }

    private func parseZclToZatoshis(_ text: String) -> UInt64? {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return nil }
        let parts = trimmed.split(separator: ".", maxSplits: 2, omittingEmptySubsequences: false)
        guard parts.count <= 2 else { return nil }
        guard let whole = UInt64(parts[0]) else { return nil }
        let frac: UInt64
        if parts.count > 1 {
            let fracStr = String(parts[1]).padding(toLength: 8, withPad: "0", startingAt: 0).prefix(8)
            guard let f = UInt64(fracStr) else { return nil }
            frac = f
        } else {
            frac = 0
        }
        return whole.multipliedReportingOverflow(by: 100_000_000).overflow ? nil : whole * 100_000_000 + frac
    }

    private func fillMaxAmount() {
        let whole = maxSpendable / 100_000_000
        let frac = maxSpendable % 100_000_000
        amountText = frac == 0 ? "\(whole)" : String(format: "%d.%08d", whole, frac)
    }

    private func capAmountIfNeeded() {
        if parsedAmount > maxSpendable && maxSpendable > 0 {
            fillMaxAmount()
        }
    }

    private func confirmSend() {
        // C-3: Require biometric authentication before spending
        let bioAuth = AppleBiometricAuth()
        if bioAuth.isAvailable {
            DispatchQueue.global().async {
                do {
                    let authenticated = try bioAuth.authenticate(reason: "Authenticate to send ZCL")
                    DispatchQueue.main.async {
                        if authenticated {
                            performSend()
                        }
                        // User cancelled — do nothing
                    }
                } catch {
                    DispatchQueue.main.async {
                        viewModel.errorMessage = "Biometric authentication failed: \(error.localizedDescription)"
                    }
                }
            }
        } else {
            // RC-12: Biometrics unavailable — fall back to device passcode
            // SA-22: Fresh LAContext per authentication request — never reuse.
            let ctx = LAContext()
            var policyError: NSError?
            if ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &policyError) {
                DispatchQueue.global().async {
                    ctx.evaluatePolicy(
                        .deviceOwnerAuthentication,
                        localizedReason: "Authenticate to send ZCL"
                    ) { success, error in
                        DispatchQueue.main.async {
                            if success {
                                performSend()
                            } else if let error = error {
                                let laErr = error as? LAError
                                if laErr?.code != .userCancel && laErr?.code != .appCancel {
                                    viewModel.errorMessage = "Authentication failed: \(error.localizedDescription)"
                                }
                                // User cancelled — do nothing
                            }
                        }
                    }
                }
            } else {
                viewModel.errorMessage = "No authentication method available. Please enable a device passcode or biometrics in Settings."
            }
        }
    }

    private func performSend() {
        // SA-AUDIT: Zero spending key data after use
        var skData = ZipherXWrapper.loadSpendingKey()
        defer {
            let count = skData?.count ?? 0
            skData?.resetBytes(in: 0..<count)
        }
        guard let skBytes = skData, !skBytes.isEmpty else {
            viewModel.errorMessage = "Spending key not found. Please restore or import your wallet."
            return
        }
        viewModel.send(
            to: recipientAddress,
            amount: parsedAmount,
            fee: fixedFee,
            memo: memo.isEmpty ? nil : memo,
            skBytes: skBytes
        )
    }
}

// MARK: - Celebration Overlay

@available(iOS 17, macOS 14, *)
struct CelebrationOverlay: View {

    let txid: String
    let amount: UInt64
    let mempoolAccepted: Bool
    let peerStatus: String?
    let onDismiss: () -> Void

    @State private var showContent = false
    @State private var glowPulse = false
    @State private var txidCopied = false

    var body: some View {
        ZStack {
            // Dark backdrop
            ZColors.terminalBlack.opacity(0.95)
                .ignoresSafeArea()

            VStack(spacing: 24) {
                Spacer()

                // Shield icon with glow
                Image(systemName: "checkmark.shield.fill")
                    .font(.system(size: 64))
                    .foregroundColor(ZColors.success)
                    .shadow(color: ZColors.success.opacity(glowPulse ? 0.8 : 0.3), radius: glowPulse ? 20 : 8)
                    .scaleEffect(showContent ? 1.0 : 0.3)
                    .opacity(showContent ? 1.0 : 0.0)

                // Title
                Text("TRANSACTION SENT")
                    .font(ZFonts.title)
                    .foregroundColor(ZColors.success)
                    .opacity(showContent ? 1.0 : 0.0)

                // Amount
                Text(formatZCL(amount))
                    .font(ZFonts.balance)
                    .foregroundColor(ZColors.primary)
                    .shadow(color: ZColors.glow, radius: 4)
                    .opacity(showContent ? 1.0 : 0.0)

                // TXID card — L-17: Use copy button instead of text selection
                ZCard {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text("TXID")
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.primaryDark)
                            Spacer()
                            Button(action: { copyTxid(txid) }) {
                                HStack(spacing: 4) {
                                    Image(systemName: txidCopied ? "checkmark" : "doc.on.doc")
                                    // SA-5: Show auto-clear indicator after copy
                                    Text(txidCopied ? "COPIED — clears 30s" : "COPY")
                                }
                                .font(ZFonts.small)
                                .foregroundColor(txidCopied ? ZColors.success : ZColors.primaryDark)
                            }
                            .buttonStyle(.plain)
                        }
                        Text(txid)
                            .font(ZFonts.small)
                            .foregroundColor(ZColors.primary)
                            .lineLimit(2)
                    }
                }
                .padding(.horizontal, 24)
                .opacity(showContent ? 1.0 : 0.0)

                // Mempool status — dynamic
                if mempoolAccepted {
                    VStack(spacing: 6) {
                        HStack(spacing: 8) {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundColor(ZColors.success)
                            Text("MEMPOOL CLEARED")
                                .font(ZFonts.caption)
                                .foregroundColor(ZColors.success)
                            if let status = peerStatus {
                                Text("(\(status) peers)")
                                    .font(ZFonts.small)
                                    .foregroundColor(ZColors.primaryDim)
                            }
                        }
                        Text("Waiting for miners to seal it into a block...")
                            .font(ZFonts.small)
                            .foregroundColor(ZColors.primaryDim)
                    }
                    .opacity(showContent ? 1.0 : 0.0)
                } else {
                    Text("Broadcasting to network...")
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.primaryDim)
                        .opacity(showContent ? 1.0 : 0.0)
                }

                Spacer()

                // Dismiss button
                Button(action: onDismiss) {
                    Text("OK")
                        .font(ZFonts.heading)
                        .foregroundColor(ZColors.terminalBlack)
                        .padding(.horizontal, 48)
                        .padding(.vertical, 12)
                        .background(ZColors.success)
                        .shadow(color: ZColors.success.opacity(0.4), radius: 6)
                }
                .buttonStyle(.plain)
                .opacity(showContent ? 1.0 : 0.0)
                .padding(.bottom, 40)
            }
        }
        .onAppear {
            withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                showContent = true
            }
            withAnimation(.easeInOut(duration: 1.5).repeatForever(autoreverses: true)) {
                glowPulse = true
            }
        }
    }

    private func copyTxid(_ text: String) {
        #if os(iOS)
        UIPasteboard.general.string = text
        // M-23: Clear clipboard after 30 seconds
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if UIPasteboard.general.string == text {
                UIPasteboard.general.string = ""
            }
        }
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        let changeCount = NSPasteboard.general.changeCount
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if NSPasteboard.general.changeCount == changeCount {
                NSPasteboard.general.clearContents()
            }
        }
        #endif
        txidCopied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            txidCopied = false
        }
    }

    private func formatZCL(_ zatoshis: UInt64) -> String {
        let zcl = Double(zatoshis) / 1e8
        return String(format: "%.8f ZCL", zcl)
    }
}
