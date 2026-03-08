import Foundation
import Security

// MARK: - Errors

public enum SecureStorageError: Error, LocalizedError {
    case itemNotFound(String)
    case unexpectedData(String)
    case unhandledError(OSStatus)

    public var errorDescription: String? {
        switch self {
        case .itemNotFound(let id):     return "Keychain item not found: \(id)"
        case .unexpectedData(let id):   return "Unexpected data type for item: \(id)"
        case .unhandledError(let s):    return "Keychain error: OSStatus \(s)"
        }
    }
}

// MARK: - AppleSecureStorage

/// Keychain-backed secure storage for cryptographic keys.
///
/// On devices with Secure Enclave, keys are protected with
/// `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`.
/// Falls back to standard Keychain on older hardware.
public final class AppleSecureStorage: @unchecked Sendable {

    private let service: String

    public init(service: String = "com.zipherx-multi.wallet") {
        self.service = service
    }

    // MARK: - Public API

    /// Persist `data` under `identifier`. Replaces any existing entry (upsert).
    ///
    /// When `identifier` is the spending key, an `SecAccessControl` with
    /// `.userPresence` is attached so that the OS requires biometric or
    /// passcode authentication before the item can be read.
    public func storeKey(identifier: String, data: Data, requireUserPresence: Bool = false) throws {
        // Delete any existing item first (upsert pattern).
        _ = try? deleteKey(identifier: identifier)

        var query = baseQuery(for: identifier)
        query[kSecValueData as String]  = data

        // C-4: Add SecAccessControl with .userPresence for spending key
        if requireUserPresence {
            var error: Unmanaged<CFError>?
            guard let accessControl = SecAccessControlCreateWithFlags(
                kCFAllocatorDefault,
                kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                .userPresence,
                &error
            ) else {
                // SA-AUDIT: Include the error description instead of swallowing it
                let desc = error.map { ($0.takeRetainedValue() as Error).localizedDescription } ?? "unknown"
                throw SecureStorageError.unexpectedData("SecAccessControl creation failed: \(desc)")
            }
            query[kSecAttrAccessControl as String] = accessControl
        } else {
            query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        }

        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw SecureStorageError.unhandledError(status)
        }
    }

    /// Retrieve the data stored under `identifier`.
    public func loadKey(identifier: String) throws -> Data {
        var query = baseQuery(for: identifier)
        query[kSecReturnData as String]  = kCFBooleanTrue
        query[kSecMatchLimit as String]  = kSecMatchLimitOne

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        switch status {
        case errSecSuccess:
            guard let data = result as? Data else {
                throw SecureStorageError.unexpectedData(identifier)
            }
            return data
        case errSecItemNotFound:
            throw SecureStorageError.itemNotFound(identifier)
        default:
            throw SecureStorageError.unhandledError(status)
        }
    }

    /// Remove the item stored under `identifier`.
    @discardableResult
    public func deleteKey(identifier: String) throws -> Bool {
        let status = SecItemDelete(baseQuery(for: identifier) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw SecureStorageError.unhandledError(status)
        }
        return status == errSecSuccess
    }

    /// Returns `true` if an item exists for `identifier`.
    public func hasKey(identifier: String) -> Bool {
        var query = baseQuery(for: identifier)
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        return SecItemCopyMatching(query as CFDictionary, nil) == errSecSuccess
    }

    /// `true` when Secure Enclave is available on this device.
    public var isHardwareBacked: Bool {
        #if targetEnvironment(simulator)
        return false
        #else
        var error: Unmanaged<CFError>?
        let access = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            .privateKeyUsage,
            &error
        )
        return access != nil && error == nil
        #endif
    }

    // MARK: - Private Helpers

    private func baseQuery(for identifier: String) -> [String: Any] {
        return [
            kSecClass as String:       kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: identifier
        ]
    }
}
