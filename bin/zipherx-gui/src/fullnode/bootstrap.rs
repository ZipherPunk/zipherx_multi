//! Bootstrap Manager — download and extract blockchain bootstrap.
//!
//! Speeds up initial full node sync by downloading a pre-built
//! blockchain database instead of syncing from genesis.

use std::path::PathBuf;

/// Bootstrap download/extraction status.
#[derive(Debug, Clone, PartialEq)]
pub enum BootstrapStatus {
    /// No bootstrap operation in progress.
    Idle,
    /// Downloading bootstrap file.
    Downloading {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    /// Extracting bootstrap archive.
    Extracting { progress: f32 },
    /// Bootstrap complete.
    Complete,
    /// Error occurred.
    Error(String),
}

/// Manages blockchain bootstrap downloads.
pub struct BootstrapManager {
    pub status: BootstrapStatus,
    pub data_dir: PathBuf,
    /// URL for the bootstrap file (if available).
    pub bootstrap_url: Option<String>,
}

impl BootstrapManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            status: BootstrapStatus::Idle,
            data_dir,
            bootstrap_url: None,
        }
    }

    /// Check if a bootstrap is needed (no blocks directory or very few blocks).
    pub fn needs_bootstrap(&self) -> bool {
        let blocks_dir = self.data_dir.join("blocks");
        if !blocks_dir.exists() {
            return true;
        }
        // Check if blocks directory is nearly empty
        let size = dir_size(&blocks_dir);
        size < 100 * 1024 * 1024 // Less than 100MB suggests no meaningful chain data
    }

    /// Check if blockchain data already exists.
    pub fn has_chain_data(&self) -> bool {
        let blocks_dir = self.data_dir.join("blocks");
        blocks_dir.exists() && dir_size(&blocks_dir) > 100 * 1024 * 1024
    }

    /// Get the size of the blockchain data directory.
    pub fn chain_data_size(&self) -> u64 {
        dir_size(&self.data_dir)
    }

    /// Delete blockchain data to allow re-bootstrap or fresh sync.
    pub fn delete_chain_data(&self) -> Result<(), String> {
        let blocks_dir = self.data_dir.join("blocks");
        let chainstate_dir = self.data_dir.join("chainstate");

        if blocks_dir.exists() {
            std::fs::remove_dir_all(&blocks_dir)
                .map_err(|e| format!("Failed to remove blocks: {}", e))?;
        }
        if chainstate_dir.exists() {
            std::fs::remove_dir_all(&chainstate_dir)
                .map_err(|e| format!("Failed to remove chainstate: {}", e))?;
        }
        Ok(())
    }
}

/// Calculate directory size recursively.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                total += dir_size(&entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    total
}
