//! Platform abstraction traits for ZipherX cross-platform support.
//!
//! Each platform (iOS, macOS, Android, Windows) provides its own implementation
//! of these traits, which are injected into the Rust core at runtime.

pub mod secure_storage;
pub mod biometric_auth;
pub mod platform_info;
pub mod notifications;
pub mod clipboard;
pub mod logging;
pub mod test_impl;

pub use secure_storage::SecureStorage;
pub use biometric_auth::BiometricAuth;
pub use platform_info::PlatformInfo;
pub use notifications::Notifications;
pub use clipboard::Clipboard;
pub use logging::PlatformLogger;

/// Errors from platform-specific operations.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Biometric authentication failed: {0}")]
    BiometricFailed(String),

    #[error("Biometric not available")]
    BiometricNotAvailable,

    #[error("Platform operation not supported: {0}")]
    NotSupported(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Platform error: {0}")]
    Other(String),
}

/// Combined platform services bundle.
/// Platform code creates this and passes it to the Rust core.
pub struct PlatformServices {
    pub storage: Box<dyn SecureStorage>,
    pub biometric: Box<dyn BiometricAuth>,
    pub info: Box<dyn PlatformInfo>,
    pub notifications: Box<dyn Notifications>,
    pub clipboard: Box<dyn Clipboard>,
    pub logger: Box<dyn PlatformLogger>,
}
