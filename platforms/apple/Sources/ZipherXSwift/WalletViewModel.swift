/// WalletViewModel.swift
/// ZipherXSwift
///
/// Observable view model that drives the ZipherX SwiftUI layer.
/// All mutable state is written on the main actor so views can
/// bind directly without additional dispatch.
///
/// Sync and send operations are delegated to the Rust FFI layer
/// via ZipherXWrapper.  Progress callbacks marshal updates back to
/// the main queue via DispatchQueue.main.async.

#if canImport(ZipherXFFI)
import ZipherXFFI
#endif

import Foundation
import os

/// SA-10: File-scope logger instance replacing NSLog for structured, privacy-aware logging.
private let logger = AppleLogger(subsystem: "com.zipherx.wallet", category: "viewmodel")

// File-scope helpers to avoid name collisions with instance methods.
#if canImport(ZipherXFFI)
private func _ffiStartSync(callback: SyncProgressCallback) throws { try startSync(callback: callback) }
private func _ffiStopSync() { stopSync() }
private func _ffiSendWithProgress(toAddress: String, amount: UInt64, fee: UInt64, memo: String?, skBytes: [UInt8], callback: SendProgressCallback) throws { try sendWithProgress(toAddress: toAddress, amount: amount, fee: fee, memo: memo, skBytes: skBytes, callback: callback) }
#endif

// MARK: - WalletViewModel

/// Main observable model for the ZipherX wallet UI.
///
/// Use `@State private var viewModel = WalletViewModel()` in SwiftUI
/// views (requires iOS 17 / macOS 14 for @Observable).
///
/// SA-25: TODO — Async work dispatched via `DispatchQueue.global().async` should
/// adopt structured concurrency (Swift `Task`) with proper cancellation handling
/// via `Task.checkCancellation()` or `Task.isCancelled` checks.
///
/// CH-4: TODO — Mixed async/sync dispatch patterns (DispatchQueue vs async/await)
/// should be unified to use Swift structured concurrency throughout.
///
/// SA-AUDIT: TODO — Migrate to `@MainActor` isolation for compile-time thread safety.
/// Currently relies on manual `DispatchQueue.main.async` dispatch. A full `@MainActor`
/// migration requires updating all call sites and callback patterns, deferred for now.
@available(iOS 17, macOS 14, *)
@Observable
public final class WalletViewModel {

    // MARK: Published state

    /// Latest balance snapshot, or `nil` if not yet loaded.
    public var balance: Balance?

    /// Ordered list of transactions, newest first.
    public var transactions: [WalletTransaction] = []

    /// Human-readable sync phase label (e.g. "header_sync", "delta_sync").
    public var syncPhase: String = "idle"

    /// Sync progress in [0, 1].  Derived from current/target heights.
    public var syncProgress: Double = 0.0

    /// Current sync height (for "x / y" display).
    public var syncCurrentHeight: UInt64 = 0

    /// Target sync height (for "x / y" display).
    public var syncTargetHeight: UInt64 = 0

    /// Wallet state string from the FFI summary (e.g. "Ready", "Syncing").
    public var walletState: String = "unknown"

    /// Non-nil when an error has occurred that should be shown in the UI.
    public var errorMessage: String?

    /// `true` while a background sync operation is running.
    public var isSyncing: Bool = false

    /// `true` while a send operation is in flight.
    public var isSending: Bool = false

    /// The last txid produced by a successful send, for confirmation display.
    public var lastSentTxid: String?

    /// Number of connected P2P peers.
    public var connectedPeers: UInt32 = 0

    /// Wallet shielded address (from getSummary).
    public var walletAddress: String?

    /// Tor .onion address for this wallet (always-on).
    public var onionAddress: String?

    /// Current sync speed in headers/sec (rolling average).
    public var syncSpeed: Double = 0.0

    /// Estimated time remaining for sync (seconds), or nil if not syncing.
    public var syncETA: TimeInterval?

    /// Total number of sent (OUT) transactions.
    public var sentCount: UInt32 = 0

    /// Total number of received (IN) transactions.
    public var receivedCount: UInt32 = 0

    /// TX lifecycle: set when the TX is accepted by peers in the mempool.
    public var mempoolAccepted: Bool = false

    /// TX lifecycle: peers that accepted the TX (e.g. "3/4").
    public var mempoolPeerStatus: String?

    /// TX lifecycle: set when a previously-pending TX gets its first block confirmation.
    /// Contains the txid of the confirmed transaction.
    public var confirmedTxid: String?

    /// TX lifecycle: the confirmation message to display (fun cypherpunk message).
    public var confirmationMessage: String?

    /// Tracks the txid of the most recent send, for confirmation detection across syncs.
    var pendingConfirmationTxid: String?

    /// Timestamp of the last send operation, for duration tracking.
    public var sendTimestamp: Date?

    /// Amount of the last send operation (zatoshis).
    public var sendAmount: UInt64 = 0

    /// Notification for a newly detected incoming transaction.
    public var incomingTxNotification: WalletTransaction?

    /// Whether Tor is enabled for P2P connections (disabled by default).
    public var torEnabled: Bool = false

    /// H-15: Whether screenshot/app-switcher protection is enabled (default: ON).
    /// Only active when the wallet view is showing, NOT during mnemonic creation.
    /// Stored in Keychain via AppleSecureStorage (not plain UserDefaults) to prevent
    /// tampering on jailbroken devices or backup extraction.
    public var screenshotProtectionEnabled: Bool = {
        let storage = AppleSecureStorage()
        if let data = try? storage.loadKey(identifier: "screenshot_protection"),
           let str = String(data: data, encoding: .utf8) {
            return str == "1"
        }
        return true // default ON
    }() {
        didSet {
            let storage = AppleSecureStorage()
            let value = Data((screenshotProtectionEnabled ? "1" : "0").utf8)
            try? storage.storeKey(value, identifier: "screenshot_protection")
        }
    }

    /// Track known txids so we can detect newly received ones after sync.
    var knownTxids: Set<String> = []

    /// Sync tracking for ETA computation.
    var syncStartTime: Date?
    var syncStartHeight: UInt64 = 0

    // MARK: Init

    public init() {}

    // MARK: - Load

    /// Initialize the runtime + wallet and load the initial state.
    ///
    /// Call this once from `.task { viewModel.loadWallet() }` in a SwiftUI view.
    /// Uses `ensureInitialized()` which does BOTH runtime init AND wallet init
    /// (opens databases, creates peer manager). This is critical when the app
    /// skips the setup screen because a spending key already exists in Keychain.
    public func loadWallet() {
        do {
            try ZipherXWrapper.ensureInitialized()
        } catch {
            errorMessage = error.localizedDescription
            return
        }

        // Tor is initialized in Rust (start_sync). Query .onion address if available.
        onionAddress = ZipherXWrapper.getOnionAddress()

        // Derive wallet address from spending key (for Receive view)
        // SA-AUDIT: Zero spending key data after address derivation
        if walletAddress == nil {
            var skData = ZipherXWrapper.loadSpendingKey()
            defer { skData?.resetBytes(in: 0..<(skData?.count ?? 0)) }
            if let sk = skData {
                do {
                    walletAddress = try ZipherXWrapper.deriveAddressFromKey(sk)
                } catch {
                    #if DEBUG
                    logger.debug("loadWallet: address derivation failed: \(error.localizedDescription)")
                    #endif
                }
            }
        }

        refreshBalance()
        refreshHistory()
    }

    // MARK: - Sync

    /// Start a background sync.  Progress is forwarded to the UI via callbacks.
    public func startSync() {
        guard !isSyncing else { return }
        isSyncing = true
        syncPhase = "starting"
        syncProgress = 0.0
        syncSpeed = 0.0
        syncETA = nil
        syncStartTime = nil
        syncStartHeight = 0
        errorMessage = nil

        #if canImport(ZipherXFFI)
        do {
            try _ffiStartSync(callback: SyncCallback(viewModel: self))
        } catch {
            DispatchQueue.main.async { [weak self] in
                self?.isSyncing = false
                self?.errorMessage = error.localizedDescription
            }
        }
        #else
        // FFI not linked — show error in non-FFI builds.
        DispatchQueue.global().asyncAfter(deadline: .now() + 1.5) { [weak self] in
            DispatchQueue.main.async {
                self?.syncPhase = "idle"
                self?.syncProgress = 1.0
                self?.isSyncing = false
            }
        }
        #endif
    }

    /// Cancel the current sync if one is running.
    public func stopSync() {
        guard isSyncing else { return }
        #if canImport(ZipherXFFI)
        _ffiStopSync()
        #endif
        isSyncing = false
        syncPhase = "idle"
    }

    // MARK: - Send

    /// Build and broadcast a shielded transaction.
    ///
    /// - Parameters:
    ///   - to:      Recipient shielded address.
    ///   - amount:  Amount in zatoshis.
    ///   - fee:     Miner fee in zatoshis.
    ///   - memo:    Optional UTF-8 memo (max 512 bytes).
    ///   - skBytes: Spending key bytes from Secure Enclave.
    public func send(to address: String, amount: UInt64, fee: UInt64, memo: String?, skBytes: Data) {
        guard !isSending else { return }
        isSending = true
        lastSentTxid = nil
        errorMessage = nil
        mempoolAccepted = false
        mempoolPeerStatus = nil
        sendTimestamp = Date()
        sendAmount = amount

        #if canImport(ZipherXFFI)
        do {
            // SA-AUDIT: Zero spending key bytes after FFI call
            var skArray = Array(skBytes)
            defer { skArray.replaceSubrange(0..<skArray.count, with: repeatElement(0, count: skArray.count)) }
            try _ffiSendWithProgress(
                toAddress: address,
                amount: amount,
                fee: fee,
                memo: memo,
                skBytes: skArray,
                callback: SendCallback(viewModel: self)
            )
        } catch {
            DispatchQueue.main.async { [weak self] in
                self?.isSending = false
                self?.errorMessage = error.localizedDescription
            }
        }
        #else
        DispatchQueue.global().asyncAfter(deadline: .now() + 2.0) { [weak self] in
            DispatchQueue.main.async {
                self?.isSending = false
                self?.errorMessage = "ZipherXFFI not available — send is a no-op in this build."
            }
        }
        #endif
    }

    // MARK: - Refresh helpers

    /// Reload the balance from the local database.
    public func refreshBalance() {
        #if DEBUG
        logger.debug("refreshBalance() called")
        #endif
        do {
            let b = try ZipherXWrapper.getBalance()
            balance = b
            #if DEBUG
            logger.debug("refreshBalance() -> total=\(b.total), spendable=\(b.spendable), notes=\(b.noteCount)")
            #endif
        } catch ZipherXError.ffiNotAvailable {
            #if DEBUG
            logger.debug("refreshBalance(): FFI not available")
            #endif
            balance = Balance(total: 0, spendable: 0, noteCount: 0, spendableNoteCount: 0)
        } catch {
            #if DEBUG
            logger.error("refreshBalance() error: \(error.localizedDescription)")
            #endif
            errorMessage = error.localizedDescription
        }

        do {
            let summary = try ZipherXWrapper.getSummary()
            walletState = summary.state
            syncPhase = summary.syncPhase
            if let addr = summary.address, !addr.isEmpty {
                walletAddress = addr
            }
        } catch {
            // Ignore — walletState keeps its previous value.
        }

        connectedPeers = ZipherXWrapper.getConnectedPeerCount()
    }

    /// Reload the transaction list from the local database.
    public func refreshHistory() {
        #if DEBUG
        logger.debug("refreshHistory() called")
        #endif
        do {
            let rawRecords = try ZipherXWrapper.getHistory(limit: 50, offset: 0)

            // Detect self-sends: txids that appear as both sent AND received
            let grouped = Dictionary(grouping: rawRecords) { $0.txid }
            var records: [WalletTransaction] = []
            var processedTxids: Set<String> = []

            for tx in rawRecords {
                guard !processedTxids.contains(tx.txid) else { continue }
                let group = grouped[tx.txid] ?? [tx]
                let hasSent = group.contains { $0.txType == "sent" || $0.txType.hasPrefix("alpha") }
                let hasReceived = group.contains { $0.txType == "received" || $0.txType.hasPrefix("beta") }

                if hasSent && hasReceived {
                    // Self-send: merge into a single "self" entry
                    // SA-AUDIT: guard-let instead of force unwrap
                    guard let sentTx = group.first(where: { $0.txType == "sent" || $0.txType.hasPrefix("alpha") }) else { continue }
                    records.append(WalletTransaction(
                        txid: sentTx.txid, txType: "self", amount: sentTx.amount,
                        fee: sentTx.fee, address: sentTx.address, memo: sentTx.memo,
                        confirmations: sentTx.confirmations, height: sentTx.height,
                        timestamp: sentTx.timestamp
                    ))
                    processedTxids.insert(tx.txid)
                } else {
                    records.append(tx)
                    processedTxids.insert(tx.txid)
                }
            }

            // Detect newly received transactions
            if !knownTxids.isEmpty {
                let newReceived = records.filter { tx in
                    !knownTxids.contains(tx.txid) &&
                    (tx.txType == "received" || tx.txType.hasPrefix("beta"))
                }
                if let first = newReceived.first {
                    #if DEBUG
                    logger.debug("Detected \(newReceived.count) new incoming TX(s)")
                    #endif
                    incomingTxNotification = first
                }
            }
            knownTxids = Set(records.map { $0.txid })

            transactions = records
            #if DEBUG
            logger.debug("refreshHistory() -> \(records.count) records")
            #endif
        } catch ZipherXError.ffiNotAvailable {
            #if DEBUG
            logger.debug("refreshHistory(): FFI not available")
            #endif
            transactions = []
        } catch {
            #if DEBUG
            logger.error("refreshHistory() error: \(error.localizedDescription)")
            #endif
            errorMessage = error.localizedDescription
        }

        // Also refresh IN/OUT counts
        do {
            let counts = try ZipherXWrapper.getTransactionCounts()
            sentCount = counts.sent
            receivedCount = counts.received
        } catch {
            // Non-fatal — counts are informational
        }
    }

    // MARK: - TX Lifecycle

    /// Check if a pending TX just got confirmed in a block.
    /// Called after each sync completes to detect first confirmation.
    func checkForTxConfirmation() {
        guard let pendingTxid = pendingConfirmationTxid else { return }

        // Look for ANY entry (sent or received) with this txid confirmed.
        // Self-sends produce both "sent" and "received" entries for the same txid.
        let hasConfirmation = transactions.contains { $0.txid == pendingTxid && $0.confirmations > 0 }
        if hasConfirmation {
            // First confirmation detected!
            var message = Self.randomCypherpunkMessage()
            if let ts = sendTimestamp {
                let elapsed = Int(Date().timeIntervalSince(ts))
                message += "\n\nBroadcast → Confirmed: \(Self.formatDuration(elapsed))"
            }
            confirmedTxid = pendingTxid
            confirmationMessage = message
            pendingConfirmationTxid = nil
            // Clear mempool status — TX is now confirmed in a block
            mempoolAccepted = false
            mempoolPeerStatus = nil
            let confs = transactions.first { $0.txid == pendingTxid && $0.confirmations > 0 }?.confirmations ?? 0
            #if DEBUG
            logger.info("TX CONFIRMED: \(pendingTxid) (\(confs) confirmations)")
            #endif
            // Post-settlement sync: new notes from the TX may lack witnesses.
            // One more sync rebuilds them so spendable count is correct.
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in
                self?.startSync()
            }
        }
    }

    /// Clear the send lifecycle state (called when user dismisses the celebration).
    public func clearSendStatus() {
        mempoolAccepted = false
        mempoolPeerStatus = nil
    }

    /// Dismiss the confirmation notification.
    public func dismissConfirmation() {
        confirmedTxid = nil
        confirmationMessage = nil
    }

    /// Dismiss the incoming TX notification.
    public func dismissIncomingNotification() {
        incomingTxNotification = nil
    }

    /// Enable or disable Tor for P2P connections.
    /// Tor is disabled by default. Takes effect on next sync.
    public func setTorEnabled(_ enabled: Bool) {
        torEnabled = enabled
        #if canImport(ZipherXFFI)
        ZipherXFFI.setTorEnabled(enabled: enabled)
        #endif
    }

    static func formatDuration(_ seconds: Int) -> String {
        if seconds < 60 { return "\(seconds)s" }
        if seconds < 3600 { return "\(seconds / 60)m \(seconds % 60)s" }
        return "\(seconds / 3600)h \((seconds % 3600) / 60)m"
    }

    /// Random cypherpunk confirmation messages.
    static func randomCypherpunkMessage() -> String {
        let messages = [
            "Block confirmed. Your transaction is now etched into the chain. Privacy preserved.",
            "The miners have spoken. Your shielded TX is sealed in cryptographic stone.",
            "Confirmed. Zero-knowledge proof verified. The cypherpunks write code.",
            "Block mined. Your privacy is mathematically guaranteed. Satoshi would be proud.",
            "TX confirmed on-chain. No surveillance. No middlemen. Just math.",
            "The blockchain has accepted your proof. Shielded. Private. Unstoppable.",
            "Confirmed. Your ZCL moved through the void, unseen by all. As intended.",
            "Block sealed. Another victory for financial privacy. The cypherpunks win again.",
        ]
        return messages.randomElement() ?? messages[0]
    }
}

// MARK: - SyncCallback

/// Concrete `SyncProgressCallback` that routes FFI callbacks back to the
/// view model on the main queue.
#if canImport(ZipherXFFI)
@available(iOS 17, macOS 14, *)
final class SyncCallback: SyncProgressCallback {

    private weak var viewModel: WalletViewModel?

    init(viewModel: WalletViewModel) {
        self.viewModel = viewModel
    }

    /// Called by Rust on each sync progress event.
    func onProgress(phase: String, current: UInt64, target: UInt64) {
        let progress: Double = target > 0 ? Double(current) / Double(target) : 0.0
        let peers = ZipherXWrapper.getConnectedPeerCount()
        // Tor init runs in Rust before sync — pick up .onion address
        let onion = ZipherXWrapper.getOnionAddress()
        DispatchQueue.main.async { [weak self] in
            guard let vm = self?.viewModel else { return }
            vm.syncPhase = phase
            vm.syncProgress = progress
            vm.syncCurrentHeight = current
            vm.syncTargetHeight = target
            vm.connectedPeers = peers
            if vm.onionAddress == nil, let addr = onion {
                vm.onionAddress = addr
            }

            // Compute ETA from elapsed time and progress.
            // Reset tracking when the phase changes (header_sync → delta_sync).
            if vm.syncStartTime == nil && current > 0 && target > 0 {
                vm.syncStartTime = Date()
                vm.syncStartHeight = current
            } else if current < vm.syncStartHeight {
                // Phase changed (e.g., delta_sync starts at a lower height) — reset
                vm.syncStartTime = Date()
                vm.syncStartHeight = current
            }
            if let startTime = vm.syncStartTime, target > current, current >= vm.syncStartHeight {
                let elapsed = Date().timeIntervalSince(startTime)
                let synced = current - vm.syncStartHeight
                if synced > 0 && elapsed > 1.0 {
                    let rate = Double(synced) / elapsed
                    let remaining = Double(target - current)
                    vm.syncSpeed = rate
                    vm.syncETA = remaining / rate
                }
            } else {
                vm.syncETA = nil
            }
        }
    }

    /// Called by Rust when sync completes successfully.
    ///
    /// Uses strong `self` capture because the Rust `Arc` drops immediately after
    /// this method returns (spawned task ends), which would deallocate the
    /// SyncCallback before the main-queue block executes.
    func onComplete(height: UInt64) {
        // Capture viewModel strongly NOW, before Rust drops us
        guard let vm = self.viewModel else {
            #if DEBUG
            logger.debug("onComplete: viewModel already nil, skipping")
            #endif
            return
        }
        #if DEBUG
        logger.debug("SyncCallback.onComplete(height=\(height))")
        #endif
        DispatchQueue.main.async {
            #if DEBUG
            logger.debug("onComplete: refreshing balance and history...")
            #endif
            vm.isSyncing = false
            vm.syncPhase = "idle"
            vm.syncProgress = 1.0
            vm.refreshBalance()
            vm.refreshHistory()
            #if DEBUG
            logger.debug("onComplete: balance=\(String(describing: vm.balance)), transactions=\(vm.transactions.count)")
            #endif

            // TX lifecycle: check if a pending TX just got its first confirmation
            vm.checkForTxConfirmation()

            // Continuous sync: re-sync after 60s to detect incoming transactions
            DispatchQueue.main.asyncAfter(deadline: .now() + 60) { [weak vm] in
                guard let vm = vm, !vm.isSyncing else { return }
                #if DEBUG
                logger.debug("Auto-resync: checking for new transactions...")
                #endif
                vm.startSync()
            }
        }
    }

    /// Called by Rust when sync encounters an error.
    func onError(message: String) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.isSyncing = false
            vm.syncPhase = "idle"
            vm.syncProgress = 0.0
            vm.syncETA = nil
            vm.errorMessage = message
        }
    }
}
#endif

// MARK: - SendCallback

/// Concrete `SendProgressCallback` that routes FFI callbacks back to the
/// view model on the main queue.
#if canImport(ZipherXFFI)
@available(iOS 17, macOS 14, *)
final class SendCallback: SendProgressCallback {

    private weak var viewModel: WalletViewModel?

    init(viewModel: WalletViewModel) {
        self.viewModel = viewModel
    }

    /// Called by Rust on each send phase transition.
    func onPhase(phase: String, current: UInt32, total: UInt32) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.syncPhase = phase

            // TX lifecycle: detect mempool acceptance from peer_response phase
            if phase == "peer_response" && current > 0 {
                vm.mempoolAccepted = true
                vm.mempoolPeerStatus = "\(current)/\(total)"
            }
        }
    }

    /// Called by Rust when the transaction has been broadcast successfully.
    func onComplete(txid: String, amount: UInt64, fee: UInt64) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.isSending = false
            vm.lastSentTxid = txid
            vm.mempoolAccepted = true
            vm.pendingConfirmationTxid = txid
            vm.refreshBalance()
            vm.refreshHistory()

            // Auto-sync periodically to catch block confirmation.
            // Zclassic block time is ~75s. Retry every 30s up to 6 times.
            for attempt in 1...6 {
                DispatchQueue.main.asyncAfter(deadline: .now() + Double(attempt * 30)) { [weak vm] in
                    guard let vm = vm, vm.pendingConfirmationTxid != nil else { return }
                    vm.startSync()
                }
            }
        }
    }

    /// Called by Rust when the send operation fails.
    func onError(message: String) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.isSending = false
            vm.mempoolAccepted = false
            vm.mempoolPeerStatus = nil
            vm.errorMessage = message
        }
    }
}
#endif
