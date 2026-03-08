//! Wallet lifecycle — create, restore, lock, sync, balance, send.
//!
//! This is the top-level orchestrator that ties together crypto, network,
//! storage, and platform services into a coherent wallet API.
//!
//! CRITICAL INVARIANTS:
//! - All spending key access requires biometric auth (platform layer)
//! - Balance MUST use `get_total_unspent_balance()` (FIX #1210)
//! - MUST post `transactionHistoryUpdated` after DB changes (FIX #1170)
//! - NEVER bypass anchor validation (FIX #1279)
//! - Block listeners MUST be running for TX confirmation (FIX #1263)
//! - TX confirmation ONLY via block scanner (FIX #1259)

use std::sync::Arc;

use crate::send::{self, SpendableNote};
use crate::sync::{self, StartupMode, SyncGuards, SyncStatus, WalletState};
use crate::CoreError;

// ============================================================================
// Types
// ============================================================================

/// Wallet lifecycle state.
#[derive(Debug, Clone, PartialEq)]
pub enum WalletLifecycleState {
    /// No wallet exists — need to create or restore.
    Uninitialized,
    /// Wallet exists but spending key is locked (needs biometric).
    Locked,
    /// Wallet unlocked and ready for operations.
    Ready,
    /// Wallet is currently syncing to chain tip.
    Syncing,
    /// Wallet is repairing (tree rebuild, witness fix, full rescan).
    Repairing,
}

/// Configuration for wallet initialization.
#[derive(Debug, Clone)]
pub struct WalletConfig {
    /// Path to the SQLCipher wallet database.
    pub db_path: String,
    /// Path to the header store database.
    pub header_store_path: String,
    /// Directory for delta CMU storage.
    pub delta_store_dir: String,
    /// Path to sapling-spend.params (47MB).
    pub spend_params_path: String,
    /// Path to sapling-output.params (3.5MB).
    pub output_params_path: String,
    /// Account index (default 0).
    pub account_index: u32,
    /// Optional 32-byte encryption key for SQLCipher database encryption.
    /// When provided, the database is encrypted at rest.
    pub db_encryption_key: Option<Vec<u8>>,
}

/// Balance information.
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceInfo {
    /// Total balance of all unspent notes (FIX #1210).
    /// Uses `get_total_unspent_balance()` — does NOT require witness.
    pub total: u64,
    /// Spendable balance — only notes with valid witnesses.
    pub spendable: u64,
    /// Number of unspent notes.
    pub note_count: usize,
    /// Number of notes with valid witnesses.
    pub spendable_note_count: usize,
}

/// Summary of wallet state for UI display.
#[derive(Debug, Clone)]
pub struct WalletSummary {
    /// Current wallet state.
    pub state: WalletLifecycleState,
    /// Shielded address (if available).
    pub address: Option<String>,
    /// Balance info (if available).
    pub balance: Option<BalanceInfo>,
    /// Last synced height.
    pub last_synced_height: u64,
    /// Chain tip height (from peers).
    pub chain_tip: u64,
    /// Startup mode determined.
    pub startup_mode: Option<StartupMode>,
    /// Current sync status.
    pub sync_status: SyncStatus,
}

/// A record from transaction history for UI display.
#[derive(Debug, Clone)]
pub struct TransactionDisplay {
    /// Transaction ID (hex, display format).
    pub txid: String,
    /// Transaction type: "sent", "received", "change".
    pub tx_type: String,
    /// Amount in zatoshis.
    pub amount: u64,
    /// Fee in zatoshis (0 for received).
    pub fee: u64,
    /// Destination or source address (if known).
    pub address: Option<String>,
    /// Optional memo text.
    pub memo: Option<String>,
    /// Number of confirmations.
    pub confirmations: u64,
    /// Block height (0 = unconfirmed).
    pub height: u64,
    /// Timestamp (seconds since epoch, estimated).
    pub timestamp: u64,
}

// ============================================================================
// Wallet Core
// ============================================================================

/// The main wallet orchestrator.
///
/// Holds references to all subsystems and manages wallet lifecycle.
pub struct WalletCore {
    /// Wallet configuration.
    pub config: WalletConfig,
    /// Current lifecycle state.
    state: std::sync::Mutex<WalletLifecycleState>,
    /// Sync guard flags.
    pub guards: Arc<SyncGuards>,
}

impl WalletCore {
    /// Create a new WalletCore with the given configuration.
    pub fn new(config: WalletConfig) -> Self {
        Self {
            config,
            state: std::sync::Mutex::new(WalletLifecycleState::Uninitialized),
            guards: Arc::new(SyncGuards::new()),
        }
    }

    /// Get current wallet lifecycle state.
    ///
    /// RC-4: Uses `unwrap_or_else` to recover from a poisoned mutex rather than
    /// panicking. A poisoned mutex means a thread panicked while holding the lock,
    /// but the WalletLifecycleState is still valid — we can safely read it.
    pub fn state(&self) -> WalletLifecycleState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Set wallet lifecycle state.
    pub fn set_state(&self, new_state: WalletLifecycleState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = new_state;
    }

    /// Determine startup mode based on current wallet state.
    pub fn determine_startup_mode(&self, wallet_state: &WalletState) -> StartupMode {
        sync::determine_startup_mode(wallet_state)
    }

    /// Create a new wallet — generate mnemonic and derive keys.
    ///
    /// Returns the 24-word mnemonic phrase.
    pub fn create_wallet(&self) -> Result<Vec<String>, CoreError> {
        let phrase = zipherx_crypto::mnemonic::generate()
            .map_err(|e| CoreError::Crypto(format!("Mnemonic generation failed: {e}")))?;

        let words: Vec<String> = phrase.split_whitespace().map(String::from).collect();

        if words.len() != 24 {
            return Err(CoreError::Crypto(format!(
                "Expected 24 words, got {}",
                words.len()
            )));
        }

        // Verify the phrase is valid by deriving a key
        let seed = zipherx_crypto::mnemonic::to_seed(&phrase)
            .map_err(|e| CoreError::Crypto(format!("Seed derivation failed: {e}")))?;
        let _sk = zipherx_crypto::keys::derive_spending_key(&seed, self.config.account_index)
            .map_err(|e| CoreError::Crypto(format!("Key derivation failed: {e}")))?;

        self.set_state(WalletLifecycleState::Locked);
        Ok(words)
    }

    /// Restore a wallet from a mnemonic phrase.
    ///
    /// Validates the phrase and derives keys. Does NOT trigger sync.
    pub fn restore_wallet(&self, words: &[String]) -> Result<(), CoreError> {
        if words.len() != 24 {
            return Err(CoreError::Crypto(format!(
                "Expected 24 words, got {}",
                words.len()
            )));
        }

        let phrase = words.join(" ");
        let seed = zipherx_crypto::mnemonic::to_seed(&phrase)
            .map_err(|e| CoreError::Crypto(format!("Invalid mnemonic: {e}")))?;

        // Verify key derivation works
        let _sk = zipherx_crypto::keys::derive_spending_key(&seed, self.config.account_index)
            .map_err(|e| CoreError::Crypto(format!("Key derivation failed: {e}")))?;

        self.set_state(WalletLifecycleState::Locked);
        Ok(())
    }

    /// Import a wallet from raw spending key bytes (169-byte ExtendedSpendingKey).
    ///
    /// Validates the key by deriving an address, then sets wallet state to Locked.
    pub fn import_wallet_from_key(&self, sk_bytes: &[u8]) -> Result<(), CoreError> {
        // Validate key by deriving address
        let (_addr_bytes, _index) = zipherx_crypto::keys::derive_address(sk_bytes, 0)
            .map_err(|e| CoreError::Crypto(format!("Invalid spending key: {e}")))?;

        self.set_state(WalletLifecycleState::Locked);
        Ok(())
    }

    /// Derive the shielded payment address for the current account.
    pub fn get_address(&self, sk_bytes: &[u8]) -> Result<String, CoreError> {
        let (addr_bytes, _index) = zipherx_crypto::keys::derive_address(sk_bytes, 0)
            .map_err(|e| CoreError::Crypto(format!("Address derivation failed: {e}")))?;
        let address = zipherx_crypto::address::encode_address(&addr_bytes)
            .map_err(|e| CoreError::Crypto(format!("Address encoding failed: {e}")))?;
        Ok(address)
    }

    /// Compute balance from a set of notes (FIX #1210).
    ///
    /// - `total`: Sum of ALL unspent notes (regardless of witness state)
    /// - `spendable`: Sum of unspent notes that have valid witnesses
    pub fn compute_balance(notes: &[zipherx_storage::types::Note]) -> BalanceInfo {
        let unspent: Vec<&zipherx_storage::types::Note> =
            notes.iter().filter(|n| !n.is_spent).collect();

        let total: u64 = unspent
            .iter()
            .fold(0u64, |acc, n| acc.saturating_add(n.value));
        let spendable_notes: Vec<&&zipherx_storage::types::Note> = unspent
            .iter()
            .filter(|n| n.witness.is_some() && n.anchor.is_some())
            .collect();
        let spendable: u64 = spendable_notes
            .iter()
            .fold(0u64, |acc, n| acc.saturating_add(n.value));

        BalanceInfo {
            total,
            spendable,
            note_count: unspent.len(),
            spendable_note_count: spendable_notes.len(),
        }
    }

    /// Get spendable notes (notes with valid witnesses that can be used in TX).
    pub fn get_spendable_notes(notes: &[zipherx_storage::types::Note]) -> Vec<SpendableNote> {
        notes
            .iter()
            .filter(|n| !n.is_spent)
            .filter_map(|n| send::note_to_spendable(n))
            .collect()
    }

    /// Convert a TransactionRecord to a display-friendly format.
    pub fn transaction_to_display(
        record: &zipherx_storage::types::TransactionRecord,
    ) -> TransactionDisplay {
        let tx_type = match record.tx_type {
            zipherx_storage::types::TxType::Sent => "sent",
            zipherx_storage::types::TxType::Received => "received",
            zipherx_storage::types::TxType::Change => "change",
            zipherx_storage::types::TxType::SelfTransfer => "self",
        };

        TransactionDisplay {
            txid: record.txid.clone(),
            tx_type: tx_type.to_string(),
            amount: record.amount,
            fee: record.fee,
            address: record.address.clone(),
            memo: record.memo.clone(),
            confirmations: record.confirmations as u64,
            height: record.height,
            timestamp: record.timestamp.unwrap_or(0),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use zipherx_storage::types::{Note, TransactionRecord, TxStatus, TxType};

    fn test_config() -> WalletConfig {
        WalletConfig {
            db_path: ":memory:".into(),
            header_store_path: ":memory:".into(),
            delta_store_dir: "/tmp/test_delta".into(),
            spend_params_path: "/tmp/spend.params".into(),
            output_params_path: "/tmp/output.params".into(),
            account_index: 0,
            db_encryption_key: None,
        }
    }

    fn make_note(id: i64, value: u64, spent: bool, has_witness: bool) -> Note {
        Note {
            id,
            account_id: 0,
            height: 1000,
            cmu: vec![0xAA; 32],
            epk: Some(vec![0xBB; 32]),
            ciphertext: Some(vec![0; 580]),
            value,
            rcm: Some(vec![0xCC; 32]),
            nullifier: Some(vec![0xDD; 32]),
            witness: if has_witness {
                Some(vec![0x01; 200])
            } else {
                None
            },
            anchor: if has_witness {
                Some(vec![0xEE; 32])
            } else {
                None
            },
            is_spent: spent,
            spent_in_tx: if spent { Some("abc123".into()) } else { None },
            spent_height: if spent { Some(2000) } else { None },
            memo: None,
            diversifier: Some(vec![0xFF; 11]),
            received_txid: Some("def456".into()),
            position: Some(42),
        }
    }

    // ---- Lifecycle Tests ----

    #[test]
    fn test_wallet_initial_state() {
        let wallet = WalletCore::new(test_config());
        assert_eq!(wallet.state(), WalletLifecycleState::Uninitialized);
    }

    #[test]
    fn test_wallet_state_transitions() {
        let wallet = WalletCore::new(test_config());
        wallet.set_state(WalletLifecycleState::Locked);
        assert_eq!(wallet.state(), WalletLifecycleState::Locked);
        wallet.set_state(WalletLifecycleState::Ready);
        assert_eq!(wallet.state(), WalletLifecycleState::Ready);
    }

    #[test]
    fn test_create_wallet() {
        let wallet = WalletCore::new(test_config());
        let words = wallet.create_wallet().unwrap();
        assert_eq!(words.len(), 24);
        assert_eq!(wallet.state(), WalletLifecycleState::Locked);
    }

    #[test]
    fn test_restore_wallet_valid() {
        let wallet = WalletCore::new(test_config());
        let words: Vec<String> = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
            .split_whitespace()
            .map(String::from)
            .collect();
        wallet.restore_wallet(&words).unwrap();
        assert_eq!(wallet.state(), WalletLifecycleState::Locked);
    }

    #[test]
    fn test_restore_wallet_wrong_count() {
        let wallet = WalletCore::new(test_config());
        let words = vec!["abandon".to_string(); 12]; // Only 12 words
        assert!(wallet.restore_wallet(&words).is_err());
    }

    #[test]
    fn test_get_address() {
        let wallet = WalletCore::new(test_config());
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let seed = zipherx_crypto::mnemonic::to_seed(phrase).unwrap();
        let sk = zipherx_crypto::keys::derive_spending_key(&seed, 0).unwrap();
        let address = wallet.get_address(&sk).unwrap();
        assert!(address.starts_with("zs1"));
        assert!(address.len() > 70);
    }

    // ---- Balance Tests ----

    #[test]
    fn test_compute_balance_all_unspent_with_witness() {
        let notes = vec![
            make_note(1, 50_000, false, true),
            make_note(2, 30_000, false, true),
        ];
        let balance = WalletCore::compute_balance(&notes);
        assert_eq!(balance.total, 80_000);
        assert_eq!(balance.spendable, 80_000);
        assert_eq!(balance.note_count, 2);
        assert_eq!(balance.spendable_note_count, 2);
    }

    #[test]
    fn test_compute_balance_mixed() {
        let notes = vec![
            make_note(1, 50_000, false, true),  // Spendable
            make_note(2, 30_000, false, false), // Unspent but no witness (FIX #1210)
            make_note(3, 20_000, true, true),   // Spent
        ];
        let balance = WalletCore::compute_balance(&notes);
        // FIX #1210: total includes ALL unspent (even without witness)
        assert_eq!(balance.total, 80_000); // 50K + 30K
        assert_eq!(balance.spendable, 50_000); // Only the one with witness
        assert_eq!(balance.note_count, 2);
        assert_eq!(balance.spendable_note_count, 1);
    }

    #[test]
    fn test_compute_balance_all_spent() {
        let notes = vec![
            make_note(1, 50_000, true, true),
            make_note(2, 30_000, true, true),
        ];
        let balance = WalletCore::compute_balance(&notes);
        assert_eq!(balance.total, 0);
        assert_eq!(balance.spendable, 0);
    }

    #[test]
    fn test_compute_balance_empty() {
        let balance = WalletCore::compute_balance(&[]);
        assert_eq!(
            balance,
            BalanceInfo {
                total: 0,
                spendable: 0,
                note_count: 0,
                spendable_note_count: 0,
            }
        );
    }

    // ---- Spendable Notes Tests ----

    #[test]
    fn test_get_spendable_notes() {
        let notes = vec![
            make_note(1, 50_000, false, true),  // Spendable
            make_note(2, 30_000, false, false), // No witness
            make_note(3, 20_000, true, true),   // Spent
        ];
        let spendable = WalletCore::get_spendable_notes(&notes);
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].value, 50_000);
    }

    // ---- Transaction Display Tests ----

    #[test]
    fn test_transaction_to_display_sent() {
        let record = TransactionRecord {
            id: 1,
            txid: "abc123".into(),
            tx_type: TxType::Sent,
            amount: 50_000,
            fee: 10_000,
            address: Some("zc1abc...".into()),
            memo: Some("Payment".into()),
            confirmations: 5,
            height: 1000,
            timestamp: Some(1700000000),
            status: TxStatus::Confirmed,
        };
        let display = WalletCore::transaction_to_display(&record);
        assert_eq!(display.tx_type, "sent");
        assert_eq!(display.amount, 50_000);
        assert_eq!(display.fee, 10_000);
    }

    #[test]
    fn test_transaction_to_display_received() {
        let record = TransactionRecord {
            id: 2,
            txid: "def456".into(),
            tx_type: TxType::Received,
            amount: 100_000,
            fee: 0,
            address: None,
            memo: None,
            confirmations: 0,
            height: 0,
            timestamp: None,
            status: TxStatus::Pending,
        };
        let display = WalletCore::transaction_to_display(&record);
        assert_eq!(display.tx_type, "received");
        assert_eq!(display.confirmations, 0);
        assert_eq!(display.height, 0);
    }

    // ---- Startup Mode Tests ----

    #[test]
    fn test_determine_startup_mode() {
        let wallet = WalletCore::new(test_config());
        let state = WalletState {
            has_tree_state: true,
            tree_height: 1_043_472,
            last_scanned_height: 2_951_900,
            delta_bundle_verified: true,
            delta_end_height: 2_951_900,
            boost_file_height: 2_951_853,
            boost_cmu_count: 1_043_472,
            has_valid_witnesses: true,
            chain_tip: 2_951_950,
        };
        assert_eq!(wallet.determine_startup_mode(&state), StartupMode::Instant);
    }

    // ---- Guards Integration ----

    #[test]
    fn test_wallet_guards() {
        let wallet = WalletCore::new(test_config());
        assert!(wallet.guards.can_background_sync());
        assert!(wallet.guards.try_acquire_sync());
        assert!(!wallet.guards.can_background_sync());
        wallet.guards.release_sync();
        assert!(wallet.guards.can_background_sync());
    }
}
