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

/// Encode a transparent secret key as WIF (Wallet Import Format).
///
/// WIF format: base58check( version_byte + 32_secret_bytes + 0x01_compressed_flag )
/// Zclassic uses version byte 0x80 (same as Bitcoin mainnet).
/// Returns a compressed WIF string starting with "L" or "K".
pub fn encode_wif(secret_key_bytes: &[u8]) -> Result<Zeroizing<String>, CryptoError> {
    if secret_key_bytes.len() != 32 {
        return Err(CryptoError::InvalidData(format!(
            "Secret key must be 32 bytes, got {}",
            secret_key_bytes.len()
        )));
    }

    // version(1) + key(32) + compressed_flag(1) = 34 bytes + 4 checksum = 38
    let mut payload = Vec::with_capacity(38);
    payload.push(0x80); // WIF version byte
    payload.extend_from_slice(secret_key_bytes);
    payload.push(0x01); // compressed public key flag

    let checksum = double_sha256(&payload);
    payload.extend_from_slice(&checksum[..4]);

    let wif = bs58::encode(&payload).into_string();

    // Zeroize the payload buffer
    for b in payload.iter_mut() {
        *b = 0;
    }

    Ok(Zeroizing::new(wif))
}

/// Decode a WIF-encoded private key. Returns (secret_key_bytes, t-address).
/// Validates: Base58Check checksum, version byte 0x80, compression flag 0x01.
/// Rejects uncompressed WIF keys (start with '5').
pub fn decode_wif(wif: &str) -> Result<(Zeroizing<Vec<u8>>, String), CryptoError> {
    let decoded = bs58::decode(wif)
        .into_vec()
        .map_err(|e| CryptoError::InvalidData(format!("Invalid Base58: {}", e)))?;

    if decoded.len() < 6 {
        return Err(CryptoError::InvalidData("WIF too short".into()));
    }
    // Valid compressed WIF is exactly 38 bytes (1 version + 32 key + 1 flag + 4 checksum).
    // Reject anything longer to prevent oversized input from reaching key derivation.
    if decoded.len() > 38 {
        return Err(CryptoError::InvalidData("WIF too long".into()));
    }

    // Verify checksum (last 4 bytes)
    let (payload, checksum) = decoded.split_at(decoded.len() - 4);
    let expected_checksum = double_sha256(payload);
    if &expected_checksum[..4] != checksum {
        return Err(CryptoError::InvalidData("WIF checksum mismatch".into()));
    }

    // Version byte must be 0x80 (mainnet)
    if payload[0] != 0x80 {
        return Err(CryptoError::InvalidData(format!(
            "Invalid WIF version byte: 0x{:02x} (expected 0x80)",
            payload[0]
        )));
    }

    let key_data = &payload[1..]; // strip version byte

    // Compressed WIF: 33 bytes (32-byte key + 0x01 flag)
    // Uncompressed WIF: 32 bytes (no flag)
    if key_data.len() == 32 {
        return Err(CryptoError::InvalidData(
            "Uncompressed WIF keys are not supported. Use a compressed key (starts with L or K)."
                .into(),
        ));
    }
    if key_data.len() != 33 || key_data[32] != 0x01 {
        return Err(CryptoError::InvalidData(format!(
            "Invalid WIF key length: {} bytes",
            key_data.len()
        )));
    }

    let sk_bytes = Zeroizing::new(key_data[..32].to_vec());

    // Derive the t-address from the secret key
    let secp = secp256k1::Secp256k1::new();
    let secret_key = secp256k1::SecretKey::from_slice(&sk_bytes)
        .map_err(|e| CryptoError::InvalidData(format!("Invalid secret key: {}", e)))?;
    let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
    let pub_bytes = public_key.serialize(); // compressed
    let pub_hash = hash160(&pub_bytes);
    let address = TransparentAddress::PublicKey(pub_hash);
    let encoded = encode_transparent_address(&address)?;

    Ok((sk_bytes, encoded))
}

/// Validate a WIF string without returning the secret key.
pub fn validate_wif(wif: &str) -> bool {
    decode_wif(wif).is_ok()
}

/// Derive and export the transparent private key as WIF from seed.
///
/// `is_change` selects the BIP-44 chain: `false` = external (chain 0, receiving),
/// `true` = internal (chain 1, change addresses).
pub fn export_transparent_wif(
    seed: &[u8],
    account: u32,
    child_index: u32,
    is_change: bool,
) -> Result<Zeroizing<String>, CryptoError> {
    let sk_bytes = derive_transparent_secret_key(seed, account, child_index, is_change)?;
    encode_wif(&sk_bytes)
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

    #[test]
    fn test_decode_wif_roundtrip() {
        let seed = test_seed();
        let sk = derive_transparent_secret_key(&seed, 0, 0, false).unwrap();
        let wif = encode_wif(&sk).unwrap();
        let (decoded_sk, decoded_addr) = decode_wif(&wif).unwrap();
        assert_eq!(&*decoded_sk, &*sk);
        let expected_addr = derive_transparent_address(&seed, 0, 0).unwrap();
        assert_eq!(decoded_addr, expected_addr);
    }

    #[test]
    fn test_decode_wif_rejects_uncompressed() {
        use sha2::{Digest, Sha256};
        let fake_key = [0x42u8; 32];
        let mut payload = vec![0x80];
        payload.extend_from_slice(&fake_key);
        // No compression flag — uncompressed WIF
        let hash1 = Sha256::digest(&payload);
        let hash2 = Sha256::digest(&hash1);
        payload.extend_from_slice(&hash2[..4]);
        let wif = bs58::encode(&payload).into_string();
        assert!(wif.starts_with('5'));
        let result = decode_wif(&wif);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("ncompressed"));
    }

    #[test]
    fn test_validate_wif() {
        let seed = test_seed();
        let sk = derive_transparent_secret_key(&seed, 0, 0, false).unwrap();
        let wif = encode_wif(&sk).unwrap();
        assert!(validate_wif(&wif));
        assert!(!validate_wif("not_a_wif"));
        assert!(!validate_wif(""));
    }

    #[test]
    fn test_export_transparent_wif_change_address() {
        let seed = test_seed();
        let external_wif = export_transparent_wif(&seed, 0, 0, false).unwrap();
        let change_wif = export_transparent_wif(&seed, 0, 0, true).unwrap();
        assert_ne!(&*external_wif, &*change_wif);
        let (_, ext_addr) = decode_wif(&external_wif).unwrap();
        let (_, chg_addr) = decode_wif(&change_wif).unwrap();
        let expected_ext = derive_transparent_address(&seed, 0, 0).unwrap();
        let expected_chg = derive_transparent_change_address(&seed, 0, 0).unwrap();
        assert_eq!(ext_addr, expected_ext);
        assert_eq!(chg_addr, expected_chg);
    }
}
