//! Platform trait implementations for the egui desktop app.
//!
//! Reuses the same password-encrypted file storage as the CLI:
//! Argon2id key derivation + AES-256-GCM per-file encryption.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use rand::rngs::OsRng;
use rand::RngCore;

use zipherx_platform::*;

// ============================================================================
// Constants
// ============================================================================

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

// ============================================================================
// SecureStorage — password-encrypted file storage (Argon2id + AES-256-GCM)
// ============================================================================

/// File-based key storage for desktop.
///
/// Keys are encrypted with AES-256-GCM. The encryption key is derived from
/// the user's password via Argon2id. Stored format per file:
///   salt (16 bytes) || nonce (12 bytes) || ciphertext+tag
/// All hex-encoded to the .key file.
pub struct GuiSecureStorage {
    base_dir: PathBuf,
    data_dir: PathBuf,
    cache: Mutex<HashMap<String, Vec<u8>>>,
    derived_key: Mutex<Option<[u8; KEY_LEN]>>,
}

impl GuiSecureStorage {
    pub fn new(data_dir: &PathBuf) -> Self {
        let keys_dir = data_dir.join("keys");
        let _ = fs::create_dir_all(&keys_dir);
        Self {
            base_dir: keys_dir,
            data_dir: data_dir.clone(),
            cache: Mutex::new(HashMap::new()),
            derived_key: Mutex::new(None),
        }
    }

    #[allow(dead_code)]
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    fn key_path(&self, identifier: &str) -> PathBuf {
        let safe_name = identifier.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.base_dir.join(format!("{}.key", safe_name))
    }

    /// Set the session password. Derives the AES-256 key via Argon2id.
    /// Must be called before any store/load operations.
    pub fn set_password(&self, password: &str) {
        let salt = b"ZipherX_session_"; // 16 bytes fixed salt for session key
        let mut key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .expect("Argon2 hash failed");
        *self.derived_key.lock().unwrap() = Some(key);
    }

    /// Check if a password has been set for this session.
    #[allow(dead_code)]
    pub fn has_password(&self) -> bool {
        self.derived_key.lock().unwrap().is_some()
    }

    /// Lock the storage — clears the derived key and cache.
    pub fn lock(&self) {
        if let Ok(mut dk) = self.derived_key.lock() {
            if let Some(ref mut key) = *dk {
                // Zeroize the key before dropping
                for byte in key.iter_mut() {
                    unsafe { std::ptr::write_volatile(byte, 0) };
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            }
            *dk = None;
        }
        if let Ok(mut cache) = self.cache.lock() {
            // Zeroize cached values
            for (_, v) in cache.iter_mut() {
                for byte in v.iter_mut() {
                    unsafe { std::ptr::write_volatile(byte, 0) };
                }
            }
            cache.clear();
        }
    }

    fn derive_key_from_password(session_key: &[u8; KEY_LEN], salt: &[u8]) -> [u8; KEY_LEN] {
        let mut key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(session_key, salt, &mut key)
            .expect("Argon2 per-file derivation failed");
        key
    }

    fn encrypt_data(session_key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, PlatformError> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let file_key = Self::derive_key_from_password(session_key, &salt);
        let cipher = Aes256Gcm::new_from_slice(&file_key)
            .map_err(|e| PlatformError::StorageError(format!("cipher init: {}", e)))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| PlatformError::StorageError(format!("encrypt: {}", e)))?;

        let mut output = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&salt);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt_data(session_key: &[u8; KEY_LEN], encrypted: &[u8]) -> Result<Vec<u8>, PlatformError> {
        if encrypted.len() < SALT_LEN + NONCE_LEN + 16 {
            return Err(PlatformError::StorageError("encrypted data too short".into()));
        }
        let salt = &encrypted[..SALT_LEN];
        let nonce_bytes = &encrypted[SALT_LEN..SALT_LEN + NONCE_LEN];
        let ciphertext = &encrypted[SALT_LEN + NONCE_LEN..];

        let file_key = Self::derive_key_from_password(session_key, salt);
        let cipher = Aes256Gcm::new_from_slice(&file_key)
            .map_err(|e| PlatformError::StorageError(format!("cipher init: {}", e)))?;
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| PlatformError::StorageError("decryption failed — wrong password?".into()))
    }

    /// Delete all wallet data from disk (keys, databases, everything).
    pub fn delete_all_data(&self) {
        self.lock();
        if let Err(e) = fs::remove_dir_all(&self.data_dir) {
            eprintln!("[ZipherX] Warning: failed to delete data dir: {}", e);
        }
        let _ = fs::create_dir_all(&self.data_dir);
        let _ = fs::create_dir_all(&self.base_dir);
    }

    /// Verify a password by attempting to decrypt the spending key.
    /// Does NOT affect current session state.
    pub fn verify_password(&self, password: &str) -> bool {
        let salt = b"ZipherX_session_";
        let mut test_key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut test_key)
            .expect("Argon2 hash failed");

        let path = self.key_path("spending_key");
        let hex_data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => {
                // Zeroize test key
                for b in test_key.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                return false;
            }
        };
        let encrypted = match hex::decode(hex_data.trim()) {
            Ok(d) => d,
            Err(_) => {
                for b in test_key.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
                return false;
            }
        };
        let result = Self::decrypt_data(&test_key, &encrypted).is_ok();
        for b in test_key.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        result
    }
}

impl SecureStorage for GuiSecureStorage {
    fn store_key(&self, identifier: &str, data: &[u8]) -> Result<(), PlatformError> {
        let session_key = {
            let dk = self.derived_key.lock().unwrap();
            dk.ok_or_else(|| PlatformError::StorageError("password not set".into()))?
        };
        let encrypted = Self::encrypt_data(&session_key, data)?;
        let hex_data = hex::encode(&encrypted);
        let path = self.key_path(identifier);
        fs::write(&path, hex_data)
            .map_err(|e| PlatformError::StorageError(format!("write {}: {}", path.display(), e)))?;
        self.cache.lock().unwrap().insert(identifier.to_string(), data.to_vec());
        Ok(())
    }

    fn load_key(&self, identifier: &str) -> Result<Vec<u8>, PlatformError> {
        if let Some(data) = self.cache.lock().unwrap().get(identifier) {
            return Ok(data.clone());
        }
        let session_key = {
            let dk = self.derived_key.lock().unwrap();
            dk.ok_or_else(|| PlatformError::StorageError("password not set".into()))?
        };
        let path = self.key_path(identifier);
        let hex_data = fs::read_to_string(&path)
            .map_err(|_| PlatformError::KeyNotFound(identifier.to_string()))?;
        let encrypted = hex::decode(hex_data.trim())
            .map_err(|e| PlatformError::StorageError(format!("hex decode: {}", e)))?;
        let data = Self::decrypt_data(&session_key, &encrypted)?;
        self.cache.lock().unwrap().insert(identifier.to_string(), data.clone());
        Ok(data)
    }

    fn delete_key(&self, identifier: &str) -> Result<(), PlatformError> {
        let path = self.key_path(identifier);
        let _ = fs::remove_file(path);
        self.cache.lock().unwrap().remove(identifier);
        Ok(())
    }

    fn has_key(&self, identifier: &str) -> bool {
        self.cache.lock().unwrap().contains_key(identifier) || self.key_path(identifier).exists()
    }

    fn load_encrypted_key_pair(&self, identifier: &str) -> Result<(Vec<u8>, Vec<u8>), PlatformError> {
        let encrypted = self.load_key(identifier)?;
        let enc_key = self.load_key(&format!("{}_enc", identifier))?;
        Ok((encrypted, enc_key))
    }

    fn is_hardware_backed(&self) -> bool {
        false
    }
}

// ============================================================================
// PlatformInfo
// ============================================================================

pub struct GuiPlatformInfo {
    data_dir: PathBuf,
}

impl GuiPlatformInfo {
    pub fn new() -> Self {
        let data_dir = Self::default_data_dir();
        let _ = fs::create_dir_all(&data_dir);
        Self { data_dir }
    }

    fn default_data_dir() -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("zipherx-gui")
        }
        #[cfg(target_os = "windows")]
        {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public"))
                .join("ZipherX-GUI")
        }
        #[cfg(target_os = "macos")]
        {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("ZipherX-GUI")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            PathBuf::from("/tmp/zipherx-gui")
        }
    }
}

impl PlatformInfo for GuiPlatformInfo {
    fn data_directory(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn log_directory(&self) -> PathBuf {
        let dir = self.data_dir.join("logs");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn cache_directory(&self) -> PathBuf {
        let dir = self.data_dir.join("cache");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn device_id(&self) -> String {
        "egui-desktop".to_string()
    }

    fn os_description(&self) -> String {
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
    }

    fn is_simulator(&self) -> bool {
        false
    }
    fn is_foreground(&self) -> bool {
        true
    }
}

// ============================================================================
// BiometricAuth — no biometrics on desktop
// ============================================================================

#[allow(dead_code)]
pub struct GuiBiometricAuth;

impl BiometricAuth for GuiBiometricAuth {
    fn is_available(&self) -> bool { false }
    fn biometric_type(&self) -> String { "None".to_string() }
    fn authenticate(&self, _reason: &str) -> Result<bool, PlatformError> { Ok(true) }
    fn is_enrolled(&self) -> bool { false }
}

// ============================================================================
// Notifications — no-op for desktop GUI
// ============================================================================

#[allow(dead_code)]
pub struct GuiNotifications;

impl Notifications for GuiNotifications {
    fn send_notification(&self, _title: &str, _body: &str) -> Result<(), PlatformError> {
        Ok(())
    }
    fn request_permission(&self) -> Result<bool, PlatformError> {
        Ok(true)
    }
}

// ============================================================================
// Clipboard — uses egui clipboard (set externally)
// ============================================================================

#[allow(dead_code)]
pub struct GuiClipboard;

impl Clipboard for GuiClipboard {
    fn copy_text(&self, _text: &str) -> Result<(), PlatformError> {
        // Clipboard is handled by egui context directly
        Ok(())
    }
    fn paste_text(&self) -> Result<Option<String>, PlatformError> {
        Ok(None)
    }
}

// ============================================================================
// Logger
// ============================================================================

#[allow(dead_code)]
pub struct GuiLogger;

impl logging::PlatformLogger for GuiLogger {
    fn log(&self, level: logging::LogLevel, message: &str) {
        let tag = match level {
            logging::LogLevel::Debug => "DEBUG",
            logging::LogLevel::Info => "INFO",
            logging::LogLevel::Warning => "WARN",
            logging::LogLevel::Error => "ERROR",
        };
        eprintln!("[ZipherX][{}] {}", tag, message);
    }
}

// ============================================================================
// Builder
// ============================================================================

#[allow(dead_code)]
pub fn create_platform_services(data_dir: &PathBuf) -> PlatformServices {
    PlatformServices {
        storage: Box::new(GuiSecureStorage::new(data_dir)),
        biometric: Box::new(GuiBiometricAuth),
        info: Box::new(GuiPlatformInfo::new()),
        notifications: Box::new(GuiNotifications),
        clipboard: Box::new(GuiClipboard),
        logger: Box::new(GuiLogger),
    }
}
