//! ZipherX cryptographic operations — Sapling, Groth16, tree, witness.
//!
//! This crate provides the core Sapling protocol implementation for Zclassic:
//! - Key derivation (ZIP-32 spending keys, viewing keys, addresses)
//! - Note encryption/decryption (trial decryption, parallel scanning)
//! - Commitment tree (incremental merkle tree of note commitments)
//! - Witness management (merkle paths for spending proofs)
//! - Transaction building (Groth16 zk-SNARK proofs)
//! - Equihash verification (block header PoW)
//! - Mnemonic generation (BIP-39 24-word seeds)
//!
//! All APIs are idiomatic Rust. The C FFI layer lives in `zipherx-ffi`.

pub mod types;
pub mod mnemonic;
pub mod keys;
pub mod address;
pub mod notes;
pub mod tree;
pub mod witness;
pub mod prover;
pub mod transaction;
pub mod equihash;
pub mod boost_scan;
pub mod util;
pub mod zstd_decompress;

// Re-export key types for convenience
pub use types::{ZclassicNetwork, CryptoError};
