#if canImport(ZipherXFFI)
import ZipherXFFI
#endif

import Foundation

// MARK: - UniFFI Callback Bridge

#if canImport(ZipherXFFI)
/// Bridges `AppleSecureStorage` to the UniFFI `PlatformStorageCallback` protocol
/// so Rust can perform key storage operations on Apple platforms.
///
/// Called once during wallet initialization; the registered instance is held
/// by the Rust FFI layer for the lifetime of the process.
public final class ApplePlatformStorageCallback: PlatformStorageCallback, @unchecked Sendable {

    private let storage: AppleSecureStorage

    public init(storage: AppleSecureStorage = AppleSecureStorage()) {
        self.storage = storage
    }

    // MARK: - PlatformStorageCallback

    /// Load a key from the Keychain. Returns `nil` when not found.
    /// SA-22: Each Keychain access uses the system's own authentication context;
    /// no LAContext is reused across evaluations here.
    public func loadKey(key: String) -> [UInt8]? {
        guard let data = try? storage.loadKey(identifier: key) else { return nil }
        return Array(data)
    }

    /// Persist `value` in the Keychain under `key`. Returns `true` on success.
    /// SA-7: Spending key requires biometric/passcode gating via requireUserPresence.
    /// SA-23: TODO — Consider adding `kSecAttrAccessControl` with `.applicationPassword`
    /// for spending key access to require an additional application-level PIN. Not
    /// implemented now as it requires UI changes for PIN entry flow.
    public func storeKey(key: String, value: [UInt8]) -> Bool {
        let isSpendingKey = key == "spending_key"
        return (try? storage.storeKey(identifier: key, data: Data(value), requireUserPresence: isSpendingKey)) != nil
    }

    /// Remove the item stored under `key`. Returns `true` if the item existed and was deleted.
    public func deleteKey(key: String) -> Bool {
        (try? storage.deleteKey(identifier: key)) == true
    }

    /// Returns `true` when an item is present for `key`.
    public func hasKey(key: String) -> Bool {
        storage.hasKey(identifier: key)
    }
}
#endif

// MARK: - Registration

/// Register Apple platform services with the Rust FFI layer.
///
/// Call this once at application startup, before any wallet operations.
/// Safe to call when `ZipherXFFI` is not linked (no-op in that case).
public func registerPlatformServices() {
    #if canImport(ZipherXFFI)
    let callback = ApplePlatformStorageCallback()
    setPlatformStorage(storage: callback)
    #endif
}
