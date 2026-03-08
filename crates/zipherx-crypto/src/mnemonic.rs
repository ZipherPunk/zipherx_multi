//! BIP-39 mnemonic generation, validation, and seed derivation.

use bip0039::{Count, English, Mnemonic};
use crate::types::CryptoError;

/// Generate a new 24-word BIP-39 mnemonic.
pub fn generate() -> Result<String, CryptoError> {
    let mnemonic: Mnemonic<English> = Mnemonic::generate(Count::Words24);
    Ok(mnemonic.phrase().to_string())
}

/// Validate a BIP-39 mnemonic phrase.
pub fn validate(phrase: &str) -> bool {
    Mnemonic::<English>::from_phrase(phrase).is_ok()
}

/// Convert a mnemonic phrase to a 64-byte seed (PBKDF2-SHA512, no passphrase).
///
/// # Passphrase (RCR-15)
///
/// This function always uses an empty passphrase (`""`) for BIP-39 seed
/// derivation. This is the standard default for most cryptocurrency wallets.
/// An optional passphrase parameter is not supported; adding one would change
/// the derived seed (and therefore all keys/addresses), breaking existing wallets.
/// If passphrase support is needed in the future, a separate `to_seed_with_passphrase`
/// function should be added.
pub fn to_seed(phrase: &str) -> Result<[u8; 64], CryptoError> {
    let mnemonic: Mnemonic<English> = Mnemonic::from_phrase(phrase)
        .map_err(|e| CryptoError::MnemonicError(format!("{e:?}")))?;
    let seed = mnemonic.to_seed("");
    let mut result = [0u8; 64];
    result.copy_from_slice(&seed[..64]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mnemonic() {
        let phrase = generate().unwrap();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        assert_eq!(words.len(), 24);
    }

    #[test]
    fn test_validate_mnemonic() {
        let phrase = generate().unwrap();
        assert!(validate(&phrase));
        assert!(!validate("invalid mnemonic phrase"));
        assert!(!validate(""));
    }

    #[test]
    fn test_mnemonic_to_seed() {
        let phrase = generate().unwrap();
        let seed = to_seed(&phrase).unwrap();
        assert_eq!(seed.len(), 64);
        assert!(seed.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_deterministic_seed() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        assert!(validate(phrase));
        let seed1 = to_seed(phrase).unwrap();
        let seed2 = to_seed(phrase).unwrap();
        assert_eq!(seed1, seed2);
    }
}
