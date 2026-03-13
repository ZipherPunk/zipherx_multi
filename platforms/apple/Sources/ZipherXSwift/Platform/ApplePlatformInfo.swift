import Foundation
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif
#if canImport(IOKit)
import IOKit
#endif

/// Platform information: file-system paths, device identity, OS metadata.
public final class ApplePlatformInfo: @unchecked Sendable {

    public init() {}

    // MARK: - Directories

    /// Application Support directory scoped to ZipherX.
    /// SA-4: Uses guard with fallback instead of force-unwrap.
    public var dataDirectory: URL {
        let base = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("ZipherX", isDirectory: true)
    }

    /// Log files directory nested under `dataDirectory`.
    public var logDirectory: URL {
        dataDirectory.appendingPathComponent("Logs", isDirectory: true)
    }

    /// Caches directory scoped to ZipherX.
    /// SA-4: Uses guard with fallback instead of force-unwrap.
    public var cacheDirectory: URL {
        let base = FileManager.default.urls(
            for: .cachesDirectory,
            in: .userDomainMask
        ).first ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("ZipherX", isDirectory: true)
    }

    // MARK: - Device Identity

    /// A stable identifier for the device.
    ///
    /// - macOS: Hardware UUID from IOKit (persists across reinstalls).
    /// - iOS:   `UIDevice.identifierForVendor` (resets on reinstall, but reliable per-install).
    public var deviceId: String {
        #if os(macOS)
        return macOSHardwareUUID ?? UUID().uuidString
        #elseif canImport(UIKit)
        return UIDevice.current.identifierForVendor?.uuidString ?? UUID().uuidString
        #else
        return UUID().uuidString
        #endif
    }

    // MARK: - OS Info

    /// Human-readable OS version string from `ProcessInfo`.
    public var osDescription: String {
        ProcessInfo.processInfo.operatingSystemVersionString
    }

    // MARK: - Environment Flags

    /// `true` when running inside a simulator (never true on device builds).
    public var isSimulator: Bool {
        #if targetEnvironment(simulator)
        return true
        #else
        return false
        #endif
    }

    /// `true` when the app is currently in the foreground / active state.
    public var isForeground: Bool {
        #if os(macOS)
        return NSApplication.shared.isActive
        #elseif canImport(UIKit)
        return UIApplication.shared.applicationState == .active
        #else
        return true
        #endif
    }

    // MARK: - Security Checks

    /// SA-1: Basic jailbreak / device compromise detection.
    /// Returns `true` if the device shows signs of being jailbroken.
    /// This is a best-effort heuristic; determined attackers can bypass these checks.
    public static func isDeviceCompromised() -> Bool {
        #if os(iOS) && !targetEnvironment(simulator)
        let suspiciousPaths = [
            "/Applications/Cydia.app",
            "/Library/MobileSubstrate/MobileSubstrate.dylib",
            "/bin/bash",
            "/usr/sbin/sshd",
            "/etc/apt",
            "/private/var/lib/apt/"
        ]
        for path in suspiciousPaths {
            if FileManager.default.fileExists(atPath: path) {
                return true
            }
        }
        // Check if app can write outside sandbox
        let testPath = "/private/jailbreak_test_\(UUID().uuidString)"
        do {
            try "test".write(toFile: testPath, atomically: true, encoding: .utf8)
            try FileManager.default.removeItem(atPath: testPath)
            return true
        } catch {
            return false
        }
        #else
        return false
        #endif
    }

    // MARK: - Private Helpers

    #if os(macOS)
    /// Read the IOPlatformUUID from IOKit on macOS.
    private var macOSHardwareUUID: String? {
        let service = IOServiceGetMatchingService(
            kIOMainPortDefault,
            IOServiceMatching("IOPlatformExpertDevice")
        )
        defer { IOObjectRelease(service) }
        guard service != 0 else { return nil }

        let key = "IOPlatformUUID" as CFString
        let value = IORegistryEntryCreateCFProperty(service, key, kCFAllocatorDefault, 0)
        return (value?.takeRetainedValue() as? String)
    }
    #endif
}
