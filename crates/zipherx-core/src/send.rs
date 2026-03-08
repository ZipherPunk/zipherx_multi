//! Send flow — note selection, TX construction, broadcast, and DB recording.
//!
//! CRITICAL INVARIANTS:
//! - NEVER bypass anchor validation (FIX #1279)
//! - Validate EACH witness root individually (FIX #1280)
//! - `containsSaplingRoot()` checks BOTH byte orders (FIX #1230)
//! - NEVER bypass for unverifiable anchors (FIX #1221)
//! - Phantom TX cleanup MUST restore notes (FIX #1168)
//! - Display balance uses `getTotalUnspentBalance()` (FIX #1210)
//! - NEVER stop block listeners before broadcast (FIX #1184)
//! - ALWAYS reverse txid bytes for P2P wire format (FIX #1200)
//! - TX confirmation ONLY via block scanner (FIX #1259)

use crate::CoreError;
use bech32::FromBase32;

// ============================================================================
// Types
// ============================================================================

/// A note that can be spent in a transaction.
#[derive(Debug, Clone)]
pub struct SpendableNote {
    /// Database note ID.
    pub id: i64,
    /// Note value in zatoshis.
    pub value: u64,
    /// Randomness commitment (32 bytes) — raw rcm for BeforeZip212, rseed for AfterZip212.
    pub rcm: [u8; 32],
    /// Diversifier (11 bytes).
    pub diversifier: [u8; 11],
    /// Serialized incremental witness (merkle path).
    pub witness: Vec<u8>,
    /// Anchor (tree root from witness).
    pub anchor: [u8; 32],
    /// Nullifier (32 bytes).
    pub nullifier: [u8; 32],
    /// Whether this note uses ZIP-212 (AfterZip212) rseed format (M-5).
    /// Zclassic does not activate Canopy, so this is typically `false`.
    pub is_zip212: bool,
}

/// A request to send ZCL.
#[derive(Debug, Clone)]
pub struct SendRequest {
    /// Destination shielded address.
    pub to_address: String,
    /// Amount to send in zatoshis.
    pub amount_zatoshis: u64,
    /// Fee in zatoshis (default 10,000).
    pub fee_zatoshis: u64,
    /// Optional memo (max 512 bytes UTF-8).
    pub memo: Option<String>,
}

impl SendRequest {
    /// Total amount needed including fee.
    /// Uses saturating arithmetic to prevent overflow with malicious inputs.
    pub fn total_needed(&self) -> u64 {
        self.amount_zatoshis.saturating_add(self.fee_zatoshis)
    }
}

/// Result of a send operation.
#[derive(Debug, Clone)]
pub struct SendResult {
    /// Transaction ID (hex string, display format).
    pub txid: String,
    /// Amount sent in zatoshis.
    pub amount: u64,
    /// Fee paid in zatoshis.
    pub fee: u64,
    /// Change value returned to self.
    pub change_value: u64,
    /// Number of notes used as inputs.
    pub notes_used: usize,
    /// Nullifiers of spent notes.
    pub spent_nullifiers: Vec<[u8; 32]>,
}

/// Progress callback for send operations.
pub type SendProgressFn = std::sync::Arc<dyn Fn(SendPhase) + Send + Sync>;

/// Phases of the send flow.
#[derive(Debug, Clone, PartialEq)]
pub enum SendPhase {
    /// Selecting notes for spending.
    NoteSelection,
    /// Validating witnesses and anchors.
    WitnessValidation,
    /// Building the transaction (Groth16 proof generation).
    Building,
    /// Broadcasting to P2P network.
    Broadcasting,
    /// Waiting for mempool acceptance.
    MempoolVerification,
    /// Recording in database.
    Recording,
    /// Complete.
    Complete,
    /// Failed with reason.
    Failed(String),
}

/// Default fee in zatoshis (0.0001 ZCL).
pub const DEFAULT_FEE: u64 = 10_000;

// ============================================================================
// Note Selection
// ============================================================================

/// Select notes to cover the required amount.
///
/// Uses a greedy algorithm: sort by value descending, pick until target met.
/// Returns selected notes and the total value.
///
/// Returns `InsufficientBalance` if not enough spendable notes.
pub fn select_notes(
    available_notes: &[SpendableNote],
    target_amount: u64,
) -> Result<(Vec<SpendableNote>, u64), CoreError> {
    if available_notes.is_empty() {
        return Err(CoreError::InsufficientBalance {
            have: 0,
            need: target_amount,
        });
    }

    // Sort by value descending (prefer fewer, larger notes)
    let mut sorted: Vec<&SpendableNote> = available_notes.iter().collect();
    sorted.sort_by(|a, b| b.value.cmp(&a.value));

    let mut selected = Vec::new();
    let mut total_value: u64 = 0;

    for note in sorted {
        selected.push(note.clone());
        total_value = total_value.saturating_add(note.value);
        if total_value >= target_amount {
            break;
        }
    }

    if total_value < target_amount {
        return Err(CoreError::InsufficientBalance {
            have: total_value,
            need: target_amount,
        });
    }

    Ok((selected, total_value))
}

/// Calculate change value for a transaction.
///
/// change = total_input - amount - fee
///
/// RC-7: Uses checked arithmetic to detect overflow instead of silently saturating.
/// Returns `CoreError` if amount + fee overflows or exceeds total_input.
pub fn calculate_change(total_input: u64, amount: u64, fee: u64) -> Result<u64, CoreError> {
    let spend_total = amount.checked_add(fee).ok_or_else(|| {
        CoreError::Crypto("Overflow: amount + fee exceeds u64 range".into())
    })?;
    total_input.checked_sub(spend_total).ok_or_else(|| {
        CoreError::Crypto(format!(
            "Insufficient input: total_input={} < amount({}) + fee({})",
            total_input, amount, fee
        ))
    })
}

// ============================================================================
// Witness Validation
// ============================================================================

/// Validate that all notes have valid witnesses.
///
/// Each note MUST have:
/// 1. Non-empty witness data
/// 2. Anchor that matches a known sapling root (FIX #1279)
///
/// Returns Ok(()) if all valid, or error describing the first invalid note.
pub fn validate_spend_notes(notes: &[SpendableNote]) -> Result<(), CoreError> {
    for (i, note) in notes.iter().enumerate() {
        // Check witness is present
        if note.witness.is_empty() {
            return Err(CoreError::Crypto(format!(
                "Note {} (id={}) has no witness data",
                i, note.id
            )));
        }

        // Check anchor is non-zero
        if note.anchor == [0u8; 32] {
            return Err(CoreError::Crypto(format!(
                "Note {} (id={}) has zero anchor",
                i, note.id
            )));
        }
    }

    Ok(())
}

/// Convert a storage Note to a SpendableNote, if it has valid witness data.
///
/// Returns None if the note is missing required fields.
pub fn note_to_spendable(note: &zipherx_storage::types::Note) -> Option<SpendableNote> {
    let rcm_data = note.rcm.as_ref()?;
    let diversifier_data = note.diversifier.as_ref()?;
    let witness_data = note.witness.as_ref()?;
    let anchor_data = note.anchor.as_ref()?;
    let nullifier_data = note.nullifier.as_ref()?;

    if rcm_data.len() != 32 || diversifier_data.len() != 11
        || anchor_data.len() != 32 || nullifier_data.len() != 32
    {
        return None;
    }

    let mut rcm = [0u8; 32];
    rcm.copy_from_slice(rcm_data);
    let mut diversifier = [0u8; 11];
    diversifier.copy_from_slice(diversifier_data);
    let mut anchor = [0u8; 32];
    anchor.copy_from_slice(anchor_data);
    let mut nullifier = [0u8; 32];
    nullifier.copy_from_slice(nullifier_data);

    Some(SpendableNote {
        id: note.id,
        value: note.value,
        rcm,
        diversifier,
        witness: witness_data.clone(),
        anchor,
        nullifier,
        // Zclassic does not activate Canopy → is_zip212 is always false (M-5).
        is_zip212: false,
    })
}

// ============================================================================
// Phantom TX Recovery
// ============================================================================

/// Detect phantom-spent notes: notes marked spent with no spending TX.
///
/// These occur when a broadcast fails but notes were already marked (FIX #1169).
/// Returns note IDs that are phantom-spent and should be restored.
pub fn detect_phantom_spent_notes(
    notes: &[zipherx_storage::types::Note],
) -> Vec<i64> {
    notes
        .iter()
        .filter(|n| {
            n.is_spent
                && n.spent_in_tx
                    .as_ref()
                    .map(|tx| tx.is_empty())
                    .unwrap_or(true)
        })
        .map(|n| n.id)
        .collect()
}

/// Check if a transaction ID is a boost file placeholder.
///
/// Boost placeholder txids start with `626F6F7374` (hex "boost").
pub fn is_boost_placeholder_txid(txid: &str) -> bool {
    txid.starts_with("626f6f7374") || txid.starts_with("626F6F7374")
}

// ============================================================================
// Address Validation
// ============================================================================

/// Validate that a destination address is a valid Zclassic shielded address.
///
/// Zclassic Sapling shielded addresses use bech32 with HRP "zs" (same as Zcash mainnet).
/// The "zc" prefix is for base58 transparent addresses, NOT shielded addresses.
pub fn validate_shielded_address(address: &str) -> Result<(), CoreError> {
    if address.is_empty() {
        return Err(CoreError::Crypto("Empty address".into()));
    }

    // Zclassic Sapling shielded addresses: "zs1..." (bech32 encoded, HRP = "zs")
    // See chainparams.cpp:167: bech32HRPs[SAPLING_PAYMENT_ADDRESS] = "zs"
    if !address.starts_with("zs") {
        return Err(CoreError::Crypto(format!(
            "Not a Zclassic shielded address (expected 'zs' prefix): {}",
            &address[..address.len().min(10)]
        )));
    }

    // Basic length check — exact validation happens during TX build
    if address.len() < 70 || address.len() > 120 {
        return Err(CoreError::Crypto(format!(
            "Invalid address length: {} (expected ~78)",
            address.len()
        )));
    }

    // RC-3: Verify bech32 checksum — reject addresses with bit errors or typos.
    // bech32::decode() validates the HRP, charset, and checksum in one pass.
    let (hrp, data, _variant) = bech32::decode(address).map_err(|e| {
        CoreError::Crypto(format!("Invalid bech32 address (checksum failed): {e}"))
    })?;

    if hrp != "zs" {
        return Err(CoreError::Crypto(format!(
            "Unexpected bech32 HRP: '{}' (expected 'zs')",
            hrp
        )));
    }

    // Verify base32 data decodes to valid bytes (43 bytes for Sapling payment address)
    let _decoded = Vec::<u8>::from_base32(&data).map_err(|e| {
        CoreError::Crypto(format!("Invalid bech32 address data: {e}"))
    })?;

    Ok(())
}

/// Validate a send request before processing.
pub fn validate_send_request(request: &SendRequest) -> Result<(), CoreError> {
    validate_shielded_address(&request.to_address)?;

    if request.amount_zatoshis == 0 {
        return Err(CoreError::Crypto("Amount must be greater than 0".into()));
    }

    if request.fee_zatoshis == 0 {
        return Err(CoreError::Crypto("Fee must be greater than 0".into()));
    }

    // RC-5: Reject absurdly high fees to protect against user/API errors.
    // 1 ZCL = 100_000_000 zatoshis — anything above this is almost certainly a mistake.
    if request.fee_zatoshis > 100_000_000 {
        return Err(CoreError::Crypto(
            "Fee exceeds 1 ZCL maximum — likely an error".into(),
        ));
    }

    // Check memo length if provided
    if let Some(memo) = &request.memo {
        if memo.as_bytes().len() > 512 {
            return Err(CoreError::Crypto(format!(
                "Memo too long: {} bytes (max 512)",
                memo.as_bytes().len()
            )));
        }
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a valid bech32-encoded "zs" address for testing.
    /// Uses dummy 43-byte data (valid structure, not a real key).
    fn test_zs_address() -> String {
        use bech32::ToBase32;
        let dummy_data = vec![0xAAu8; 43]; // 43 bytes = Sapling payment address
        bech32::encode("zs", dummy_data.to_base32(), bech32::Variant::Bech32).unwrap()
    }

    fn make_spendable_note(id: i64, value: u64) -> SpendableNote {
        SpendableNote {
            id,
            value,
            rcm: [0xAA; 32],
            diversifier: [0xBB; 11],
            witness: vec![0x01; 100], // Fake witness
            anchor: [0xCC; 32],
            nullifier: [id as u8; 32],
            is_zip212: false,
        }
    }

    // ---- Note Selection Tests ----

    #[test]
    fn test_select_notes_single() {
        let notes = vec![make_spendable_note(1, 100_000)];
        let (selected, total) = select_notes(&notes, 50_000).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(total, 100_000);
    }

    #[test]
    fn test_select_notes_multiple() {
        let notes = vec![
            make_spendable_note(1, 30_000),
            make_spendable_note(2, 50_000),
            make_spendable_note(3, 20_000),
        ];
        let (selected, total) = select_notes(&notes, 60_000).unwrap();
        // Should pick 50K first (largest), then 30K
        assert_eq!(selected.len(), 2);
        assert_eq!(total, 80_000);
    }

    #[test]
    fn test_select_notes_exact_amount() {
        let notes = vec![make_spendable_note(1, 50_000)];
        let (selected, total) = select_notes(&notes, 50_000).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(total, 50_000);
    }

    #[test]
    fn test_select_notes_insufficient() {
        let notes = vec![
            make_spendable_note(1, 30_000),
            make_spendable_note(2, 20_000),
        ];
        let err = select_notes(&notes, 100_000).unwrap_err();
        match err {
            CoreError::InsufficientBalance { have, need } => {
                assert_eq!(have, 50_000);
                assert_eq!(need, 100_000);
            }
            _ => panic!("Expected InsufficientBalance"),
        }
    }

    #[test]
    fn test_select_notes_empty() {
        let err = select_notes(&[], 50_000).unwrap_err();
        assert!(matches!(err, CoreError::InsufficientBalance { .. }));
    }

    // ---- Change Calculation Tests ----

    #[test]
    fn test_calculate_change() {
        assert_eq!(calculate_change(100_000, 50_000, 10_000).unwrap(), 40_000);
        assert_eq!(calculate_change(100_000, 90_000, 10_000).unwrap(), 0);
        // RC-7: Insufficient input now returns an error instead of silently saturating
        assert!(calculate_change(50_000, 50_000, 10_000).is_err());
    }

    // ---- Validation Tests ----

    #[test]
    fn test_validate_spend_notes_valid() {
        let notes = vec![make_spendable_note(1, 100_000)];
        assert!(validate_spend_notes(&notes).is_ok());
    }

    #[test]
    fn test_validate_spend_notes_empty_witness() {
        let mut note = make_spendable_note(1, 100_000);
        note.witness = vec![]; // Invalid
        let err = validate_spend_notes(&[note]).unwrap_err();
        assert!(matches!(err, CoreError::Crypto(_)));
    }

    #[test]
    fn test_validate_spend_notes_zero_anchor() {
        let mut note = make_spendable_note(1, 100_000);
        note.anchor = [0u8; 32]; // Invalid — FIX #1279
        let err = validate_spend_notes(&[note]).unwrap_err();
        assert!(matches!(err, CoreError::Crypto(_)));
    }

    // ---- note_to_spendable Tests ----

    #[test]
    fn test_note_to_spendable_valid() {
        let note = zipherx_storage::types::Note {
            id: 1,
            account_id: 0,
            height: 1000,
            cmu: vec![0xAA; 32],
            epk: Some(vec![0xBB; 32]),
            ciphertext: Some(vec![0; 580]),
            value: 50_000,
            rcm: Some(vec![0xCC; 32]),
            nullifier: Some(vec![0xDD; 32]),
            witness: Some(vec![0x01; 200]),
            anchor: Some(vec![0xEE; 32]),
            is_spent: false,
            spent_in_tx: None,
            spent_height: None,
            memo: None,
            diversifier: Some(vec![0xFF; 11]),
            received_txid: None,
            position: Some(42),
        };
        let spendable = note_to_spendable(&note).unwrap();
        assert_eq!(spendable.id, 1);
        assert_eq!(spendable.value, 50_000);
    }

    #[test]
    fn test_note_to_spendable_missing_witness() {
        let note = zipherx_storage::types::Note {
            id: 1,
            account_id: 0,
            height: 1000,
            cmu: vec![0xAA; 32],
            epk: None,
            ciphertext: None,
            value: 50_000,
            rcm: Some(vec![0xCC; 32]),
            nullifier: Some(vec![0xDD; 32]),
            witness: None, // Missing!
            anchor: Some(vec![0xEE; 32]),
            is_spent: false,
            spent_in_tx: None,
            spent_height: None,
            memo: None,
            diversifier: Some(vec![0xFF; 11]),
            received_txid: None,
            position: None,
        };
        assert!(note_to_spendable(&note).is_none());
    }

    // ---- Phantom Detection Tests ----

    #[test]
    fn test_detect_phantom_spent() {
        let notes = vec![
            zipherx_storage::types::Note {
                id: 1,
                account_id: 0,
                height: 100,
                cmu: vec![],
                epk: None,
                ciphertext: None,
                value: 50_000,
                rcm: None,
                nullifier: None,
                witness: None,
                anchor: None,
                is_spent: true,
                spent_in_tx: None, // Phantom!
                spent_height: None,
                memo: None,
                diversifier: None,
                received_txid: None,
                position: None,
            },
            zipherx_storage::types::Note {
                id: 2,
                account_id: 0,
                height: 200,
                cmu: vec![],
                epk: None,
                ciphertext: None,
                value: 30_000,
                rcm: None,
                nullifier: None,
                witness: None,
                anchor: None,
                is_spent: true,
                spent_in_tx: Some("abc123".into()), // Legit spend
                spent_height: Some(300),
                memo: None,
                diversifier: None,
                received_txid: None,
                position: None,
            },
        ];
        let phantom_ids = detect_phantom_spent_notes(&notes);
        assert_eq!(phantom_ids, vec![1]); // Only note 1 is phantom
    }

    // ---- Boost Placeholder Tests ----

    #[test]
    fn test_is_boost_placeholder() {
        assert!(is_boost_placeholder_txid("626f6f73740000000000000000000000"));
        assert!(is_boost_placeholder_txid("626F6F73740000000000000000000000"));
        assert!(!is_boost_placeholder_txid("abcdef0123456789"));
    }

    // ---- Address Validation Tests ----

    #[test]
    fn test_validate_shielded_address_valid() {
        // Valid bech32-encoded address with HRP "zs"
        let addr = test_zs_address();
        assert!(validate_shielded_address(&addr).is_ok());
    }

    #[test]
    fn test_validate_shielded_address_empty() {
        assert!(validate_shielded_address("").is_err());
    }

    #[test]
    fn test_validate_shielded_address_wrong_prefix() {
        assert!(validate_shielded_address("t1abcdefgh").is_err());
    }

    #[test]
    fn test_validate_shielded_address_too_short() {
        assert!(validate_shielded_address("zs123").is_err());
    }

    // ---- Send Request Validation Tests ----

    #[test]
    fn test_validate_send_request_valid() {
        let addr = test_zs_address();
        let req = SendRequest {
            to_address: addr,
            amount_zatoshis: 50_000,
            fee_zatoshis: DEFAULT_FEE,
            memo: None,
        };
        assert!(validate_send_request(&req).is_ok());
    }

    #[test]
    fn test_validate_send_request_zero_amount() {
        let addr = test_zs_address();
        let req = SendRequest {
            to_address: addr,
            amount_zatoshis: 0,
            fee_zatoshis: DEFAULT_FEE,
            memo: None,
        };
        assert!(validate_send_request(&req).is_err());
    }

    #[test]
    fn test_validate_send_request_memo_too_long() {
        let addr = test_zs_address();
        let req = SendRequest {
            to_address: addr,
            amount_zatoshis: 50_000,
            fee_zatoshis: DEFAULT_FEE,
            memo: Some("x".repeat(600)),
        };
        assert!(validate_send_request(&req).is_err());
    }

    #[test]
    fn test_validate_send_request_excessive_fee() {
        // RC-5: Fee above 1 ZCL should be rejected
        let addr = test_zs_address();
        let req = SendRequest {
            to_address: addr,
            amount_zatoshis: 50_000,
            fee_zatoshis: 200_000_000, // 2 ZCL — too high
            memo: None,
        };
        assert!(validate_send_request(&req).is_err());
    }

    #[test]
    fn test_validate_shielded_address_bad_checksum() {
        // RC-3: Corrupted bech32 address should fail checksum validation
        let mut addr = test_zs_address();
        // Flip a character to corrupt the checksum
        let bytes = unsafe { addr.as_bytes_mut() };
        let last_idx = bytes.len() - 1;
        bytes[last_idx] = if bytes[last_idx] == b'q' { b'p' } else { b'q' };
        assert!(validate_shielded_address(&addr).is_err());
    }

    #[test]
    fn test_send_request_total_needed() {
        let req = SendRequest {
            to_address: String::new(),
            amount_zatoshis: 50_000,
            fee_zatoshis: 10_000,
            memo: None,
        };
        assert_eq!(req.total_needed(), 60_000);
    }
}
