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
private func _ffiSetTorEnabled(enabled: Bool) { setTorEnabled(enabled: enabled) }
#endif

// MARK: - SyncTask Model

/// Status of an individual sync task.
enum SyncTaskStatus {
    case pending, inProgress, completed, failed
}

/// Represents a single phase of the sync pipeline (e.g. header_sync, delta_sync).
struct SyncTask: Identifiable {
    let id: String
    let title: String
    var status: SyncTaskStatus = .pending
    var detail: String? = nil
    var progress: Float? = nil
    var startTime: Date? = nil
    var endTime: Date? = nil
}

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
            try? storage.storeKey(identifier: "screenshot_protection", data: value)
        }
    }

    /// Track known txids so we can detect newly received ones after sync.
    var knownTxids: Set<String> = []

    /// Sync tracking for ETA computation.
    var syncStartTime: Date?
    var syncStartHeight: UInt64 = 0

    // MARK: Sync task tracking

    /// Per-task sync progress list (boost_download, boost_load, header_sync, delta_sync, block_scan, witness_update).
    public var syncTasks: [SyncTask] = []

    /// Weighted overall sync progress across all tasks [0, 1].
    public var overallProgress: Float = 0

    /// Timestamp when the current sync run began.
    public var syncRunStartTime: Date? = nil

    /// Last phase seen by the sync callback, for detecting transitions.
    var lastSyncPhase: String = ""

    // MARK: Celebration state (clearing/settlement)

    /// Clearing celebration message (mempool accepted).
    public var clearingCelebration: String? = nil

    /// Duration string for clearing (e.g. "3s").
    public var clearingDuration: String? = nil

    /// Settlement celebration message (block confirmed).
    public var settlementCelebration: String? = nil

    /// Duration string for settlement (e.g. "85s").
    public var settlementDuration: String? = nil

    /// Txid of the settled transaction for display.
    public var settlementTxid: String? = nil

    /// Timestamp when mempool first accepted the TX, for clearing duration.
    private var mempoolTimestamp: Date? = nil

    /// Snapshot of confirmed sent/self TX count at send time, for fallback detection.
    private var confirmedSentCountAtSend: Int = 0

    /// Whether a silent (background) sync is running.
    private var isSyncingSilent: Bool = false

    // MARK: Init

    public init() {}

    // MARK: - Storage Check

    /// Check if the device has enough free disk space for sync operations.
    /// First sync requires ~4 GB (boost download + header DB), subsequent syncs ~1 GB.
    /// Returns nil if space is sufficient, or an error message string if not.
    private func checkDiskSpace() -> String? {
        do {
            let homeDir = NSHomeDirectory()
            let attrs = try FileManager.default.attributesOfFileSystem(forPath: homeDir)
            guard let freeSpace = attrs[.systemFreeSize] as? Int64 else { return nil }
            let availableGB = Double(freeSpace) / (1024.0 * 1024.0 * 1024.0)

            // Check if this is first sync (no header DB yet)
            let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            let dataDir = appSupport?.appendingPathComponent("ZipherX_Multi")
            let headerDB = dataDir?.appendingPathComponent("headers.db")
            let isFirstSync = !(headerDB.map { FileManager.default.fileExists(atPath: $0.path) } ?? false)

            // First sync: 2.1 GB boost file + 1.5 GB header DB + 0.5 GB delta + 1 GB working = ~6 GB
            let requiredGB: Double = isFirstSync ? 6.0 : 1.0
            let requiredLabel = isFirstSync ? "6 GB" : "1 GB"

            if availableGB < requiredGB {
                return String(
                    format: "Insufficient storage: %.1f GB available, %@ required for %@. Free up space and try again.",
                    availableGB, requiredLabel,
                    isFirstSync ? "initial sync" : "sync"
                )
            }
            return nil
        } catch {
            return nil // Can't check — proceed and let sync fail with its own error if needed
        }
    }

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
            defer {
                let count = skData?.count ?? 0
                skData?.resetBytes(in: 0..<count)
            }
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
        // Check available disk space before starting sync
        if let storageError = checkDiskSpace() {
            errorMessage = storageError
            return
        }
        isSyncing = true
        syncPhase = "starting"
        syncProgress = 0.0
        syncSpeed = 0.0
        syncETA = nil
        syncStartTime = nil
        syncStartHeight = 0
        errorMessage = nil
        overallProgress = 0
        syncRunStartTime = Date()
        lastSyncPhase = ""

        // Initialize per-task tracking
        syncTasks = [
            SyncTask(id: "boost_download", title: "Downloading boost file"),
            SyncTask(id: "boost_load", title: "Loading boost headers"),
            SyncTask(id: "header_sync", title: "Syncing block headers"),
            SyncTask(id: "delta_sync", title: "Downloading shielded outputs"),
            SyncTask(id: "block_scan", title: "Scanning for transactions"),
            SyncTask(id: "witness_update", title: "Verifying witnesses"),
        ]

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
    /// Uses TWO strategies: exact txid match OR detecting a new confirmed sent/self TX
    /// that appeared since the send. The second strategy handles txid mismatches.
    func checkForTxConfirmation() {
        guard let pendingTxid = pendingConfirmationTxid else { return }

        // Strategy 1: exact txid match
        let matchedByTxid = transactions.contains { $0.txid == pendingTxid && $0.confirmations > 0 }
        // Strategy 2: count confirmed sent/self TXs — if more than at send time, our TX confirmed
        let currentConfirmedCount = transactions.filter {
            $0.confirmations > 0 && ($0.txType == "sent" || $0.txType == "self")
        }.count
        let matchedByCount = currentConfirmedCount > confirmedSentCountAtSend

        if matchedByTxid || matchedByCount {
            // Settlement detected — show celebration
            let elapsed: Int? = sendTimestamp.map { Int(Date().timeIntervalSince($0)) }
            let durationStr = elapsed.flatMap { $0 > 0 ? Self.formatDuration($0) : nil }
            // Find the confirmed TX (prefer exact match, fallback to newest)
            let confirmedTx = transactions.first { $0.txid == pendingTxid && $0.confirmations > 0 }
                ?? transactions.first { $0.confirmations > 0 && ($0.txType == "sent" || $0.txType == "self") }
            settlementTxid = confirmedTx?.txid ?? pendingTxid
            settlementCelebration = Self.randomSettlementMessage()
            settlementDuration = durationStr
            pendingConfirmationTxid = nil
            setPendingTxFastPoll(enabled: false)
            // Clear mempool status — TX is now confirmed in a block
            mempoolAccepted = false
            mempoolPeerStatus = nil
            // Also clear old-style confirmation state
            confirmedTxid = confirmedTx?.txid ?? pendingTxid
            confirmationMessage = settlementCelebration
            #if DEBUG
            let confs = confirmedTx?.confirmations ?? 0
            logger.info("TX SETTLED: \(settlementTxid ?? pendingTxid) (\(confs) confirmations)")
            #endif
            // Post-settlement: the FFI background loop will rebuild witnesses
            // on the next sync cycle (within 30s).
        }
    }

    /// Clear the send lifecycle state (called when user dismisses the celebration).
    public func clearSendStatus() {
        mempoolAccepted = false
        mempoolPeerStatus = nil
    }

    /// Dismiss the clearing (mempool) celebration.
    public func dismissClearing() {
        clearingCelebration = nil
        clearingDuration = nil
    }

    /// Dismiss the settlement (confirmation) celebration.
    public func dismissSettlement() {
        settlementCelebration = nil
        settlementDuration = nil
        settlementTxid = nil
    }

    /// Dismiss the confirmation notification (legacy, calls dismissSettlement).
    public func dismissConfirmation() {
        confirmedTxid = nil
        confirmationMessage = nil
        dismissSettlement()
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
        _ffiSetTorEnabled(enabled: enabled)
        #endif
    }

    // MARK: - Sync Task Helpers

    /// Mark a phase transition: complete the old phase, start the new one.
    /// Also marks any skipped phases (between old and new) as completed.
    func markPhaseTransition(from oldPhase: String, to newPhase: String) {
        let now = Date()
        let taskIds = syncTasks.map { $0.id }
        let newPhaseIdx = taskIds.firstIndex(of: newPhase) ?? -1

        syncTasks = syncTasks.enumerated().map { (idx, task) in
            var t = task
            if newPhaseIdx >= 0 && idx < newPhaseIdx && t.status != .completed {
                // Mark all phases BEFORE the new phase as completed (handles skipped phases)
                t.status = .completed
                t.progress = 1.0
                if t.startTime == nil { t.startTime = now }
                if t.endTime == nil { t.endTime = now }
            } else if t.id == newPhase {
                t.status = .inProgress
                t.startTime = now
            }
            return t
        }
        recalculateOverallProgress()
    }

    /// Update progress and detail text for a specific sync phase.
    func updateTaskProgress(phase: String, current: UInt64, target: UInt64, progress: Float) {
        let detail: String
        switch phase {
        case "boost_download":
            let mb = current / (1024 * 1024)
            let totalMb = target > 0 ? target / (1024 * 1024) : 0
            detail = totalMb > 0 ? "\(mb)MB / \(totalMb)MB" : "\(mb)MB downloaded"
        case "boost_load":
            detail = "\(current) / \(target) headers"
        case "header_sync":
            detail = "Height \(current) / \(target)"
        case "delta_sync":
            detail = "Height \(current) / \(target)"
        case "block_scan":
            detail = "Block \(current) / \(target)"
        case "witness_update":
            detail = "\(current) / \(target) notes"
        default:
            detail = target > 0 ? "\(current) / \(target)" : ""
        }
        syncTasks = syncTasks.map { task in
            if task.id == phase {
                var t = task
                t.progress = progress
                t.detail = detail
                return t
            }
            return task
        }
        recalculateOverallProgress()
    }

    /// Mark all sync tasks as completed.
    func markAllTasksCompleted() {
        let now = Date()
        syncTasks = syncTasks.map { task in
            if task.status != .completed {
                var t = task
                t.status = .completed
                t.progress = 1.0
                t.endTime = now
                return t
            }
            return task
        }
        overallProgress = 1.0
    }

    /// Mark the current task as failed with an error message.
    func markCurrentTaskFailed(phase: String, message: String) {
        syncTasks = syncTasks.map { task in
            if task.id == phase {
                var t = task
                t.status = .failed
                t.detail = message
                return t
            }
            return task
        }
    }

    /// Recalculate overall progress as a weighted average across all tasks.
    func recalculateOverallProgress() {
        guard !syncTasks.isEmpty else { return }
        let totalWeight = Float(syncTasks.count)
        var weighted: Float = 0
        for task in syncTasks {
            switch task.status {
            case .completed:
                weighted += 1.0
            case .inProgress:
                weighted += task.progress ?? 0
            default:
                break
            }
        }
        overallProgress = weighted / totalWeight
    }

    // MARK: - Silent Sync

    /// Silent sync -- updates DB confirmations without showing sync task bar UI.
    /// Used for background confirmation polling after send.
    #if canImport(ZipherXFFI)
    func syncSilent() {
        guard !isSyncing && !isSyncingSilent else { return }
        isSyncingSilent = true

        do {
            try _ffiStartSync(callback: SilentSyncCallback(viewModel: self))
        } catch {
            isSyncingSilent = false
        }
    }
    #else
    func syncSilent() {
        // FFI not linked — no-op
    }
    #endif

    // MARK: - Delete All Data (state reset)

    /// Reset all celebration and sync task state. File/Keychain deletion is handled
    /// by SettingsView.deleteAllWalletData().
    public func resetAllState() {
        isSyncing = false
        syncPhase = "idle"
        syncProgress = 0.0
        overallProgress = 0
        syncTasks = []
        balance = nil
        transactions = []
        mempoolAccepted = false
        mempoolPeerStatus = nil
        pendingConfirmationTxid = nil
        setPendingTxFastPoll(enabled: false)
        clearingCelebration = nil
        clearingDuration = nil
        settlementCelebration = nil
        settlementDuration = nil
        settlementTxid = nil
        confirmedTxid = nil
        confirmationMessage = nil
        errorMessage = nil
    }

    // MARK: - Duration & Messages

    static func formatDuration(_ seconds: Int) -> String {
        if seconds < 60 { return "\(seconds)s" }
        if seconds < 3600 { return "\(seconds / 60)m \(seconds % 60)s" }
        return "\(seconds / 3600)h \((seconds % 3600) / 60)m"
    }

    // Clearing messages (mempool accepted)
    private static let clearingMessages = [
        "Transaction accepted by the network mempool.\nYour zero-knowledge proof passed validation.",
        "Peers accepted your shielded transaction.\nWaiting for a miner to seal it into a block.",
        "Mempool cleared. Your TX is queued for the next block.\nThe network validates. Trust the math.",
        "Proof verified by peers. Transaction is in the mempool.\nNo identity revealed. Awaiting block inclusion.",
        "Network nodes accepted your transaction.\nShielded, validated, waiting for settlement.",
    ]

    // Settlement messages (block confirmed)
    private static let settlementMessages = [
        "Your transaction is now etched into the chain.\nPrivacy preserved. No trace left behind.",
        "The miners have spoken.\nYour shielded TX is sealed in cryptographic stone forever.",
        "Zero-knowledge proof verified.\nAnother private transaction joins the immutable ledger.",
        "Confirmation received.\nYour funds moved without leaving a trace.\nThe chain remembers. The world does not.",
        "Block mined. Cypherpunks write code.\nMiners write history.\nYour privacy is now permanent.",
        "Trust math, not middlemen.\nYour transaction is confirmed and irreversible.",
        "The proof is in the block.\nShielded, verified, sealed.\nThis is financial sovereignty.",
        "Another block, another victory for privacy.\nNo KYC. No surveillance. Just math.",
        "Your transaction joined the longest chain.\nCensorship-resistant. Permissionless. Private.",
        "Confirmed. The network accepted your proof.\nNo identity revealed. No trail to follow.",
    ]

    /// Random clearing (mempool) celebration message.
    static func randomClearingMessage() -> String {
        clearingMessages.randomElement() ?? clearingMessages[0]
    }

    /// Random settlement (block confirmed) celebration message.
    static func randomSettlementMessage() -> String {
        settlementMessages.randomElement() ?? settlementMessages[0]
    }

    /// Random cypherpunk confirmation messages (legacy, delegates to settlement).
    static func randomCypherpunkMessage() -> String {
        randomSettlementMessage()
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
        let phaseProgress: Float = target > 0 ? Float(current) / Float(target) : 0.0
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

            // Per-task tracking: detect phase transitions
            if phase != vm.lastSyncPhase {
                vm.markPhaseTransition(from: vm.lastSyncPhase, to: phase)
                vm.lastSyncPhase = phase
            }
            vm.updateTaskProgress(phase: phase, current: current, target: target, progress: phaseProgress)

            // Compute ETA from elapsed time and progress.
            // Reset tracking when the phase changes (header_sync -> delta_sync).
            if vm.syncStartTime == nil && current > 0 && target > 0 {
                vm.syncStartTime = Date()
                vm.syncStartHeight = current
            } else if current < vm.syncStartHeight {
                // Phase changed (e.g., delta_sync starts at a lower height) -- reset
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
            let wasInitialSync = vm.isSyncing
            #if DEBUG
            logger.debug("onComplete: refreshing balance and history... (initial=\(wasInitialSync))")
            #endif
            vm.isSyncing = false
            vm.syncPhase = "idle"
            vm.syncProgress = 1.0

            if wasInitialSync {
                // Initial sync done — mark tasks complete, then clear after delay
                vm.overallProgress = 1.0
                vm.markAllTasksCompleted()
                DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak vm] in
                    vm?.syncTasks = []
                    vm?.overallProgress = 0
                }
            }
            // Background syncs from the FFI 30s loop: no task UI needed.
            // The FFI already runs wallet.sync() every 30s and calls
            // onComplete when new blocks are found — no need to re-trigger
            // startSync() which would kill the background loop.

            vm.refreshBalance()
            vm.refreshHistory()
            #if DEBUG
            logger.debug("onComplete: balance=\(String(describing: vm.balance)), transactions=\(vm.transactions.count)")
            #endif

            // TX lifecycle: check if a pending TX just got its first confirmation
            vm.checkForTxConfirmation()
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
            vm.markCurrentTaskFailed(phase: vm.lastSyncPhase, message: message)
        }
    }

    /// Called by Rust when an incoming TX is detected in the mempool.
    func onMempoolTx(txid: String, amount: UInt64) {
        guard let vm = self.viewModel else { return }
        #if DEBUG
        logger.debug("Mempool TX detected: \(txid) (\(amount) zatoshis)")
        #endif
        // Skip change outputs from our own sends
        if txid == vm.pendingConfirmationTxid {
            #if DEBUG
            logger.debug("Mempool TX \(txid) is our own send (change output) — skipping")
            #endif
            return
        }
        DispatchQueue.main.async {
            let mempoolTx = Transaction(
                txid: txid,
                txType: "received",
                amount: Int64(amount),
                fee: 0,
                address: nil,
                memo: nil,
                confirmations: 0,
                height: 0,
                timestamp: UInt64(Date().timeIntervalSince1970)
            )
            vm.incomingTxNotification = mempoolTx
        }
    }
}
#endif

// MARK: - SilentSyncCallback

/// Sync callback for background confirmation polling — no UI task bar updates.
#if canImport(ZipherXFFI)
@available(iOS 17, macOS 14, *)
final class SilentSyncCallback: SyncProgressCallback {

    private weak var viewModel: WalletViewModel?

    init(viewModel: WalletViewModel) {
        self.viewModel = viewModel
    }

    func onProgress(phase: String, current: UInt64, target: UInt64) {
        // Silent — only update peer count
        let peers = ZipherXWrapper.getConnectedPeerCount()
        DispatchQueue.main.async { [weak self] in
            self?.viewModel?.connectedPeers = peers
        }
    }

    func onComplete(height: UInt64) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.isSyncingSilent = false
            vm.refreshBalance()
            vm.refreshHistory()
            vm.checkForTxConfirmation()
        }
    }

    func onError(message: String) {
        guard let vm = self.viewModel else { return }
        DispatchQueue.main.async {
            vm.isSyncingSilent = false
        }
    }

    func onMempoolTx(txid: String, amount: UInt64) {
        guard let vm = self.viewModel else { return }
        #if DEBUG
        vm.logger.debug("Mempool TX (silent): \(txid) (\(amount) zatoshis)")
        #endif
        // Skip change outputs from our own sends
        if txid == vm.pendingConfirmationTxid {
            #if DEBUG
            vm.logger.debug("Mempool TX \(txid) is our own send — skipping")
            #endif
            return
        }
        DispatchQueue.main.async {
            let mempoolTx = Transaction(
                txid: txid,
                txType: "received",
                amount: Int64(amount),
                fee: 0,
                address: nil,
                memo: nil,
                confirmations: 0,
                height: 0,
                timestamp: UInt64(Date().timeIntervalSince1970)
            )
            vm.incomingTxNotification = mempoolTx
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
                vm.mempoolTimestamp = Date()
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
            setPendingTxFastPoll(enabled: true)
            // Snapshot confirmed sent/self count for fallback detection
            vm.confirmedSentCountAtSend = vm.transactions.filter {
                $0.confirmations > 0 && ($0.txType == "sent" || $0.txType == "self")
            }.count
            // Show clearing (mempool) celebration
            let clearingElapsed = vm.mempoolTimestamp.map { Int(Date().timeIntervalSince($0)) }
            vm.clearingCelebration = WalletViewModel.randomClearingMessage()
            vm.clearingDuration = clearingElapsed.flatMap { $0 > 0 ? "\($0)s" : nil }
            // Don't refreshBalance() here — spent notes are already marked but change
            // note won't appear until the TX is mined, so balance would show 0.
            // Balance will update naturally when the first auto-sync completes.
            vm.refreshHistory()

            // The FFI background loop handles confirmation polling automatically:
            // setPendingTxFastPoll(true) switches it to 10s interval.
            // On each onComplete callback, checkForTxConfirmation() checks if
            // the pending TX got confirmed, and clears the fast poll flag.
            // Safety: auto-clear after 6 minutes if still pending.
            DispatchQueue.main.asyncAfter(deadline: .now() + 360) { [weak vm] in
                guard let vm = vm else { return }
                if vm.pendingConfirmationTxid != nil {
                    #if DEBUG
                    vm.logger.warning("Safety: auto-clearing pending TX after timeout")
                    #endif
                    vm.pendingConfirmationTxid = nil
                    setPendingTxFastPoll(enabled: false)
                    vm.mempoolAccepted = false
                    vm.mempoolPeerStatus = nil
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
