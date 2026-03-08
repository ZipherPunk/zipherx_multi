/// WalletSetupView.swift
/// ZipherXSwift
///
/// Onboarding screen — create, restore, or import a wallet before entering
/// the main wallet view. Matches the original ZipherX Cypherpunk design.

import SwiftUI

@available(iOS 17, macOS 14, *)
public struct WalletSetupView: View {

    enum SetupMode {
        case welcome
        case showMnemonic
        case restoreSeed
        case importKey
    }

    @State private var mode: SetupMode = .welcome
    @State private var mnemonicWords: [String] = []
    @State private var restoreInput: String = ""
    @State private var keyInput: String = ""
    @State private var errorMessage: String?
    @State private var isWorking = false
    @State private var seedWords: [String] = Array(repeating: "", count: 24)
    @FocusState private var focusedField: Int?


    var onWalletReady: () -> Void

    public init(onWalletReady: @escaping () -> Void) {
        self.onWalletReady = onWalletReady
    }

    public var body: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            switch mode {
            case .welcome:
                welcomeScreen
            case .showMnemonic:
                mnemonicDisplay
            case .restoreSeed:
                restoreForm
            case .importKey:
                importKeyForm
            }
        }
        .foregroundColor(ZColors.primary)
    }

    // MARK: - Welcome

    private var welcomeScreen: some View {
        VStack(spacing: 24) {
            Spacer()

            // Logo
            Image(systemName: "lock.shield.fill")
                .font(.system(size: 64))
                .foregroundColor(ZColors.primary)
                .shadow(color: ZColors.primary.opacity(0.5), radius: 10)

            Text("ZIPHERX")
                .font(.system(size: 32, weight: .bold, design: .monospaced))
                .foregroundColor(ZColors.primary)
                .shadow(color: ZColors.glow, radius: 5)

            Text("Privacy-First Cryptocurrency Wallet")
                .font(ZFonts.body)
                .foregroundColor(ZColors.primaryDark)

            Spacer()

            // Features
            VStack(alignment: .leading, spacing: 8) {
                featureRow("checkmark.shield.fill", "Shielded transactions (Sapling)")
                featureRow("network", "Peer-to-peer — no trusted servers")
                featureRow("key.fill", "Your keys, your coins")
                featureRow("eye.slash.fill", "Zero-knowledge proofs")
            }
            .padding(.horizontal, 32)

            Spacer()

            // Buttons
            VStack(spacing: 12) {
                ZButton("Create New Wallet", icon: "plus.circle.fill", action: createWallet)

                ZButton("Restore From Seed", icon: "arrow.counterclockwise", style: .secondary, action: {
                    errorMessage = nil
                    mode = .restoreSeed
                })

                ZButton("Import Private Key", icon: "key.fill", style: .secondary, action: {
                    errorMessage = nil
                    mode = .importKey
                })
            }
            .padding(.horizontal, 32)
            .disabled(isWorking)

            if isWorking {
                HStack(spacing: 8) {
                    ProgressView()
                        .tint(ZColors.primary)
                    Text("INITIALIZING...")
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.primaryDark)
                }
                .padding(.top, 8)
            }

            if let error = errorMessage {
                Text(error)
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.error)
                    .padding(.horizontal, 32)
            }

            Spacer()
        }
    }

    // MARK: - Mnemonic Display

    private var mnemonicDisplay: some View {
        VStack(spacing: 20) {
            Text("YOUR SEED PHRASE")
                .font(ZFonts.title)
                .foregroundColor(ZColors.primary)

            Text("Write these 24 words down and store them safely.\nThis is the ONLY way to recover your wallet.")
                .font(ZFonts.caption)
                .foregroundColor(ZColors.primaryDark)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            ZCard {
                LazyVGrid(columns: [
                    GridItem(.flexible()), GridItem(.flexible()), GridItem(.flexible())
                ], spacing: 8) {
                    ForEach(Array(mnemonicWords.enumerated()), id: \.offset) { idx, word in
                        HStack(spacing: 4) {
                            Text("\(idx + 1).")
                                .font(ZFonts.small)
                                .foregroundColor(ZColors.primaryDim)
                                .frame(width: 22, alignment: .trailing)
                            Text(word)
                                .font(ZFonts.mono)
                                .foregroundColor(ZColors.primary)
                            Spacer()
                        }
                        .padding(.vertical, 2)
                    }
                }
            }
            .padding(.horizontal, 24)

            HStack(spacing: 4) {
                Image(systemName: "exclamationmark.triangle")
                    .foregroundColor(ZColors.warning)
                Text("Never share your seed phrase with anyone.")
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.warning)
            }

            ZButton("I Have Saved My Seed Phrase", icon: "checkmark.circle.fill", action: {
                // H-14 + SA-11: Zero out mnemonic from view state, then clear array
                mnemonicWords = Array(repeating: "", count: mnemonicWords.count)
                mnemonicWords.removeAll()
                // Also clear any residual input fields
                restoreInput = ""
                keyInput = ""
                onWalletReady()
            })
            .padding(.horizontal, 32)
        }
    }

    // MARK: - Restore Seed Form

    private var restoreForm: some View {
        ScrollView {
            VStack(spacing: 16) {
                Text("RESTORE WALLET")
                    .font(ZFonts.title)
                    .foregroundColor(ZColors.primary)

                Text("Enter your 24-word seed phrase.")
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.primaryDark)
                    .multilineTextAlignment(.center)

                ZCard {
                    LazyVGrid(columns: [
                        GridItem(.flexible(), spacing: 6),
                        GridItem(.flexible(), spacing: 6),
                        GridItem(.flexible(), spacing: 6)
                    ], spacing: 6) {
                        ForEach(0..<24) { idx in
                            let i = idx
                            HStack(spacing: 3) {
                                Text("#\(i + 1)")
                                    .font(.system(size: 10, weight: .medium, design: .monospaced))
                                    .foregroundColor(ZColors.primaryDim)
                                    .frame(width: 24, alignment: .trailing)
                                TextField("", text: Binding(
                                    get: { seedWords[i] },
                                    set: { seedWords[i] = $0 }
                                ))
                                    .font(.system(size: 13, design: .monospaced))
                                    .foregroundColor(ZColors.primary)
                                    #if os(iOS)
                                    .textInputAutocapitalization(.never)
                                    #endif
                                    .autocorrectionDisabled()
                                    .focused($focusedField, equals: i)
                                    .padding(.horizontal, 4)
                                    .padding(.vertical, 3)
                                    .background(ZColors.terminalBlack)
                                    .overlay(
                                        RoundedRectangle(cornerRadius: 3)
                                            .stroke(focusedField == i ? ZColors.primary : ZColors.primaryDim.opacity(0.4), lineWidth: 1)
                                    )
                                    .onChange(of: seedWords[i]) { _, newValue in
                                        handleWordInput(index: i, value: newValue)
                                    }
                            }
                        }
                    }
                    .padding(4)
                }
                .padding(.horizontal, 16)

                if let error = errorMessage {
                    Text(error)
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.error)
                }

                HStack(spacing: 12) {
                    ZButton("Back", style: .secondary, action: {
                        mode = .welcome
                        errorMessage = nil
                        seedWords = Array(repeating: "", count: 24)
                        focusedField = nil
                    })
                    ZButton("Restore", icon: "arrow.counterclockwise", action: restoreWallet)
                }
                .padding(.horizontal, 32)
                .disabled(isWorking)

                if isWorking {
                    HStack(spacing: 8) {
                        ProgressView()
                            .tint(ZColors.primary)
                        Text("RESTORING...")
                            .font(ZFonts.caption)
                            .foregroundColor(ZColors.primaryDark)
                    }
                }
            }
            .padding(.vertical, 20)
        }
    }

    /// Detects multi-word paste and distributes across fields, or auto-advances focus on single word entry.
    private func handleWordInput(index: Int, value: String) {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        let parts = trimmed.split(separator: " ").map(String.init)

        if parts.count > 1 && parts.count <= 24 {
            // Multi-word paste detected — distribute starting from index 0 if 24 words, otherwise from current index
            let startIndex = (parts.count == 24) ? 0 : index
            for i in 0..<parts.count {
                let targetIndex = startIndex + i
                if targetIndex < 24 {
                    seedWords[targetIndex] = parts[i].lowercased()
                }
            }
            let nextFocus = min(startIndex + parts.count, 23)
            focusedField = nextFocus
        } else if !trimmed.isEmpty && !trimmed.contains(" ") {
            // Single word entered — auto-advance to next field
            if index < 23 {
                focusedField = index + 1
            }
        }
    }

    // MARK: - Import Private Key Form

    private var importKeyForm: some View {
        VStack(spacing: 20) {
            Text("IMPORT PRIVATE KEY")
                .font(ZFonts.title)
                .foregroundColor(ZColors.primary)

            Text("Paste your spending key below.\nBech32 (secret-extended-key-main1...) or hex format.")
                .font(ZFonts.caption)
                .foregroundColor(ZColors.primaryDark)
                .multilineTextAlignment(.center)

            ZCard {
                TextEditor(text: $keyInput)
                    .font(ZFonts.mono)
                    .foregroundColor(ZColors.primary)
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 80)
            }
            .padding(.horizontal, 24)

            HStack(spacing: 4) {
                Image(systemName: "exclamationmark.triangle")
                    .foregroundColor(ZColors.warning)
                Text("Never share your private key with anyone.")
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.warning)
            }

            if let error = errorMessage {
                Text(error)
                    .font(ZFonts.caption)
                    .foregroundColor(ZColors.error)
            }

            HStack(spacing: 12) {
                ZButton("Back", style: .secondary, action: {
                    mode = .welcome
                    errorMessage = nil
                })
                ZButton("Import", icon: "key.fill", action: importPrivateKey)
            }
            .padding(.horizontal, 32)
            .disabled(isWorking)

            if isWorking {
                HStack(spacing: 8) {
                    ProgressView()
                        .tint(ZColors.primary)
                    Text("IMPORTING...")
                        .font(ZFonts.caption)
                        .foregroundColor(ZColors.primaryDark)
                }
            }
        }
    }

    // MARK: - Helpers

    private func featureRow(_ icon: String, _ text: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(ZFonts.body)
                .foregroundColor(ZColors.primaryDark)
                .frame(width: 20)
            Text(text)
                .font(ZFonts.body)
                .foregroundColor(ZColors.primaryDark)
        }
    }

    private func createWallet() {
        isWorking = true
        errorMessage = nil
        DispatchQueue.global().async {
            do {
                try ZipherXWrapper.ensureInitialized()
                let words = try ZipherXWrapper.createWallet()
                DispatchQueue.main.async {
                    isWorking = false
                    mnemonicWords = words
                    mode = .showMnemonic
                }
            } catch {
                DispatchQueue.main.async {
                    isWorking = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }

    private func restoreWallet() {
        let words = seedWords.map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
        let filledWords = words.filter { !$0.isEmpty }

        guard filledWords.count == 24 else {
            errorMessage = "Expected 24 words, got \(filledWords.count)."
            return
        }

        isWorking = true
        errorMessage = nil
        DispatchQueue.global().async {
            do {
                try ZipherXWrapper.ensureInitialized()
                try ZipherXWrapper.restoreWallet(words: words)
                DispatchQueue.main.async {
                    isWorking = false
                    // SA-12: Clear sensitive input after successful restore
                    seedWords = Array(repeating: "", count: 24)
                    restoreInput = ""
                    keyInput = ""
                    onWalletReady()
                }
            } catch {
                DispatchQueue.main.async {
                    isWorking = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }

    private func importPrivateKey() {
        let key = keyInput.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !key.isEmpty else {
            errorMessage = "Enter a private spending key."
            return
        }

        isWorking = true
        errorMessage = nil
        DispatchQueue.global().async {
            do {
                try ZipherXWrapper.ensureInitialized()
                try ZipherXWrapper.importSpendingKey(key)
                DispatchQueue.main.async {
                    isWorking = false
                    // SA-12: Clear sensitive input after successful import
                    restoreInput = ""
                    keyInput = ""
                    onWalletReady()
                }
            } catch {
                DispatchQueue.main.async {
                    isWorking = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }
}
