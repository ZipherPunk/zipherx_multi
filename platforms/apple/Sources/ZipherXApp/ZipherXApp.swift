import SwiftUI

@main
struct ZipherXApp: App {

    @State private var walletReady: Bool
    /// SA-24: Observe scenePhase at the app level for wallet lock on background.
    @Environment(\.scenePhase) private var scenePhase

    init() {
        // Register Apple platform services (Keychain bridge) before any wallet ops.
        registerPlatformServices()

        // Check if a spending key is already stored in Keychain.
        // If so, skip the setup screen and go straight to the wallet.
        // SA-AUDIT: Use hasKey() to avoid loading the full key into memory
        let hasExistingKey = AppleSecureStorage().hasKey(identifier: "spending_key")
        _walletReady = State(initialValue: hasExistingKey)
    }

    var body: some Scene {
        WindowGroup {
            if #available(macOS 14, iOS 17, *) {
                if walletReady {
                    WalletView()
                        .frame(minWidth: 400, minHeight: 600)
                } else {
                    WalletSetupView(onWalletReady: {
                        walletReady = true
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
        // SA-24: TODO — Implement wallet lock behavior when app goes to background.
        // Currently, WalletView handles privacy overlay via its own scenePhase observer.
        // A full lock (requiring re-authentication) should be implemented here by
        // setting a `walletLocked` state and presenting an authentication gate.
        .onChange(of: scenePhase) { oldPhase, newPhase in
            // SA-24: Placeholder for app-level background lock.
            // When `newPhase == .background`, the wallet should be locked.
            // When `newPhase == .active`, require re-authentication before unlocking.
            _ = (oldPhase, newPhase) // Suppress unused variable warnings
        }
    }
}
