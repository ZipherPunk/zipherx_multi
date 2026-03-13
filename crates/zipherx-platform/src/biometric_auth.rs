//! Biometric authentication trait.
//!
//! Platform implementations:
//! - iOS: Face ID / Touch ID (LocalAuthentication)
//! - macOS: Touch ID (LocalAuthentication)
//! - Android: AndroidX Biometric (fingerprint / face)
//! - Windows: Windows Hello (face / fingerprint / PIN)

use crate::PlatformError;

/// Biometric authentication for transaction authorization.
pub trait BiometricAuth: Send + Sync {
    /// Whether biometric authentication is available on this device.
    fn is_available(&self) -> bool;

    /// The type of biometric available (e.g., "FaceID", "TouchID", "Fingerprint", "WindowsHello").
    fn biometric_type(&self) -> String;

    /// Authenticate the user with biometrics.
    /// `reason` is displayed to the user explaining why authentication is needed.
    /// Returns Ok(true) if authenticated, Ok(false) if user cancelled.
    fn authenticate(&self, reason: &str) -> Result<bool, PlatformError>;

    /// Whether the user has enrolled biometrics on this device.
    fn is_enrolled(&self) -> bool;
}
