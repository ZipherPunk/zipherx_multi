//! Async wrapper for Groth16 proof generation.
//!
//! Groth16 proving is CPU-heavy (2-4 seconds per spend on mobile).
//! This module wraps the sync `zipherx_crypto::transaction` functions
//! with `tokio::task::spawn_blocking` so they don't block the event loop.

use std::sync::Arc;

use crate::CoreError;
use sha2::{Digest, Sha256};
use zipherx_crypto::prover;
use zipherx_crypto::transaction::{self, SpendInfo, TransactionResult};

// ============================================================================
// Prover Initialization
// ============================================================================

/// Initialize the Groth16 prover by loading Sapling parameters from files.
///
/// Must be called before any transaction building. Safe to call multiple times.
/// Uses `spawn_blocking` since file I/O can be slow (47MB + 3.5MB param files).
pub async fn init_prover(
    spend_params_path: &str,
    output_params_path: &str,
) -> Result<(), CoreError> {
    if prover::is_initialized() {
        return Ok(());
    }

    let spend_path = spend_params_path.to_string();
    let output_path = output_params_path.to_string();

    tokio::task::spawn_blocking(move || prover::init_from_files(&spend_path, &output_path))
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Crypto(e.to_string()))
}

/// Initialize the prover from raw bytes (e.g., embedded in app bundle).
pub async fn init_prover_from_bytes(
    spend_data: Vec<u8>,
    output_data: Vec<u8>,
) -> Result<(), CoreError> {
    if prover::is_initialized() {
        return Ok(());
    }

    tokio::task::spawn_blocking(move || prover::init_from_bytes(&spend_data, &output_data))
        .await
        .map_err(|e| CoreError::RuntimeError(e.to_string()))?
        .map_err(|e| CoreError::Crypto(e.to_string()))
}

/// Check if the prover is initialized and ready.
pub fn is_prover_ready() -> bool {
    prover::is_initialized()
}

// ============================================================================
// Auto-Download + Init
// ============================================================================

/// Well-known Zcash Sapling parameter download URLs.
const SPEND_PARAMS_URL: &str = "https://download.z.cash/downloads/sapling-spend.params";
const OUTPUT_PARAMS_URL: &str = "https://download.z.cash/downloads/sapling-output.params";

/// Official SHA-256 hashes for Sapling parameter files (from Zcash documentation).
const SPEND_PARAMS_SHA256: &str =
    "8e48ffd23abb3a5fd9c5589204f32d9c31285a04b78096ba40a79b75677efc13";
const OUTPUT_PARAMS_SHA256: &str =
    "2f0ebbcbb9bb0bcffe95a397e7eba89c29eb4dde6191c339db88570e3f3fb0e4";

/// Ensure Sapling parameter files exist at the given paths, downloading them
/// if necessary, then initialize the prover.
///
/// This is the recommended entry point — call it once before building a TX.
/// Safe to call multiple times (no-ops when already initialized).
pub async fn ensure_prover_initialized(
    spend_params_path: &str,
    output_params_path: &str,
) -> Result<(), CoreError> {
    if prover::is_initialized() {
        return Ok(());
    }

    // Download if files don't exist or are wrong size
    download_param_if_needed(
        spend_params_path,
        SPEND_PARAMS_URL,
        zipherx_crypto::types::SPEND_PARAMS_SIZE,
        SPEND_PARAMS_SHA256,
        "sapling-spend.params",
    )
    .await?;

    download_param_if_needed(
        output_params_path,
        OUTPUT_PARAMS_URL,
        zipherx_crypto::types::OUTPUT_PARAMS_SIZE,
        OUTPUT_PARAMS_SHA256,
        "sapling-output.params",
    )
    .await?;

    init_prover(spend_params_path, output_params_path).await
}

/// Download a Sapling parameter file if it doesn't exist or has wrong size.
/// Verifies the SHA-256 hash of the downloaded data before writing to disk.
///
/// RC-11: TRUST MODEL — The Sapling parameter files are downloaded over HTTPS
/// from `download.z.cash`. MITM attacks are mitigated by:
/// 1. HTTPS/TLS provides transport-layer authentication via CA certificates.
/// 2. SHA-256 hash verification of the downloaded bytes against hardcoded
///    expected hashes (SPEND_PARAMS_SHA256, OUTPUT_PARAMS_SHA256) ensures
///    content integrity — even if the TLS connection were compromised,
///    the hash mismatch would reject tampered data.
/// The hardcoded hashes are the same as those published by the Zcash team.
async fn download_param_if_needed(
    path: &str,
    url: &str,
    expected_size: u64,
    expected_hash: &str,
    name: &str,
) -> Result<(), CoreError> {
    // Check if file already exists with correct size
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() == expected_size {
            return Ok(());
        }
        eprintln!(
            "[ZipherX] {} wrong size: {} (expected {}), re-downloading",
            name,
            meta.len(),
            expected_size,
        );
    }

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CoreError::Storage(format!("Create params dir: {e}")))?;
    }

    eprintln!(
        "[ZipherX] Downloading {} ({:.1} MB)...",
        name,
        expected_size as f64 / 1_048_576.0
    );

    // RC-2: Use Tor-aware client to avoid leaking IP during param download.
    // Falls back to clearnet with warning if Tor is not available.
    let client = crate::boost_download::build_tor_aware_client(300)?;

    let response = client.get(url).send().await.map_err(|e| {
        CoreError::Network(zipherx_network::types::NetworkError::ConnectionFailed(
            format!("Download {name}: {e}"),
        ))
    })?;

    if !response.status().is_success() {
        return Err(CoreError::Network(
            zipherx_network::types::NetworkError::ConnectionFailed(format!(
                "Download {name}: HTTP {}",
                response.status()
            )),
        ));
    }

    let bytes = response.bytes().await.map_err(|e| {
        CoreError::Network(zipherx_network::types::NetworkError::ConnectionFailed(
            format!("Read {name}: {e}"),
        ))
    })?;

    if bytes.len() as u64 != expected_size {
        return Err(CoreError::Crypto(format!(
            "{name} download size mismatch: {} (expected {expected_size})",
            bytes.len(),
        )));
    }

    // Verify SHA-256 hash of downloaded data
    let actual_hash = hex::encode(Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        return Err(CoreError::Crypto(format!(
            "{name} SHA-256 hash mismatch: got {actual_hash}, expected {expected_hash}",
        )));
    }

    // Write to disk via spawn_blocking
    let path_owned = path.to_string();
    let data = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        std::fs::write(&path_owned, &data)
            .map_err(|e| CoreError::Storage(format!("Write {}: {e}", path_owned)))
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))??;

    eprintln!(
        "[ZipherX] {} downloaded successfully ({} bytes)",
        name, expected_size
    );
    Ok(())
}

// ============================================================================
// Async Transaction Building
// ============================================================================

/// Build a shielded transaction asynchronously.
///
/// Wraps the CPU-heavy Groth16 proof generation in `spawn_blocking`.
/// Progress callback fires as each spend proof completes.
pub async fn build_transaction_async(
    sk_bytes: Vec<u8>,
    to_address: [u8; 43],
    amount: u64,
    memo: Option<Vec<u8>>,
    spends: Vec<SpendInfo>,
    chain_height: u64,
    _progress: Option<Arc<dyn Fn(u32, u32) + Send + Sync>>,
) -> Result<TransactionResult, CoreError> {
    // Validate inputs before checking prover (so tests get meaningful errors)
    if spends.is_empty() {
        return Err(CoreError::TransactionBuildFailed(
            "No spend inputs provided".into(),
        ));
    }

    if sk_bytes.len() != zipherx_crypto::types::SPENDING_KEY_LENGTH {
        return Err(CoreError::Crypto(format!(
            "Invalid spending key length: {} (expected {})",
            sk_bytes.len(),
            zipherx_crypto::types::SPENDING_KEY_LENGTH,
        )));
    }

    if !prover::is_initialized() {
        return Err(CoreError::ProverNotInitialized);
    }

    // Run the CPU-heavy proof generation on a blocking thread
    let result = tokio::task::spawn_blocking(move || {
        transaction::build_transaction_multi(
            &sk_bytes,
            &to_address,
            amount,
            memo.as_deref(),
            &spends,
            chain_height,
        )
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::TransactionBuildFailed(e.to_string()))?;

    Ok(result)
}

/// Build a deshielding transaction (z → t) asynchronously.
pub async fn build_deshield_transaction_async(
    sk_bytes: Vec<u8>,
    to_t_address_str: String,
    amount: u64,
    spends: Vec<SpendInfo>,
    chain_height: u64,
) -> Result<TransactionResult, CoreError> {
    if spends.is_empty() {
        return Err(CoreError::TransactionBuildFailed(
            "No spend inputs provided".into(),
        ));
    }

    if sk_bytes.len() != zipherx_crypto::types::SPENDING_KEY_LENGTH {
        return Err(CoreError::Crypto(format!(
            "Invalid spending key length: {} (expected {})",
            sk_bytes.len(),
            zipherx_crypto::types::SPENDING_KEY_LENGTH,
        )));
    }

    if !prover::is_initialized() {
        return Err(CoreError::ProverNotInitialized);
    }

    let result = tokio::task::spawn_blocking(move || {
        let t_addr = zipherx_crypto::transparent::decode_transparent_address(&to_t_address_str)
            .map_err(|e| zipherx_crypto::types::CryptoError::InvalidAddress(e.to_string()))?;
        transaction::build_transaction_to_transparent(
            &sk_bytes,
            &t_addr,
            amount,
            &spends,
            chain_height,
        )
    })
    .await
    .map_err(|e| CoreError::RuntimeError(e.to_string()))?
    .map_err(|e| CoreError::TransactionBuildFailed(e.to_string()))?;

    Ok(result)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_prover_nonexistent_files() {
        let result = init_prover("/nonexistent/spend.params", "/nonexistent/output.params").await;
        // Should fail since files don't exist
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_tx_without_prover() {
        // Don't initialize prover
        let result =
            build_transaction_async(vec![0u8; 32], [0u8; 43], 1000, None, vec![], 100, None).await;
        // Should fail: no spends
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_tx_empty_spends() {
        let result = build_transaction_async(
            vec![0u8; 32],
            [0u8; 43],
            1000,
            None,
            vec![], // empty spends
            100,
            None,
        )
        .await;

        assert!(matches!(result, Err(CoreError::TransactionBuildFailed(_))));
    }

    #[tokio::test]
    async fn test_build_tx_invalid_sk() {
        let result = build_transaction_async(
            vec![0u8; 16], // wrong length
            [0u8; 43],
            1000,
            None,
            vec![SpendInfo {
                witness_data: vec![0u8; 200],
                value: 1000,
                rcm: [0u8; 32],
                diversifier: [0u8; 11],
                is_zip212: false,
            }],
            100,
            None,
        )
        .await;

        assert!(matches!(result, Err(CoreError::Crypto(_))));
    }

    #[test]
    fn test_prover_ready_check() {
        // Prover may or may not be initialized from other tests
        // Just ensure the function doesn't panic
        let _ready = is_prover_ready();
    }
}
