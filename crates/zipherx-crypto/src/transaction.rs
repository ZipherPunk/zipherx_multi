//! Transaction building — Sapling shielded transactions with Groth16 proofs.
//!
//! Supports:
//! - Single-input transactions
//! - Multi-input transactions (up to 100 spends)
//!
//! TX FLOW: buildTx() -> verifyAnchor() -> broadcast() -> mempool -> block -> confirm
//!
//! CRITICAL: Change address uses random diversifier index for privacy (P-ADDR-002).

use std::io::{Cursor, Read as _};
use std::sync::atomic::{AtomicU64, Ordering};

use zcash_primitives::{
    consensus::BlockHeight,
    memo::MemoBytes,
    sapling::{
        value::NoteValue,
        Diversifier, PaymentAddress, Rseed,
    },
    transaction::{
        builder::Builder,
        components::Amount,
        fees::fixed::FeeRule,
    },
    zip32::{DiversifierIndex, sapling::ExtendedSpendingKey},
};
use ff::PrimeField;
use rand::rngs::OsRng;
use rand::Rng;

use crate::types::{ZclassicNetwork, CryptoError, SPENDING_KEY_LENGTH, DEFAULT_FEE};

/// Maximum serialized transaction size in bytes.
/// Sapling transactions with up to 100 spends should not exceed this.
/// Matches the Zcash network consensus rule for max TX size.
const MAX_TX_SIZE_BYTES: usize = 100_000;

/// Base diversifier index for change addresses (P-ADDR-002).
/// Change addresses use indices starting at this offset to keep them
/// well-separated from normal receiving addresses (indices 0..N).
const CHANGE_DIVERSIFIER_BASE: u64 = 1_000_000_000;

/// Range size for random change diversifier offset.
/// The actual change diversifier index = CHANGE_DIVERSIFIER_BASE + rand(0..this).
const CHANGE_DIVERSIFIER_RANGE: u64 = 1_000_000_000;

/// Maximum number of spend inputs per transaction.
const MAX_SPENDS_PER_TX: usize = 100;

/// Stores the last change diversifier index used in TX construction.
/// Read by the caller after a successful TX build.
///
/// DEPRECATED: This global is kept for backward compatibility. Callers should
/// use `TransactionResult.change_diversifier_index` instead, which is populated
/// from a local variable to avoid TOCTOU races between concurrent TX builds.
static LAST_CHANGE_DIVERSIFIER_INDEX: AtomicU64 = AtomicU64::new(0);

/// Information about a note to spend in a transaction.
#[derive(Debug, Clone)]
pub struct SpendInfo {
    /// Serialized witness (merkle path).
    pub witness_data: Vec<u8>,
    /// Note value in zatoshis.
    pub value: u64,
    /// Note randomness commitment (32 bytes) — raw rcm for BeforeZip212, rseed for AfterZip212.
    pub rcm: [u8; 32],
    /// Note diversifier (11 bytes).
    pub diversifier: [u8; 11],
    /// Whether this note uses ZIP-212 (AfterZip212) rseed format (M-5).
    /// If true, `rcm` contains the rseed and actual rcm is PRF-derived.
    pub is_zip212: bool,
}

/// Result of building a transaction.
#[derive(Debug, Clone)]
pub struct TransactionResult {
    /// Serialized transaction bytes (ready for broadcast).
    pub tx_bytes: Vec<u8>,
    /// Nullifiers for all spent notes (32 bytes each).
    pub nullifiers: Vec<[u8; 32]>,
    /// Change diversifier index used.
    pub change_diversifier_index: u64,
}

/// Get the last change diversifier index used.
pub fn last_change_diversifier_index() -> u64 {
    LAST_CHANGE_DIVERSIFIER_INDEX.load(Ordering::SeqCst)
}

/// Build a shielded transaction with a single input note.
///
/// # Arguments
/// * `sk_bytes` - ExtendedSpendingKey (169 bytes)
/// * `to_address` - Destination payment address (43 bytes)
/// * `amount` - Amount to send in zatoshis
/// * `memo` - Optional memo (up to 512 bytes)
/// * `spend` - Note to spend (witness, value, rcm, diversifier)
/// * `chain_height` - Current chain height for branch ID selection
pub fn build_transaction(
    sk_bytes: &[u8],
    to_address: &[u8; 43],
    amount: u64,
    memo: Option<&[u8]>,
    spend: &SpendInfo,
    chain_height: u64,
) -> Result<TransactionResult, CryptoError> {
    build_transaction_multi(sk_bytes, to_address, amount, memo, &[spend.clone()], chain_height)
}

/// Build a shielded transaction with multiple input notes.
///
/// # Arguments
/// * `sk_bytes` - ExtendedSpendingKey (169 bytes)
/// * `to_address` - Destination payment address (43 bytes)
/// * `amount` - Amount to send in zatoshis
/// * `memo` - Optional memo (up to 512 bytes)
/// * `spends` - Notes to spend (1-100)
/// * `chain_height` - Current chain height for branch ID selection
pub fn build_transaction_multi(
    sk_bytes: &[u8],
    to_address: &[u8; 43],
    amount: u64,
    memo: Option<&[u8]>,
    spends: &[SpendInfo],
    chain_height: u64,
) -> Result<TransactionResult, CryptoError> {
    if sk_bytes.len() != SPENDING_KEY_LENGTH {
        return Err(CryptoError::InvalidSpendingKey);
    }
    if spends.is_empty() || spends.len() > MAX_SPENDS_PER_TX {
        return Err(CryptoError::TransactionBuildFailed(
            format!("Invalid spend count: {} (must be 1-{})", spends.len(), MAX_SPENDS_PER_TX),
        ));
    }

    // RCR-NEW-5: Prover errors are propagated via Result — no unwrap in hot path.
    // get_prover() returns Err on lock poisoning or uninitialized state.
    let prover_guard = crate::prover::get_prover()?;
    let prover = prover_guard
        .as_ref()
        .ok_or(CryptoError::ProverNotInitialized)?;

    // Deserialize spending key
    let extsk = ExtendedSpendingKey::read(&mut &sk_bytes[..])
        .map_err(|e| CryptoError::TransactionBuildFailed(format!("Invalid SK: {e:?}")))?;

    // Parse destination address
    let to_addr = PaymentAddress::from_bytes(to_address)
        .ok_or_else(|| CryptoError::InvalidAddress("Invalid payment address".into()))?;

    let fee = DEFAULT_FEE;

    // RCR-NEW-1: Use checked arithmetic to prevent silent overflow on total input sum
    let total_input: u64 = spends.iter().try_fold(0u64, |acc, s| acc.checked_add(s.value))
        .ok_or_else(|| CryptoError::TransactionBuildFailed("total input overflow".into()))?;
    // RCR-8: Use checked arithmetic for amount + fee to prevent overflow
    let amount_plus_fee = amount.checked_add(fee)
        .ok_or_else(|| CryptoError::TransactionBuildFailed("amount + fee overflow".into()))?;
    if total_input < amount_plus_fee {
        return Err(CryptoError::TransactionBuildFailed(format!(
            "Insufficient funds: have {total_input}, need {amount_plus_fee}",
        )));
    }

    // L-5: Guard against height truncation
    if chain_height > u32::MAX as u64 {
        return Err(CryptoError::TransactionBuildFailed(format!(
            "Chain height {} exceeds u32::MAX",
            chain_height,
        )));
    }
    // Create builder
    // RCR-NEW-4: The Builder internally sets expiry_height = target_height + DEFAULT_TX_EXPIRY_DELTA (40 blocks).
    // The third parameter is the orchard anchor (None — ZCL has no Orchard support).
    let target_height = BlockHeight::from_u32(chain_height as u32);
    let mut builder = Builder::new(ZclassicNetwork, target_height, None);

    // Add all spends
    let mut nullifiers = Vec::with_capacity(spends.len());
    for (i, spend) in spends.iter().enumerate() {
        // Parse diversifier and get payment address
        let diversifier = Diversifier(spend.diversifier);
        let fvk = extsk.to_diversifiable_full_viewing_key();
        let note_addr = fvk
            .fvk()
            .vk
            .to_payment_address(diversifier)
            .ok_or_else(|| CryptoError::TransactionBuildFailed(format!("Invalid diversifier for spend {i}")))?;

        // Create note with correct Rseed type (M-5: respect is_zip212 flag)
        let note = if spend.is_zip212 {
            zcash_primitives::sapling::Note::from_parts(
                note_addr,
                NoteValue::from_raw(spend.value),
                Rseed::AfterZip212(spend.rcm),
            )
        } else {
            let rcm = jubjub::Fr::from_repr(spend.rcm)
                .into_option()
                .ok_or_else(|| CryptoError::TransactionBuildFailed(format!("Invalid rcm for spend {i}")))?;
            zcash_primitives::sapling::Note::from_parts(
                note_addr,
                NoteValue::from_raw(spend.value),
                Rseed::BeforeZip212(rcm),
            )
        };

        // Compute nullifier for this note (position is determined by witness)
        let nk = fvk.fvk().vk.nk;

        // Deserialize witness
        // RCR-NEW-3: Bound witness deserialization to 10 MB to prevent unbounded reads
        let mut reader = Cursor::new(&spend.witness_data).take(10 * 1024 * 1024);
        let witness = zcash_primitives::merkle_tree::read_incremental_witness(&mut reader)
            .map_err(|e| CryptoError::TransactionBuildFailed(format!("Invalid witness for spend {i}: {e:?}")))?;

        let position = u64::from(witness.witnessed_position());
        let nullifier = note.nf(&nk, position);
        nullifiers.push(nullifier.0);

        // Get merkle path
        let merkle_path = witness
            .path()
            .ok_or_else(|| CryptoError::TransactionBuildFailed(format!("No path for spend {i}")))?;

        // Add spend to builder
        builder
            .add_sapling_spend(extsk.clone(), diversifier, note, merkle_path)
            .map_err(|e| CryptoError::TransactionBuildFailed(format!("Failed to add spend {i}: {e:?}")))?;
    }

    // Prepare memo
    let memo_bytes = if let Some(m) = memo {
        let mut buf = [0u8; 512];
        let len = m.len().min(512);
        buf[..len].copy_from_slice(&m[..len]);
        // RCR-NEW-7: Use a generic error message to avoid leaking memo content
        MemoBytes::from_bytes(&buf)
            .map_err(|_| CryptoError::TransactionBuildFailed("Invalid memo format".into()))?
    } else {
        MemoBytes::empty()
    };

    // Add output to recipient
    // RCR-1: Guard against u64 → i64 overflow before cast
    if amount > i64::MAX as u64 {
        return Err(CryptoError::TransactionBuildFailed(
            format!("Amount {} exceeds i64::MAX", amount),
        ));
    }
    let amount_val = Amount::from_i64(amount as i64)
        .map_err(|_| CryptoError::TransactionBuildFailed("Invalid amount".into()))?;

    builder
        .add_sapling_output(Some(extsk.expsk.ovk), to_addr, amount_val, memo_bytes)
        .map_err(|e| CryptoError::TransactionBuildFailed(format!("Failed to add output: {e:?}")))?;

    // Add change output if needed
    // RCR-8: Use checked arithmetic for change computation
    let change = total_input.checked_sub(amount_plus_fee)
        .ok_or_else(|| CryptoError::TransactionBuildFailed(
            format!("Insufficient funds: have {total_input}, need {amount_plus_fee}")
        ))?;
    // RCR-NEW-2: Use a local variable for change_diversifier_index to avoid
    // TOCTOU re-read from the global atomic in the return value.
    let mut local_change_div_index: u64 = 0;
    if change > 0 {
        // RCR-1: Guard against u64 → i64 overflow before cast
        if change > i64::MAX as u64 {
            return Err(CryptoError::TransactionBuildFailed(
                format!("Change amount {} exceeds i64::MAX", change),
            ));
        }
        let change_amount = Amount::from_i64(change as i64)
            .map_err(|_| CryptoError::TransactionBuildFailed("Invalid change amount".into()))?;

        // P-ADDR-002: Diversified change address with random offset
        let dfvk = extsk.to_diversifiable_full_viewing_key();
        let change_offset: u64 = OsRng.gen_range(0u64..CHANGE_DIVERSIFIER_RANGE);
        let change_index = CHANGE_DIVERSIFIER_BASE + change_offset;
        local_change_div_index = change_index;
        LAST_CHANGE_DIVERSIFIER_INDEX.store(change_index, Ordering::SeqCst);

        let change_j = DiversifierIndex::from(change_index);
        let (_, change_addr) = dfvk
            .find_address(change_j)
            .unwrap_or_else(|| dfvk.default_address());

        builder
            .add_sapling_output(
                Some(extsk.expsk.ovk),
                change_addr,
                change_amount,
                MemoBytes::empty(),
            )
            .map_err(|e| CryptoError::TransactionBuildFailed(format!("Failed to add change: {e:?}")))?;
    }

    // Build the transaction with Groth16 proofs
    // RCR-1: Guard against u64 → i64 overflow before cast
    if fee > i64::MAX as u64 {
        return Err(CryptoError::TransactionBuildFailed(
            format!("Fee {} exceeds i64::MAX", fee),
        ));
    }
    let fee_amount = Amount::from_i64(fee as i64)
        .map_err(|_| CryptoError::TransactionBuildFailed("Invalid fee".into()))?;

    let (tx, _) = builder
        .build(prover, &FeeRule::non_standard(fee_amount))
        .map_err(|e| CryptoError::TransactionBuildFailed(format!("Build failed: {e:?}")))?;

    // Serialize
    let mut tx_bytes = Vec::new();
    tx.write(&mut tx_bytes)
        .map_err(|e| CryptoError::TransactionBuildFailed(format!("Serialize failed: {e:?}")))?;

    if tx_bytes.len() > MAX_TX_SIZE_BYTES {
        return Err(CryptoError::TransactionBuildFailed(format!(
            "TX too large: {} bytes (max {})",
            tx_bytes.len(), MAX_TX_SIZE_BYTES
        )));
    }

    // RCR-6: SECURITY — `extsk` is dropped here as defense-in-depth, but Rust's
    // default `Drop` does NOT zero memory. The `&[u8]` slice received here cannot
    // be zeroed by this function (immutable borrow).
    //
    // IMPORTANT: Callers MUST wrap `sk_bytes` in `zeroize::Zeroizing<Vec<u8>>` to
    // ensure spending key material is securely zeroed on drop. Without `Zeroizing`,
    // key material may persist in freed memory.
    drop(extsk);

    Ok(TransactionResult {
        tx_bytes,
        nullifiers,
        // RCR-NEW-2: Use local value to avoid TOCTOU race with concurrent TX builds
        change_diversifier_index: local_change_div_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_transaction_no_prover() {
        // Without prover init, should fail with ProverNotInitialized.
        // RCR-NEW-8: The all-zero spending key is intentional — this test exercises
        // the error path (prover not initialized), not the key validity path.
        // An all-zero key is NOT a valid ExtendedSpendingKey.
        let sk = vec![0u8; SPENDING_KEY_LENGTH];
        let to_addr = [0u8; 43];
        let spend = SpendInfo {
            witness_data: vec![],
            value: 100_000,
            rcm: [0u8; 32],
            diversifier: [0u8; 11],
            is_zip212: false,
        };

        let result = build_transaction(&sk, &to_addr, 50_000, None, &spend, 3_000_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_transaction_invalid_sk() {
        let result = build_transaction(
            &[0u8; 16], // Wrong length
            &[0u8; 43],
            50_000,
            None,
            &SpendInfo {
                witness_data: vec![],
                value: 100_000,
                rcm: [0u8; 32],
                diversifier: [0u8; 11],
                is_zip212: false,
            },
            3_000_000,
        );
        assert!(matches!(result, Err(CryptoError::InvalidSpendingKey)));
    }

    #[test]
    fn test_build_transaction_empty_spends() {
        let sk = vec![0u8; SPENDING_KEY_LENGTH];
        let result = build_transaction_multi(&sk, &[0u8; 43], 50_000, None, &[], 3_000_000);
        assert!(result.is_err());
    }

    #[test]
    fn test_last_change_diversifier_index() {
        LAST_CHANGE_DIVERSIFIER_INDEX.store(42, Ordering::SeqCst);
        assert_eq!(last_change_diversifier_index(), 42);
    }
}
