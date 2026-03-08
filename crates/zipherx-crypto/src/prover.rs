//! Groth16 prover initialization and proof progress tracking.
//!
//! The prover generates zk-SNARK proofs for Sapling spend and output circuits.
//! Parameters: sapling-spend.params (47MB), sapling-output.params (3.5MB).

use std::sync::Mutex;
use std::path::Path;

use zcash_proofs::prover::LocalTxProver;
use zcash_proofs::ZcashParameters;

use crate::types::{CryptoError, SPEND_PARAMS_SIZE, OUTPUT_PARAMS_SIZE};

/// Global prover instance (thread-safe).
static PROVER: Mutex<Option<LocalTxProver>> = Mutex::new(None);

/// Global verifying keys (for pre-broadcast TX verification).
#[allow(dead_code)]
static VERIFYING_KEYS: Mutex<Option<ZcashParameters>> = Mutex::new(None);

/// Initialize the prover from file paths.
///
/// Validates file sizes before loading:
/// - Spend params: exactly 47,958,396 bytes
/// - Output params: exactly 3,592,860 bytes
pub fn init_from_files(spend_path: &str, output_path: &str) -> Result<(), CryptoError> {
    // Validate file sizes
    let spend_meta = std::fs::metadata(spend_path)
        .map_err(|e| CryptoError::ProverInitFailed(format!("Spend params: {e}")))?;
    let output_meta = std::fs::metadata(output_path)
        .map_err(|e| CryptoError::ProverInitFailed(format!("Output params: {e}")))?;

    if spend_meta.len() != SPEND_PARAMS_SIZE {
        return Err(CryptoError::ProverInitFailed(format!(
            "Spend params wrong size: {} (expected {SPEND_PARAMS_SIZE})",
            spend_meta.len()
        )));
    }
    if output_meta.len() != OUTPUT_PARAMS_SIZE {
        return Err(CryptoError::ProverInitFailed(format!(
            "Output params wrong size: {} (expected {OUTPUT_PARAMS_SIZE})",
            output_meta.len()
        )));
    }

    let prover = LocalTxProver::new(
        Path::new(spend_path),
        Path::new(output_path),
    );

    let mut guard = PROVER.lock()
        .map_err(|e| CryptoError::ProverInitFailed(format!("Lock: {e}")))?;
    *guard = Some(prover);

    Ok(())
}

/// Initialize the prover from in-memory byte slices.
pub fn init_from_bytes(spend_data: &[u8], output_data: &[u8]) -> Result<(), CryptoError> {
    if spend_data.len() as u64 != SPEND_PARAMS_SIZE {
        return Err(CryptoError::ProverInitFailed(format!(
            "Spend params wrong size: {} (expected {SPEND_PARAMS_SIZE})",
            spend_data.len()
        )));
    }
    if output_data.len() as u64 != OUTPUT_PARAMS_SIZE {
        return Err(CryptoError::ProverInitFailed(format!(
            "Output params wrong size: {} (expected {OUTPUT_PARAMS_SIZE})",
            output_data.len()
        )));
    }

    let prover = LocalTxProver::from_bytes(spend_data, output_data);

    let mut guard = PROVER.lock()
        .map_err(|e| CryptoError::ProverInitFailed(format!("Lock: {e}")))?;
    *guard = Some(prover);

    Ok(())
}

/// Check if the prover is initialized.
pub fn is_initialized() -> bool {
    PROVER.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

/// Get a reference to the global prover (for transaction building).
///
/// Returns an error if the prover is not initialized.
///
/// # Concurrency (RCR-14)
///
/// This returns a `MutexGuard` over the global `PROVER` instance. The lock is
/// held for the entire duration of transaction building (proof generation),
/// which means concurrent `build_transaction` calls will block on this lock.
/// This is intentional — the prover is not reentrant and proof generation is
/// CPU-intensive. Callers should not attempt parallel TX builds.
pub(crate) fn get_prover() -> Result<std::sync::MutexGuard<'static, Option<LocalTxProver>>, CryptoError> {
    let guard = PROVER.lock()
        .map_err(|e| CryptoError::ProverInitFailed(format!("Lock: {e}")))?;
    if guard.is_none() {
        return Err(CryptoError::ProverNotInitialized);
    }
    Ok(guard)
}

/// Get a reference to the verifying keys (for TX verification).
#[allow(dead_code)]
pub(crate) fn get_verifying_keys() -> Result<std::sync::MutexGuard<'static, Option<ZcashParameters>>, CryptoError> {
    let guard = VERIFYING_KEYS.lock()
        .map_err(|e| CryptoError::ProverInitFailed(format!("Lock: {e}")))?;
    Ok(guard)
}

// Proof progress tracking — uses atomics from zcash_primitives
// These are re-exported for the FFI layer to poll during proof generation.

/// Get the total number of proofs to generate for the current TX.
pub fn proof_total() -> u32 {
    zcash_primitives::transaction::components::sapling::builder::GROTH16_PROOFS_TOTAL
        .load(std::sync::atomic::Ordering::Relaxed) as u32
}

/// Get the number of proofs completed so far.
pub fn proof_completed() -> u32 {
    zcash_primitives::transaction::components::sapling::builder::GROTH16_PROOFS_COMPLETED
        .load(std::sync::atomic::Ordering::Relaxed) as u32
}

/// Get the number of proof threads in use.
pub fn proof_threads() -> u32 {
    zcash_primitives::transaction::components::sapling::builder::GROTH16_PROOF_THREADS
        .load(std::sync::atomic::Ordering::Relaxed) as u32
}

/// Cancel ongoing proof generation.
pub fn cancel_proof() {
    zcash_primitives::transaction::components::sapling::builder::GROTH16_CANCEL
        .store(true, std::sync::atomic::Ordering::SeqCst);
}
