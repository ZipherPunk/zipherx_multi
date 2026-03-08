//! ZipherX storage layer — SQLCipher database + file I/O.
//!
//! Provides encrypted persistent storage for:
//! - Shielded notes (received ZCL)
//! - Transaction history
//! - Sync state and checkpoints
//! - Block headers (HeaderStore)
//! - Delta CMU bundles
//! - Sapling roots

pub mod schema;
pub mod types;
pub mod encryption;

pub mod database;
pub mod header_store_impl;

pub mod delta_cmu;

// Future modules (Phase 4+):
// pub mod boost_file;        // Boost file reader/writer (Phase 4)
// pub mod migration;         // Schema migrations (Phase 4)

pub use database::WalletDatabase;
pub use header_store_impl::SqliteHeaderStore;
pub use delta_cmu::DeltaCMUStore;
