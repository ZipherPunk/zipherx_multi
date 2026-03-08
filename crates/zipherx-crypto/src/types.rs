//! Shared types for ZipherX crypto — network parameters, errors, constants.

use thiserror::Error;
use zcash_primitives::consensus::{BlockHeight, NetworkUpgrade, Parameters};

// ============================================================================
// Zclassic Network Parameters
// ============================================================================

/// Zclassic mainnet parameters.
///
/// Implements the `Parameters` trait from zcash_primitives with
/// Zclassic-specific activation heights and network identifiers.
#[derive(Clone, Copy, Debug)]
pub struct ZclassicNetwork;

impl Parameters for ZclassicNetwork {
    fn activation_height(&self, nu: NetworkUpgrade) -> Option<BlockHeight> {
        match nu {
            NetworkUpgrade::Overwinter => Some(BlockHeight::from_u32(476_969)),
            NetworkUpgrade::Sapling => Some(BlockHeight::from_u32(476_969)),
            // Zcash-specific upgrades — not applicable to Zclassic
            NetworkUpgrade::Blossom => None,
            NetworkUpgrade::Heartwood => None,
            NetworkUpgrade::Canopy => None,
            NetworkUpgrade::Nu5 => None,
            // Zclassic Buttercup (branch ID 0x930b540d) — currently active
            NetworkUpgrade::ZclassicButtercup => Some(BlockHeight::from_u32(707_000)),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    fn coin_type(&self) -> u32 {
        147 // ZCL SLIP-44 coin type
    }

    fn address_network(&self) -> Option<zcash_address::Network> {
        Some(zcash_address::Network::Main)
    }

    fn hrp_sapling_extended_spending_key(&self) -> &str {
        "secret-extended-key-main"
    }

    fn hrp_sapling_extended_full_viewing_key(&self) -> &str {
        "zviews"
    }

    fn hrp_sapling_payment_address(&self) -> &str {
        "zs"
    }

    fn b58_pubkey_address_prefix(&self) -> [u8; 2] {
        [0x1C, 0xB8] // Zclassic t1 prefix
    }

    fn b58_script_address_prefix(&self) -> [u8; 2] {
        [0x1C, 0xBD] // Zclassic t3 prefix
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Cryptographic operation errors.
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Invalid seed: expected 32 or 64 bytes, got {0}")]
    InvalidSeed(usize),

    #[error("Invalid spending key data")]
    InvalidSpendingKey,

    #[error("Invalid viewing key data")]
    InvalidViewingKey,

    #[error("Invalid diversifier at index {0}")]
    InvalidDiversifier(u64),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Note decryption failed")]
    DecryptionFailed,

    #[error("Invalid note commitment")]
    InvalidCommitment,

    #[error("Invalid nullifier")]
    InvalidNullifier,

    #[error("Tree operation failed: {0}")]
    TreeError(String),

    #[error("Tree is corrupted")]
    TreeCorrupted,

    #[error("Witness operation failed: {0}")]
    WitnessError(String),

    #[error("Invalid witness data")]
    InvalidWitness,

    #[error("Prover not initialized")]
    ProverNotInitialized,

    #[error("Prover initialization failed: {0}")]
    ProverInitFailed(String),

    #[error("Proof generation failed: {0}")]
    ProofGenerationFailed(String),

    #[error("Proof generation cancelled")]
    ProofCancelled,

    #[error("Transaction build failed: {0}")]
    TransactionBuildFailed(String),

    #[error("Equihash verification failed")]
    EquihashVerificationFailed,

    #[error("Invalid block header")]
    InvalidBlockHeader,

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Mnemonic error: {0}")]
    MnemonicError(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Buffer too small: need {needed}, have {available}")]
    BufferTooSmall { needed: usize, available: usize },

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),
}

// ============================================================================
// Constants
// ============================================================================

/// Sapling commitment tree depth.
pub const SAPLING_TREE_DEPTH: u8 = 32;

/// Sapling activation height for Zclassic.
pub const SAPLING_ACTIVATION_HEIGHT: u32 = 476_969;

/// Buttercup activation height for Zclassic.
pub const BUTTERCUP_ACTIVATION_HEIGHT: u32 = 707_000;

/// Default transaction fee in zatoshis.
pub const DEFAULT_FEE: u64 = 10_000;

/// Spending key serialized length (ExtendedSpendingKey).
pub const SPENDING_KEY_LENGTH: usize = 169;

/// Diversifier length.
pub const DIVERSIFIER_LENGTH: usize = 11;

/// Payment address length (diversifier + pk_d).
pub const PAYMENT_ADDRESS_LENGTH: usize = 43;

/// Note commitment (CMU) length.
pub const CMU_LENGTH: usize = 32;

/// Nullifier length.
pub const NULLIFIER_LENGTH: usize = 32;

/// Ephemeral public key length.
pub const EPK_LENGTH: usize = 32;

/// Encrypted ciphertext length (Sapling note encryption).
pub const ENC_CIPHERTEXT_LEN: usize = 580;

/// Memo field length.
pub const MEMO_LENGTH: usize = 512;

/// Decrypted note output: diversifier(11) + value(8) + rcm(32) + memo(512) = 563.
/// Actual output from try_decrypt is 564 bytes (includes an extra byte).
pub const DECRYPTED_NOTE_LENGTH: usize = 564;

/// Boost file output record size: height(4) + index(4) + cmu(32) + epk(32) + ciphertext(580) + txid(32).
pub const BOOST_OUTPUT_SIZE: usize = 684;

/// Boost file spend record size: height(4) + nullifier(32) + txid(32).
pub const BOOST_SPEND_SIZE: usize = 68;

/// Sapling spend params file size (for validation).
pub const SPEND_PARAMS_SIZE: u64 = 47_958_396;

/// Sapling output params file size (for validation).
pub const OUTPUT_PARAMS_SIZE: u64 = 3_592_860;

/// Equihash(192,7) solution size (post-Bubbles).
pub const EQUIHASH_SOLUTION_SIZE_192_7: usize = 400;

/// Equihash(200,9) solution size (pre-Bubbles).
pub const EQUIHASH_SOLUTION_SIZE_200_9: usize = 1344;

/// Block header base size (without solution).
pub const BLOCK_HEADER_BASE_SIZE: usize = 140;

/// AES-GCM encrypted spending key bundle size: nonce(12) + ciphertext(169) + tag(16) = 197.
pub const ENCRYPTED_SK_SIZE: usize = 197;
