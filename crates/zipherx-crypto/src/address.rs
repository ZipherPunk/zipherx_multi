//! Sapling payment address encoding, decoding, and validation.
//!
//! Addresses use Bech32 encoding with HRP "zs" for Zclassic mainnet.
//! Raw address = diversifier(11 bytes) + pk_d(32 bytes) = 43 bytes.

use zcash_primitives::consensus::Parameters;
use zcash_primitives::sapling::PaymentAddress;
use bech32::{ToBase32, FromBase32, Variant};

use crate::types::{ZclassicNetwork, CryptoError, PAYMENT_ADDRESS_LENGTH};

/// Encode a raw 43-byte address to bech32 "zs1..." string.
pub fn encode_address(address_bytes: &[u8]) -> Result<String, CryptoError> {
    if address_bytes.len() != PAYMENT_ADDRESS_LENGTH {
        return Err(CryptoError::InvalidAddress(format!(
            "Expected {PAYMENT_ADDRESS_LENGTH} bytes, got {}",
            address_bytes.len()
        )));
    }

    let encoded = bech32::encode(
        ZclassicNetwork.hrp_sapling_payment_address(),
        address_bytes.to_base32(),
        Variant::Bech32,
    ).map_err(|e| CryptoError::InvalidAddress(format!("Bech32 encode: {e}")))?;

    Ok(encoded)
}

/// Decode a bech32 "zs1..." address to raw 43 bytes.
pub fn decode_address(address_str: &str) -> Result<Vec<u8>, CryptoError> {
    let (hrp, data, variant) = bech32::decode(address_str)
        .map_err(|e| CryptoError::InvalidAddress(format!("Bech32 decode: {e}")))?;

    // RCR-5: Validate Bech32 variant (reject Bech32m)
    if variant != Variant::Bech32 {
        return Err(CryptoError::InvalidAddress(
            "Invalid Bech32 variant: expected Bech32, got Bech32m".into(),
        ));
    }

    if hrp != ZclassicNetwork.hrp_sapling_payment_address() {
        return Err(CryptoError::InvalidAddress(format!(
            "Wrong HRP: expected {}, got {hrp}",
            ZclassicNetwork.hrp_sapling_payment_address()
        )));
    }

    let bytes = Vec::<u8>::from_base32(&data)
        .map_err(|e| CryptoError::InvalidAddress(format!("Base32 decode: {e}")))?;

    if bytes.len() != PAYMENT_ADDRESS_LENGTH {
        return Err(CryptoError::InvalidAddress(format!(
            "Expected {PAYMENT_ADDRESS_LENGTH} bytes, got {}",
            bytes.len()
        )));
    }

    // Validate that pk_d is a valid jubjub point on the curve.
    // PaymentAddress::from_bytes performs the point decompression check internally.
    let addr_array: [u8; 43] = bytes[..43].try_into()
        .map_err(|_| CryptoError::InvalidAddress("Failed to convert to [u8; 43]".into()))?;
    if PaymentAddress::from_bytes(&addr_array).is_none() {
        return Err(CryptoError::InvalidAddress(
            "Invalid payment address: pk_d is not a valid jubjub point".into(),
        ));
    }

    Ok(bytes)
}

/// Validate a Zclassic Sapling payment address string.
///
/// Checks: valid bech32, correct HRP, correct length.
pub fn validate_address(address_str: &str) -> bool {
    decode_address(address_str).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;
    use crate::mnemonic;

    fn test_seed() -> [u8; 64] {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        mnemonic::to_seed(phrase).unwrap()
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let sk = keys::derive_spending_key(&test_seed(), 0).unwrap();
        let (addr_bytes, _) = keys::derive_address(&sk, 0).unwrap();

        let encoded = encode_address(&addr_bytes).unwrap();
        assert!(encoded.starts_with("zs1"));

        let decoded = decode_address(&encoded).unwrap();
        assert_eq!(addr_bytes, decoded);
    }

    #[test]
    fn test_validate_real_address() {
        let sk = keys::derive_spending_key(&test_seed(), 0).unwrap();
        let (addr_bytes, _) = keys::derive_address(&sk, 0).unwrap();
        let encoded = encode_address(&addr_bytes).unwrap();

        assert!(validate_address(&encoded));
    }

    #[test]
    fn test_invalid_address() {
        assert!(!validate_address(""));
        assert!(!validate_address("invalid"));
        assert!(!validate_address("zs1invalid"));
    }
}
