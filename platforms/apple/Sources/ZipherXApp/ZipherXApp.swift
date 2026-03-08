import SwiftUI
import LocalAuthentication

@main
struct ZipherXApp: App {

    @State private var walletReady: Bool
    @State private var walletLocked: Bool
    @Environment(\.scenePhase) private var scenePhase

    init() {
        // Register Apple platform services (Keychain bridge) before any wallet ops.
        registerPlatformServices()

        // Check if a spending key is already stored in Keychain.
        let hasExistingKey = AppleSecureStorage().hasKey(identifier: "spending_key")
        _walletReady = State(initialValue: hasExistingKey)
        // If wallet exists, start locked — require auth before showing wallet
        _walletLocked = State(initialValue: hasExistingKey)
    }

    var body: some Scene {
        WindowGroup {
            if #available(macOS 14, iOS 17, *) {
                if walletReady && walletLocked {
                    LockScreenView(onUnlocked: {
                        walletLocked = false
                    })
                    .frame(minWidth: 400, minHeight: 600)
                } else if walletReady {
                    WalletView()
                        .frame(minWidth: 400, minHeight: 600)
                } else {
                    WalletSetupView(onWalletReady: {
                        walletReady = true
                        walletLocked = false
                    })
                    .frame(minWidth: 400, minHeight: 600)
                }
            } else {
                Text("ZipherX requires macOS 14+ or iOS 17+")
                    .frame(minWidth: 400, minHeight: 600)
            }
        }
        #if os(macOS)
        .defaultSize(width: 480, height: 720)
        #endif
        .onChange(of: scenePhase) { oldPhase, newPhase in
            // Lock wallet when app goes to background (iOS) or resigns active (macOS)
            if newPhase == .background && walletReady {
                walletLocked = true
            }
            _ = oldPhase
        }
    }
}

// MARK: - Lock Screen

@available(iOS 17, macOS 14, *)
struct LockScreenView: View {
    var onUnlocked: () -> Void

    @State private var authFailed = false
    @State private var errorMessage: String?

    var body: some View {
        ZStack {
            ZColors.terminalBlack.ignoresSafeArea()

            VStack(spacing: 24) {
                Spacer()

                Image(systemName: "lock.shield.fill")
                    .font(.system(size: 64))
                    .foregroundColor(ZColors.primary)
                    .shadow(color: ZColors.primary.opacity(0.5), radius: 10)

                Text("ZIPHERX MULTI")
                    .font(.system(size: 28, weight: .bold, design: .monospaced))
                    .foregroundColor(ZColors.primary)
                    .shadow(color: ZColors.glow, radius: 5)

                Text("WALLET LOCKED")
                    .font(.system(size: 14, weight: .medium, design: .monospaced))
                    .foregroundColor(ZColors.primaryDark)

                Spacer()

                ZButton("Unlock", icon: "faceid", action: authenticate)
                    .padding(.horizontal, 32)

                if let error = errorMessage {
                    Text(error)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(ZColors.error)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 32)
                }

                Spacer()
            }
        }
        .foregroundColor(ZColors.primary)
        .onAppear {
            // Auto-prompt biometric on appear
            authenticate()
        }
    }

    private func authenticate() {
        errorMessage = nil
        // SA-22: Fresh LAContext per request, use deviceOwnerAuthentication
        // (biometric + passcode fallback) so the user can always unlock
        let ctx = LAContext()
        var evalError: NSError?
        // Use .deviceOwnerAuthentication — allows passcode fallback if biometrics fail
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &evalError) else {
            // No auth available at all — unlock anyway (no way to gate)
            onUnlocked()
            return
        }

        ctx.evaluatePolicy(
            .deviceOwnerAuthentication,
            localizedReason: "Unlock ZipherX Multi"
        ) { success, error in
            DispatchQueue.main.async {
                if success {
                    onUnlocked()
                } else if let err = error as? LAError, err.code == .userCancel {
                    errorMessage = "Authentication cancelled."
                } else {
                    errorMessage = "Authentication failed. Tap Unlock to try again."
                }
            }
        }
    }
}
