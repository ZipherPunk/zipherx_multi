//! Secure key storage trait.
//!
//! Platform implementations:
//! - iOS/macOS: Keychain + Secure Enclave
//! - Android: Android Keystore + StrongBox
//! - Windows: DPAPI + TPM 2.0

use crate::PlatformError;

/// Secure storage for cryptographic keys and sensitive data.
///
/// Keys stored through this trait should be protected by the platform's
/// hardware security module (Secure Enclave, StrongBox, TPM) when available.
pub trait SecureStorage: Send + Sync {
    /// Store a key with the given identifier.
    /// The platform should encrypt the data using hardware-backed keys if available.
    fn store_key(&self, identifier: &str, data: &[u8]) -> Result<(), PlatformError>;

    /// Retrieve a previously stored key.
    /// Returns the decrypted key data.
    fn load_key(&self, identifier: &str) -> Result<Vec<u8>, PlatformError>;

    /// Delete a stored key.
    fn delete_key(&self, identifier: &str) -> Result<(), PlatformError>;

    /// Check if a key exists.
    fn has_key(&self, identifier: &str) -> bool;

    /// Store an encrypted spending key (197 bytes AES-GCM format).
    /// Used for VUL-002 mitigation: key never leaves encrypted form in memory.
    fn store_encrypted_key(
        &self,
        identifier: &str,
        encrypted_data: &[u8],
    ) -> Result<(), PlatformError> {
        self.store_key(identifier, encrypted_data)
    }

    /// Load encrypted key + encryption key pair for FFI operations.
    /// Returns (encrypted_key, encryption_key) for passing to Rust crypto layer.
    fn load_encrypted_key_pair(
        &self,
        identifier: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), PlatformError>;

    /// Whether hardware-backed secure storage is available (Secure Enclave, StrongBox, TPM).
    fn is_hardware_backed(&self) -> bool;
}
