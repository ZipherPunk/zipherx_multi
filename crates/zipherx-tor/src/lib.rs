//! ZipherX Tor integration — embedded Arti client.
//!
//! Provides:
//! - Embedded Tor client via Arti (Rust Tor implementation)
//! - SOCKS5 proxy for P2P connections
//! - Hidden service hosting (.onion addresses)
//! - Encrypted messaging (Cypherpunk chat)
//! - HTTP downloads with resume support via Tor

pub mod client;
pub mod download;

pub mod hidden_service;

// Future modules (Phase 2):
// pub mod chat;            // Cypherpunk encrypted messaging
// pub mod socks5;          // SOCKS5 proxy helpers

use thiserror::Error;

/// Tor connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TorState {
    Disconnected = 0,
    Connecting = 1,
    Bootstrapping = 2,
    Connected = 3,
    Error = 4,
}

impl TorState {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Disconnected),
            1 => Some(Self::Connecting),
            2 => Some(Self::Bootstrapping),
            3 => Some(Self::Connected),
            4 => Some(Self::Error),
            _ => None,
        }
    }
}

/// Tor-related errors.
#[derive(Debug, Error)]
pub enum TorError {
    #[error("Tor client not initialized")]
    NotInitialized,

    #[error("Tor bootstrap failed: {0}")]
    BootstrapFailed(String),

    #[error("SOCKS proxy failed to start: {0}")]
    SocksProxyFailed(String),

    #[error("Connection through Tor failed: {0}")]
    ConnectionFailed(String),

    #[error("Hidden service error: {0}")]
    HiddenServiceError(String),

    #[error("Download error: {0}")]
    DownloadError(String),

    #[error("Download cancelled")]
    DownloadCancelled,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_state_from_u8() {
        assert_eq!(TorState::from_u8(0), Some(TorState::Disconnected));
        assert_eq!(TorState::from_u8(1), Some(TorState::Connecting));
        assert_eq!(TorState::from_u8(2), Some(TorState::Bootstrapping));
        assert_eq!(TorState::from_u8(3), Some(TorState::Connected));
        assert_eq!(TorState::from_u8(4), Some(TorState::Error));
        assert_eq!(TorState::from_u8(5), None);
        assert_eq!(TorState::from_u8(255), None);
    }

    #[test]
    fn test_tor_state_repr() {
        assert_eq!(TorState::Disconnected as u8, 0);
        assert_eq!(TorState::Connected as u8, 3);
    }

    #[test]
    fn test_tor_error_display() {
        let err = TorError::NotInitialized;
        assert_eq!(err.to_string(), "Tor client not initialized");
        let err = TorError::BootstrapFailed("timeout".into());
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_client_state_queries_dont_panic() {
        // Smoke test: state queries work regardless of current state
        // (actual state may vary due to parallel test execution)
        let _state = client::get_state();
        let _progress = client::get_bootstrap_progress();
        let _port = client::get_socks_port();
        let _running = client::is_socks_running();
        let _err = client::get_last_error();
    }

    #[test]
    fn test_client_tor_data_dir() {
        let dir = client::get_tor_data_dir();
        // Should return a valid path (platform-dependent)
        assert!(!dir.to_string_lossy().is_empty());
    }

    #[test]
    fn test_download_initial_state() {
        download::reset_state();
        let (bytes, _total, speed) = download::get_progress();
        assert_eq!(bytes, 0);
        assert_eq!(speed, 0.0);
        assert!(!download::is_cancelled());
    }

    #[test]
    fn test_download_progress_tracking() {
        download::reset_state();
        download::set_total_size(1_000_000);
        download::update_progress(500_000, 50.5);

        let (bytes, total, speed) = download::get_progress();
        assert_eq!(bytes, 500_000);
        assert_eq!(total, 1_000_000);
        assert!((speed - 50.5).abs() < 0.001);
    }

    #[test]
    fn test_download_cancel() {
        download::reset_state();
        assert!(!download::is_cancelled());
        download::cancel();
        assert!(download::is_cancelled());
        download::reset_state();
        assert!(!download::is_cancelled());
    }
}
