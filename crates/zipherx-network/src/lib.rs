//! ZipherX P2P networking layer.
//!
//! Implements the Bitcoin-derived P2P protocol for Zclassic:
//! - TCP peer connections (via tokio)
//! - Message framing and parsing
//! - Peer scoring, banning, rotation
//! - Block fetching and header sync
//! - Transaction broadcast
//! - SOCKS5 proxy for Tor routing

pub mod block_fetcher;
pub mod broadcast;
pub mod constants;
pub mod dispatcher;
pub mod header_sync;
pub mod messages;
pub mod peer;
pub mod peer_manager;
pub mod protocol;
pub mod socks5;
pub mod types;
