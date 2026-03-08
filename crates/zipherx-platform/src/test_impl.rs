//! In-memory mock implementations of all platform traits.
//!
//! Used for testing the Rust core without platform-specific dependencies.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::*;

// ============================================================================
// Mock SecureStorage
// ============================================================================

/// In-memory secure storage for testing.
pub struct MockSecureStorage {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

impl MockSecureStorage {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl SecureStorage for MockSecureStorage {
    fn store_key(&self, identifier: &str, data: &[u8]) -> Result<(), PlatformError> {
        self.store
            .lock()
            .unwrap()
            .insert(identifier.to_string(), data.to_vec());
        Ok(())
    }

    fn load_key(&self, identifier: &str) -> Result<Vec<u8>, PlatformError> {
        self.store
            .lock()
            .unwrap()
            .get(identifier)
            .cloned()
            .ok_or_else(|| PlatformError::KeyNotFound(identifier.to_string()))
    }

    fn delete_key(&self, identifier: &str) -> Result<(), PlatformError> {
        self.store.lock().unwrap().remove(identifier);
        Ok(())
    }

    fn has_key(&self, identifier: &str) -> bool {
        self.store.lock().unwrap().contains_key(identifier)
    }

    fn load_encrypted_key_pair(&self, identifier: &str) -> Result<(Vec<u8>, Vec<u8>), PlatformError> {
        let encrypted = self.load_key(identifier)?;
        let enc_key = self.load_key(&format!("{}_enc", identifier))?;
        Ok((encrypted, enc_key))
    }

    fn is_hardware_backed(&self) -> bool {
        false // Mock — no hardware
    }
}

// ============================================================================
// Mock BiometricAuth
// ============================================================================

/// Mock biometric auth that always succeeds.
pub struct MockBiometricAuth {
    available: bool,
    should_succeed: bool,
}

impl MockBiometricAuth {
    pub fn new(available: bool, should_succeed: bool) -> Self {
        Self { available, should_succeed }
    }
}

impl BiometricAuth for MockBiometricAuth {
    fn is_available(&self) -> bool {
        self.available
    }

    fn biometric_type(&self) -> String {
        "MockBiometric".to_string()
    }

    fn authenticate(&self, _reason: &str) -> Result<bool, PlatformError> {
        if !self.available {
            return Err(PlatformError::BiometricNotAvailable);
        }
        Ok(self.should_succeed)
    }

    fn is_enrolled(&self) -> bool {
        self.available
    }
}

// ============================================================================
// Mock PlatformInfo
// ============================================================================

/// Mock platform info returning temp paths.
pub struct MockPlatformInfo;

impl PlatformInfo for MockPlatformInfo {
    fn data_directory(&self) -> PathBuf {
        PathBuf::from("/tmp/zipherx_test/data")
    }

    fn log_directory(&self) -> PathBuf {
        PathBuf::from("/tmp/zipherx_test/logs")
    }

    fn cache_directory(&self) -> PathBuf {
        PathBuf::from("/tmp/zipherx_test/cache")
    }

    fn device_id(&self) -> String {
        "test-device-id-0000".to_string()
    }

    fn os_description(&self) -> String {
        "TestOS 1.0".to_string()
    }

    fn is_simulator(&self) -> bool {
        true
    }

    fn is_foreground(&self) -> bool {
        true
    }
}

// ============================================================================
// Mock Notifications
// ============================================================================

/// Mock notifications that record messages.
pub struct MockNotifications {
    messages: Mutex<Vec<(String, String)>>,
}

impl MockNotifications {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }

    pub fn get_messages(&self) -> Vec<(String, String)> {
        self.messages.lock().unwrap().clone()
    }
}

impl Notifications for MockNotifications {
    fn send_notification(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        self.messages
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
        Ok(())
    }

    fn request_permission(&self) -> Result<bool, PlatformError> {
        Ok(true)
    }
}

// ============================================================================
// Mock Clipboard
// ============================================================================

/// Mock clipboard with in-memory buffer.
pub struct MockClipboard {
    content: Mutex<Option<String>>,
}

impl MockClipboard {
    pub fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for MockClipboard {
    fn copy_text(&self, text: &str) -> Result<(), PlatformError> {
        *self.content.lock().unwrap() = Some(text.to_string());
        Ok(())
    }

    fn paste_text(&self) -> Result<Option<String>, PlatformError> {
        Ok(self.content.lock().unwrap().clone())
    }
}

// ============================================================================
// Mock PlatformLogger
// ============================================================================

/// Mock logger that records log messages.
pub struct MockLogger {
    messages: Mutex<Vec<(logging::LogLevel, String)>>,
}

impl MockLogger {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }

    pub fn get_messages(&self) -> Vec<(logging::LogLevel, String)> {
        self.messages.lock().unwrap().clone()
    }
}

impl logging::PlatformLogger for MockLogger {
    fn log(&self, level: logging::LogLevel, message: &str) {
        self.messages
            .lock()
            .unwrap()
            .push((level, message.to_string()));
    }
}

// ============================================================================
// PlatformServices Builder
// ============================================================================

/// Create a full PlatformServices with all mock implementations.
pub fn create_mock_platform() -> PlatformServices {
    PlatformServices {
        storage: Box::new(MockSecureStorage::new()),
        biometric: Box::new(MockBiometricAuth::new(true, true)),
        info: Box::new(MockPlatformInfo),
        notifications: Box::new(MockNotifications::new()),
        clipboard: Box::new(MockClipboard::new()),
        logger: Box::new(MockLogger::new()),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_secure_storage() {
        let storage = MockSecureStorage::new();
        assert!(!storage.has_key("test_key"));

        storage.store_key("test_key", &[1, 2, 3]).unwrap();
        assert!(storage.has_key("test_key"));
        assert_eq!(storage.load_key("test_key").unwrap(), vec![1, 2, 3]);

        storage.delete_key("test_key").unwrap();
        assert!(!storage.has_key("test_key"));
        assert!(storage.load_key("test_key").is_err());
    }

    #[test]
    fn test_mock_secure_storage_overwrite() {
        let storage = MockSecureStorage::new();
        storage.store_key("k", &[1]).unwrap();
        storage.store_key("k", &[2]).unwrap();
        assert_eq!(storage.load_key("k").unwrap(), vec![2]);
    }

    #[test]
    fn test_mock_secure_storage_not_hardware_backed() {
        let storage = MockSecureStorage::new();
        assert!(!storage.is_hardware_backed());
    }

    #[test]
    fn test_mock_biometric_available() {
        let bio = MockBiometricAuth::new(true, true);
        assert!(bio.is_available());
        assert!(bio.is_enrolled());
        assert_eq!(bio.biometric_type(), "MockBiometric");
        assert!(bio.authenticate("Test").unwrap());
    }

    #[test]
    fn test_mock_biometric_unavailable() {
        let bio = MockBiometricAuth::new(false, false);
        assert!(!bio.is_available());
        assert!(!bio.is_enrolled());
        assert!(bio.authenticate("Test").is_err());
    }

    #[test]
    fn test_mock_biometric_fails() {
        let bio = MockBiometricAuth::new(true, false);
        assert!(bio.is_available());
        assert!(!bio.authenticate("Test").unwrap()); // User cancelled
    }

    #[test]
    fn test_mock_platform_info() {
        let info = MockPlatformInfo;
        assert_eq!(info.data_directory(), PathBuf::from("/tmp/zipherx_test/data"));
        assert!(info.is_simulator());
        assert!(info.is_foreground());
        assert!(!info.device_id().is_empty());
    }

    #[test]
    fn test_mock_notifications() {
        let notif = MockNotifications::new();
        notif.send_notification("TX Sent", "0.5 ZCL sent").unwrap();
        let msgs = notif.get_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "TX Sent");
    }

    #[test]
    fn test_mock_clipboard() {
        let clip = MockClipboard::new();
        assert!(clip.paste_text().unwrap().is_none());
        clip.copy_text("zs1abc...").unwrap();
        assert_eq!(clip.paste_text().unwrap(), Some("zs1abc...".to_string()));
    }

    #[test]
    fn test_mock_logger() {
        let logger = MockLogger::new();
        logger.log(logging::LogLevel::Info, "Test message");
        logger.log(logging::LogLevel::Error, "Error message");
        let msgs = logger.get_messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, logging::LogLevel::Info);
        assert_eq!(msgs[1].0, logging::LogLevel::Error);
    }

    #[test]
    fn test_create_mock_platform() {
        let platform = create_mock_platform();
        assert!(platform.biometric.is_available());
        assert!(!platform.storage.has_key("nonexistent"));
        assert!(platform.info.is_simulator());
    }

    #[test]
    fn test_notification_permission() {
        let notif = MockNotifications::new();
        assert!(notif.request_permission().unwrap());
    }
}
