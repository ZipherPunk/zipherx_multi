//! ZIP-32 key derivation — spending keys, viewing keys, addresses.
//!
//! Implements the Sapling key hierarchy:
//! seed → spending_key → full_viewing_key → incoming_viewing_key → payment_address

use bech32::{FromBase32, ToBase32, Variant};
use zcash_primitives::{
    consensus::Parameters,
    sapling::keys::FullViewingKey,
    zip32::{sapling::ExtendedSpendingKey, ChildIndex, DiversifierIndex},
};
use zeroize::Zeroizing;

use crate::types::{CryptoError, ZclassicNetwork, SPENDING_KEY_LENGTH};

/// Derive a Sapling extended spending key from seed.
///
/// Path: m/32'/147'/account'
/// Seed must be 64 bytes (from BIP-39 PBKDF2).
/// Returns the 169-byte serialized ExtendedSpendingKey.
pub fn derive_spending_key(seed: &[u8], account: u32) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if seed.len() != 64 {
        return Err(CryptoError::InvalidSeed(seed.len()));
    }

    let master = ExtendedSpendingKey::master(seed);

    // Derive to account level: m/32'/147'/account'
    let account_key = master
        .derive_child(ChildIndex::Hardened(32))
        .derive_child(ChildIndex::Hardened(ZclassicNetwork.coin_type()))
        .derive_child(ChildIndex::Hardened(account));

    let mut buf = Vec::new();
    account_key
        .write(&mut buf)
        .map_err(|e| CryptoError::InvalidData(format!("SK serialize: {e}")))?;

    // Defense-in-depth: explicitly drop key material to shorten its lifetime.
    // Rust's Drop doesn't zero memory, but explicit drop ensures the values
    // are not held longer than necessary.
    drop(account_key);
    drop(master);

    if buf.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidData(format!(
            "Unexpected SK length: {} (expected {SPENDING_KEY_LENGTH})",
            buf.len()
        )));
    }

    // Wrap in Zeroizing to ensure the spending key bytes are securely zeroed
    // when the caller drops them.
    Ok(Zeroizing::new(buf))
}

/// Derive a payment address from a spending key at a given diversifier index.
///
/// Returns (address_bytes[43], actual_diversifier_index).
/// The actual index may differ from requested if the requested index produces
/// an invalid diversifier (not all diversifier indices are valid).
pub fn derive_address(
    sk_bytes: &[u8],
    diversifier_index: u64,
) -> Result<(Vec<u8>, u64), CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let sk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let dfvk = sk.to_diversifiable_full_viewing_key();

    let (actual_j, address) = if diversifier_index == 0 {
        dfvk.default_address()
    } else {
        let j = DiversifierIndex::from(diversifier_index);
        dfvk.find_address(j)
            .ok_or(CryptoError::InvalidDiversifier(diversifier_index))?
    };

    // Serialize address: 43 bytes
    let addr_bytes = address.to_bytes();

    // Convert DiversifierIndex ([u8; 11]) back to u64 (little-endian, first 8 bytes)
    let mut idx_bytes = [0u8; 8];
    idx_bytes.copy_from_slice(&actual_j.0[..8]);
    let actual_index = u64::from_le_bytes(idx_bytes);

    Ok((addr_bytes.to_vec(), actual_index))
}

/// Derive the incoming viewing key from a spending key.
///
/// Returns the 32-byte IVK.
pub fn derive_ivk(sk_bytes: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let sk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let fvk = FullViewingKey::from_expanded_spending_key(&sk.expsk);
    let ivk = fvk.vk.ivk();
    let ivk_bytes = ivk.to_repr();

    Ok(ivk_bytes.to_vec())
}

/// Derive the outgoing viewing key from a spending key.
///
/// Returns the 32-byte OVK.
pub fn derive_ovk(sk_bytes: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let sk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|_| CryptoError::InvalidSpendingKey)?;

    let fvk = FullViewingKey::from_expanded_spending_key(&sk.expsk);
    Ok(fvk.ovk.0.to_vec())
}

/// Encode a spending key as bech32 string.
///
/// Uses HRP "secret-extended-key-main".
pub fn encode_spending_key(sk_bytes: &[u8]) -> Result<String, CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    let encoded = bech32::encode(
        ZclassicNetwork.hrp_sapling_extended_spending_key(),
        sk_bytes.to_base32(),
        Variant::Bech32,
    )
    .map_err(|e| CryptoError::InvalidData(format!("Bech32 encode: {e}")))?;

    Ok(encoded)
}

/// Decode a bech32-encoded spending key.
///
/// Returns the 169-byte raw spending key.
pub fn decode_spending_key(encoded: &str) -> Result<Vec<u8>, CryptoError> {
    let (hrp, data, variant) = bech32::decode(encoded)
        .map_err(|e| CryptoError::InvalidData(format!("Bech32 decode: {e}")))?;

    // RCR-5: Validate Bech32 variant (reject Bech32m)
    if variant != Variant::Bech32 {
        return Err(CryptoError::InvalidData(
            "Invalid Bech32 variant: expected Bech32, got Bech32m".into(),
        ));
    }

    if hrp != ZclassicNetwork.hrp_sapling_extended_spending_key() {
        return Err(CryptoError::InvalidData(format!(
            "Wrong HRP: expected {}, got {hrp}",
            ZclassicNetwork.hrp_sapling_extended_spending_key()
        )));
    }

    let bytes = Vec::<u8>::from_base32(&data)
        .map_err(|e| CryptoError::InvalidData(format!("Base32 decode: {e}")))?;

    if bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic;

    fn test_seed() -> [u8; 64] {
        // Generate a deterministic seed from a known mnemonic
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        mnemonic::to_seed(phrase).unwrap()
    }

    #[test]
    fn test_derive_spending_key() {
        let sk = derive_spending_key(&test_seed(), 0).unwrap();
        assert_eq!(sk.len(), SPENDING_KEY_LENGTH);
    }

    #[test]
    fn test_derive_spending_key_deterministic() {
        let sk1 = derive_spending_key(&test_seed(), 0).unwrap();
        let sk2 = derive_spending_key(&test_seed(), 0).unwrap();
        assert_eq!(sk1, sk2);
    }

    #[test]
    fn test_derive_different_accounts() {
        let sk0 = derive_spending_key(&test_seed(), 0).unwrap();
        let sk1 = derive_spending_key(&test_seed(), 1).unwrap();
        assert_ne!(sk0, sk1);
    }

    #[test]
    fn test_derive_address() {
        let sk = derive_spending_key(&test_seed(), 0).unwrap();
        let (addr, _idx) = derive_address(&sk, 0).unwrap();
        assert_eq!(addr.len(), 43);
    }

    #[test]
    fn test_derive_ivk() {
        let sk = derive_spending_key(&test_seed(), 0).unwrap();
        let ivk = derive_ivk(&sk).unwrap();
        assert_eq!(ivk.len(), 32);
    }

    #[test]
    fn test_derive_ovk() {
        let sk = derive_spending_key(&test_seed(), 0).unwrap();
        let ovk = derive_ovk(&sk).unwrap();
        assert_eq!(ovk.len(), 32);
    }

    #[test]
    fn test_encode_decode_spending_key() {
        let sk = derive_spending_key(&test_seed(), 0).unwrap();
        let encoded = encode_spending_key(&sk).unwrap();
        assert!(encoded.starts_with("secret-extended-key-main1"));
        let decoded = decode_spending_key(&encoded).unwrap();
        assert_eq!(*sk, decoded);
    }

    #[test]
    fn test_invalid_seed_length() {
        let result = derive_spending_key(&[0u8; 16], 0);
        assert!(matches!(result, Err(CryptoError::InvalidSeed(16))));
    }
}
