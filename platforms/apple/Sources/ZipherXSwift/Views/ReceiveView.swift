/// ReceiveView.swift
/// ZipherXSwift
///
/// Shielded address display with Cypherpunk terminal design.

import SwiftUI

@available(iOS 16, macOS 13, *)
public struct ReceiveView: View {

    public let address: String?

    @State private var copied: Bool = false

    @Environment(\.dismiss) private var dismiss

    public init(address: String?) {
        self.address = address
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
                    Text("RECEIVE ZCL")
                        .font(ZFonts.title)
                        .foregroundColor(ZColors.primary)
                    Spacer()
                    // Spacer for symmetry
                    Text("CLOSE")
                        .font(ZFonts.caption)
                        .foregroundColor(.clear)
                }
                .padding(16)
                .background(ZColors.surface)
                .overlay(Rectangle().fill(ZColors.primaryDim).frame(height: 1), alignment: .bottom)

                ScrollView {
                    VStack(spacing: 24) {
                        Spacer().frame(height: 16)

                        // Shield icon
                        Image(systemName: "shield.checkered")
                            .font(.system(size: 48))
                            .foregroundColor(ZColors.primary)
                            .shadow(color: ZColors.glow, radius: 5)

                        Text("SHIELDED ADDRESS")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDark)

                        // Address display
                        if let addr = address {
                            ZCard {
                                VStack(spacing: 12) {
                                    Text(addr)
                                        .font(ZFonts.mono)
                                        .foregroundColor(ZColors.primary)
                                        .multilineTextAlignment(.center)
                                        .textSelection(.enabled)
                                        .lineSpacing(4)

                                    Rectangle()
                                        .fill(ZColors.primaryDim.opacity(0.3))
                                        .frame(height: 1)

                                    HStack(spacing: 4) {
                                        Image(systemName: "lock.fill")
                                            .font(ZFonts.small)
                                        Text("Sapling shielded (zs1...)")
                                            .font(ZFonts.small)
                                    }
                                    .foregroundColor(ZColors.primaryDim)
                                }
                            }
                            .padding(.horizontal, 16)

                            // Copy button
                            Button(action: { copyToClipboard(addr) }) {
                                HStack(spacing: 8) {
                                    Image(systemName: copied ? "checkmark" : "doc.on.doc")
                                        .font(ZFonts.body)
                                    // SA-5: Show auto-clear indicator after copy
                                    Text(copied ? "COPIED — auto-clears in 30s" : "COPY ADDRESS")
                                        .font(ZFonts.body)
                                }
                                .foregroundColor(copied ? ZColors.success : ZColors.terminalBlack)
                                .padding(.horizontal, 24)
                                .padding(.vertical, 12)
                                .frame(maxWidth: .infinity)
                                .background(copied ? ZColors.success.opacity(0.2) : ZColors.primary)
                                .overlay(Rectangle().stroke(copied ? ZColors.success : ZColors.primary, lineWidth: 1))
                                .shadow(color: (copied ? ZColors.success : ZColors.primary).opacity(0.3), radius: 2)
                            }
                            .buttonStyle(.plain)
                            .animation(.easeInOut(duration: 0.2), value: copied)
                            .padding(.horizontal, 16)

                        } else {
                            ZCard {
                                VStack(spacing: 8) {
                                    Image(systemName: "exclamationmark.triangle")
                                        .font(.system(size: 28))
                                        .foregroundColor(ZColors.warning)
                                    Text("Address not available.")
                                        .font(ZFonts.body)
                                        .foregroundColor(ZColors.primaryDark)
                                    Text("Initialize the wallet first.")
                                        .font(ZFonts.caption)
                                        .foregroundColor(ZColors.primaryDim)
                                }
                                .frame(maxWidth: .infinity)
                            }
                            .padding(.horizontal, 16)
                        }

                        // Security notice
                        HStack(spacing: 6) {
                            Image(systemName: "eye.slash.fill")
                                .font(ZFonts.small)
                            Text("Zero-knowledge shielded transactions protect your privacy.")
                                .font(ZFonts.small)
                        }
                        .foregroundColor(ZColors.primaryDim)
                        .padding(.horizontal, 16)
                    }
                }
            }
        }
    }

    private func copyToClipboard(_ text: String) {
        #if os(iOS)
        UIPasteboard.general.string = text
        // M-23: Clear clipboard after 30 seconds for privacy
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if UIPasteboard.general.string == text {
                UIPasteboard.general.string = ""
            }
        }
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        // M-23: Clear clipboard after 30 seconds for privacy
        let changeCount = NSPasteboard.general.changeCount
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            if NSPasteboard.general.changeCount == changeCount {
                NSPasteboard.general.clearContents()
            }
        }
        #endif
        copied = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            copied = false
        }
    }
}
