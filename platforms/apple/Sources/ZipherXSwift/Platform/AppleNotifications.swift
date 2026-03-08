import Foundation
import UserNotifications

// MARK: - Errors

public enum NotificationError: Error, LocalizedError {
    case permissionDenied
    case schedulingFailed(Error)

    public var errorDescription: String? {
        switch self {
        case .permissionDenied:           return "Notification permission was denied by the user."
        case .schedulingFailed(let err):  return "Failed to schedule notification: \(err.localizedDescription)"
        }
    }
}

// MARK: - AppleNotifications

/// Local push notifications via `UNUserNotificationCenter`.
public final class AppleNotifications: @unchecked Sendable {

    public init() {}

    // MARK: - Permission

    /// Request notification permission from the user.
    ///
    /// - Returns: `true` when permission was granted, `false` when denied.
    /// - Throws: `NotificationError.permissionDenied` if the system returns an error.
    public func requestPermission() throws -> Bool {
        let semaphore = DispatchSemaphore(value: 0)
        var granted = false
        var requestError: Error?

        UNUserNotificationCenter.current().requestAuthorization(
            options: [.alert, .sound, .badge]
        ) { isGranted, error in
            granted = isGranted
            requestError = error
            semaphore.signal()
        }

        semaphore.wait()

        if let err = requestError {
            throw NotificationError.schedulingFailed(err)
        }
        if !granted { throw NotificationError.permissionDenied }
        return granted
    }

    // MARK: - Sending

    /// Deliver an immediate local notification with the given `title` and `body`.
    ///
    /// - Throws: `NotificationError` if delivery fails.
    public func sendNotification(title: String, body: String) throws {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body  = body
        content.sound = .default

        // Fire immediately (nil trigger).
        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil
        )

        let semaphore = DispatchSemaphore(value: 0)
        var scheduleError: Error?

        UNUserNotificationCenter.current().add(request) { error in
            scheduleError = error
            semaphore.signal()
        }

        semaphore.wait()

        if let err = scheduleError {
            throw NotificationError.schedulingFailed(err)
        }
    }
}
