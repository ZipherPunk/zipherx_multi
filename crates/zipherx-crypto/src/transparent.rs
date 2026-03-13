//! Transparent address (t-address) key derivation and encoding.
//!
//! Implements BIP-44 key hierarchy for Zclassic transparent addresses:
//! seed → m/44'/147'/account'/external_or_internal/child_index → address
//!
//! T-addresses use secp256k1 (Bitcoin-style) keys with base58check encoding.
//! Zclassic uses prefix [0x1C, 0xB8] for P2PKH ("t1...") addresses.

use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use zcash_primitives::{
    consensus::Parameters,
    legacy::{
        keys::{AccountPrivKey, IncomingViewingKey},
        TransparentAddress,
    },
    zip32::AccountId,
};
use zeroize::Zeroizing;

use crate::types::{CryptoError, ZclassicNetwork};

/// Derive a transparent account private key from seed.
///
/// Path: m/44'/147'/account'
/// Returns serialized BIP-32 extended private key bytes.
pub fn derive_transparent_account_key(
    seed: &[u8],
    account: u32,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if seed.len() != 64 {
        return Err(CryptoError::InvalidSeed(seed.len()));
    }

    let account_id = AccountId::from(account);
    let account_key = AccountPrivKey::from_seed(&ZclassicNetwork, seed, account_id)
        .map_err(|e| CryptoError::InvalidData(format!("BIP-44 derivation: {e}")))?;

    Ok(Zeroizing::new(account_key.to_bytes()))
}

/// Derive a transparent address (P2PKH) from seed at the given account and child index.
///
/// Uses external chain (0) for receiving addresses.
/// Returns the base58check-encoded t1 address string.
pub fn derive_transparent_address(
    seed: &[u8],
    account: u32,
    child_index: u32,
) -> Result<String, CryptoError> {
    if seed.len() != 64 {
        return Err(CryptoError::InvalidSeed(seed.len()));
    }

    let account_id = AccountId::from(account);
    let account_key = AccountPrivKey::from_seed(&ZclassicNetwork, seed, account_id)
        .map_err(|e| CryptoError::InvalidData(format!("BIP-44 derivation: {e}")))?;

    let pubkey = account_key
        .to_account_pubkey()
        .derive_external_ivk()
        .map_err(|e| CryptoError::InvalidData(format!("External IVK: {e}")))?;

    let address = pubkey
        .derive_address(child_index)
        .map_err(|e| CryptoError::InvalidData(format!("Address derivation: {e}")))?;

    encode_transparent_address(&address)
}

/// Derive a transparent change address (internal chain = 1).
pub fn derive_transparent_change_address(
    seed: &[u8],
    account: u32,
    child_index: u32,
) -> Result<String, CryptoError> {
    if seed.len() != 64 {
        return Err(CryptoError::InvalidSeed(seed.len()));
    }

    let account_id = AccountId::from(account);
    let account_key = AccountPrivKey::from_seed(&ZclassicNetwork, seed, account_id)
        .map_err(|e| CryptoError::InvalidData(format!("BIP-44 derivation: {e}")))?;

    let pubkey = account_key
        .to_account_pubkey()
        .derive_internal_ivk()
        .map_err(|e| CryptoError::InvalidData(format!("Internal IVK: {e}")))?;

    let address = pubkey
        .derive_address(child_index)
        .map_err(|e| CryptoError::InvalidData(format!("Change address derivation: {e}")))?;

    encode_transparent_address(&address)
}

/// Derive the secp256k1 secret key for spending a transparent UTXO.
///
/// `is_change` = true uses internal chain (1), false uses external chain (0).
pub fn derive_transparent_secret_key(
    seed: &[u8],
    account: u32,
    child_index: u32,
    is_change: bool,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if seed.len() != 64 {
        return Err(CryptoError::InvalidSeed(seed.len()));
    }

    let account_id = AccountId::from(account);
    let account_key = AccountPrivKey::from_seed(&ZclassicNetwork, seed, account_id)
        .map_err(|e| CryptoError::InvalidData(format!("BIP-44 derivation: {e}")))?;

    let sk = if is_change {
        account_key.derive_internal_secret_key(child_index)
    } else {
        account_key.derive_external_secret_key(child_index)
    }
    .map_err(|e| CryptoError::InvalidData(format!("Secret key derivation: {e}")))?;

    Ok(Zeroizing::new(sk.secret_bytes().to_vec()))
}

/// Encode a TransparentAddress to base58check string.
///
/// Zclassic P2PKH: prefix [0x1C, 0xB8] → "t1..."
/// Zclassic P2SH:  prefix [0x1C, 0xBD] → "t3..."
pub fn encode_transparent_address(addr: &TransparentAddress) -> Result<String, CryptoError> {
    let (prefix, hash) = match addr {
        TransparentAddress::PublicKey(h) => (ZclassicNetwork.b58_pubkey_address_prefix(), h),
        TransparentAddress::Script(h) => (ZclassicNetwork.b58_script_address_prefix(), h),
    };

    // base58check: prefix(2) + hash(20) → 22 bytes → double SHA-256 → first 4 bytes checksum
    let mut payload = Vec::with_capacity(26);
    payload.extend_from_slice(&prefix);
    payload.extend_from_slice(hash);

    let checksum = double_sha256(&payload);
    payload.extend_from_slice(&checksum[..4]);

    Ok(bs58::encode(payload).into_string())
}

/// Decode a base58check transparent address string to TransparentAddress.
pub fn decode_transparent_address(address: &str) -> Result<TransparentAddress, CryptoError> {
    let decoded = bs58::decode(address)
        .into_vec()
        .map_err(|e| CryptoError::InvalidAddress(format!("Base58 decode: {e}")))?;

    if decoded.len() != 26 {
        return Err(CryptoError::InvalidAddress(format!(
            "Expected 26 bytes, got {}",
            decoded.len()
        )));
    }

    // Verify checksum
    let payload = &decoded[..22];
    let checksum = &decoded[22..26];
    let expected = double_sha256(payload);
    if checksum != &expected[..4] {
        return Err(CryptoError::InvalidAddress(
            "Invalid checksum".into(),
        ));
    }

    let prefix = [decoded[0], decoded[1]];
    let mut hash = [0u8; 20];
    hash.copy_from_slice(&decoded[2..22]);

    let p2pkh_prefix = ZclassicNetwork.b58_pubkey_address_prefix();
    let p2sh_prefix = ZclassicNetwork.b58_script_address_prefix();

    if prefix == p2pkh_prefix {
        Ok(TransparentAddress::PublicKey(hash))
    } else if prefix == p2sh_prefix {
        Ok(TransparentAddress::Script(hash))
    } else {
        Err(CryptoError::InvalidAddress(format!(
            "Unknown address prefix: {:02x}{:02x}",
            prefix[0], prefix[1]
        )))
    }
}

/// Validate a transparent address string.
pub fn validate_transparent_address(address: &str) -> bool {
    decode_transparent_address(address).is_ok()
}

/// Compute RIPEMD-160(SHA-256(data)) — standard Bitcoin hash160.
pub fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let ripemd = Ripemd160::digest(sha);
    let mut out = [0u8; 20];
    out.copy_from_slice(&ripemd);
    out
}

/// Check if a scriptPubKey matches a known transparent address.
///
/// P2PKH: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
/// P2SH:  OP_HASH160 <20 bytes> OP_EQUAL
pub fn extract_address_from_script(script: &[u8]) -> Option<TransparentAddress> {
    // P2PKH: 76 a9 14 <20 bytes> 88 ac (25 bytes total)
    if script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
    {
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&script[3..23]);
        return Some(TransparentAddress::PublicKey(hash));
    }

    // P2SH: a9 14 <20 bytes> 87 (23 bytes total)
    if script.len() == 23
        && script[0] == 0xa9
        && script[1] == 0x14
        && script[22] == 0x87
    {
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&script[2..22]);
        return Some(TransparentAddress::Script(hash));
    }

    None
}

fn double_sha256(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    let second = Sha256::digest(first);
    let mut out = [0u8; 32];
    out.copy_from_slice(&second);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic;

    fn test_seed() -> [u8; 64] {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        mnemonic::to_seed(phrase).expect("test seed")
    }

    #[test]
    fn test_derive_transparent_address() {
        let addr = derive_transparent_address(&test_seed(), 0, 0).expect("derive t-addr");
        assert!(addr.starts_with("t1"), "Expected t1 prefix, got: {addr}");
    }

    #[test]
    fn test_derive_transparent_address_deterministic() {
        let a1 = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        let a2 = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        assert_eq!(a1, a2);
    }

    #[test]
    fn test_different_child_indexes_produce_different_addresses() {
        let a0 = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        let a1 = derive_transparent_address(&test_seed(), 0, 1).unwrap();
        assert_ne!(a0, a1);
    }

    #[test]
    fn test_different_accounts_produce_different_addresses() {
        let a0 = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        let a1 = derive_transparent_address(&test_seed(), 1, 0).unwrap();
        assert_ne!(a0, a1);
    }

    #[test]
    fn test_change_address_differs_from_external() {
        let external = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        let change = derive_transparent_change_address(&test_seed(), 0, 0).unwrap();
        assert_ne!(external, change);
        assert!(change.starts_with("t1"));
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let addr_str = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        let decoded = decode_transparent_address(&addr_str).unwrap();
        let re_encoded = encode_transparent_address(&decoded).unwrap();
        assert_eq!(addr_str, re_encoded);
    }

    #[test]
    fn test_validate_transparent_address() {
        let addr = derive_transparent_address(&test_seed(), 0, 0).unwrap();
        assert!(validate_transparent_address(&addr));
        assert!(!validate_transparent_address("invalid"));
        assert!(!validate_transparent_address(""));
    }

    #[test]
    fn test_derive_secret_key() {
        let sk = derive_transparent_secret_key(&test_seed(), 0, 0, false).unwrap();
        assert_eq!(sk.len(), 32, "secp256k1 secret key should be 32 bytes");
    }

    #[test]
    fn test_extract_p2pkh_script() {
        // Build a P2PKH script for a known hash
        let hash = [0xAA; 20];
        let mut script = vec![0x76, 0xa9, 0x14];
        script.extend_from_slice(&hash);
        script.extend_from_slice(&[0x88, 0xac]);

        let addr = extract_address_from_script(&script);
        assert_eq!(addr, Some(TransparentAddress::PublicKey(hash)));
    }

    #[test]
    fn test_extract_p2sh_script() {
        let hash = [0xBB; 20];
        let mut script = vec![0xa9, 0x14];
        script.extend_from_slice(&hash);
        script.push(0x87);

        let addr = extract_address_from_script(&script);
        assert_eq!(addr, Some(TransparentAddress::Script(hash)));
    }

    #[test]
    fn test_invalid_seed_length() {
        let result = derive_transparent_address(&[0u8; 16], 0, 0);
        assert!(matches!(result, Err(CryptoError::InvalidSeed(16))));
    }
}
