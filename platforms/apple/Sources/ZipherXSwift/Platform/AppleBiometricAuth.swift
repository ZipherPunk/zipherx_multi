import Foundation
import LocalAuthentication

// MARK: - Errors

public enum BiometricAuthError: Error, LocalizedError {
    case notAvailable
    case notEnrolled
    case evaluationFailed(Error)

    public var errorDescription: String? {
        switch self {
        case .notAvailable:               return "Biometric authentication is not available on this device."
        case .notEnrolled:                return "No biometrics are enrolled on this device."
        case .evaluationFailed(let err):  return "Authentication failed: \(err.localizedDescription)"
        }
    }
}

// MARK: - AppleBiometricAuth

/// Face ID / Touch ID authentication via LocalAuthentication.
///
/// **Architecture note (SA-6):** This class provides biometric-only authentication
/// using `.deviceOwnerAuthenticationWithBiometrics`. It does NOT fall back to the
/// device passcode automatically. `SendView` handles passcode fallback separately
/// by catching `BiometricAuthError` and presenting its own passcode entry UI.
/// Callers that require passcode fallback should implement their own fallback
/// mechanism rather than relying on this class to provide it.
public final class AppleBiometricAuth: @unchecked Sendable {

    public init() {}

    // MARK: - Availability

    /// `true` when biometric hardware is present and at least one biometric is enrolled.
    public var isAvailable: Bool {
        var error: NSError?
        let can = LAContext().canEvaluatePolicy(
            .deviceOwnerAuthenticationWithBiometrics,
            error: &error
        )
        return can
    }

    /// Alias of `isAvailable` — `true` when biometrics are enrolled.
    public var isEnrolled: Bool { isAvailable }

    /// Human-readable biometric type: `"FaceID"`, `"TouchID"`, or `"None"`.
    public var biometricType: String {
        let ctx = LAContext()
        var error: NSError?
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            return "None"
        }
        switch ctx.biometryType {
        case .faceID:   return "FaceID"
        case .touchID:  return "TouchID"
        case .opticID:  return "OpticID"
        default:        return "None"
        }
    }

    // MARK: - Authentication

    /// Synchronously evaluate biometric auth. Blocks the calling thread on a semaphore.
    ///
    /// - Returns: `true` on success, `false` on user-cancel.
    /// - Throws: `BiometricAuthError` when hardware is unavailable or the OS reports an error.
    ///
    /// SA-22: A fresh `LAContext` is created for each authentication request to prevent
    /// reuse of a previously-evaluated context, which could bypass re-authentication.
    public func authenticate(reason: String) throws -> Bool {
        guard isAvailable else { throw BiometricAuthError.notAvailable }

        let semaphore = DispatchSemaphore(value: 0)
        var authResult: Bool = false
        var authError: Error? = nil

        // SA-22: Always create a new LAContext — never reuse across evaluations.
        let ctx = LAContext()
        ctx.evaluatePolicy(
            .deviceOwnerAuthenticationWithBiometrics,
            localizedReason: reason
        ) { success, error in
            authResult = success
            authError  = error
            semaphore.signal()
        }

        semaphore.wait()

        if let err = authError {
            let laErr = err as? LAError
            if laErr?.code == .userCancel || laErr?.code == .appCancel {
                return false
            }
            throw BiometricAuthError.evaluationFailed(err)
        }
        return authResult
    }
}
