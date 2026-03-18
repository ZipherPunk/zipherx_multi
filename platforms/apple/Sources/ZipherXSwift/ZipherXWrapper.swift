/// ZipherXWrapper.swift
/// ZipherXSwift
///
/// Thin, idiomatic Swift wrappers around the UniFFI-generated ZipherXFFI
/// module produced by the zipherx-ffi Rust crate.
///
/// All direct FFI calls are gated with `#if canImport(ZipherXFFI)`.
/// When the generated bindings are absent (e.g. simulator builds without
/// the Rust library) the `#else` branches throw `ZipherXError.ffiNotAvailable`
/// so that the rest of the Swift package can still compile and the UI can
/// display a meaningful diagnostic.

#if canImport(ZipherXFFI)
import ZipherXFFI
#endif

import Foundation

// MARK: - File-scope FFI helpers
// These call the UniFFI global functions from file scope, avoiding name
// collisions with ZipherXWrapper static methods that have the same names.
#if canImport(ZipherXFFI)
private func _ffiGenerateMnemonic() throws -> String { try generateMnemonic() }
private func _ffiValidateMnemonic(phrase: String) -> Bool { validateMnemonic(phrase: phrase) }
private func _ffiMnemonicToSeed(phrase: String) throws -> [UInt8] { try mnemonicToSeed(phrase: phrase) }
private func _ffiDeriveSpendingKey(seed: [UInt8], accountIndex: UInt32) throws -> [UInt8] { try deriveSpendingKey(seed: seed, accountIndex: accountIndex) }
private func _ffiDeriveAddress(skBytes: [UInt8], diversifierIndex: UInt64) throws -> String { try deriveAddress(skBytes: skBytes, diversifierIndex: diversifierIndex) }
private func _ffiValidateAddress(address: String) -> Bool { validateAddress(address: address) }
private func _ffiValidateTransparentAddress(address: String) -> Bool { validateTransparentAddress(address: address) }
private func _ffiInitializeWallet(config: WalletConfigFfi) throws { try initializeWallet(config: config) }
private func _ffiCreateWalletNew() throws -> [String] { try createWalletNew() }
private func _ffiRestoreWallet(words: [String]) throws { try restoreWallet(words: words) }
private func _ffiGetBalance() throws -> BalanceInfo { try getBalance() }
private func _ffiGetTransparentBalance() throws -> UInt64 { try getTransparentBalance() }
private func _ffiGetWalletSummary() throws -> WalletSummaryFfi { try getWalletSummary() }
private func _ffiGetTransactionHistory(limit: UInt32, offset: UInt32) throws -> [TransactionDisplayFfi] { try getTransactionHistory(limit: limit, offset: offset) }
private func _ffiGetTransactionCounts() throws -> TransactionCountsFfi { try getTransactionCounts() }
private func _ffiDecodeSpendingKey(encoded: String) throws -> [UInt8] { try decodeSpendingKey(encoded: encoded) }
private func _ffiEncodeSpendingKey(skBytes: [UInt8]) throws -> String { try encodeSpendingKey(skBytes: skBytes) }
private func _ffiImportWalletFromKey(skBytes: [UInt8]) throws { try importWalletFromKey(skBytes: skBytes) }
private func _ffiGetConnectedPeerCount() throws -> UInt32 { try getConnectedPeerCount() }
// Tor state queries (read-only — Tor lifecycle is managed in Rust)
private func _ffiGetOnionAddress() -> String? { getOnionAddress() }
private func _ffiGetTorState() -> UInt8 { getTorState() }
private func _ffiGetTorBootstrapProgress() -> UInt8 { getTorBootstrapProgress() }
private func _ffiSetTorEnabled(enabled: Bool) { setTorEnabled(enabled: enabled) }
private func _ffiIsTorEnabled() -> Bool { isTorEnabled() }
// Peer management
private func _ffiGetConnectedPeers() throws -> [ConnectedPeerInfoFfi] { try getConnectedPeers() }
private func _ffiGetBannedPeers() throws -> [BannedPeerInfoFfi] { try getBannedPeers() }
private func _ffiAddCustomPeer(host: String, port: UInt16) throws -> Bool { try addCustomPeer(host: host, port: port) }
private func _ffiUnbanPeer(host: String) throws -> Bool { try unbanPeer(host: host) }
private func _ffiDisconnectPeer(peerId: String) throws -> Bool { try disconnectPeer(peerId: peerId) }
// Funded transparent key export & WIF import
private func _ffiExportFundedTransparentWifs() throws -> [FundedTransparentKeyFfi] { try exportFundedTransparentWifs() }
private func _ffiValidateWifKeys(wifs: [String]) throws -> [WifValidationResultFfi] { try validateWifKeys(wifs: wifs) }
private func _ffiImportWifKeys(encryptedKeys: [[UInt8]], addresses: [String]) throws -> WifImportResultFfi { try importWifKeys(encryptedKeys: encryptedKeys, addresses: addresses) }
private func _ffiGetImportedKeyCount() throws -> UInt32 { try getImportedKeyCount() }
#endif

/// Keychain identifier for the spending key.
private let kSpendingKeyIdentifier = "spending_key"
/// Keychain identifier for the wallet seed (used for transparent address derivation).
private let kWalletSeedIdentifier = "wallet_seed"
/// Keychain identifier for the BIP39 mnemonic phrase (for recovery phrase export).
private let kWalletMnemonicIdentifier = "wallet_mnemonic"

// MARK: - Swift-native error type

/// Unified error type surfaced to Swift callers.
public enum ZipherXError: Error, LocalizedError {
    case ffiNotAvailable
    case cryptoError(String)
    case networkError(String)
    case storageError(String)
    case invalidInput(String)
    case insufficientBalance
    case walletLocked
    case notInitialized
    case syncInProgress
    case runtimeError(String)
    case broadcastFailed(String)
    case invalidAnchor
    case unknown(String)

    public var errorDescription: String? {
        switch self {
        case .ffiNotAvailable:
            return "ZipherXFFI bindings are not available in this build."
        case .cryptoError(let msg):   return "Crypto error: \(msg)"
        case .networkError(let msg):  return "Network error: \(msg)"
        case .storageError(let msg):  return "Storage error: \(msg)"
        case .invalidInput(let msg):  return "Invalid input: \(msg)"
        case .insufficientBalance:    return "Insufficient balance."
        case .walletLocked:           return "Wallet is locked."
        case .notInitialized:         return "Wallet is not initialized."
        case .syncInProgress:         return "Sync is in progress."
        case .runtimeError(let msg):  return "Runtime error: \(msg)"
        case .broadcastFailed(let msg): return "Broadcast failed: \(msg)"
        case .invalidAnchor:          return "Invalid anchor."
        case .unknown(let msg):       return "Unknown error: \(msg)"
        }
    }
}

// MARK: - Swift-native wrapper types

/// Wallet balance snapshot.
public struct Balance: Equatable {
    public let total: UInt64
    public let spendable: UInt64
    public let noteCount: UInt32
    public let spendableNoteCount: UInt32

    public init(total: UInt64, spendable: UInt64, noteCount: UInt32, spendableNoteCount: UInt32) {
        self.total = total
        self.spendable = spendable
        self.noteCount = noteCount
        self.spendableNoteCount = spendableNoteCount
    }
}

/// High-level wallet configuration passed to `initializeWallet`.
public struct WalletConfig {
    public let dbPath: String
    public let headerStorePath: String
    public let deltaStoreDir: String
    public let spendParamsPath: String
    public let outputParamsPath: String
    public let accountIndex: UInt32
    public let dbEncryptionKey: [UInt8]?
    public let boostCacheDir: String?

    public init(
        dbPath: String,
        headerStorePath: String,
        deltaStoreDir: String,
        spendParamsPath: String,
        outputParamsPath: String,
        accountIndex: UInt32 = 0,
        dbEncryptionKey: [UInt8]? = nil,
        boostCacheDir: String? = nil
    ) {
        self.dbPath = dbPath
        self.headerStorePath = headerStorePath
        self.deltaStoreDir = deltaStoreDir
        self.spendParamsPath = spendParamsPath
        self.outputParamsPath = outputParamsPath
        self.accountIndex = accountIndex
        self.dbEncryptionKey = dbEncryptionKey
        self.boostCacheDir = boostCacheDir
    }
}

/// Wallet state summary.
public struct WalletSummary {
    public let state: String
    public let address: String?
    public let totalBalance: UInt64
    public let spendableBalance: UInt64
    public let noteCount: UInt32
    public let lastSyncedHeight: UInt64
    public let chainTip: UInt64
    public let startupMode: String?
    public let syncPhase: String

    public init(
        state: String,
        address: String?,
        totalBalance: UInt64,
        spendableBalance: UInt64,
        noteCount: UInt32,
        lastSyncedHeight: UInt64,
        chainTip: UInt64,
        startupMode: String?,
        syncPhase: String
    ) {
        self.state = state
        self.address = address
        self.totalBalance = totalBalance
        self.spendableBalance = spendableBalance
        self.noteCount = noteCount
        self.lastSyncedHeight = lastSyncedHeight
        self.chainTip = chainTip
        self.startupMode = startupMode
        self.syncPhase = syncPhase
    }
}

/// A single entry from the transaction history.
public struct WalletTransaction: Identifiable {
    public let id: String
    public let txid: String
    public let txType: String
    public let amount: UInt64
    public let fee: UInt64
    public let address: String?
    public let memo: String?
    public let confirmations: UInt64
    public let height: UInt64
    public let timestamp: UInt64

    public init(
        txid: String,
        txType: String,
        amount: UInt64,
        fee: UInt64,
        address: String?,
        memo: String?,
        confirmations: UInt64,
        height: UInt64,
        timestamp: UInt64
    ) {
        self.id = "\(txid)_\(txType)"
        self.txid = txid
        self.txType = txType
        self.amount = amount
        self.fee = fee
        self.address = address
        self.memo = memo
        self.confirmations = confirmations
        self.height = height
        self.timestamp = timestamp
    }
}

/// Connected peer information for display.
public struct ConnectedPeerInfo: Identifiable {
    public let id: String
    public let address: String
    public let protocolVersion: UInt32
    public let userAgent: String
    public let startHeight: UInt32

    public init(address: String, protocolVersion: UInt32, userAgent: String, startHeight: UInt32) {
        self.id = address
        self.address = address
        self.protocolVersion = protocolVersion
        self.userAgent = userAgent
        self.startHeight = startHeight
    }
}

/// Banned peer information for display.
public struct BannedPeerInfo: Identifiable {
    public let id: String
    public let host: String
    public let reason: String
    public let isPermanent: Bool
    public let remainingSeconds: UInt64

    public init(host: String, reason: String, isPermanent: Bool, remainingSeconds: UInt64) {
        self.id = host
        self.host = host
        self.reason = reason
        self.isPermanent = isPermanent
        self.remainingSeconds = remainingSeconds
    }
}

// MARK: - ZipherXWrapper

/// Entry point for all ZipherX operations from Swift.
///
/// All methods are static; the underlying Rust runtime manages global state.
/// Call `initialize()` once at app launch, then `shutdown()` on termination.
///
/// SA-INFO-1: TODO — Consider implementing certificate pinning for any future
/// HTTPS endpoints (e.g., boost file downloads, price feeds) to prevent MITM attacks.
///
/// SA-INFO-2: TODO — Consider adding runtime integrity checks (code signing validation)
/// to detect tampering in release builds.
public enum ZipherXWrapper {

    // MARK: Runtime

    /// Initialize the global Tokio runtime inside the Rust FFI layer.
    /// Must be called once before any other method.
    public static func initialize() throws {
        #if canImport(ZipherXFFI)
        do {
            try initializeRuntime()
        } catch let e {
            throw ZipherXError.runtimeError(e.localizedDescription)
        }
        #else
        // FFI not available — no-op in stub mode.
        #endif
    }

    /// Shut down the global Tokio runtime.
    /// After calling this no FFI functions should be invoked.
    public static func shutdown() {
        #if canImport(ZipherXFFI)
        shutdownRuntime()
        #endif
    }

    // MARK: Mnemonic / Address

    /// Generate a fresh 24-word BIP39 mnemonic phrase.
    public static func generateMnemonic() throws -> String {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiGenerateMnemonic()
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Return `true` when `phrase` is a valid BIP39 mnemonic.
    public static func validateMnemonic(_ phrase: String) -> Bool {
        #if canImport(ZipherXFFI)
        return _ffiValidateMnemonic(phrase: phrase)
        #else
        return false
        #endif
    }

    /// Derive a shielded Zclassic address from `seed` at `accountIndex`.
    ///
    /// - Parameters:
    ///   - seed: 64-byte seed derived from BIP39 mnemonic.
    ///   - accountIndex: HD account index (usually 0).
    public static func deriveAddress(seed: Data, accountIndex: UInt32) throws -> String {
        #if canImport(ZipherXFFI)
        do {
            var skBytes = try _ffiDeriveSpendingKey(seed: Array(seed), accountIndex: accountIndex)
            defer {
                // H-16: Zero out sensitive key material after use
                skBytes.replaceSubrange(0..<skBytes.count, with: repeatElement(0, count: skBytes.count))
            }
            return try _ffiDeriveAddress(skBytes: skBytes, diversifierIndex: 0)
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Derive the shielded payment address from spending key bytes.
    public static func deriveAddressFromKey(_ skBytes: Data) throws -> String {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiDeriveAddress(skBytes: Array(skBytes), diversifierIndex: 0)
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Return `true` when `address` is a valid Zclassic shielded address.
    public static func validateAddress(_ address: String) -> Bool {
        #if canImport(ZipherXFFI)
        return _ffiValidateAddress(address: address)
        #else
        return false
        #endif
    }

    /// Return `true` when `address` is a valid Zclassic transparent address.
    public static func validateTransparentAddress(_ address: String) -> Bool {
        #if canImport(ZipherXFFI)
        return _ffiValidateTransparentAddress(address: address)
        #else
        return false
        #endif
    }

    // MARK: Wallet Lifecycle

    /// Build a default WalletConfig using standard Application Support paths.
    public static func defaultConfig() -> WalletConfig {
        let appSupport: String
        #if os(macOS)
        appSupport = (NSSearchPathForDirectoriesInDomains(.applicationSupportDirectory, .userDomainMask, true).first ?? "~/Library/Application Support") + "/ZipherX_Multi"
        #else
        appSupport = (NSSearchPathForDirectoriesInDomains(.applicationSupportDirectory, .userDomainMask, true).first ?? NSHomeDirectory() + "/Library/Application Support") + "/ZipherX_Multi"
        #endif

        // Ensure directory exists
        try? FileManager.default.createDirectory(atPath: appSupport, withIntermediateDirectories: true)

        // SA-2: Apply NSFileProtection so wallet data is encrypted at rest when device is locked
        #if os(iOS)
        try? FileManager.default.setAttributes(
            [.protectionKey: FileProtectionType.complete],
            ofItemAtPath: appSupport
        )
        #endif

        let paramsDir = appSupport + "/params"
        try? FileManager.default.createDirectory(atPath: paramsDir, withIntermediateDirectories: true)

        // Get or create a 32-byte DB encryption key from Keychain
        let dbKey = getOrCreateDbEncryptionKey()

        return WalletConfig(
            dbPath: appSupport + "/wallet.db",
            headerStorePath: appSupport + "/headers.db",
            deltaStoreDir: appSupport + "/delta",
            spendParamsPath: paramsDir + "/sapling-spend.params",
            outputParamsPath: paramsDir + "/sapling-output.params",
            accountIndex: 0,
            dbEncryptionKey: dbKey
        )
    }

    /// Get or create a 32-byte AES-256 encryption key for the SQLCipher database.
    /// Stored in iOS Keychain, separate from the spending key.
    ///
    /// **SA-8: Design decision** — The DB encryption key is intentionally stored
    /// *without* biometric gating (`requireUserPresence: false`). This is because
    /// background sync needs to read/write the database while the app is not in
    /// the foreground. If biometric authentication were required to load this key,
    /// background sync would fail whenever the user has not recently authenticated.
    /// The spending key, seed, and mnemonic are stored with
    /// `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly` + `.userPresence`
    /// (C-4) so the OS enforces biometric/passcode before every read.
    /// SA-AUDIT: The returned key array is stored in WalletConfig and passed to Rust FFI.
    /// We cannot zero the local copy before return since it IS the return value.
    /// The caller (defaultConfig) embeds it in WalletConfig which lives for the
    /// duration of the wallet session — this is an accepted limitation.
    private static func getOrCreateDbEncryptionKey() -> [UInt8]? {
        let storage = AppleSecureStorage()
        let identifier = "db_encryption_key"
        if let existing = try? storage.loadKey(identifier: identifier), existing.count == 32 {
            return Array(existing)
        }
        var key = [UInt8](repeating: 0, count: 32)
        let status = SecRandomCopyBytes(kSecRandomDefault, 32, &key)
        guard status == errSecSuccess else { return nil }
        // SA-AUDIT: Zero the Data copy used for Keychain storage
        var keyData = Data(key)
        defer { keyData.resetBytes(in: 0..<keyData.count) }
        try? storage.storeKey(identifier: identifier, data: keyData)
        return key
    }

    /// Convenience: initialize runtime + wallet with default config.
    /// Safe to call multiple times (both are idempotent).
    /// SA-1: Logs a warning if device appears compromised (jailbroken).
    public static func ensureInitialized() throws {
        // SA-1: Check for jailbreak/compromise on first init
        // SECURITY: Jailbreak detection logs a warning. Full enforcement (refusing to
        // initialize) is deferred to allow legitimate security researchers to test.
        // A future release will add a Settings toggle for "Allow Compromised Devices".
        // Blocking initialization would lock out jailbroken users who already have funds.
        if ApplePlatformInfo.isDeviceCompromised() {
            let logger = AppleLogger(subsystem: "com.zipherx.wallet", category: "security")
            logger.warning("Device appears compromised (jailbreak detected). Wallet security may be reduced.")
        }

        try initialize()
        try initializeWallet(config: defaultConfig())
    }

    // Tor is initialized in Rust (start_sync). Swift only reads state.

    /// Get the .onion address (nil if Tor not initialized).
    public static func getOnionAddress() -> String? {
        #if canImport(ZipherXFFI)
        return _ffiGetOnionAddress()
        #else
        return nil
        #endif
    }

    /// Get the Tor connection state (0=Disconnected, 3=Connected, etc).
    public static func getTorState() -> UInt8 {
        #if canImport(ZipherXFFI)
        return _ffiGetTorState()
        #else
        return 0
        #endif
    }

    /// Get Tor bootstrap progress (0-100).
    public static func getTorBootstrapProgress() -> UInt8 {
        #if canImport(ZipherXFFI)
        return _ffiGetTorBootstrapProgress()
        #else
        return 0
        #endif
    }

    /// Enable or disable Tor for P2P connections.
    /// Tor is disabled by default. Takes effect on next sync.
    public static func setTorEnabled(_ enabled: Bool) {
        #if canImport(ZipherXFFI)
        _ffiSetTorEnabled(enabled: enabled)
        #endif
    }

    /// Check whether Tor is currently enabled.
    public static func isTorEnabled() -> Bool {
        #if canImport(ZipherXFFI)
        return _ffiIsTorEnabled()
        #else
        return false
        #endif
    }

    /// Initialize the wallet with the provided configuration.
    /// Must be called after `initialize()`.
    public static func initializeWallet(config: WalletConfig) throws {
        #if canImport(ZipherXFFI)
        let ffiConfig = WalletConfigFfi(
            dbPath: config.dbPath,
            headerStorePath: config.headerStorePath,
            deltaStoreDir: config.deltaStoreDir,
            spendParamsPath: config.spendParamsPath,
            outputParamsPath: config.outputParamsPath,
            accountIndex: config.accountIndex,
            dbEncryptionKey: config.dbEncryptionKey,
            boostCacheDir: config.boostCacheDir
        )
        do {
            try _ffiInitializeWallet(config: ffiConfig)
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Create a new wallet and return its 24-word mnemonic.
    /// Also derives and stores the spending key in Keychain.
    public static func createWallet() throws -> [String] {
        #if canImport(ZipherXFFI)
        do {
            let words = try _ffiCreateWalletNew()
            // Derive spending key from mnemonic and persist in Keychain
            let phrase = words.joined(separator: " ")
            var seed = try _ffiMnemonicToSeed(phrase: phrase)
            var skBytes = try _ffiDeriveSpendingKey(seed: seed, accountIndex: 0)
            // SA-AUDIT: Zero the Data copy used for Keychain storage
            var keyData = Data(skBytes)
            var seedData = Data(seed)
            defer {
                // H-16: Zero out sensitive key material after use
                seed.replaceSubrange(0..<seed.count, with: repeatElement(0, count: seed.count))
                skBytes.replaceSubrange(0..<skBytes.count, with: repeatElement(0, count: skBytes.count))
                keyData.resetBytes(in: 0..<keyData.count)
                seedData.resetBytes(in: 0..<seedData.count)
            }
            try storeSpendingKey(keyData)
            try storeSeed(seedData)
            // Store mnemonic phrase for recovery phrase export (parity with Android)
            try storeMnemonic(phrase)
            return words
        } catch let e as ZipherXError {
            throw e
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Restore a wallet from an existing mnemonic word list.
    /// Also derives and stores the spending key in Keychain.
    public static func restoreWallet(words: [String]) throws {
        #if canImport(ZipherXFFI)
        do {
            try _ffiRestoreWallet(words: words)
            // Derive spending key from mnemonic and persist in Keychain
            let phrase = words.joined(separator: " ")
            var seed = try _ffiMnemonicToSeed(phrase: phrase)
            var skBytes = try _ffiDeriveSpendingKey(seed: seed, accountIndex: 0)
            // SA-AUDIT: Zero the Data copy used for Keychain storage
            var keyData = Data(skBytes)
            var seedData = Data(seed)
            defer {
                // H-16: Zero out sensitive key material after use
                seed.replaceSubrange(0..<seed.count, with: repeatElement(0, count: seed.count))
                skBytes.replaceSubrange(0..<skBytes.count, with: repeatElement(0, count: skBytes.count))
                keyData.resetBytes(in: 0..<keyData.count)
                seedData.resetBytes(in: 0..<seedData.count)
            }
            try storeSpendingKey(keyData)
            try storeSeed(seedData)
            // Store mnemonic phrase for recovery phrase export (parity with Android)
            try storeMnemonic(phrase)
        } catch let e as ZipherXError {
            throw e
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Import a wallet from a spending key string.
    ///
    /// Accepts two formats:
    /// - **Bech32**: `secret-extended-key-main1...`
    /// - **Hex**: exactly 338 hex characters (169 bytes)
    ///
    /// Validates by deriving the shielded address from the decoded key.
    public static func importSpendingKey(_ keyString: String) throws {
        let cleaned = keyString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleaned.isEmpty else {
            throw ZipherXError.invalidInput("Empty spending key.")
        }

        #if canImport(ZipherXFFI)
        var skBytes: [UInt8]

        if cleaned.lowercased().hasPrefix("secret-extended-key-main") {
            // Bech32 format
            do {
                skBytes = try _ffiDecodeSpendingKey(encoded: cleaned)
            } catch let e {
                throw ZipherXError.cryptoError("Invalid Bech32 key: \(e.localizedDescription)")
            }
        } else if cleaned.count == 338, cleaned.allSatisfy({ $0.isHexDigit }) {
            // 169-byte hex format
            skBytes = []
            var idx = cleaned.startIndex
            while idx < cleaned.endIndex {
                let next = cleaned.index(idx, offsetBy: 2)
                guard let byte = UInt8(cleaned[idx..<next], radix: 16) else {
                    throw ZipherXError.invalidInput("Invalid hex byte.")
                }
                skBytes.append(byte)
                idx = next
            }
        } else {
            throw ZipherXError.invalidInput(
                "Invalid key format. Expected Bech32 (secret-extended-key-main1...) or 338-character hex string."
            )
        }

        // H-16: Zero out sensitive key material when we're done
        defer {
            skBytes.replaceSubrange(0..<skBytes.count, with: repeatElement(0, count: skBytes.count))
        }

        // Validate key and initialize wallet with it
        do {
            try _ffiImportWalletFromKey(skBytes: skBytes)
            // Persist spending key in Keychain
            // SA-AUDIT: Zero the Data copy used for Keychain storage
            var keyData = Data(skBytes)
            defer { keyData.resetBytes(in: 0..<keyData.count) }
            try storeSpendingKey(keyData)
        } catch let e as ZipherXError {
            throw e
        } catch let e {
            throw ZipherXError.cryptoError("Invalid spending key: \(e.localizedDescription)")
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    // MARK: Network

    /// Get the number of currently connected P2P peers.
    public static func getConnectedPeerCount() -> UInt32 {
        #if canImport(ZipherXFFI)
        return (try? _ffiGetConnectedPeerCount()) ?? 0
        #else
        return 0
        #endif
    }

    /// Get list of currently connected peers with details.
    public static func getConnectedPeers() -> [ConnectedPeerInfo] {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiGetConnectedPeers().map { p in
                ConnectedPeerInfo(
                    address: p.address,
                    protocolVersion: p.protocolVersion,
                    userAgent: p.userAgent,
                    startHeight: p.startHeight
                )
            }
        } catch {
            return []
        }
        #else
        return []
        #endif
    }

    /// Get list of banned peers.
    public static func getBannedPeers() -> [BannedPeerInfo] {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiGetBannedPeers().map { p in
                BannedPeerInfo(
                    host: p.host,
                    reason: p.reason,
                    isPermanent: p.isPermanent,
                    remainingSeconds: p.remainingSeconds
                )
            }
        } catch {
            return []
        }
        #else
        return []
        #endif
    }

    /// Add a custom peer by IP address and port.
    public static func addCustomPeer(host: String, port: UInt16) -> Bool {
        let trimmed = host.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed.count <= 253 else { return false }
        #if canImport(ZipherXFFI)
        return (try? _ffiAddCustomPeer(host: trimmed, port: port)) ?? false
        #else
        return false
        #endif
    }

    /// Unban a peer by host address.
    public static func unbanPeer(host: String) -> Bool {
        #if canImport(ZipherXFFI)
        return (try? _ffiUnbanPeer(host: host)) ?? false
        #else
        return false
        #endif
    }

    /// Disconnect a peer by address/ID.
    public static func disconnectPeer(peerId: String) -> Bool {
        #if canImport(ZipherXFFI)
        return (try? _ffiDisconnectPeer(peerId: peerId)) ?? false
        #else
        return false
        #endif
    }

    // MARK: Key Encoding

    /// Encode spending key bytes to bech32 string for export display.
    public static func encodeSpendingKey(_ skBytes: [UInt8]) throws -> String {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiEncodeSpendingKey(skBytes: skBytes)
        } catch let e {
            throw ZipherXError.cryptoError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    // MARK: Balance and History

    /// Fetch the current balance from the local database.
    public static func getBalance() throws -> Balance {
        #if canImport(ZipherXFFI)
        do {
            let info = try _ffiGetBalance()
            return Balance(
                total: info.total,
                spendable: info.spendable,
                noteCount: info.noteCount,
                spendableNoteCount: info.spendableNoteCount
            )
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Fetch the transparent (t-address) balance in zatoshis.
    public static func getTransparentBalance() throws -> UInt64 {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiGetTransparentBalance()
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Fetch a high-level summary of the wallet state.
    public static func getSummary() throws -> WalletSummary {
        #if canImport(ZipherXFFI)
        do {
            let s = try _ffiGetWalletSummary()
            return WalletSummary(
                state: s.state,
                address: s.address,
                totalBalance: s.totalBalance,
                spendableBalance: s.spendableBalance,
                noteCount: s.noteCount,
                lastSyncedHeight: s.lastSyncedHeight,
                chainTip: s.chainTip,
                startupMode: s.startupMode,
                syncPhase: s.syncPhase
            )
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    // MARK: Spending Key Persistence

    /// Store the spending key in Keychain (overwrite if exists).
    /// C-4: Uses SecAccessControl with `.userPresence` so the OS requires
    /// biometric or passcode authentication before the key can be read.
    public static func storeSpendingKey(_ data: Data) throws {
        let storage = AppleSecureStorage()
        try storage.storeKey(identifier: kSpendingKeyIdentifier, data: data, requireUserPresence: true)
    }

    /// Load the spending key from Keychain.
    /// Returns `nil` if no key is stored.
    public static func loadSpendingKey() -> Data? {
        let storage = AppleSecureStorage()
        return try? storage.loadKey(identifier: kSpendingKeyIdentifier)
    }

    /// Store the wallet seed in Keychain for transparent address scanning.
    public static func storeSeed(_ data: Data) throws {
        let storage = AppleSecureStorage()
        try storage.storeKey(identifier: kWalletSeedIdentifier, data: data, requireUserPresence: true)
    }

    /// Load the wallet seed from Keychain.
    /// Returns `nil` if no seed is stored.
    public static func loadSeed() -> Data? {
        let storage = AppleSecureStorage()
        return try? storage.loadKey(identifier: kWalletSeedIdentifier)
    }

    /// Store the BIP39 mnemonic phrase in Keychain for recovery phrase export.
    /// Requires biometric/passcode authentication to read back (userPresence).
    public static func storeMnemonic(_ phrase: String) throws {
        let storage = AppleSecureStorage()
        var data = Data(phrase.utf8)
        defer { data.resetBytes(in: 0..<data.count) }
        try storage.storeKey(identifier: kWalletMnemonicIdentifier, data: data, requireUserPresence: true)
    }

    /// Load the BIP39 mnemonic phrase from Keychain.
    /// Returns `nil` if no mnemonic is stored (e.g., wallet imported from key).
    public static func loadMnemonic() -> String? {
        let storage = AppleSecureStorage()
        guard let data = try? storage.loadKey(identifier: kWalletMnemonicIdentifier), !data.isEmpty else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    /// Load a named key from Keychain.
    /// Returns `nil` if the key doesn't exist.
    public static func loadKey(_ identifier: String) -> Data? {
        let storage = AppleSecureStorage()
        return try? storage.loadKey(identifier: identifier)
    }

    // MARK: Balance and History

    /// Fetch paginated transaction history.
    ///
    /// - Parameters:
    ///   - limit:  Maximum number of records to return.
    ///   - offset: Number of records to skip (for pagination).
    public static func getHistory(limit: UInt32, offset: UInt32) throws -> [WalletTransaction] {
        #if canImport(ZipherXFFI)
        do {
            let records = try _ffiGetTransactionHistory(limit: limit, offset: offset)
            return records.map { r in
                WalletTransaction(
                    txid: r.txid,
                    txType: r.txType,
                    amount: r.amount,
                    fee: r.fee,
                    address: r.address,
                    memo: r.memo,
                    confirmations: r.confirmations,
                    height: r.height,
                    timestamp: r.timestamp
                )
            }
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    /// Get total IN (received) and OUT (sent) transaction counts.
    public static func getTransactionCounts() throws -> (sent: UInt32, received: UInt32) {
        #if canImport(ZipherXFFI)
        do {
            let counts = try _ffiGetTransactionCounts()
            return (sent: counts.sentCount, received: counts.receivedCount)
        } catch let e {
            throw ZipherXError.storageError(e.localizedDescription)
        }
        #else
        throw ZipherXError.ffiNotAvailable
        #endif
    }

    // MARK: Funded Transparent Key Export & WIF Import

    /// Export all funded transparent addresses with their WIF private keys.
    public static func exportFundedTransparentWifs() -> [(address: String, wif: String, balance: UInt64, isChange: Bool, isImported: Bool)] {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiExportFundedTransparentWifs().map { k in
                (address: k.address, wif: k.wif, balance: k.balance, isChange: k.isChange, isImported: k.isImported)
            }
        } catch {
            return []
        }
        #else
        return []
        #endif
    }

    /// Validate WIF private keys, returning validity and derived address for each.
    public static func validateWifKeys(_ wifs: [String]) -> [(valid: Bool, address: String, errorMessage: String)] {
        #if canImport(ZipherXFFI)
        do {
            return try _ffiValidateWifKeys(wifs: wifs).map { r in
                (valid: r.valid, address: r.address, errorMessage: r.errorMessage)
            }
        } catch {
            return []
        }
        #else
        return []
        #endif
    }

    /// Import WIF keys via encrypted key blobs and addresses.
    public static func importWifKeys(encryptedKeys: [[UInt8]], addresses: [String]) -> (imported: [String], errors: [(String, String)], duplicates: [String]) {
        #if canImport(ZipherXFFI)
        do {
            let result = try _ffiImportWifKeys(encryptedKeys: encryptedKeys, addresses: addresses)
            return (
                imported: result.imported.map { $0.address },
                errors: result.errors.map { ($0.address, $0.errorMessage) },
                duplicates: result.duplicates
            )
        } catch {
            return (imported: [], errors: [], duplicates: [])
        }
        #else
        return (imported: [], errors: [], duplicates: [])
        #endif
    }

    /// Get the number of imported transparent keys.
    public static func getImportedKeyCount() -> UInt32 {
        #if canImport(ZipherXFFI)
        return (try? _ffiGetImportedKeyCount()) ?? 0
        #else
        return 0
        #endif
    }
}
