import Foundation
import os

/// Structured logging via `os.Logger` (unified Apple logging system).
///
/// Log output appears in Console.app and `log stream` filtered by subsystem/category.
public final class AppleLogger: @unchecked Sendable {

    private let logger: os.Logger

    public init(
        subsystem: String = "com.zipherx.wallet",
        category: String  = "core"
    ) {
        self.logger = os.Logger(subsystem: subsystem, category: category)
    }

    // MARK: - Logging

    /// Emit a structured log entry at the specified `level`.
    ///
    /// Accepted level strings: `"debug"`, `"info"`, `"warning"`, `"error"`.
    /// Unknown values are treated as `"info"`.
    public func log(level: String, message: String) {
        // M-20: Use .private to prevent sensitive data from leaking into system logs
        switch level {
        case "debug":
            logger.debug("\(message, privacy: .private)")
        case "info":
            logger.info("\(message, privacy: .private)")
        case "warning":
            logger.warning("\(message, privacy: .private)")
        case "error":
            logger.error("\(message, privacy: .private)")
        default:
            logger.info("\(message, privacy: .private)")
        }
    }

    // MARK: - Convenience

    public func debug(_ message: String)   { log(level: "debug",   message: message) }
    public func info(_ message: String)    { log(level: "info",    message: message) }
    public func warning(_ message: String) { log(level: "warning", message: message) }
    public func error(_ message: String)   { log(level: "error",   message: message) }
}
