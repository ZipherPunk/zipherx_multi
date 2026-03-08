//! Integration test — exercises the full wallet flow using in-memory storage.
//!
//! Flow: create wallet → get address → insert note → check balance →
//!       validate send → select notes → verify guards
//!
//! TODO [BI-17]: Add automated end-to-end integration tests that exercise the
//! full sync pipeline (boost download -> delta scan -> tree build -> witness rebuild)
//! against a regtest/testnet node. Current tests use mocked data only.

use zipherx_core::scanner;
use zipherx_core::send;
use zipherx_core::sync;
use zipherx_core::wallet;
use zipherx_network::block_fetcher::{CompactBlock, ShieldedOutput, ShieldedSpend};
use zipherx_storage::types::{Note, TxStatus, TxType};
use zipherx_storage::WalletDatabase;

// ============================================================================
// Helpers
// ============================================================================

fn test_mnemonic_phrase() -> &'static str {
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"
}

fn test_sk() -> Vec<u8> {
    let seed = zipherx_crypto::mnemonic::to_seed(test_mnemonic_phrase()).unwrap();
    zipherx_crypto::keys::derive_spending_key(&seed, 0)
        .unwrap()
        .to_vec()
}

fn make_test_note(id: i64, value: u64, has_witness: bool) -> Note {
    Note {
        id,
        account_id: 0,
        height: 2_951_900,
        cmu: vec![0xAA; 32],
        epk: Some(vec![0xBB; 32]),
        ciphertext: Some(vec![0; 580]),
        value,
        rcm: Some(vec![0xCC; 32]),
        nullifier: Some(vec![id as u8; 32]),
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
        is_spent: false,
        spent_in_tx: None,
        spent_height: None,
        memo: None,
        diversifier: Some(vec![0xFF; 11]),
        received_txid: Some(format!("rx_note_{}", id)),
        position: Some(id as u64),
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_full_wallet_lifecycle() {
    // 1. Create wallet and verify mnemonic
    let config = wallet::WalletConfig {
        db_path: ":memory:".into(),
        header_store_path: ":memory:".into(),
        delta_store_dir: "/tmp/zipherx_test_delta".into(),
        spend_params_path: "/tmp/spend.params".into(),
        output_params_path: "/tmp/output.params".into(),
        account_index: 0,
        db_encryption_key: None,
    };
    let wallet = wallet::WalletCore::new(config);
    assert_eq!(wallet.state(), wallet::WalletLifecycleState::Uninitialized);

    let words = wallet.create_wallet().unwrap();
    assert_eq!(words.len(), 24);
    assert_eq!(wallet.state(), wallet::WalletLifecycleState::Locked);

    // 2. Restore from known mnemonic and derive address
    let config2 = wallet::WalletConfig {
        db_path: ":memory:".into(),
        header_store_path: ":memory:".into(),
        delta_store_dir: "/tmp/zipherx_test_delta2".into(),
        spend_params_path: "/tmp/spend.params".into(),
        output_params_path: "/tmp/output.params".into(),
        account_index: 0,
        db_encryption_key: None,
    };
    let wallet2 = wallet::WalletCore::new(config2);
    let restore_words: Vec<String> = test_mnemonic_phrase()
        .split_whitespace()
        .map(String::from)
        .collect();
    wallet2.restore_wallet(&restore_words).unwrap();
    assert_eq!(wallet2.state(), wallet::WalletLifecycleState::Locked);

    let sk = test_sk();
    let address = wallet2.get_address(&sk).unwrap();
    assert!(address.starts_with("zs1"));
    assert!(address.len() > 70);

    // 3. Address is deterministic
    let address2 = wallet2.get_address(&sk).unwrap();
    assert_eq!(address, address2);
}

#[test]
fn test_balance_computation_flow() {
    // Simulate notes discovered from scanning
    let notes = vec![
        make_test_note(1, 100_000, true), // Spendable
        make_test_note(2, 50_000, true),  // Spendable
        make_test_note(3, 30_000, false), // No witness
    ];

    // FIX #1210: Total includes ALL unspent, spendable only those with witnesses
    let balance = wallet::WalletCore::compute_balance(&notes);
    assert_eq!(balance.total, 180_000); // 100K + 50K + 30K
    assert_eq!(balance.spendable, 150_000); // 100K + 50K
    assert_eq!(balance.note_count, 3);
    assert_eq!(balance.spendable_note_count, 2);

    // Get spendable notes for TX building
    let spendable = wallet::WalletCore::get_spendable_notes(&notes);
    assert_eq!(spendable.len(), 2);
}

#[test]
fn test_send_validation_flow() {
    // Generate a valid bech32 address for testing
    use bech32::ToBase32;
    let dummy_data = vec![0xAAu8; 43];
    let addr = bech32::encode("zs", dummy_data.to_base32(), bech32::Variant::Bech32).unwrap();

    // Valid request
    let request = send::SendRequest {
        to_address: addr.clone(),
        amount_zatoshis: 50_000,
        fee_zatoshis: send::DEFAULT_FEE,
        memo: Some("Test payment".into()),
    };
    assert!(send::validate_send_request(&request).is_ok());
    assert_eq!(request.total_needed(), 60_000);

    // Note selection
    let notes = vec![send::SpendableNote {
        id: 1,
        value: 100_000,
        rcm: [0xAA; 32],
        diversifier: [0xBB; 11],
        witness: vec![0x01; 200],
        anchor: [0xCC; 32],
        nullifier: [0x01; 32],
        is_zip212: false,
    }];

    let (selected, total) = send::select_notes(&notes, request.total_needed()).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(total, 100_000);

    let change =
        send::calculate_change(total, request.amount_zatoshis, request.fee_zatoshis).unwrap();
    assert_eq!(change, 40_000); // 100K - 50K - 10K

    // Validate spend notes
    assert!(send::validate_spend_notes(&selected).is_ok());
}

#[test]
fn test_scanner_flow() {
    let sk = test_sk();

    // Create some blocks to scan
    let blocks = vec![
        CompactBlock {
            height: 2_951_900,
            hash: [0; 32],
            timestamp: 1700000000,
            final_sapling_root: [0xAB; 32],
            outputs: vec![ShieldedOutput {
                txid: [0x01; 32],
                cmu: [0x02; 32],
                epk: [0; 32],
                ciphertext: vec![0; 580],
                cv: [0; 32],
            }],
            spends: vec![],
        },
        CompactBlock {
            height: 2_951_901,
            hash: [0; 32],
            timestamp: 1700000060,
            final_sapling_root: [0xCD; 32],
            outputs: vec![],
            spends: vec![ShieldedSpend {
                txid: [0x03; 32],
                nullifier: [0xDD; 32],
            }],
        },
    ];

    // Scan blocks
    let result = scanner::scan_blocks(&blocks, &sk, 1_043_472, None).unwrap();
    assert_eq!(result.last_scanned_height, 2_951_901);
    assert_eq!(result.cmus_appended, 1);
    assert_eq!(result.sapling_roots.len(), 2);
    assert_eq!(result.spent_nullifiers.len(), 1);
    assert_eq!(result.spent_nullifiers[0].0, [0xDD; 32]);

    // Extract CMUs (sorted by height — FIX #1199)
    let cmus = scanner::extract_cmus_from_blocks(&blocks);
    assert_eq!(cmus[0].0, 2_951_900);
    assert_eq!(cmus[0].1.len(), 1);
    assert_eq!(cmus[1].0, 2_951_901);
    assert_eq!(cmus[1].1.len(), 0);

    // Check for TX confirmation (FIX #1259)
    let mut pending = std::collections::HashMap::new();
    pending.insert([0xDD; 32], "my_tx_abc".to_string());
    let confirmations = scanner::check_block_for_confirmation(&blocks[1], &pending);
    assert_eq!(confirmations.len(), 1);
    assert_eq!(confirmations[0].0, "my_tx_abc");
    assert_eq!(confirmations[0].1, 2_951_901);
}

#[test]
fn test_sync_orchestration_flow() {
    // Determine startup mode
    let state = sync::WalletState {
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
    assert_eq!(
        sync::determine_startup_mode(&state),
        sync::StartupMode::Instant
    );

    // Calculate delta sync range
    let range = sync::calculate_delta_sync_range(2_951_900, 2_951_950, 2_951_950);
    assert_eq!(range, Some((2_951_901, 2_951_950)));

    // Size guard for tree operations (FIX #978/#1281)
    let skip = sync::calculate_witness_skip_count(1_050_000, 1_043_472);
    assert_eq!(skip, 6_528); // 1_050_000 - 1_043_472

    // Gap detection
    let heights = vec![100, 101, 105, 106, 107, 110];
    let gaps = sync::detect_gaps(&heights, 100, 110);
    assert_eq!(gaps.len(), 2);
    assert_eq!(
        gaps[0],
        sync::DeltaGap {
            start: 102,
            end: 104
        }
    );
    assert_eq!(
        gaps[1],
        sync::DeltaGap {
            start: 108,
            end: 109
        }
    );

    // Root validation (FIX #1230 — both byte orders)
    let root_a = [0x01; 32];
    let mut root_b = [0u8; 32];
    for i in 0..32 {
        root_b[i] = root_a[31 - i];
    }
    assert!(sync::roots_match(&root_a, &root_b));

    // Sync guards
    let guards = sync::SyncGuards::new();
    assert!(guards.can_background_sync());
    guards
        .is_broadcasting
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(!guards.can_background_sync()); // FIX #1184: blocked during broadcast
}

#[test]
fn test_storage_roundtrip() {
    // Create in-memory DB and verify basic operations
    let db = WalletDatabase::open_in_memory().unwrap();

    // Insert a note — signature: account_id, height, cmu, value,
    // nullifier, rcm, epk, ciphertext, memo, diversifier, witness, received_txid, position
    db.insert_note(
        0, // account_id
        2_951_900,
        &[0xAA; 32],                     // cmu
        50_000,                          // value
        Some(&[0xDDu8; 32] as &[u8]),    // nullifier
        Some(&[0xCCu8; 32] as &[u8]),    // rcm
        Some(&[0xBBu8; 32] as &[u8]),    // epk
        Some(vec![0u8; 580].as_slice()), // ciphertext
        Some("Test memo"),
        Some(&[0xFFu8; 11] as &[u8]), // diversifier
        None,                         // witness (no witness yet)
        Some("tx_abc123"),            // received_txid
        Some(42),                     // position
    )
    .unwrap();

    // Query balance (FIX #1210)
    let total = db.get_total_unspent_balance(0).unwrap();
    assert_eq!(total, 50_000);

    // Balance requiring witness
    let balance = db.get_balance(0).unwrap();
    assert_eq!(balance, 0); // No witness yet

    // Update witness
    db.update_note_witness(1, &[0x01; 200]).unwrap();
    db.update_note_anchor(1, &[0xEE; 32]).unwrap();
    let balance = db.get_balance(0).unwrap();
    assert_eq!(balance, 50_000); // Now has witness

    // Record a transaction — signature: txid, height, timestamp, tx_type, amount, fee, address, memo, status
    db.insert_transaction(
        "tx_abc123",
        2_951_900,        // height
        Some(1700000000), // timestamp
        TxType::Received,
        50_000,
        0,    // fee
        None, // address
        Some("Test memo"),
        TxStatus::Confirmed,
    )
    .unwrap();

    let history = db.get_transaction_history(10, 0).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].txid, "tx_abc123");
    assert_eq!(history[0].amount, 50_000);

    // Sync state
    let sync_state = db.get_sync_state().unwrap();
    assert_eq!(sync_state.last_scanned_height, 0);
    db.update_last_scanned_height(2_951_900).unwrap();
    let sync_state = db.get_sync_state().unwrap();
    assert_eq!(sync_state.last_scanned_height, 2_951_900);
}

#[test]
fn test_phantom_tx_detection() {
    // Notes where spending TX doesn't exist = phantom (FIX #1169)
    let notes = vec![
        Note {
            id: 1,
            account_id: 0,
            height: 100,
            cmu: vec![0xAA; 32],
            epk: None,
            ciphertext: None,
            value: 50_000,
            rcm: None,
            nullifier: None,
            witness: None,
            anchor: None,
            is_spent: true,
            spent_in_tx: None, // PHANTOM — spent but no TX ID!
            spent_height: None,
            memo: None,
            diversifier: None,
            received_txid: None,
            position: None,
        },
        Note {
            id: 2,
            account_id: 0,
            height: 200,
            cmu: vec![0xBB; 32],
            epk: None,
            ciphertext: None,
            value: 30_000,
            rcm: None,
            nullifier: None,
            witness: None,
            anchor: None,
            is_spent: true,
            spent_in_tx: Some("valid_tx".into()), // Legit spend
            spent_height: Some(300),
            memo: None,
            diversifier: None,
            received_txid: None,
            position: None,
        },
    ];

    let phantom_ids = send::detect_phantom_spent_notes(&notes);
    assert_eq!(phantom_ids, vec![1]); // Only note 1 is phantom
}

#[test]
fn test_broadcast_helpers() {
    use zipherx_network::broadcast;

    // Wire format reversal (FIX #1200)
    let txid = "0100000000000000000000000000000000000000000000000000000000000002";
    let wire = broadcast::reverse_txid_for_wire(txid).unwrap();
    assert_eq!(wire[0], 0x02);
    assert_eq!(wire[31], 0x01);

    // Roundtrip
    let display = broadcast::wire_txid_to_display(&wire);
    assert_eq!(display, txid);

    // DUPLICATE = SUCCESS
    assert!(broadcast::is_reject_actually_success(0x12));
    assert!(!broadcast::is_reject_actually_success(0x10));

    // Mined check (FIX #1250)
    assert!(!broadcast::is_mined(0));
    assert!(broadcast::is_mined(1));
}

#[test]
fn test_crypto_key_derivation_deterministic() {
    // Same mnemonic always produces same keys and address
    let phrase = test_mnemonic_phrase();
    let seed1 = zipherx_crypto::mnemonic::to_seed(phrase).unwrap();
    let seed2 = zipherx_crypto::mnemonic::to_seed(phrase).unwrap();
    assert_eq!(seed1, seed2);

    let sk1 = zipherx_crypto::keys::derive_spending_key(&seed1, 0).unwrap();
    let sk2 = zipherx_crypto::keys::derive_spending_key(&seed2, 0).unwrap();
    assert_eq!(sk1, sk2);

    let (addr1, _) = zipherx_crypto::keys::derive_address(&sk1, 0).unwrap();
    let (addr2, _) = zipherx_crypto::keys::derive_address(&sk2, 0).unwrap();
    assert_eq!(addr1, addr2);

    // Different account = different key
    let sk_other = zipherx_crypto::keys::derive_spending_key(&seed1, 1).unwrap();
    assert_ne!(sk1, sk_other);
}
