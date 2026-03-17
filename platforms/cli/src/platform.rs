#![allow(dead_code)]
//! Platform trait implementations for the CLI (Linux, Windows, macOS).
//!
//! Provides password-encrypted file storage, console logging, and OS-specific paths.
//! Keys are encrypted with AES-256-GCM using a key derived from the user's password
//! via Argon2id. This replaces the old plaintext hex storage.

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

/// Salt length for Argon2id key derivation.
const SALT_LEN: usize = 16;
/// Nonce length for AES-256-GCM.
const NONCE_LEN: usize = 12;
/// Derived key length (256-bit for AES-256).
const KEY_LEN: usize = 32;

// ============================================================================
// CLI SecureStorage — password-encrypted file storage
// ============================================================================

/// File-based key storage for desktop CLI.
///
/// Keys are encrypted with AES-256-GCM. The encryption key is derived from
/// the user's password via Argon2id. The stored format per file is:
///   salt (16 bytes) || nonce (12 bytes) || ciphertext+tag
/// All hex-encoded to the .key file.
pub struct CliSecureStorage {
    base_dir: PathBuf,
    cache: Mutex<HashMap<String, Vec<u8>>>,
    /// Cached password-derived encryption key (set once at startup).
    derived_key: Mutex<Option<[u8; KEY_LEN]>>,
}

impl CliSecureStorage {
    pub fn new(data_dir: &PathBuf) -> Self {
        let keys_dir = data_dir.join("keys");
        let _ = fs::create_dir_all(&keys_dir);
        Self {
            base_dir: keys_dir,
            cache: Mutex::new(HashMap::new()),
            derived_key: Mutex::new(None),
        }
    }

    fn key_path(&self, identifier: &str) -> PathBuf {
        // Sanitize identifier for filesystem
        let safe_name = identifier.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        self.base_dir.join(format!("{}.key", safe_name))
    }

    /// Set the session password. Derives the AES-256 key via Argon2id and caches it.
    /// Must be called before any store/load operations.
    pub fn set_password(&self, password: &str) {
        // Use a fixed application salt mixed with the password to derive the key.
        // Each stored file also has its own random salt for per-file key derivation,
        // but for the session cache we derive from the password alone using a fixed context.
        // Actually, per-file salt is used at encrypt/decrypt time; this just flags
        // that the password is available.
        let mut dk = self.derived_key.lock().unwrap();
        // Store raw password hash for per-file derivation. We actually derive per-file
        // with unique salts, so we store the password itself (hashed once for session key).
        // We'll re-derive per file, but cache the password hash as a session marker.
        let salt = b"ZipherX_session_"; // 16 bytes fixed salt for session key
        let mut key = [0u8; KEY_LEN];
        // Note: Argon2::default() uses m=4096 KiB, t=1, p=1 (argon2id).
        // Stronger parameters (m=65536, t=3) are recommended for new deployments
        // but cannot be changed without a migration path for existing encrypted files.
        // The memory-hard property of Argon2id provides substantial protection even
        // with default parameters.
        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .expect("Argon2 hash failed");
        *dk = Some(key);
    }

    /// Check if a password has been set for this session.
    pub fn has_password(&self) -> bool {
        self.derived_key.lock().unwrap().is_some()
    }

    /// Derive an AES-256 key from the user's password and a per-file random salt.
    fn derive_key_from_password(
        password_session_key: &[u8; KEY_LEN],
        salt: &[u8],
    ) -> [u8; KEY_LEN] {
        // Use the session key as "password" input to Argon2 with per-file salt.
        // This gives us a unique encryption key per file.
        let mut key = [0u8; KEY_LEN];
        Argon2::default()
            .hash_password_into(password_session_key, salt, &mut key)
            .expect("Argon2 per-file derivation failed");
        key
    }

    /// Encrypt data with AES-256-GCM using a fresh random salt and nonce.
    /// Returns: salt (16) || nonce (12) || ciphertext+tag
    fn encrypt_data(
        session_key: &[u8; KEY_LEN],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, PlatformError> {
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

        // Concatenate: salt || nonce || ciphertext+tag
        let mut output = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&salt);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    /// Decrypt data produced by `encrypt_data`.
    fn decrypt_data(
        session_key: &[u8; KEY_LEN],
        encrypted: &[u8],
    ) -> Result<Vec<u8>, PlatformError> {
        if encrypted.len() < SALT_LEN + NONCE_LEN + 16 {
            return Err(PlatformError::StorageError(
                "encrypted data too short".into(),
            ));
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
}

impl SecureStorage for CliSecureStorage {
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

        self.cache
            .lock()
            .unwrap()
            .insert(identifier.to_string(), data.to_vec());
        Ok(())
    }

    fn load_key(&self, identifier: &str) -> Result<Vec<u8>, PlatformError> {
        // Check cache first
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
        self.cache
            .lock()
            .unwrap()
            .insert(identifier.to_string(), data.clone());
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

    fn load_encrypted_key_pair(
        &self,
        identifier: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), PlatformError> {
        let encrypted = self.load_key(identifier)?;
        let enc_key = self.load_key(&format!("{}_enc", identifier))?;
        Ok((encrypted, enc_key))
    }

    fn is_hardware_backed(&self) -> bool {
        false // No Secure Enclave on desktop
    }
}

// ============================================================================
// CLI BiometricAuth — always succeeds (no biometrics on desktop)
// ============================================================================

pub struct CliBiometricAuth;

impl BiometricAuth for CliBiometricAuth {
    fn is_available(&self) -> bool {
        false
    }
    fn biometric_type(&self) -> String {
        "None".to_string()
    }
    fn authenticate(&self, _reason: &str) -> Result<bool, PlatformError> {
        // CLI: no biometric, always allow
        Ok(true)
    }
    fn is_enrolled(&self) -> bool {
        false
    }
}

// ============================================================================
// CLI PlatformInfo — OS-specific paths
// ============================================================================

pub struct CliPlatformInfo {
    data_dir: PathBuf,
}

impl CliPlatformInfo {
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
                .join("zipherx")
        }
        #[cfg(target_os = "windows")]
        {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\Users\\Public"))
                .join("ZipherX")
        }
        #[cfg(target_os = "macos")]
        {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("ZipherX")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            PathBuf::from("/tmp/zipherx")
        }
    }
}

impl PlatformInfo for CliPlatformInfo {
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
        "cli-desktop".to_string()
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
// CLI Notifications — print to stdout
// ============================================================================

pub struct CliNotifications;

impl Notifications for CliNotifications {
    fn send_notification(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        println!("[NOTIFICATION] {}: {}", title, body);
        Ok(())
    }

    fn request_permission(&self) -> Result<bool, PlatformError> {
        Ok(true)
    }
}

// ============================================================================
// CLI Clipboard — in-memory (no system clipboard in headless)
// ============================================================================

pub struct CliClipboard {
    content: Mutex<Option<String>>,
}

impl CliClipboard {
    pub fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for CliClipboard {
    fn copy_text(&self, text: &str) -> Result<(), PlatformError> {
        *self.content.lock().unwrap() = Some(text.to_string());
        println!("[CLIPBOARD] Copied to clipboard.");
        Ok(())
    }

    fn paste_text(&self) -> Result<Option<String>, PlatformError> {
        Ok(self.content.lock().unwrap().clone())
    }
}

// ============================================================================
// CLI Logger — stderr
// ============================================================================

pub struct CliLogger;

impl logging::PlatformLogger for CliLogger {
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

/// Create platform services for the CLI.
pub fn create_cli_platform() -> PlatformServices {
    let info = CliPlatformInfo::new();
    let data_dir = info.data_directory();
    PlatformServices {
        storage: Box::new(CliSecureStorage::new(&data_dir)),
        biometric: Box::new(CliBiometricAuth),
        info: Box::new(info),
        notifications: Box::new(CliNotifications),
        clipboard: Box::new(CliClipboard::new()),
        logger: Box::new(CliLogger),
    }
}

/// Create a CliSecureStorage instance directly (for use by main.rs).
pub fn create_secure_storage(data_dir: &PathBuf) -> CliSecureStorage {
    CliSecureStorage::new(data_dir)
}
