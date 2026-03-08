//! Utility functions — version, branch ID, hashing, memory management.

use ff::PrimeField;
use sha2::{Digest, Sha256};
use zcash_primitives::consensus::{BlockHeight, BranchId, Parameters};

use crate::types::{CryptoError, ZclassicNetwork};

/// Get the library version.
pub fn version() -> u32 {
    1
}

/// Get the consensus branch ID for a given block height.
pub fn get_branch_id(height: u64) -> Result<u32, CryptoError> {
    // L-5: Guard against height truncation
    if height > u32::MAX as u64 {
        return Err(CryptoError::InvalidData(format!(
            "Block height {} exceeds u32::MAX",
            height
        )));
    }
    let bh = BlockHeight::from_u32(height as u32);
    let branch = BranchId::for_height(&ZclassicNetwork, bh);
    Ok(u32::from(branch))
}

/// Check if the Buttercup upgrade is supported by this build.
pub fn verify_buttercup_support() -> bool {
    use zcash_primitives::consensus::NetworkUpgrade;
    ZclassicNetwork
        .activation_height(NetworkUpgrade::ZclassicButtercup)
        .is_some()
}

/// Compute double-SHA256 hash.
pub fn double_sha256(data: &[u8]) -> [u8; 32] {
    let hash1 = Sha256::digest(data);
    let hash2 = Sha256::digest(hash1);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash2);
    result
}

/// Zero-out sensitive data in memory (best effort).
///
/// Uses `write_volatile` to prevent the compiler from optimizing away the zeroing,
/// followed by a `SeqCst` compiler fence to ensure the writes are not reordered.
/// This is the standard pre-`zeroize` crate approach and is sufficient for our use case.
/// The `zeroize` crate would provide `Zeroize` trait integration but volatile writes
/// with a compiler fence achieve the same underlying effect.
///
/// # Limitations
///
/// This function cannot prevent the OS from paging memory to disk before zeroing
/// occurs, nor can it prevent speculative execution side channels. For production
/// hardening, consider using the `zeroize` crate (which provides the same volatile
/// write approach but with `Zeroize`/`ZeroizeOnDrop` traits) and `mlock(2)` to
/// pin sensitive pages in RAM.
pub fn secure_zero(data: &mut [u8]) {
    for byte in data.iter_mut() {
        unsafe {
            std::ptr::write_volatile(byte, 0);
        }
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Compute a random scalar on the jubjub curve.
pub fn random_scalar() -> [u8; 32] {
    use ff::Field;
    let scalar = jubjub::Fr::random(&mut rand::rngs::OsRng);
    scalar.to_repr()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(version(), 1);
    }

    #[test]
    fn test_buttercup_support() {
        assert!(verify_buttercup_support());
    }

    #[test]
    fn test_branch_id_at_buttercup() {
        let branch = get_branch_id(707_001).unwrap();
        assert_eq!(branch, 0x930b540d);
    }

    #[test]
    fn test_double_sha256() {
        let hash = double_sha256(b"hello");
        assert_eq!(hash.len(), 32);
        // Deterministic
        assert_eq!(hash, double_sha256(b"hello"));
    }

    #[test]
    fn test_random_scalar() {
        let s1 = random_scalar();
        let s2 = random_scalar();
        assert_ne!(s1, s2);
        assert_eq!(s1.len(), 32);
    }

    #[test]
    fn test_secure_zero() {
        let mut data = vec![0xFF; 32];
        secure_zero(&mut data);
        assert!(data.iter().all(|&b| b == 0));
    }
}
