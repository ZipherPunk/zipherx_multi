//! Field-level AES-GCM-256 encryption for sensitive database fields.
//!
//! Used to encrypt spending keys, seeds, and other sensitive data
//! before storing in SQLCipher. This provides a second layer of
//! encryption beyond SQLCipher's page-level encryption.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::rngs::OsRng;
use rand::RngCore;
use thiserror::Error;

/// Encryption error types.
#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("Encryption failed")]
    EncryptionFailed,

    #[error("Decryption failed — wrong key or corrupted data")]
    DecryptionFailed,

    #[error("Invalid key length: expected 32, got {0}")]
    InvalidKeyLength(usize),

    #[error("Invalid ciphertext: too short (minimum 28 bytes)")]
    CiphertextTooShort,
}

/// Encrypted bundle: nonce(12) + ciphertext(N) + tag(16).
/// Total = 28 + plaintext length.
const NONCE_SIZE: usize = 12;

/// Encrypt data with AES-256-GCM.
///
/// Returns: nonce(12) || ciphertext(N) || tag(16)
pub fn encrypt(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if key.len() != 32 {
        return Err(EncryptionError::InvalidKeyLength(key.len()));
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    // Generate random 12-byte nonce using OS-level CSPRNG (H-6: avoid thread_rng)
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt (ciphertext includes 16-byte auth tag appended by AES-GCM)
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| EncryptionError::EncryptionFailed)?;

    // Prepend nonce
    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data encrypted with AES-256-GCM.
///
/// Input format: nonce(12) || ciphertext(N) || tag(16)
pub fn decrypt(encrypted: &[u8], key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
    if key.len() != 32 {
        return Err(EncryptionError::InvalidKeyLength(key.len()));
    }

    // Minimum: nonce(12) + tag(16) = 28 bytes (empty plaintext)
    if encrypted.len() < NONCE_SIZE + 16 {
        return Err(EncryptionError::CiphertextTooShort);
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| EncryptionError::DecryptionFailed)?;

    let nonce = Nonce::from_slice(&encrypted[..NONCE_SIZE]);
    let ciphertext = &encrypted[NONCE_SIZE..];

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptionError::DecryptionFailed)
}

/// Zero-out sensitive data in memory (best effort).
///
/// Uses `write_volatile` to prevent the compiler from optimizing away the zeroing,
/// followed by a `SeqCst` compiler fence to ensure the writes are not reordered.
/// This is the standard pre-`zeroize` crate approach and is sufficient for our use case.
/// The `zeroize` crate would provide `Zeroize` trait integration but volatile writes
/// with a compiler fence achieve the same underlying effect.
pub fn secure_zero(data: &mut [u8]) {
    for byte in data.iter_mut() {
        unsafe {
            std::ptr::write_volatile(byte, 0);
        }
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let plaintext = b"spending key data here";

        let encrypted = encrypt(plaintext, &key).unwrap();
        assert_ne!(&encrypted[NONCE_SIZE..], plaintext);
        assert_eq!(encrypted.len(), NONCE_SIZE + plaintext.len() + 16);

        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key = [0x42u8; 32];
        let wrong_key = [0x43u8; 32];
        let plaintext = b"secret";

        let encrypted = encrypt(plaintext, &key).unwrap();
        let result = decrypt(&encrypted, &wrong_key);
        assert!(matches!(result, Err(EncryptionError::DecryptionFailed)));
    }

    #[test]
    fn test_invalid_key_length() {
        let short_key = [0u8; 16];
        let result = encrypt(b"test", &short_key);
        assert!(matches!(result, Err(EncryptionError::InvalidKeyLength(16))));
    }

    #[test]
    fn test_empty_plaintext() {
        let key = [0x42u8; 32];
        let encrypted = encrypt(b"", &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_ciphertext_too_short() {
        let key = [0x42u8; 32];
        let result = decrypt(&[0u8; 10], &key);
        assert!(matches!(result, Err(EncryptionError::CiphertextTooShort)));
    }

    #[test]
    fn test_secure_zero() {
        let mut data = vec![0xFF; 32];
        secure_zero(&mut data);
        assert!(data.iter().all(|&b| b == 0));
    }
}
