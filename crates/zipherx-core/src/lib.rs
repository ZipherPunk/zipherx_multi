//! ZipherX core wallet logic — orchestrates crypto, network, and storage.
//!
//! This crate contains the high-level wallet operations that coordinate
//! between the crypto, network, storage, and platform layers.

pub mod scanner;
pub mod sync;
pub mod send;
pub mod wallet;
pub mod runtime;
pub mod async_block_fetch;
pub mod async_sync;
pub mod async_send;
pub mod async_wallet;
pub mod async_prover;
pub mod boost_download;
pub mod health_check;
pub mod tree_repair;
pub mod peer_watchdog;
pub mod auto_recovery;

use thiserror::Error;

/// Core wallet errors.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Network error: {0}")]
    Network(#[from] zipherx_network::types::NetworkError),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Platform error: {0}")]
    Platform(#[from] zipherx_platform::PlatformError),

    #[error("Wallet not initialized")]
    WalletNotInitialized,

    #[error("Wallet locked — biometric auth required")]
    WalletLocked,

    // RC-15: PRIVACY NOTE — This error variant exposes exact wallet balance (`have`)
    // and the attempted spend amount (`need`) in its Display impl. The FFI layer
    // should sanitize this before showing to untrusted contexts (e.g., logs shipped
    // to analytics). The values are needed by the UI to display meaningful messages.
    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },

    #[error("Invalid anchor — tree root not found on blockchain")]
    InvalidAnchor,

    #[error("Broadcast failed: {0}")]
    BroadcastFailed(String),

    #[error("Sync in progress")]
    SyncInProgress,

    #[error("Repair in progress")]
    RepairInProgress,

    #[error("Runtime not initialized — call initialize_runtime() first")]
    RuntimeNotInitialized,

    #[error("Runtime has been shut down")]
    RuntimeShutdown,

    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Broadcasting in progress")]
    BroadcastingInProgress,

    #[error("Gap-fill in progress")]
    GapFillInProgress,

    #[error("Invalid witness: {0}")]
    InvalidWitness(String),

    #[error("Prover not initialized — load Sapling params first")]
    ProverNotInitialized,

    #[error("Transaction build failed: {0}")]
    TransactionBuildFailed(String),
}
