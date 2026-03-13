//! Platform information trait.

use std::path::PathBuf;

/// Platform-specific information and paths.
pub trait PlatformInfo: Send + Sync {
    /// The app's data directory for persistent storage.
    /// - iOS: Documents/
    /// - macOS: ~/Library/Application Support/ZipherX/
    /// - Android: getFilesDir()
    /// - Windows: %APPDATA%/ZipherX/
    fn data_directory(&self) -> PathBuf;

    /// Directory for log files.
    fn log_directory(&self) -> PathBuf;

    /// Directory for cached data (boost files, etc.).
    fn cache_directory(&self) -> PathBuf;

    /// Unique device identifier for encryption key derivation.
    /// - iOS: identifierForVendor
    /// - macOS: Hardware UUID (IOKit)
    /// - Android: ANDROID_ID
    /// - Windows: Machine GUID
    fn device_id(&self) -> String;

    /// OS name and version (e.g., "iOS 17.2", "macOS 14.1", "Android 14", "Windows 11").
    fn os_description(&self) -> String;

    /// Whether the app is running on a simulator/emulator.
    fn is_simulator(&self) -> bool;

    /// Whether the app is in the foreground.
    fn is_foreground(&self) -> bool;
}
