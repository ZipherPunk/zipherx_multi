//! Hidden service (.onion) address generation and persistence.
//!
//! Phase 1: Generate ed25519 keypair, derive v3 .onion address, persist to disk.
//! Phase 2: Use arti-client to register the hidden service with the Tor network.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::TorError;

static ONION_ADDRESS: Mutex<Option<String>> = Mutex::new(None);

/// Get the .onion address (None if not yet generated).
pub fn get_onion_address() -> Option<String> {
    ONION_ADDRESS.lock().ok().and_then(|g| g.clone())
}

/// Generate or load the hidden service keypair and derive the v3 .onion address.
///
/// - If `<data_dir>/hidden_service/hs_ed25519_secret_key` exists, load it.
/// - Otherwise, generate a new ed25519 keypair and save it.
/// - Derive the v3 .onion address: base32(pubkey[32] + checksum[2] + version[1])
/// - checksum = SHA3-256(".onion checksum" || pubkey || version)[:2]
pub fn init_hidden_service(data_dir: PathBuf) -> Result<String, TorError> {
    let hs_dir = data_dir.join("hidden_service");
    std::fs::create_dir_all(&hs_dir)
        .map_err(|e| TorError::HiddenServiceError(format!("mkdir: {e}")))?;

    let key_path = hs_dir.join("hs_ed25519_secret_key");
    let pub_path = hs_dir.join("hs_ed25519_public_key");

    let pubkey_bytes: [u8; 32] = if key_path.exists() {
        // Load existing public key
        let pub_data = std::fs::read(&pub_path)
            .map_err(|e| TorError::HiddenServiceError(format!("read pubkey: {e}")))?;
        if pub_data.len() < 32 {
            return Err(TorError::HiddenServiceError("pubkey too short".into()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&pub_data[..32]);
        arr
    } else {
        // Generate new ed25519 keypair
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // RT-4: Extract key bytes, write to disk, then zero from memory.
        // The signing key contains sensitive cryptographic material that
        // should not persist in memory longer than necessary.
        let mut key_bytes = signing_key.to_bytes();

        // Save secret key (32 bytes)
        std::fs::write(&key_path, &key_bytes)
            .map_err(|e| TorError::HiddenServiceError(format!("write secret: {e}")))?;

        // TOR-001: Zero key bytes using write_volatile to prevent the compiler
        // from optimizing away the zeroing. Note: ed25519_dalek::SigningKey does
        // not implement Zeroize, so `drop(signing_key)` below does NOT zero the
        // key material inside the struct. This write_volatile on key_bytes is
        // defense-in-depth for the extracted copy.
        unsafe {
            for b in key_bytes.iter_mut() {
                std::ptr::write_volatile(b, 0);
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        }
        drop(signing_key);
        // RT-2: Restrict secret key file to owner-only read/write (0600).
        // Without this, other users on the system could read the hidden
        // service private key and impersonate this .onion address.
        //
        // TOR-002: On Windows, Unix-style file permissions are not available.
        // The key file will have default ACLs. A future improvement could use
        // Windows ACL APIs (e.g., `SetNamedSecurityInfoW`) to restrict access
        // to the current user only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| TorError::HiddenServiceError(format!("chmod secret key: {e}")))?;
        }
        // Save public key (32 bytes)
        std::fs::write(&pub_path, verifying_key.to_bytes())
            .map_err(|e| TorError::HiddenServiceError(format!("write pubkey: {e}")))?;

        verifying_key.to_bytes()
    };

    // Derive v3 .onion address per Tor spec (rend-spec-v3 section 6)
    // checksum = SHA3-256(".onion checksum" || pubkey || version)[:2]
    let version: u8 = 3;

    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(b".onion checksum");
    hasher.update(pubkey_bytes);
    hasher.update([version]);
    let hash = hasher.finalize();
    let checksum = [hash[0], hash[1]];

    // onion_address = base32(pubkey || checksum || version)
    let mut onion_input = Vec::with_capacity(35);
    onion_input.extend_from_slice(&pubkey_bytes);
    onion_input.extend_from_slice(&checksum);
    onion_input.push(version);

    let encoded = data_encoding::BASE32_NOPAD
        .encode(&onion_input)
        .to_lowercase();
    let address = format!("{encoded}.onion");

    if let Ok(mut guard) = ONION_ADDRESS.lock() {
        *guard = Some(address.clone());
    }

    Ok(address)
}

/// Base32-encode without padding (RFC 4648).
/// Used for .onion address derivation.
#[cfg(test)]
fn onion_address_len() -> usize {
    // 35 bytes * 8 / 5 = 56 base32 chars + ".onion" = 62
    56 + 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_hidden_service_generates_address() {
        let tmp = std::env::temp_dir().join("zipherx_hs_test_1");
        let _ = std::fs::remove_dir_all(&tmp);

        let addr = init_hidden_service(tmp.clone()).unwrap();
        assert!(addr.ends_with(".onion"));
        assert_eq!(addr.len(), onion_address_len());

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_init_hidden_service_persists_keypair() {
        let tmp = std::env::temp_dir().join("zipherx_hs_test_2");
        let _ = std::fs::remove_dir_all(&tmp);

        let addr1 = init_hidden_service(tmp.clone()).unwrap();
        let addr2 = init_hidden_service(tmp.clone()).unwrap();
        assert_eq!(addr1, addr2, "same keypair should produce same address");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_onion_address_before_init() {
        // Reset static
        if let Ok(mut guard) = ONION_ADDRESS.lock() {
            *guard = None;
        }
        // Can't test get_onion_address reliably in parallel tests
        // because of shared static, but at least verify it doesn't panic
        let _ = get_onion_address();
    }

    #[test]
    fn test_different_dirs_different_addresses() {
        let tmp1 = std::env::temp_dir().join("zipherx_hs_test_3a");
        let tmp2 = std::env::temp_dir().join("zipherx_hs_test_3b");
        let _ = std::fs::remove_dir_all(&tmp1);
        let _ = std::fs::remove_dir_all(&tmp2);

        let addr1 = init_hidden_service(tmp1.clone()).unwrap();
        let addr2 = init_hidden_service(tmp2.clone()).unwrap();
        assert_ne!(addr1, addr2, "different keys should produce different addresses");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp1);
        let _ = std::fs::remove_dir_all(&tmp2);
    }
}
