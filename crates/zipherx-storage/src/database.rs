//! WalletDatabase — SQLCipher-backed persistent storage for the ZipherX wallet.
//!
//! Wraps `rusqlite::Connection` in a `Mutex` for `Send + Sync`.
//! All operations are synchronous — callers use tokio::task::spawn_blocking
//! when integrating with async code.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::schema;
use crate::types::*;

/// Recover from a poisoned mutex. Logs a warning on poison recovery.
/// This pattern is used instead of unwrap() to prevent app crashes when
/// a thread panics while holding the database lock. The recovered state
/// may be inconsistent if the panic occurred mid-transaction.
fn recover_lock(
    result: std::sync::LockResult<MutexGuard<'_, rusqlite::Connection>>,
) -> MutexGuard<'_, rusqlite::Connection> {
    match result {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[ZipherX] WARN: Database mutex poisoned — a thread panicked while holding the lock. Recovering.");
            e.into_inner()
        }
    }
}

/// A transparent address with a non-zero unspent balance.
#[derive(Debug, Clone)]
pub struct FundedAddress {
    /// Encoded transparent address (t1...).
    pub address: String,
    /// Total unspent balance in zatoshis.
    pub balance: u64,
    /// Whether this is a change address (internal derivation chain).
    pub is_change: bool,
    /// BIP-44 child index used to derive the address.
    pub child_index: u32,
    /// Whether this address was imported via WIF rather than derived.
    pub is_imported: bool,
}

/// The main wallet database.
pub struct WalletDatabase {
    conn: Mutex<rusqlite::Connection>,
    /// Cached sent/received counts from the last `get_transaction_history` call.
    /// Updated every time the full history is processed (even for paginated calls).
    cached_sent_count: AtomicU32,
    cached_received_count: AtomicU32,
}

impl WalletDatabase {
    /// Open a file-backed database with optional SQLCipher encryption.
    pub fn open(path: &str, encryption_key: Option<&[u8]>) -> Result<Self, StorageError> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| StorageError::OpenFailed(e.to_string()))?;

        if let Some(key) = encryption_key {
            let mut hex_key = hex::encode(key);
            // SAFETY: hex::encode only produces ASCII hex digits [0-9a-f], so this
            // format string cannot contain SQL injection. Assert as defense-in-depth.
            assert!(
                hex_key.chars().all(|c| c.is_ascii_hexdigit()),
                "Invalid hex key"
            );
            let pragma_result = conn
                .execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))
                .map_err(|e| StorageError::OpenFailed(format!("PRAGMA key: {e}")));
            // STOR-002: Zero key material from memory using write_volatile to
            // prevent the compiler from optimizing away the zeroing.
            // SAFETY: hex_key is a valid String; as_bytes_mut returns the UTF-8 buffer.
            // Filling with zeros produces valid UTF-8 (all NUL bytes).
            unsafe {
                let bytes = hex_key.as_bytes_mut();
                for b in bytes.iter_mut() {
                    std::ptr::write_volatile(b, 0);
                }
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            }
            drop(hex_key);
            pragma_result?;
            // Verify encryption works
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
                .map_err(|e| StorageError::OpenFailed(format!("Key verification: {e}")))?;
        }

        Self::setup(conn)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| StorageError::OpenFailed(e.to_string()))?;
        Self::setup(conn)
    }

    fn setup(conn: rusqlite::Connection) -> Result<Self, StorageError> {
        // Apply performance pragmas
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -16000;
             PRAGMA temp_store = MEMORY;",
        )
        .map_err(|e| StorageError::OpenFailed(format!("Pragmas: {e}")))?;

        // Create schema
        for stmt in schema::all_create_statements() {
            conn.execute_batch(stmt)
                .map_err(|e| StorageError::SchemaFailed(format!("{e}: {stmt}")))?;
        }

        // Ensure sync state singleton exists
        conn.execute(
            "INSERT OR IGNORE INTO sync_state (id, last_scanned_height) VALUES (1, 0)",
            [],
        )
        .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;

        // Migration: upgrade transaction_history UNIQUE(txid) → UNIQUE(txid, tx_type)
        // so that the same txid can have both a "received" and "sent" entry.
        Self::migrate_tx_history_unique_constraint(&conn)?;

        // Migration v2: force clean re-scan of boost data.
        // Previous boost scan ran under the old UNIQUE(txid) + INSERT OR REPLACE schema,
        // corrupting transaction history (sent entries overwrote received entries).
        // This one-time migration clears notes/history and resets tree_height so the
        // boost scan re-runs with the corrected schema.
        Self::migrate_data_rescan_v2(&conn)?;

        // Migration v3: force boost re-scan with per-TX aggregated history
        // (previous scan inserted per-NOTE entries showing confusing change amounts)
        Self::migrate_data_rescan_v3(&conn)?;

        // Migration v4: force full re-download of delta store with v2 format (txids)
        Self::migrate_full_redownload_v4(&conn)?;

        // Migration v5: re-scan boost + cross-reference post-boost nullifiers against
        // boost-era notes (fixes balance gap from undetected post-boost spends)
        Self::migrate_data_rescan_v5(&conn)?;

        // Migration v6: re-scan with CMU mismatch diagnostics + full delta nullifier cross-ref
        Self::migrate_data_rescan_v6(&conn)?;

        // Migration v7: re-scan to trigger post-boost full-block spend scan
        // Downloads ALL blocks in post-boost range (not just output blocks) to find
        // nullifiers from spend-only blocks that the delta sync missed.
        Self::migrate_data_rescan_v7(&conn)?;

        // Migration v8: re-scan with position probe diagnostic to diagnose
        // nullifier mismatches (31 boost notes falsely marked unspent)
        Self::migrate_data_rescan_v8(&conn)?;

        // Migration v9: re-scan with full post-boost block download.
        // Previous spend scan only downloaded spend-only blocks (5290 of 5591).
        // Now downloads ALL blocks to also discover received notes missing
        // from the delta store (~569,998 zatoshis missing).
        Self::migrate_data_rescan_v9(&conn)?;

        // Migration v10: re-run with peer reconnection fix.
        // v9 ran but full block scan got 0/5607 blocks because peers
        // disconnected during the ~30s boost scan. Added peer recovery
        // and listener restart before download loop.
        Self::migrate_data_rescan_v10(&conn)?;

        // Migration v11: re-run with full reconnect fix.
        // v10 used stop/start_all_block_listeners which silently fails because
        // start_block_listener does reader.take() — once consumed, reader is
        // gone forever. Now uses full peer_manager.connect() for fresh readers.
        Self::migrate_data_rescan_v11(&conn)?;

        // Migration v12: re-run boost scan with enhanced diagnostics.
        // Adds: index field analysis, all-note logging, diversifier grouping,
        // index_field vs array_pos nullifier comparison.
        Self::migrate_data_rescan_v12(&conn)?;

        // Migration v13: re-run boost scan with ZIP-212 version byte fix.
        // plaintext_version_is_valid now accepts both 0x01 and 0x02 on Zclassic,
        // fixing notes created by wallets using post-Canopy format (569,998 gap).
        Self::migrate_data_rescan_v13(&conn)?;

        // Migration v14: re-run boost scan with comprehensive balance diagnostics.
        // Adds: spent value breakdown, change output analysis, gap search,
        // per-note DB audit, post-boost output counts, INSERT OR IGNORE verification.
        Self::migrate_data_rescan_v14(&conn)?;

        // Migration v15: re-run boost scan with tree root validation + compact
        // decryption fallback to find the missing 569,998 zatoshis.
        Self::migrate_data_rescan_v15(&conn)?;

        // Migration v16: full rescan with last_scanned_height reset.
        // Previous migrations reset tree_height but NOT last_scanned_height,
        // causing post_boost_full_block_scan() to skip (sees last_scanned >= chain_tip).
        // Post-boost spends were never re-detected → inflated balance.
        Self::migrate_data_rescan_v16(&conn)?;

        // Migration: add last_transparent_scanned column to sync_state.
        // Tracks transparent scanning independently of shielded scanning,
        // so blocks covered by delta (shielded-only) still get transparently scanned.
        let _ = conn.execute(
            "ALTER TABLE sync_state ADD COLUMN last_transparent_scanned INTEGER NOT NULL DEFAULT 0",
            [],
        ); // Ignores error if column already exists

        // Migration: add tboost_applied flag to sync_state.
        // Tracks whether the transparent boost file has been downloaded and applied,
        // independent of last_transparent_scanned (which is set by peer-based scanning).
        let _ = conn.execute(
            "ALTER TABLE sync_state ADD COLUMN tboost_applied INTEGER NOT NULL DEFAULT 0",
            [],
        ); // Ignores error if column already exists

        // Migration: add is_imported column to transparent_utxos.
        // Tracks whether a UTXO belongs to an imported (WIF) key rather than a derived key.
        let _ = conn.execute(
            "ALTER TABLE transparent_utxos ADD COLUMN is_imported INTEGER NOT NULL DEFAULT 0",
            [],
        ); // Ignores error if column already exists

        Ok(Self {
            conn: Mutex::new(conn),
            cached_sent_count: AtomicU32::new(0),
            cached_received_count: AtomicU32::new(0),
        })
    }

    // RS-N3 TODO: Add database backup/export functionality.
    // A `backup_to(path)` method using SQLite's online backup API
    // (`sqlite3_backup_init`) would allow safe hot backups without
    // locking the database. This is important for wallet recovery
    // and migration scenarios.

    /// Force WAL checkpoint.
    pub fn checkpoint(&self) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Note CRUD
    // ---------------------------------------------------------------

    /// Insert a shielded note. Nullifier is SHA-256 hashed before storage (VUL-009).
    /// Uses INSERT OR IGNORE to handle duplicate CMUs from rescanning.
    pub fn insert_note(
        &self,
        account_id: i64,
        height: u64,
        cmu: &[u8],
        value: u64,
        nullifier: Option<&[u8]>,
        rcm: Option<&[u8]>,
        epk: Option<&[u8]>,
        ciphertext: Option<&[u8]>,
        memo: Option<&str>,
        diversifier: Option<&[u8]>,
        witness: Option<&[u8]>,
        received_txid: Option<&str>,
        position: Option<u64>,
    ) -> Result<i64, StorageError> {
        let hashed_nf = nullifier.map(hash_nullifier);
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "INSERT OR IGNORE INTO notes
             (account_id, height, cmu, value, nullifier, rcm, epk, ciphertext,
              memo, diversifier, witness, received_txid, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                account_id,
                height as i64,
                cmu,
                value as i64,
                hashed_nf,
                rcm,
                epk,
                ciphertext,
                memo,
                diversifier,
                witness,
                received_txid,
                position.map(|p| p as i64),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a note by its database ID.
    pub fn get_note_by_id(&self, id: i64) -> Result<Option<Note>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row("SELECT * FROM notes WHERE id = ?1", params![id], |row| {
            row_to_note(row)
        })
        .optional()
        .map_err(Into::into)
    }

    /// Get a note by raw nullifier (auto-hashes for lookup, VUL-009).
    pub fn get_note_by_nullifier(&self, nullifier: &[u8]) -> Result<Option<Note>, StorageError> {
        let hashed = hash_nullifier(nullifier);
        self.get_note_by_hashed_nullifier(&hashed)
    }

    /// Get a note by pre-hashed nullifier.
    pub fn get_note_by_hashed_nullifier(
        &self,
        hashed_nf: &[u8],
    ) -> Result<Option<Note>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT * FROM notes WHERE nullifier = ?1",
            params![hashed_nf],
            |row| row_to_note(row),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Get all unspent notes for an account.
    pub fn get_all_unspent_notes(&self, account_id: i64) -> Result<Vec<Note>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt =
            conn.prepare("SELECT * FROM notes WHERE account_id = ?1 AND is_spent = 0")?;
        let notes = stmt
            .query_map(params![account_id], |row| row_to_note(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(notes)
    }

    /// Get all notes for an account (spent + unspent).
    pub fn get_all_notes(&self, account_id: i64) -> Result<Vec<Note>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare("SELECT * FROM notes WHERE account_id = ?1")?;
        let notes = stmt
            .query_map(params![account_id], |row| row_to_note(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(notes)
    }

    /// Count unspent notes for an account.
    pub fn count_unspent_notes(&self, account_id: i64) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE account_id = ?1 AND is_spent = 0",
            params![account_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Mark a note as spent by raw nullifier (auto-hashes, VUL-009).
    pub fn mark_note_spent(
        &self,
        nullifier: &[u8],
        txid: &str,
        spent_height: u64,
    ) -> Result<bool, StorageError> {
        let hashed = hash_nullifier(nullifier);
        self.mark_note_spent_by_hashed_nullifier(&hashed, txid, spent_height)
    }

    /// Mark a note as spent by pre-hashed nullifier.
    ///
    /// If the note is already marked spent (from the send flow) but has
    /// spent_height = 0 (unconfirmed), update the height to the confirmed
    /// block height so confirmation detection works.
    pub fn mark_note_spent_by_hashed_nullifier(
        &self,
        hashed_nf: &[u8],
        txid: &str,
        spent_height: u64,
    ) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let updated = conn.execute(
            "UPDATE notes SET is_spent = 1, spent_in_tx = ?1, spent_height = ?2
             WHERE nullifier = ?3 AND is_spent = 0",
            params![txid, spent_height as i64, hashed_nf],
        )?;

        // FIX: If note was already marked spent (from send flow) with
        // spent_height = 0, update the height now that we found it on-chain.
        if updated == 0 && spent_height > 0 {
            conn.execute(
                "UPDATE notes SET spent_height = ?1
                 WHERE nullifier = ?2 AND is_spent = 1
                 AND (spent_height IS NULL OR spent_height = 0)",
                params![spent_height as i64, hashed_nf],
            )?;
        }

        Ok(updated > 0)
    }

    /// Cross-reference a raw nullifier against unspent notes. If a match is found,
    /// marks the note as spent and returns `Some(value)`. Otherwise returns `None`.
    /// Used by post-boost delta scan to detect boost-era notes spent after boost height.
    pub fn cross_ref_nullifier_spend(
        &self,
        raw_nullifier: &[u8],
        txid: &str,
        spent_height: u64,
    ) -> Result<Option<u64>, StorageError> {
        let hashed = hash_nullifier(raw_nullifier);
        let conn = recover_lock(self.conn.lock());

        // RS-3: Wrap SELECT + UPDATE in an IMMEDIATE transaction to prevent
        // TOCTOU race where another thread could mark the note spent between
        // our SELECT and UPDATE.
        conn.execute_batch("BEGIN IMMEDIATE")?;

        let result = (|| -> Result<Option<u64>, StorageError> {
            // Check for an unspent note matching this nullifier
            let value: Option<u64> = conn
                .query_row(
                    "SELECT value FROM notes WHERE nullifier = ?1 AND is_spent = 0 LIMIT 1",
                    params![hashed],
                    |row| row.get::<_, i64>(0).map(|v| v.max(0) as u64),
                )
                .optional()?;

            if let Some(val) = value {
                conn.execute(
                    "UPDATE notes SET is_spent = 1, spent_in_tx = ?1, spent_height = ?2
                     WHERE nullifier = ?3 AND is_spent = 0",
                    params![txid, spent_height as i64, hashed],
                )?;
                Ok(Some(val))
            } else {
                Ok(None)
            }
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    /// Restore notes spent by a phantom/rejected TX (FIX #1168).
    /// Returns (count_restored, total_value_restored).
    pub fn restore_notes_spent_by_phantom_tx(
        &self,
        txid: &str,
    ) -> Result<(usize, u64), StorageError> {
        let conn = recover_lock(self.conn.lock());
        // Get value of notes being restored
        let total_value: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(value), 0) FROM notes
                 WHERE spent_in_tx = ?1 AND is_spent = 1",
                params![txid],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let restored = conn.execute(
            "UPDATE notes SET is_spent = 0, spent_in_tx = NULL, spent_height = NULL
             WHERE spent_in_tx = ?1 AND is_spent = 1",
            params![txid],
        )?;
        Ok((restored, total_value.max(0) as u64))
    }

    /// Mark a note as spent by its database primary key.
    /// Used by the send flow to reliably mark spent notes even when
    /// the stored nullifier doesn't match (due to incomplete delta store
    /// causing wrong positions → wrong nullifiers).
    pub fn mark_note_spent_by_id(
        &self,
        note_id: i64,
        txid: &str,
        spent_height: u64,
    ) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let updated = conn.execute(
            "UPDATE notes SET is_spent = 1, spent_in_tx = ?1, spent_height = ?2
             WHERE id = ?3 AND is_spent = 0",
            params![txid, spent_height as i64, note_id],
        )?;
        Ok(updated > 0)
    }

    /// Find all distinct txids that have notes marked as spent but were never mined
    /// (spent_height = 0). These are candidates for auto-recovery if the TX has expired.
    /// FIX #1300: Auto-recover notes from unconfirmed sends.
    pub fn get_unconfirmed_spent_txids(&self) -> Result<Vec<String>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT DISTINCT spent_in_tx FROM notes
             WHERE is_spent = 1 AND spent_height = 0 AND spent_in_tx IS NOT NULL",
        )?;
        let txids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(txids)
    }

    /// Check if a transaction was confirmed (mined into a block).
    /// Returns true if the txid appears in transaction_history with height > 0.
    pub fn is_transaction_confirmed(&self, txid: &str) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transaction_history
             WHERE txid = ?1 AND height > 0",
            params![txid],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Auto-recover notes from unconfirmed sends that have expired.
    /// Called after sync completes. If a TX was never mined and enough blocks
    /// have passed (expiry_blocks), restore the notes to spendable.
    /// Returns total notes restored and total value restored.
    /// FIX #1300: Automatic recovery of notes from failed/expired sends.
    pub fn auto_recover_expired_sends(
        &self,
        _current_height: u64,
        broadcast_expiry_blocks: u64,
    ) -> Result<(usize, u64), StorageError> {
        // FIX I11: Single lock acquisition + IMMEDIATE transaction to eliminate
        // TOCTOU race between checking confirmation status and restoring notes.
        // Previously, get_unconfirmed_spent_txids() acquired/released the lock,
        // then a second lock was acquired — a TX could be confirmed in between.
        let conn = recover_lock(self.conn.lock());

        // Inline the unconfirmed txid query (was get_unconfirmed_spent_txids)
        let unconfirmed_txids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT spent_in_tx FROM notes
                 WHERE is_spent = 1 AND spent_height = 0 AND spent_in_tx IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        if unconfirmed_txids.is_empty() {
            return Ok((0, 0));
        }

        let mut total_restored = 0usize;
        let mut total_value = 0u64;

        // Use IMMEDIATE transaction so the confirmation check + note restore
        // are atomic — no other writer can insert a confirmation in between.
        conn.execute_batch("BEGIN IMMEDIATE")?;

        let result: Result<(), StorageError> = (|| {
            for txid in &unconfirmed_txids {
                // Check if this TX was confirmed in any block
                let confirmed: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM transaction_history
                     WHERE txid = ?1 AND height > 0",
                        params![txid],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                if confirmed > 0 {
                    continue; // TX was mined — don't recover
                }

                // FIX #1300: Use BROADCAST TIMESTAMP for expiry, NOT note height.
                // The note height is when the note was RECEIVED, which could be thousands
                // of blocks ago. The broadcast time is when the TX was actually sent.
                // Without a valid timestamp, we cannot determine expiry — skip recovery.
                let tx_timestamp: Option<i64> = conn
                    .query_row(
                        "SELECT timestamp FROM transaction_history WHERE txid = ?1",
                        params![txid],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                let expired = match tx_timestamp {
                    Some(ts) if ts > 0 => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        let elapsed_secs = now - ts;
                        // 20 blocks × 75s = 1500s = 25 minutes
                        let expiry_secs = broadcast_expiry_blocks as i64 * 75;
                        elapsed_secs > expiry_secs
                    }
                    _ => {
                        // No timestamp — legacy TX from before FIX #1300.
                        // Use a conservative fallback: check if enough blocks have passed
                        // since the CURRENT chain tip minus expiry window. If the TX has been
                        // sitting unconfirmed and we're fully synced, recover it.
                        // Only recover if we've completed at least 2 full syncs past the
                        // expiry window (to avoid premature recovery of legacy TXs).
                        false // Don't auto-recover TXs without timestamps — require manual rescan
                    }
                };

                if expired {
                    // Restore notes
                    let value: i64 = conn
                        .query_row(
                            "SELECT COALESCE(SUM(value), 0) FROM notes
                         WHERE spent_in_tx = ?1 AND is_spent = 1",
                            params![txid],
                            |row| row.get(0),
                        )
                        .unwrap_or(0);

                    let restored = conn.execute(
                        "UPDATE notes SET is_spent = 0, spent_in_tx = NULL, spent_height = NULL
                         WHERE spent_in_tx = ?1 AND is_spent = 1",
                        params![txid],
                    )?;

                    // Mark the TX history as rejected
                    conn.execute(
                        "UPDATE transaction_history SET status = 'rejected'
                         WHERE txid = ?1 AND height = 0",
                        params![txid],
                    )?;

                    if restored > 0 {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[ZipherX] AUTO-RECOVERY: Restored {} note(s) worth {} zatoshis from expired TX {}",
                            restored,
                            value,
                            &txid[..16.min(txid.len())]
                        );
                        total_restored += restored;
                        total_value += value.max(0) as u64;
                    }
                }
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }

        Ok((total_restored, total_value))
    }

    /// Update witness for a note.
    ///
    /// RS-8: The witness blob is stored and retrieved without cryptographic
    /// integrity verification (e.g., MAC or hash). The only validation is
    /// a minimum length check (`LENGTH(witness) >= 100`) in balance queries.
    /// This means a corrupted witness blob would not be detected until the
    /// Sapling prover attempts to use it for transaction construction, at
    /// which point the anchor validation against the blockchain will fail.
    /// A future improvement could store a hash alongside the witness and
    /// verify it on retrieval.
    pub fn update_note_witness(&self, note_id: i64, witness: &[u8]) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE notes SET witness = ?1 WHERE id = ?2",
            params![witness, note_id],
        )?;
        Ok(())
    }

    /// Update anchor for a note.
    pub fn update_note_anchor(&self, note_id: i64, anchor: &[u8]) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE notes SET anchor = ?1 WHERE id = ?2",
            params![anchor, note_id],
        )?;
        Ok(())
    }

    /// Clear witness for a single note.
    pub fn clear_witness_for_note(&self, note_id: i64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE notes SET witness = NULL WHERE id = ?1",
            params![note_id],
        )?;
        Ok(())
    }

    /// Clear all witnesses (FIX #1238). Returns count of cleared notes.
    pub fn clear_all_witnesses(&self) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let count = conn.execute(
            "UPDATE notes SET witness = NULL WHERE witness IS NOT NULL",
            [],
        )?;
        Ok(count)
    }

    /// Update the nullifier and position for a note identified by CMU.
    /// Used to fix nullifiers after correct tree positions are computed.
    /// The raw_nullifier is hashed before storage (VUL-009).
    pub fn update_note_nullifier_by_cmu(
        &self,
        cmu: &[u8],
        raw_nullifier: &[u8],
        position: u64,
    ) -> Result<bool, StorageError> {
        let hashed_nf = hash_nullifier(raw_nullifier);
        let conn = recover_lock(self.conn.lock());
        let updated = conn.execute(
            "UPDATE notes SET nullifier = ?1, position = ?2 WHERE cmu = ?3",
            params![hashed_nf, position as i64, cmu],
        )?;
        Ok(updated > 0)
    }

    /// Delete all notes (for full wipe).
    pub fn delete_all_notes(&self) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute("DELETE FROM notes", [])?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Balance queries
    // ---------------------------------------------------------------

    /// Spendable balance: unspent notes WITH valid witnesses (FIX #1210).
    pub fn get_balance(&self, account_id: i64) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let val: i64 = conn.query_row(
            "SELECT COALESCE(SUM(value), 0) FROM notes
             WHERE account_id = ?1 AND is_spent = 0
             AND witness IS NOT NULL AND LENGTH(witness) >= 100",
            params![account_id],
            |row| row.get(0),
        )?;
        Ok(val.max(0) as u64)
    }

    /// Total unspent balance: includes notes WITHOUT witnesses + orphan-spent (FIX #1210/1233).
    pub fn get_total_unspent_balance(&self, account_id: i64) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let val: i64 = conn.query_row(
            "SELECT COALESCE(SUM(value), 0) FROM notes
             WHERE account_id = ?1
             AND (is_spent = 0 OR (is_spent = 1 AND (spent_in_tx IS NULL OR spent_in_tx = '')))",
            params![account_id],
            |row| row.get(0),
        )?;
        Ok(val.max(0) as u64)
    }

    /// Get info about notes without valid witnesses.
    /// Returns (count, total_value, min_height).
    pub fn get_notes_without_witnesses(
        &self,
        account_id: i64,
    ) -> Result<(usize, u64, u64), StorageError> {
        let conn = recover_lock(self.conn.lock());
        let (count, value, min_h): (i64, i64, Option<i64>) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(value), 0), MIN(height) FROM notes
             WHERE account_id = ?1 AND is_spent = 0
             AND (witness IS NULL OR LENGTH(witness) < 100)",
            params![account_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((
            count as usize,
            value.max(0) as u64,
            min_h.unwrap_or(0) as u64,
        ))
    }

    // ---------------------------------------------------------------
    // Transaction history
    // ---------------------------------------------------------------

    /// Insert or replace a transaction record.
    pub fn insert_transaction(
        &self,
        txid: &str,
        height: u64,
        timestamp: Option<u64>,
        tx_type: TxType,
        amount: u64,
        fee: u64,
        address: Option<&str>,
        memo: Option<&str>,
        status: TxStatus,
    ) -> Result<i64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "INSERT OR IGNORE INTO transaction_history
             (txid, height, timestamp, tx_type, amount, fee, address, memo, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                txid,
                height as i64,
                timestamp.map(|t| t as i64),
                tx_type.as_str(),
                amount as i64,
                fee as i64,
                address,
                memo,
                status.as_str(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a transaction by txid.
    pub fn get_transaction_by_txid(
        &self,
        txid: &str,
    ) -> Result<Option<TransactionRecord>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT * FROM transaction_history WHERE txid = ?1",
            params![txid],
            |row| row_to_tx_record(row),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Get paginated transaction history (newest first).
    ///
    /// Filters out change outputs at query time:
    /// - Explicit "change" type entries are excluded.
    /// - "received" entries where the same TX also spent our notes are excluded
    ///   (these are change outputs, not real receives).
    /// - "sent" amounts are adjusted to net (total_input - change - fee).
    pub fn get_transaction_history(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TransactionRecord>, StorageError> {
        let conn = recover_lock(self.conn.lock());

        // Fetch raw records, excluding explicit "change" type
        let mut stmt = conn.prepare(
            "SELECT * FROM transaction_history
             WHERE tx_type != 'change'
             ORDER BY height DESC, id DESC",
        )?;
        let all_records: Vec<TransactionRecord> = stmt
            .query_map([], |row| row_to_tx_record(row))?
            .collect::<Result<Vec<_>, _>>()?;

        // Build set of txids where we SPENT notes (from notes table — authoritative).
        // If a "received" entry has the same txid as a spent note, it's change.
        let mut spend_txids = std::collections::HashSet::new();
        let mut spend_stmt = conn.prepare(
            "SELECT DISTINCT spent_in_tx FROM notes
             WHERE is_spent = 1 AND spent_in_tx IS NOT NULL AND spent_in_tx != ''",
        )?;
        let txids = spend_stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok());
        for txid in txids {
            spend_txids.insert(txid);
        }

        // Get total input from notes table for "sent" TXs.
        // This is more reliable than the stored "sent" amount which may be raw or net.
        let mut input_stmt = conn.prepare(
            "SELECT COALESCE(SUM(value), 0) FROM notes
             WHERE spent_in_tx = ?1 AND is_spent = 1",
        )?;

        // Get total change from notes table (authoritative — captures ALL outputs).
        // The transaction_history has UNIQUE(txid, tx_type), so INSERT OR IGNORE
        // drops duplicate "received" entries when a TX has multiple outputs to us
        // (e.g., send-to-self with change). The notes table has no such limit.
        let mut notes_change_stmt = conn.prepare(
            "SELECT COALESCE(SUM(value), 0) FROM notes
             WHERE received_txid = ?1",
        )?;

        // Build set of txids that already have "sent" entries
        let sent_txids: std::collections::HashSet<String> = all_records
            .iter()
            .filter(|r| r.tx_type == TxType::Sent)
            .map(|r| r.txid.clone())
            .collect();

        // Filter and adjust
        let mut result = Vec::new();
        for mut rec in all_records.clone() {
            // Skip "received" entries that are actually change
            if rec.tx_type == TxType::Received && spend_txids.contains(&rec.txid) {
                continue;
            }

            // For "sent" entries, compute correct net amount using notes table
            if rec.tx_type == TxType::Sent {
                // Get authoritative total input from notes table
                let total_input: i64 = input_stmt
                    .query_row(params![rec.txid], |row| row.get(0))
                    .unwrap_or(0);

                // Get total change from notes table (all outputs back to us)
                let change_amount: i64 = notes_change_stmt
                    .query_row(params![rec.txid], |row| row.get(0))
                    .unwrap_or(0);

                if total_input > 0 && change_amount > 0 {
                    let net = total_input - change_amount - rec.fee as i64;
                    if net <= 0 {
                        // Net is zero or negative — this is a send-to-self.
                        // Show as SelfTransfer with the fee as the "cost".
                        rec.tx_type = TxType::SelfTransfer;
                        rec.amount = rec.fee;
                    } else {
                        rec.amount = net as u64;
                    }
                }

                // Check if this "sent" tx has a transparent UTXO belonging to us.
                // If so, shielded value went to our own t-address → z→t self-send.
                // SKIP if the sent entry already has a transparent destination
                // (starts with "t1"/"t3") — that means it's a real t→t send with
                // change, not a z→t deshield. Overwriting would hide the real send.
                let already_transparent_send = rec.address.as_ref()
                    .map_or(false, |a| a.starts_with("t1") || a.starts_with("t3"));
                if rec.tx_type == TxType::Sent && !already_transparent_send {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[ZipherX] z→t check: txid={} (len={})",
                        &rec.txid, rec.txid.len()
                    );

                    // Try exact match — exclude change UTXOs (is_change=1) which are
                    // internal outputs, not real z→t deshielding destinations.
                    let t_utxo: Option<(i64, String)> = conn
                        .prepare(
                            "SELECT value, address FROM transparent_utxos WHERE txid = ?1 AND is_change = 0 LIMIT 1",
                        )
                        .ok()
                        .and_then(|mut stmt| {
                            stmt.query_row(params![rec.txid], |row| {
                                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                            })
                            .ok()
                        });

                    if let Some((t_value, t_addr)) = t_utxo {
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX] MATCH! z→t self-send: value={} addr={}", t_value, t_addr);
                        rec.tx_type = TxType::SelfZ2T;
                        rec.amount = t_value as u64;
                        rec.address = Some(t_addr);
                    }
                } else if already_transparent_send {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[ZipherX] z→t check: skip txid={} (already transparent send to {})",
                        &rec.txid[..16.min(rec.txid.len())],
                        rec.address.as_deref().unwrap_or("?"),
                    );
                }
            }

            result.push(rec);
        }

        // Synthesize "sent" records for spend txids that have NO "sent" entry.
        // This handles the case where notes were marked spent (spent_in_tx set)
        // but no "sent" TX history entry was ever created (e.g., post-boost scan
        // marked spends without creating history entries in older code).
        #[cfg(debug_assertions)]
        eprintln!("[ZipherX] Synthesize check: {} spend_txids, {} sent_txids", spend_txids.len(), sent_txids.len());
        for txid in &spend_txids {
            if sent_txids.contains(txid) {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX] Synthesize: skip {} (has real sent entry)", &txid[..16.min(txid.len())]);
                continue; // Already has a real "sent" entry
            }
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Synthesize: processing {} (no real sent entry)", &txid[..16.min(txid.len())]);

            // Check if we already synthesized or have this in result
            if result.iter().any(|r| {
                &r.txid == txid && (r.tx_type == TxType::Sent || r.tx_type == TxType::SelfTransfer || r.tx_type == TxType::SelfZ2T || r.tx_type == TxType::SelfT2Z)
            }) {
                continue;
            }

            let total_input: i64 = input_stmt
                .query_row(params![txid], |row| row.get(0))
                .unwrap_or(0);

            if total_input <= 0 {
                continue;
            }

            let change: i64 = notes_change_stmt
                .query_row(params![txid], |row| row.get(0))
                .unwrap_or(0);

            // Get height, timestamp, and fee from any existing record for this txid
            // FIX I13: Use the actual fee from the existing record instead of hardcoding 10,000
            let (height, timestamp, rec_fee) = all_records
                .iter()
                .find(|r| r.txid == *txid)
                .map(|r| (r.height, r.timestamp, r.fee))
                .unwrap_or((0, None, 10_000));
            let fee = if rec_fee > 0 { rec_fee as i64 } else { 10_000i64 };
            let net = total_input - change - fee;

            if net <= 0 {
                // Send-to-self: all outputs went back to us
                result.push(TransactionRecord {
                    id: 0,
                    txid: txid.clone(),
                    tx_type: TxType::SelfTransfer,
                    amount: fee as u64,
                    fee: fee as u64,
                    address: None,
                    memo: None,
                    confirmations: 0,
                    timestamp,
                    status: TxStatus::Confirmed,
                    height,
                });
            } else {
                // Check if this is a z→t self-send
                let t_utxo: Option<(i64, String)> = conn
                    .prepare(
                        "SELECT value, address FROM transparent_utxos WHERE txid = ?1 LIMIT 1",
                    )
                    .ok()
                    .and_then(|mut stmt| {
                        stmt.query_row(params![txid], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })
                        .ok()
                    });
                if let Some((t_value, t_addr)) = t_utxo {
                    result.push(TransactionRecord {
                        id: 0,
                        txid: txid.clone(),
                        tx_type: TxType::SelfZ2T,
                        amount: t_value as u64,
                        fee: fee as u64,
                        address: Some(t_addr),
                        memo: None,
                        confirmations: 0,
                        timestamp,
                        status: TxStatus::Confirmed,
                        height,
                    });
                } else {
                    result.push(TransactionRecord {
                        id: 0,
                        txid: txid.clone(),
                        tx_type: TxType::Sent,
                        amount: net as u64,
                        fee: fee as u64,
                        address: None,
                        memo: None,
                        confirmations: 0,
                        timestamp,
                        status: TxStatus::Confirmed,
                        height,
                    });
                }
            }
        }

        // Self-send fallback: if same txid has both "sent" AND "received" entries,
        // it's a self-send that wasn't detected by the notes-based method above.
        // This catches cases where nullifier matching failed (wrong positions)
        // so the notes-based total_input was 0 and the "received" entry wasn't filtered.
        {
            // Build map of txid → indices for "sent" entries
            let mut sent_idx_map: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for (i, r) in result.iter().enumerate() {
                if r.tx_type == TxType::Sent {
                    sent_idx_map.insert(r.txid.clone(), i);
                }
            }
            // Find "received" entries whose txid also has a "sent" entry
            let mut recv_to_remove: Vec<usize> = Vec::new();
            let mut sent_to_convert: Vec<usize> = Vec::new();
            for (i, r) in result.iter().enumerate() {
                if r.tx_type == TxType::Received {
                    if let Some(&si) = sent_idx_map.get(&r.txid) {
                        recv_to_remove.push(i);
                        sent_to_convert.push(si);
                    }
                }
            }
            // Convert matched "sent" entries to SelfTransfer, remove "received" dupes
            for &si in &sent_to_convert {
                result[si].tx_type = TxType::SelfTransfer;
                result[si].amount = result[si].fee;
            }
            recv_to_remove.sort_unstable();
            for &ri in recv_to_remove.iter().rev() {
                result.remove(ri);
            }
        }

        // Re-sort by height DESC (synthesized records may be at different heights)
        result.sort_by(|a, b| b.height.cmp(&a.height).then(b.id.cmp(&a.id)));

        // Cache total counts before pagination (avoids a separate full re-fetch)
        let sent = result
            .iter()
            .filter(|r| r.tx_type == TxType::Sent || r.tx_type == TxType::SelfTransfer || r.tx_type == TxType::SelfZ2T || r.tx_type == TxType::SelfT2Z)
            .count() as u32;
        let received = result
            .iter()
            .filter(|r| r.tx_type == TxType::Received)
            .count() as u32;
        self.cached_sent_count.store(sent, Ordering::Relaxed);
        self.cached_received_count
            .store(received, Ordering::Relaxed);

        // Apply pagination
        let start = offset.min(result.len());
        let end = (start + limit).min(result.len());
        Ok(result[start..end].to_vec())
    }

    /// Get total IN (received) and OUT (sent) transaction counts.
    ///
    /// Returns counts cached by the last `get_transaction_history` call.
    /// These use the same filtering (excludes change, consolidation, phantom)
    /// so counts match visible history. Returns (0, 0) if history hasn't
    /// been fetched yet.
    pub fn get_transaction_counts(&self) -> Result<(u32, u32), StorageError> {
        Ok((
            self.cached_sent_count.load(Ordering::Relaxed),
            self.cached_received_count.load(Ordering::Relaxed),
        ))
    }

    /// Get pending transactions.
    pub fn get_pending_transactions(&self) -> Result<Vec<TransactionRecord>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt =
            conn.prepare("SELECT * FROM transaction_history WHERE status = 'pending'")?;
        let records = stmt
            .query_map([], |row| row_to_tx_record(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    /// Update transaction status and optionally height/timestamp.
    pub fn update_transaction_status(
        &self,
        txid: &str,
        status: TxStatus,
        height: Option<u64>,
        timestamp: Option<u64>,
    ) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let updated = if let (Some(h), Some(ts)) = (height, timestamp) {
            conn.execute(
                "UPDATE transaction_history SET status = ?1, height = ?2, timestamp = ?3
                 WHERE txid = ?4",
                params![status.as_str(), h as i64, ts as i64, txid],
            )?
        } else if let Some(h) = height {
            conn.execute(
                "UPDATE transaction_history SET status = ?1, height = ?2 WHERE txid = ?3",
                params![status.as_str(), h as i64, txid],
            )?
        } else {
            conn.execute(
                "UPDATE transaction_history SET status = ?1 WHERE txid = ?2",
                params![status.as_str(), txid],
            )?
        };
        Ok(updated > 0)
    }

    /// Update confirmations for all confirmed TXs based on chain height.
    pub fn update_all_confirmations(&self, chain_height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());

        // FIX: "sent" entries from record_sent_transaction_atomic have height=0
        // (unconfirmed at send time). Once a "received" entry for the same txid
        // appears at a known height (from the sync), copy that height to the
        // "sent" entry so confirmations can be computed. This is critical for
        // the client's checkPendingConfirmation() to detect confirmation.
        // All height-backfill queries use ORDER BY ... DESC to always pick the
        // highest (confirmed) height, not a stale height=0 row from an earlier
        // unconfirmed detection.
        conn.execute(
            "UPDATE transaction_history SET height = (
                 SELECT h2.height FROM transaction_history h2
                 WHERE h2.txid = transaction_history.txid AND h2.height > 0
                 ORDER BY h2.height DESC LIMIT 1
             )
             WHERE height = 0 AND EXISTS (
                 SELECT 1 FROM transaction_history h2
                 WHERE h2.txid = transaction_history.txid AND h2.height > 0
             )",
            [],
        )?;

        // For transparent-only sends (t→t): copy height from transparent_utxos.
        // SECURITY: Scope to transparent-related tx_types.
        conn.execute(
            "UPDATE transaction_history SET height = (
                 SELECT tu.height FROM transparent_utxos tu
                 WHERE tu.txid = transaction_history.txid AND tu.height > 0
                 ORDER BY tu.height DESC LIMIT 1
             )
             WHERE height = 0
             AND tx_type IN ('sent', 'self_z2t', 'self_t2z')
             AND (address LIKE 't1%' OR address LIKE 't3%' OR tx_type IN ('self_z2t', 'self_t2z'))
             AND EXISTS (
                 SELECT 1 FROM transparent_utxos tu
                 WHERE tu.txid = transaction_history.txid AND tu.height > 0
             )",
            [],
        )?;

        // For sends with no change output: copy from notes.spent_height.
        conn.execute(
            "UPDATE transaction_history SET height = (
                 SELECT n.spent_height FROM notes n
                 WHERE n.spent_in_tx = transaction_history.txid AND n.spent_height > 0
                 ORDER BY n.spent_height DESC LIMIT 1
             )
             WHERE height = 0 AND EXISTS (
                 SELECT 1 FROM notes n
                 WHERE n.spent_in_tx = transaction_history.txid AND n.spent_height > 0
             )",
            [],
        )?;

        // For transparent sends: copy from transparent_utxos.spent_height.
        // SECURITY: Scope to transparent-related tx_types.
        conn.execute(
            "UPDATE transaction_history SET height = (
                 SELECT tu.spent_height FROM transparent_utxos tu
                 WHERE tu.spent_in_tx = transaction_history.txid AND tu.spent_height > 0
                 ORDER BY tu.spent_height DESC LIMIT 1
             )
             WHERE height = 0
             AND tx_type IN ('sent', 'self_z2t', 'self_t2z')
             AND EXISTS (
                 SELECT 1 FROM transparent_utxos tu
                 WHERE tu.spent_in_tx = transaction_history.txid AND tu.spent_height > 0
             )",
            [],
        )?;

        conn.execute(
            "UPDATE transaction_history SET confirmations = ?1 - height + 1
             WHERE height > 0 AND height <= ?1",
            params![chain_height as i64],
        )?;
        Ok(())
    }

    /// Check if there are any unconfirmed sent transactions (height=0, tx_type='sent').
    /// Used to enable faster sync polling while awaiting confirmation.
    pub fn has_pending_sent_transactions(&self) -> bool {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT COUNT(*) FROM transaction_history WHERE height = 0 AND tx_type = 'sent'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
    }

    /// Get distinct block heights for transactions with missing timestamps.
    pub fn get_heights_needing_timestamps(&self) -> Result<Vec<u64>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT DISTINCT height FROM transaction_history
             WHERE (timestamp IS NULL OR timestamp = 0) AND height > 0",
        )?;
        let heights = stmt
            .query_map([], |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .map(|h| h as u64)
            .collect();
        Ok(heights)
    }

    /// Set timestamps for all transactions at a given height (only if currently NULL/0).
    pub fn set_timestamps_for_height(
        &self,
        height: u64,
        timestamp: u64,
    ) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let updated = conn.execute(
            "UPDATE transaction_history SET timestamp = ?1
             WHERE height = ?2 AND (timestamp IS NULL OR timestamp = 0)",
            params![timestamp as i64, height as i64],
        )?;
        Ok(updated)
    }

    /// Delete a phantom transaction. Returns deleted amount if found.
    pub fn delete_phantom_transaction(&self, txid: &str) -> Result<Option<u64>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let amount: Option<i64> = conn
            .query_row(
                "SELECT amount FROM transaction_history WHERE txid = ?1",
                params![txid],
                |row| row.get(0),
            )
            .optional()?;

        if amount.is_some() {
            conn.execute(
                "DELETE FROM transaction_history WHERE txid = ?1",
                params![txid],
            )?;
        }
        Ok(amount.map(|a| a as u64))
    }

    /// Get total transaction count.
    pub fn get_transaction_count(&self) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM transaction_history", [], |row| {
            row.get(0)
        })?;
        Ok(count as usize)
    }

    /// Count transactions by type (sent, received, change).
    pub fn get_transaction_type_counts(&self) -> Result<(usize, usize, usize), StorageError> {
        let conn = recover_lock(self.conn.lock());
        let sent: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transaction_history WHERE tx_type = 'sent'",
            [],
            |row| row.get(0),
        )?;
        let received: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transaction_history WHERE tx_type = 'received'",
            [],
            |row| row.get(0),
        )?;
        let change: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transaction_history WHERE tx_type = 'change'",
            [],
            |row| row.get(0),
        )?;
        Ok((sent as usize, received as usize, change as usize))
    }

    /// Clear all transaction history.
    pub fn clear_transaction_history(&self) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute("DELETE FROM transaction_history", [])?;
        Ok(())
    }

    /// Reconcile change outputs in transaction history.
    ///
    /// When a TX both spends our notes (nullifier match → "sent" entry) AND
    /// creates new notes to us (trial decrypt → "received" entry), the received
    /// notes are CHANGE, not real receives.
    ///
    /// This method:
    /// 1. Reclassifies "received" → "change" for txids that also have a "sent" entry
    /// 2. Recomputes "sent" amounts from the authoritative notes table
    ///    (SUM of spent note values - change - fee) to avoid double-subtraction
    ///    when the boost scan already stored net amounts.
    pub fn reconcile_change_outputs(&self) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());

        // Find txids with BOTH "sent" and "received" entries.
        // The "received" entries on these txids are change, not real receives.
        let mut stmt = conn.prepare(
            "SELECT r.txid, r.amount as change_amount
             FROM transaction_history r
             JOIN transaction_history s ON r.txid = s.txid AND s.tx_type = 'sent'
             WHERE r.tx_type = 'received'",
        )?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(0);
        }

        let count = rows.len();
        #[cfg(debug_assertions)]
        eprintln!(
            "[ZipherX] Reconciling {} change outputs in transaction history",
            count,
        );

        // Prepare statement to get total input from notes table (authoritative).
        // spent_in_tx stores the txid hex string for each spent note.
        let mut input_stmt = conn.prepare(
            "SELECT COALESCE(SUM(value), 0) FROM notes WHERE spent_in_tx = ?1 AND is_spent = 1",
        )?;

        // FIX I13: Fetch the actual fee from the existing 'sent' entry instead of hardcoding 10,000.
        let mut fee_stmt = conn.prepare(
            "SELECT COALESCE(fee, 10000) FROM transaction_history
             WHERE txid = ?1 AND tx_type = 'sent' LIMIT 1",
        )?;

        for (txid, change_amount) in &rows {
            // Reclassify "received" → "change"
            conn.execute(
                "UPDATE transaction_history SET tx_type = 'change'
                 WHERE txid = ?1 AND tx_type = 'received'",
                params![txid],
            )?;

            // Get the TRUE total input from the notes table.
            // This is the sum of all note values spent in this TX.
            let total_input: i64 = input_stmt.query_row(params![txid], |row| row.get(0))?;

            if total_input > 0 {
                // Recompute net sent from authoritative source:
                // net = total_input - change - fee
                // FIX I13: Use actual fee from the sent record, fallback to 10,000
                let fee: i64 = fee_stmt.query_row(params![txid], |row| row.get(0)).unwrap_or(10_000i64);
                let net_sent = total_input
                    .saturating_sub(*change_amount)
                    .saturating_sub(fee);
                let net_sent = if net_sent < 0 { 0i64 } else { net_sent };

                conn.execute(
                    "UPDATE transaction_history SET amount = ?1
                     WHERE txid = ?2 AND tx_type = 'sent'",
                    params![net_sent, txid],
                )?;

                #[cfg(debug_assertions)]
                eprintln!(
                    "[ZipherX]   Reconciled tx {}...: input={}, change={}, net_sent={}",
                    &txid[..16.min(txid.len())],
                    total_input,
                    change_amount,
                    net_sent,
                );
            }
            // If total_input == 0, the notes may have been pruned or the txid
            // format doesn't match — leave the sent amount as-is.
        }

        Ok(count)
    }

    // ---------------------------------------------------------------
    // Atomic TX recording (FIX #291)
    // ---------------------------------------------------------------

    /// Atomically mark a note as spent AND insert TX history.
    /// If either operation fails, both are rolled back.
    pub fn record_sent_transaction_atomic(
        &self,
        raw_nullifier: &[u8],
        txid: &str,
        spent_height: u64,
        amount: u64,
        fee: u64,
        memo: Option<&str>,
    ) -> Result<i64, StorageError> {
        // FIX: The DB stores hash_nullifier(raw_nf), so we must hash before comparison.
        // Previously this compared raw bytes against hashed bytes → 0 matches → input
        // note never marked as spent → balance inflation on self-sends.
        let hashed_nf = hash_nullifier(raw_nullifier);

        let mut conn = recover_lock(self.conn.lock());
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| StorageError::TransactionFailed(e.to_string()))?;

        // Step 1: Mark note as spent
        tx.execute(
            "UPDATE notes SET is_spent = 1, spent_in_tx = ?1, spent_height = ?2
             WHERE nullifier = ?3 AND is_spent = 0",
            params![txid, spent_height as i64, hashed_nf],
        )
        .map_err(|e| StorageError::TransactionFailed(format!("Mark spent: {e}")))?;

        // Step 2: Insert transaction history
        // RS-7: Use INSERT OR IGNORE to prevent overwriting existing records.
        // REPLACE would delete and re-insert, losing any fields set by other
        // code paths (e.g., confirmations, address, timestamp).
        // FIX #1300: Store broadcast timestamp for auto-recovery expiry calculation.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        tx.execute(
            "INSERT OR IGNORE INTO transaction_history
             (txid, height, tx_type, amount, fee, memo, status, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'confirmed', ?7)",
            params![
                txid,
                spent_height as i64,
                TxType::Sent.as_str(),
                amount as i64,
                fee as i64,
                memo,
                now,
            ],
        )
        .map_err(|e| StorageError::TransactionFailed(format!("Insert TX: {e}")))?;

        let history_id = tx.last_insert_rowid();
        tx.commit()
            .map_err(|e| StorageError::TransactionFailed(format!("Commit: {e}")))?;

        Ok(history_id)
    }

    // ---------------------------------------------------------------
    // Sync state
    // ---------------------------------------------------------------

    /// Get sync state (singleton row id=1).
    pub fn get_sync_state(&self) -> Result<SyncState, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT last_scanned_height, verified_checkpoint_height, tree_state,
                    tree_height, boost_file_height, boost_cmu_count, delta_bundle_verified
             FROM sync_state WHERE id = 1",
            [],
            |row| {
                Ok(SyncState {
                    last_scanned_height: row.get::<_, i64>(0)? as u64,
                    verified_checkpoint_height: row.get::<_, i64>(1)? as u64,
                    tree_state: row.get(2)?,
                    tree_height: row.get::<_, i64>(3)? as u64,
                    boost_file_height: row.get::<_, i64>(4)? as u64,
                    boost_cmu_count: row.get::<_, i64>(5)? as u64,
                    delta_bundle_verified: row.get::<_, i64>(6)? != 0,
                })
            },
        )
        .map_err(Into::into)
    }

    /// Get last transparent scanned height.
    pub fn get_last_transparent_scanned(&self) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT last_transparent_scanned FROM sync_state WHERE id = 1",
            [],
            |row| Ok(row.get::<_, i64>(0)? as u64),
        )
        .map_err(Into::into)
    }

    /// Update last transparent scanned height.
    pub fn update_last_transparent_scanned(&self, height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET last_transparent_scanned = ?1 WHERE id = 1",
            params![height as i64],
        )?;
        Ok(())
    }

    /// Check if the transparent boost file has been applied.
    pub fn get_tboost_applied(&self) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.query_row(
            "SELECT tboost_applied FROM sync_state WHERE id = 1",
            [],
            |row| Ok(row.get::<_, i64>(0)? != 0),
        )
        .map_err(Into::into)
    }

    /// Mark the transparent boost file as applied.
    pub fn set_tboost_applied(&self, applied: bool) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET tboost_applied = ?1 WHERE id = 1",
            params![applied as i64],
        )?;
        Ok(())
    }

    /// Update last scanned height.
    pub fn update_last_scanned_height(&self, height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET last_scanned_height = ?1 WHERE id = 1",
            params![height as i64],
        )?;
        Ok(())
    }

    /// Save tree state blob + height.
    pub fn save_tree_state(&self, state: &[u8], height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET tree_state = ?1, tree_height = ?2 WHERE id = 1",
            params![state, height as i64],
        )?;
        Ok(())
    }

    /// Get tree state blob.
    pub fn get_tree_state(&self) -> Result<Option<Vec<u8>>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let state: Option<Vec<u8>> = conn.query_row(
            "SELECT tree_state FROM sync_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(state)
    }

    /// Get tree height.
    pub fn get_tree_height(&self) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let h: i64 = conn.query_row(
            "SELECT tree_height FROM sync_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(h as u64)
    }

    /// Update tree height only.
    pub fn update_tree_height(&self, height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET tree_height = ?1 WHERE id = 1",
            params![height as i64],
        )?;
        Ok(())
    }

    /// Clear tree state only — preserves witnesses (FIX #1210).
    pub fn clear_tree_state_only(&self) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1",
            [],
        )?;
        Ok(())
    }

    /// Clear tree state AND all witnesses (for full rebuild).
    pub fn clear_tree_state_for_rebuild(&self) -> Result<(), StorageError> {
        let mut conn = recover_lock(self.conn.lock());
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1",
            [],
        )?;
        tx.execute(
            "UPDATE notes SET witness = NULL WHERE witness IS NOT NULL",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// One-time migration: delete notes with all-zero received_txid.
    ///
    /// Before synthetic txids were introduced, the catch-up scan stored notes
    /// with txid [0u8; 32] → hex "000...000". This caused:
    ///   - INSERT OR REPLACE in transaction_history collapsed all notes to 1 row
    ///   - INSERT OR IGNORE on UNIQUE(cmu) prevents fixing on re-scan
    ///
    /// This method atomically:
    ///   1. Deletes notes with all-zero received_txid
    ///   2. Deletes the corresponding transaction_history entry
    ///   3. Resets tree state + height to 0 (forces catch-up re-scan)
    ///
    /// Returns the number of notes cleaned up (0 = no migration needed).
    pub fn fix_zero_txid_notes(&self) -> Result<usize, StorageError> {
        let zero_txid = "0000000000000000000000000000000000000000000000000000000000000000";
        let mut conn = recover_lock(self.conn.lock());

        // Check if any notes have the zero txid
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE received_txid = ?1",
            params![zero_txid],
            |row| row.get(0),
        )?;

        if count == 0 {
            return Ok(0);
        }

        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM notes WHERE received_txid = ?1",
            params![zero_txid],
        )?;
        tx.execute(
            "DELETE FROM transaction_history WHERE txid = ?1",
            params![zero_txid],
        )?;
        tx.execute(
            "UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1",
            [],
        )?;
        tx.commit()?;

        Ok(count)
    }

    /// Atomically clear all notes, transaction history, transparent UTXOs,
    /// and tree state.
    ///
    /// Used before boost scan to start fresh with correct data.
    /// Also resets last_scanned_height and delta_bundle_verified so the
    /// full block scan re-processes the entire post-boost range (detecting
    /// spends that were previously missed).
    pub fn clear_notes_and_history(&self) -> Result<(), StorageError> {
        let mut conn = recover_lock(self.conn.lock());
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM notes", [])?;
        tx.execute("DELETE FROM transaction_history", [])?;
        tx.execute("DELETE FROM transparent_utxos", [])?;
        tx.execute(
            "UPDATE sync_state SET tree_state = NULL, tree_height = 0, \
             last_scanned_height = 0, delta_bundle_verified = 0 WHERE id = 1",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Nuclear reset for full rescan: delete ALL data and reset ALL sync state.
    ///
    /// This is the only reliable way to fix corrupted witnesses/anchors.
    /// Matches the official ZipherX full rescan approach:
    /// 1. Delete all notes (forces boost scan to re-insert with correct positions)
    /// 2. Delete all transaction history
    /// 3. Reset tree state + height
    /// 4. Reset last_scanned_height to 0 (triggers fresh scan from boost)
    /// 5. Reset delta_bundle_verified (forces fresh delta validation)
    pub fn full_rescan_reset(&self) -> Result<(), StorageError> {
        let mut conn = recover_lock(self.conn.lock());
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM notes", [])?;
        tx.execute("DELETE FROM transaction_history", [])?;
        // Also clear transparent UTXOs — they will be re-discovered during scan.
        // Without this, UTXOs incorrectly marked as spent (e.g., from a failed
        // transparent send) remain stuck.
        tx.execute("DELETE FROM transparent_utxos", [])?;
        tx.execute(
            "UPDATE sync_state SET tree_state = NULL, tree_height = 0, \
             last_scanned_height = 0, delta_bundle_verified = 0 WHERE id = 1",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Restore transparent UTXOs that were incorrectly marked spent.
    /// Only restores if the spending TX was never mined AND is older than
    /// `expiry_secs` (to avoid undoing valid pending sends).
    /// Returns count of restored UTXOs.
    pub fn restore_stuck_transparent_utxos(&self, expiry_secs: i64) -> Result<usize, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Find transparent UTXOs marked spent where the spending TX:
        // 1. Was never confirmed (height = 0 or missing from history)
        // 2. Is old enough to be considered expired (timestamp check)
        // 3. Was NOT detected during block scanning (spent_height = 0)
        //    — if spent_height > 0, the spend was found on-chain and is real.
        let restored = conn.execute(
            "UPDATE transparent_utxos SET is_spent = 0, spent_in_tx = NULL, spent_height = NULL
             WHERE is_spent = 1
             AND spent_in_tx IS NOT NULL
             AND (spent_height IS NULL OR spent_height = 0)
             AND NOT EXISTS (
                 SELECT 1 FROM transaction_history
                 WHERE txid = transparent_utxos.spent_in_tx AND height > 0
             )
             AND (
                 -- TX has a timestamp and it's expired
                 EXISTS (
                     SELECT 1 FROM transaction_history
                     WHERE txid = transparent_utxos.spent_in_tx
                     AND timestamp IS NOT NULL AND timestamp > 0
                     AND (?1 - timestamp) > ?2
                 )
                 -- OR TX has no history entry at all (orphaned spend mark)
                 OR NOT EXISTS (
                     SELECT 1 FROM transaction_history
                     WHERE txid = transparent_utxos.spent_in_tx
                 )
             )",
            params![now, expiry_secs],
        )?;
        if restored > 0 {
            eprintln!("[ZipherX] Restored {} stuck transparent UTXO(s)", restored);
        }
        Ok(restored)
    }

    /// Get delta bundle verified flag.
    pub fn get_delta_bundle_verified(&self) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let val: i64 = conn.query_row(
            "SELECT delta_bundle_verified FROM sync_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(val != 0)
    }

    /// Set delta bundle verified flag.
    pub fn set_delta_bundle_verified(&self, verified: bool) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET delta_bundle_verified = ?1 WHERE id = 1",
            params![verified as i64],
        )?;
        Ok(())
    }

    /// Get verified checkpoint height.
    pub fn get_verified_checkpoint_height(&self) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let h: i64 = conn.query_row(
            "SELECT verified_checkpoint_height FROM sync_state WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(h as u64)
    }

    /// Update verified checkpoint height.
    pub fn update_verified_checkpoint_height(&self, height: u64) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "UPDATE sync_state SET verified_checkpoint_height = ?1 WHERE id = 1",
            params![height as i64],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Generic transaction helper
    // ---------------------------------------------------------------

    /// Execute a closure inside a database transaction.
    pub fn execute_in_transaction<T, F>(&self, f: F) -> Result<T, StorageError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T, StorageError>,
    {
        let mut conn = recover_lock(self.conn.lock());
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Migrate transaction_history from UNIQUE(txid) to UNIQUE(txid, tx_type).
    ///
    /// The same txid can legitimately appear as both "received" (change output)
    /// and "sent" (spending notes). The old UNIQUE(txid) caused INSERT OR REPLACE
    /// to overwrite one type with the other, losing transaction history.
    fn migrate_tx_history_unique_constraint(
        conn: &rusqlite::Connection,
    ) -> Result<(), StorageError> {
        // Check the CREATE TABLE SQL in sqlite_master
        let create_sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='transaction_history'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| StorageError::SchemaFailed(format!("Check tx_history schema: {e}")))?;

        let Some(sql) = create_sql else {
            return Ok(()); // Table doesn't exist yet — will be created by schema
        };

        // Already migrated if UNIQUE(txid, tx_type) is present
        if sql.contains("txid, tx_type") {
            return Ok(());
        }

        // Old schema has UNIQUE(txid) — need to recreate table
        eprintln!("[ZipherX] Migrating transaction_history: UNIQUE(txid) → UNIQUE(txid, tx_type)");

        conn.execute_batch(
            "BEGIN TRANSACTION;
             CREATE TABLE transaction_history_new (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 txid TEXT NOT NULL,
                 tx_type TEXT NOT NULL,
                 amount INTEGER NOT NULL,
                 fee INTEGER NOT NULL DEFAULT 10000,
                 address TEXT,
                 memo TEXT,
                 confirmations INTEGER NOT NULL DEFAULT 0,
                 timestamp INTEGER,
                 status TEXT NOT NULL DEFAULT 'pending',
                 height INTEGER NOT NULL DEFAULT 0,
                 raw_tx BLOB,
                 UNIQUE(txid, tx_type)
             );
             INSERT OR IGNORE INTO transaction_history_new
                 SELECT * FROM transaction_history;
             DROP TABLE transaction_history;
             ALTER TABLE transaction_history_new RENAME TO transaction_history;
             CREATE INDEX IF NOT EXISTS idx_tx_history_txid ON transaction_history(txid);
             CREATE INDEX IF NOT EXISTS idx_tx_history_height ON transaction_history(height);
             CREATE INDEX IF NOT EXISTS idx_tx_history_status ON transaction_history(status);
             COMMIT;",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Migrate tx_history: {e}")))?;

        eprintln!("[ZipherX] Migration complete: transaction_history now supports same txid as both sent/received");
        Ok(())
    }

    /// One-time migration: force clean re-scan of boost data.
    ///
    /// The initial boost scan ran under the old UNIQUE(txid) + INSERT OR REPLACE
    /// schema, which corrupted transaction history (sent entries overwrote received
    /// entries for the same txid). Although the schema is now fixed (UNIQUE(txid, tx_type)
    /// + INSERT OR IGNORE), the DATA is still from the corrupted run.
    ///
    /// This migration clears all notes and history and resets tree_height to 0,
    /// forcing `boost_scan_if_needed()` to re-run with the corrected schema.
    /// Uses a `_migrations` table to ensure it only runs once.
    fn migrate_data_rescan_v2(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        // Create migration tracker table if it doesn't exist
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        // Check if this migration already ran
        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v2'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        // Check if data exists that needs rescan
        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v2: clearing stale data (tree_height={}), \
                 boost scan will re-run with corrected schema",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v2');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v2: {e}")))?;

            eprintln!("[ZipherX] Migration data_rescan_v2: data cleared, tree_height reset to 0");
        } else {
            // No data yet — just mark migration as applied
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v2')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v3: Force boost re-scan with per-TX aggregated history.
    /// Previous boost scan inserted per-NOTE tx_history entries (showing confusing
    /// change amounts). The new logic aggregates by txid, detects change outputs,
    /// and shows clean net sent/received amounts.
    fn migrate_data_rescan_v3(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        // _migrations table already exists from v2
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v3'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v3: clearing data for aggregated TX history \
                 (tree_height={}), boost scan will re-run",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v3');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v3: {e}")))?;

            eprintln!("[ZipherX] Migration data_rescan_v3: data cleared, boost will re-run with aggregated history");
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v3')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v4: Force full re-download of delta store (v2 format with txids).
    /// Sets a flag that the sync pipeline reads to clear the delta store before syncing.
    /// Also clears notes/history/tree to force boost re-scan.
    fn migrate_full_redownload_v4(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'full_redownload_v4'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        eprintln!(
            "[ZipherX] Migration full_redownload_v4: clearing all data, delta store will be \
             re-downloaded with v2 format (txids)"
        );
        conn.execute_batch(
            "BEGIN TRANSACTION;
             DELETE FROM notes;
             DELETE FROM transaction_history;
             UPDATE sync_state SET tree_state = NULL, tree_height = 0, last_scanned_height = 0 WHERE id = 1;
             INSERT OR REPLACE INTO _migrations (name, applied_at) VALUES ('full_redownload_v4', 1);
             COMMIT;",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("full_redownload_v4: {e}")))?;

        eprintln!(
            "[ZipherX] Migration full_redownload_v4: data cleared, ready for full re-download"
        );
        Ok(())
    }

    /// Migration v5: Force boost re-scan with post-boost nullifier cross-reference.
    /// The cross-reference detects boost-era notes spent after boost height, fixing the
    /// balance gap. No delta re-download needed — delta store is already v2 format.
    fn migrate_data_rescan_v5(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v5'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v5: clearing data for nullifier cross-reference \
                 (tree_height={}), boost scan will re-run",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v5');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v5: {e}")))?;

            eprintln!("[ZipherX] Migration data_rescan_v5: data cleared, boost + cross-ref will run on next sync");
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v5')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v6: Re-scan with CMU mismatch diagnostics and full delta nullifier cross-ref.
    fn migrate_data_rescan_v6(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v6'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v6: clearing data for CMU diagnostics + full cross-ref \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v6');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v6: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v6')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v7: Force boost rescan to trigger the new post-boost spend scan.
    ///
    /// The previous sync only downloaded blocks with new outputs (where finalsaplingroot
    /// changed). Blocks with ONLY sapling spends were missed, causing phantom unspent
    /// notes. This migration clears notes/history so the boost scan re-runs, followed
    /// by the new post_boost_spend_scan() that downloads ALL blocks in the post-boost
    /// range to find nullifiers from spend-only blocks.
    fn migrate_data_rescan_v7(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v7'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v7: clearing data for post-boost spend scan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v7');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v7: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v7')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v8: Force boost rescan with position probe diagnostic.
    ///
    /// The boost scan computes nullifiers using `position = i` (0-indexed output index).
    /// 31 notes appear "unspent" but should be spent on-chain. This migration forces
    /// a full rescan so the position probe diagnostic in boost_scan.rs fires,
    /// logging whether a nearby position (±3/±100/±10000) produces the correct nullifier.
    fn migrate_data_rescan_v8(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v8'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v8: clearing data for position probe diagnostic \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v8');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v8: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v8')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v9: Force boost rescan + full post-boost block download.
    ///
    /// The previous spend scan only downloaded "spend-only" blocks (5,290 of 5,591),
    /// missing blocks that had BOTH outputs and spends. The new `post_boost_full_block_scan`
    /// downloads ALL blocks in the post-boost range and trial-decrypts their outputs,
    /// finding received notes the delta store never captured (~569,998 zatoshis missing).
    fn migrate_data_rescan_v9(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v9'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v9: clearing data for full post-boost block scan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v9');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v9: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v9')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v10: Force re-run with peer reconnection fix.
    ///
    /// v9's full block scan got 0 blocks because peers disconnected during the
    /// 30+ second boost scan (loading 750MB, parallel decryption). The code now
    /// checks peer connectivity and restarts block listeners before the download
    /// loop begins.
    fn migrate_data_rescan_v10(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v10'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v10: clearing data for peer-recovery full block scan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v10');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v10: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v10')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v11: Force re-run with full reconnect fix.
    ///
    /// v10's stop/start_all_block_listeners silently failed because
    /// `start_block_listener` does `reader.take()` — once consumed by the
    /// listener task, the reader is gone and can never be restored. Now uses
    /// `peer_manager.connect()` to create entirely fresh peer objects with
    /// fresh TCP connections and readers.
    fn migrate_data_rescan_v11(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v11'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v11: clearing data for reconnect-based full block scan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v11');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v11: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v11')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    fn migrate_data_rescan_v12(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v12'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v12: clearing for diagnostic boost scan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v12');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v12: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v12')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    fn migrate_data_rescan_v13(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v13'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v13: ZIP-212 version byte fix — clearing for rescan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v13');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v13: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v13')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    fn migrate_data_rescan_v14(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v14'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v14: balance diagnostic rescan \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v14');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v14: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v14')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    fn migrate_data_rescan_v15(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v15'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v15: tree root validation + compact fallback \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v15');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v15: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v15')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Migration v16: Full rescan with last_scanned_height reset.
    ///
    /// ROOT CAUSE FIX: Previous migrations (v2-v15) reset tree_height = 0 to
    /// trigger boost rescan, but left last_scanned_height at the old chain tip.
    /// After boost scan re-inserted notes, post_boost_full_block_scan() checked
    /// `last_scanned >= chain_tip` and SKIPPED — post-boost spends were never
    /// re-detected, inflating the balance.
    ///
    /// This migration resets last_scanned_height = 0 AND delta_bundle_verified = 0
    /// so the full block scan processes the entire post-boost range.
    /// Also clears transparent_utxos to prevent stale data.
    fn migrate_data_rescan_v16(conn: &rusqlite::Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL DEFAULT 0
            )",
        )
        .map_err(|e| StorageError::SchemaFailed(format!("Create _migrations: {e}")))?;

        let already_applied: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM _migrations WHERE name = 'data_rescan_v16'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if already_applied {
            return Ok(());
        }

        let tree_height: i64 = conn
            .query_row(
                "SELECT tree_height FROM sync_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if tree_height > 0 {
            eprintln!(
                "[ZipherX] Migration data_rescan_v16: FULL rescan with last_scanned_height reset \
                 (tree_height={})",
                tree_height,
            );
            conn.execute_batch(
                "BEGIN TRANSACTION;
                 DELETE FROM notes;
                 DELETE FROM transaction_history;
                 DELETE FROM transparent_utxos;
                 UPDATE sync_state SET tree_state = NULL, tree_height = 0, \
                    last_scanned_height = 0, delta_bundle_verified = 0 WHERE id = 1;
                 INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v16');
                 COMMIT;",
            )
            .map_err(|e| StorageError::SchemaFailed(format!("data_rescan_v16: {e}")))?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO _migrations (name) VALUES ('data_rescan_v16')",
                [],
            )
            .map_err(|e| StorageError::SchemaFailed(e.to_string()))?;
        }

        Ok(())
    }

    // ====================================================================
    // Transparent UTXO operations
    // ====================================================================

    /// Insert a transparent UTXO discovered during block scanning.
    pub fn insert_transparent_utxo(
        &self,
        height: u64,
        txid: &str,
        output_index: u32,
        script_pubkey: &[u8],
        address: &str,
        value: u64,
        is_change: bool,
        child_index: u32,
        is_imported: bool,
    ) -> Result<i64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "INSERT OR IGNORE INTO transparent_utxos
                (account_id, height, txid, output_index, script_pubkey, address, value, is_change, child_index, is_imported)
             VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                height as i64,
                txid,
                output_index,
                script_pubkey,
                address,
                value as i64,
                is_change as i32,
                child_index,
                is_imported as i32,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all unspent transparent UTXOs.
    pub fn get_unspent_transparent_utxos(&self) -> Result<Vec<TransparentUtxo>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT id, height, txid, output_index, script_pubkey, address, value, is_change, child_index, is_imported
             FROM transparent_utxos WHERE is_spent = 0 ORDER BY height",
        )?;
        let utxos = stmt
            .query_map([], |row| {
                Ok(TransparentUtxo {
                    id: row.get(0)?,
                    height: row.get::<_, i64>(1)? as u64,
                    txid: row.get(2)?,
                    output_index: row.get::<_, i64>(3)? as u32,
                    script_pubkey: row.get(4)?,
                    address: row.get(5)?,
                    value: row.get::<_, i64>(6)? as u64,
                    is_change: row.get::<_, i32>(7)? != 0,
                    child_index: row.get::<_, i64>(8)? as u32,
                    is_imported: row.get::<_, i32>(9)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(utxos)
    }

    /// Get the total transparent balance (sum of unspent UTXOs).
    pub fn get_transparent_balance(&self) -> Result<u64, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let balance: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(value), 0) FROM transparent_utxos WHERE is_spent = 0",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(balance.max(0) as u64)
    }

    /// Mark a transparent UTXO as spent.
    pub fn mark_transparent_utxo_spent(
        &self,
        txid: &str,
        output_index: u32,
        spent_in_tx: &str,
        spent_height: u64,
    ) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());

        // DEBUG: Check if this prevout matches any of our UTXOs
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM transparent_utxos WHERE txid = ?1 AND output_index = ?2",
                params![txid, output_index],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) > 0;
        if exists {
            eprintln!(
                "[ZipherX] SPEND MATCH: prevout {}..vout={} FOUND in DB (spending_tx={}.. height={})",
                &txid[..16.min(txid.len())], output_index,
                &spent_in_tx[..16.min(spent_in_tx.len())], spent_height,
            );
        }

        let updated = conn.execute(
            "UPDATE transparent_utxos SET is_spent = 1, spent_in_tx = ?1, spent_height = ?2
             WHERE txid = ?3 AND output_index = ?4 AND is_spent = 0",
            params![spent_in_tx, spent_height as i64, txid, output_index],
        )?;

        // FIX: If UTXO was already marked spent (from send flow) with
        // spent_height = 0, update the height now that we found it on-chain.
        if updated == 0 && spent_height > 0 {
            conn.execute(
                "UPDATE transparent_utxos SET spent_height = ?1
                 WHERE txid = ?2 AND output_index = ?3 AND is_spent = 1
                 AND (spent_height IS NULL OR spent_height = 0)",
                params![spent_height as i64, txid, output_index],
            )?;
        }

        Ok(updated > 0)
    }

    /// Debug: dump all transparent UTXOs for diagnostics.
    pub fn dump_transparent_utxos(&self) -> Result<String, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT txid, output_index, value, is_spent, height FROM transparent_utxos ORDER BY height",
        )?;
        let mut output = String::new();
        let mut count = 0u32;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            if let Ok((txid, vout, value, spent, height)) = row {
                output.push_str(&format!(
                    "[ZipherX]   utxo: {}..vout={} value={} spent={} height={}\n",
                    &txid[..16.min(txid.len())], vout, value, spent, height,
                ));
                count += 1;
            }
        }
        let header = format!("[ZipherX] DIAG: {} total transparent UTXOs:\n", count);
        Ok(format!("{}{}", header, output))
    }

    /// Mark a transparent UTXO as spent by matching prevout (txid + index).
    /// Used during block scanning when we see a vin referencing our UTXO.
    pub fn mark_transparent_spent_by_prevout(
        &self,
        prevout_txid: &str,
        prevout_index: u32,
        spending_txid: &str,
        height: u64,
    ) -> Result<bool, StorageError> {
        self.mark_transparent_utxo_spent(prevout_txid, prevout_index, spending_txid, height)
    }

    /// Backfill transaction_history from existing transparent_utxos that have no
    /// matching history entry. This handles the case where UTXOs were stored before
    /// history recording was added (upgrade path).
    pub fn backfill_transparent_history(&self) -> Result<u32, StorageError> {
        let conn = recover_lock(self.conn.lock());
        // Aggregate by txid to handle multi-output TXs (same txid, multiple UTXOs).
        // SUM(value) gives the total received amount per txid.
        let count = conn.execute(
            "INSERT OR IGNORE INTO transaction_history
                (txid, height, timestamp, tx_type, amount, fee, address, memo, status)
             SELECT u.txid, MAX(u.height), NULL, 'received', SUM(u.value), 0, MIN(u.address), NULL, 'confirmed'
             FROM transparent_utxos u
             WHERE u.is_change = 0
               AND NOT EXISTS (
                   SELECT 1 FROM transaction_history h
                   WHERE h.txid = u.txid AND h.tx_type = 'received'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM transaction_history h
                   WHERE h.txid = u.txid AND h.tx_type = 'sent'
               )
             GROUP BY u.txid",
            [],
        )?;
        if count > 0 {
            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] Transparent history backfill: {} entries added",
                count
            );
        }
        Ok(count as u32)
    }

    /// Get the value of a transparent UTXO by txid and output index.
    pub fn get_transparent_utxo_value(
        &self,
        txid: &str,
        output_index: u32,
    ) -> Result<Option<u64>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT value FROM transparent_utxos WHERE txid = ?1 AND output_index = ?2",
        )?;
        let val = stmt
            .query_row(params![txid, output_index], |row| {
                row.get::<_, i64>(0).map(|v| v as u64)
            })
            .optional()?;
        Ok(val)
    }

    /// Delete all transparent UTXOs (for rescan).
    pub fn delete_all_transparent_utxos(&self) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute("DELETE FROM transparent_utxos", [])?;
        Ok(())
    }

    /// Get all transparent addresses that have unspent funds, grouped by address.
    /// Returns addresses sorted by balance descending, for use in WIF export
    /// (only addresses with funds need their private keys exported).
    pub fn get_funded_transparent_addresses(&self) -> Result<Vec<FundedAddress>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT address, SUM(value) as total, is_change, child_index, is_imported
             FROM transparent_utxos
             WHERE is_spent = 0
             GROUP BY address
             HAVING total > 0
             ORDER BY total DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FundedAddress {
                address: row.get(0)?,
                balance: row.get::<_, i64>(1)? as u64,
                is_change: row.get::<_, i32>(2)? != 0,
                child_index: row.get::<_, i64>(3)? as u32,
                is_imported: row.get::<_, i32>(4)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// I2: Get the next available transparent change child_index.
    /// Returns MAX(child_index) + 1 among change UTXOs, or 0 if none exist.
    /// Used for change address rotation to avoid address reuse.
    pub fn next_transparent_change_index(&self) -> Result<u32, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let next: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(child_index), -1) + 1 FROM transparent_utxos WHERE is_change = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(next as u32)
    }

    /// Check if the delta store should be cleared (set by migration v4).
    /// Returns true if the flag is set, then clears it.
    pub fn check_and_clear_redownload_flag(&self) -> Result<bool, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let flag: i64 = conn
            .query_row(
                "SELECT applied_at FROM _migrations WHERE name = 'full_redownload_v4'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if flag == 1 {
            // Clear the flag so it only fires once
            conn.execute(
                "UPDATE _migrations SET applied_at = 0 WHERE name = 'full_redownload_v4'",
                [],
            )?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    // ====================================================================
    // Imported transparent key operations (WIF import)
    // ====================================================================

    /// Store an imported transparent private key (encrypted).
    pub fn store_imported_transparent_key(
        &self,
        address: &str,
        encrypted_sk: &[u8],
    ) -> Result<(), StorageError> {
        let conn = recover_lock(self.conn.lock());
        conn.execute(
            "INSERT OR REPLACE INTO imported_transparent_keys (address, encrypted_secret_key)
             VALUES (?1, ?2)",
            params![address, encrypted_sk],
        )?;
        Ok(())
    }

    /// Get all imported transparent addresses with their database IDs.
    pub fn get_imported_transparent_addresses(&self) -> Result<Vec<(i64, String)>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let mut stmt = conn.prepare(
            "SELECT id, address FROM imported_transparent_keys ORDER BY imported_at",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Get the encrypted secret key for an imported transparent address.
    pub fn get_imported_transparent_secret(
        &self,
        address: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let result = conn
            .query_row(
                "SELECT encrypted_secret_key FROM imported_transparent_keys WHERE address = ?1",
                params![address],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Get the count of imported transparent keys.
    pub fn get_imported_key_count(&self) -> Result<u32, StorageError> {
        let conn = recover_lock(self.conn.lock());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM imported_transparent_keys",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count as u32)
    }
}

/// SHA-256 hash a nullifier for privacy storage (VUL-009).
pub fn hash_nullifier(nullifier: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    // RS-6: Domain separator prevents cross-protocol hash collisions.
    // If the same SHA-256 hash is used elsewhere (e.g., txid hashing),
    // the domain separator ensures nullifier hashes are distinct.
    hasher.update(b"ZipherX_nullifier_v1");
    hasher.update(nullifier);
    hasher.finalize().to_vec()
}

/// Convert a rusqlite Row to a Note.
fn row_to_note(row: &rusqlite::Row<'_>) -> Result<Note, rusqlite::Error> {
    Ok(Note {
        id: row.get("id")?,
        account_id: row.get("account_id")?,
        height: row.get::<_, i64>("height")? as u64,
        cmu: row.get("cmu")?,
        epk: row.get("epk")?,
        ciphertext: row.get("ciphertext")?,
        value: row.get::<_, i64>("value")? as u64,
        rcm: row.get("rcm")?,
        nullifier: row.get("nullifier")?,
        witness: row.get("witness")?,
        anchor: row.get("anchor")?,
        is_spent: row.get::<_, i64>("is_spent")? != 0,
        spent_in_tx: row.get("spent_in_tx")?,
        spent_height: row.get::<_, Option<i64>>("spent_height")?.map(|h| h as u64),
        memo: row.get("memo")?,
        diversifier: row.get("diversifier")?,
        received_txid: row.get("received_txid")?,
        position: row.get::<_, Option<i64>>("position")?.map(|p| p as u64),
    })
}

/// Convert a rusqlite Row to a TransactionRecord.
fn row_to_tx_record(row: &rusqlite::Row<'_>) -> Result<TransactionRecord, rusqlite::Error> {
    let tx_type_str: String = row.get("tx_type")?;
    let status_str: String = row.get("status")?;
    Ok(TransactionRecord {
        id: row.get("id")?,
        txid: row.get("txid")?,
        tx_type: TxType::from_str(&tx_type_str).unwrap_or(TxType::Received),
        amount: row.get::<_, i64>("amount")? as u64,
        fee: row.get::<_, i64>("fee")? as u64,
        address: row.get("address")?,
        memo: row.get("memo")?,
        confirmations: row.get::<_, i64>("confirmations")? as u32,
        timestamp: row.get::<_, Option<i64>>("timestamp")?.map(|t| t as u64),
        status: TxStatus::from_str(&status_str).unwrap_or(TxStatus::Pending),
        height: row.get::<_, i64>("height")? as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> WalletDatabase {
        WalletDatabase::open_in_memory().unwrap()
    }

    #[test]
    fn test_open_in_memory() {
        let db = test_db();
        let state = db.get_sync_state().unwrap();
        assert_eq!(state.last_scanned_height, 0);
    }

    #[test]
    fn test_insert_note() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let nf = [0xBBu8; 32];
        let id = db
            .insert_note(
                0,
                100,
                &cmu,
                50000,
                Some(&nf),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(id > 0);

        let note = db.get_note_by_id(id).unwrap().unwrap();
        assert_eq!(note.value, 50000);
        assert_eq!(note.height, 100);
        assert!(!note.is_spent);
    }

    #[test]
    fn test_insert_note_duplicate_cmu_ignored() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        db.insert_note(
            0, 100, &cmu, 50000, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();
        // Same CMU should be ignored (INSERT OR IGNORE)
        db.insert_note(
            0, 100, &cmu, 99999, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();
        assert_eq!(db.count_unspent_notes(0).unwrap(), 1);
    }

    #[test]
    fn test_nullifier_hashing_vul009() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let raw_nf = [0xBBu8; 32];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            Some(&raw_nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        // Stored nullifier should be SHA-256 hash, not raw
        let note = db.get_note_by_nullifier(&raw_nf).unwrap().unwrap();
        let expected_hash = hash_nullifier(&raw_nf);
        assert_eq!(note.nullifier.unwrap(), expected_hash);
    }

    #[test]
    fn test_mark_note_spent() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let nf = [0xBBu8; 32];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            Some(&nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let marked = db.mark_note_spent(&nf, "tx123", 200).unwrap();
        assert!(marked);

        let note = db.get_note_by_nullifier(&nf).unwrap().unwrap();
        assert!(note.is_spent);
        assert_eq!(note.spent_in_tx, Some("tx123".into()));
        assert_eq!(note.spent_height, Some(200));
    }

    #[test]
    fn test_restore_notes_phantom_tx() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let nf = [0xBBu8; 32];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            Some(&nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db.mark_note_spent(&nf, "phantom_tx", 200).unwrap();

        let (count, value) = db.restore_notes_spent_by_phantom_tx("phantom_tx").unwrap();
        assert_eq!(count, 1);
        assert_eq!(value, 50000);

        let note = db.get_note_by_nullifier(&nf).unwrap().unwrap();
        assert!(!note.is_spent);
        assert!(note.spent_in_tx.is_none());
    }

    #[test]
    fn test_get_balance_requires_witness() {
        let db = test_db();
        // Note without witness — not spendable
        let cmu1 = [0xAAu8; 32];
        db.insert_note(
            0, 100, &cmu1, 50000, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();

        assert_eq!(db.get_balance(0).unwrap(), 0);

        // Note with valid witness (>= 100 bytes)
        let cmu2 = [0xBBu8; 32];
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            100,
            &cmu2,
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();

        assert_eq!(db.get_balance(0).unwrap(), 30000);
    }

    #[test]
    fn test_get_total_unspent_balance() {
        let db = test_db();
        // Note without witness — still counted in total
        let cmu1 = [0xAAu8; 32];
        db.insert_note(
            0, 100, &cmu1, 50000, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();
        // Note with witness
        let cmu2 = [0xBBu8; 32];
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            100,
            &cmu2,
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();

        assert_eq!(db.get_total_unspent_balance(0).unwrap(), 80000);
    }

    #[test]
    fn test_total_balance_includes_orphan_spent() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let nf = [0xBBu8; 32];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            Some(&nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        // Simulate orphan: is_spent=1 but spent_in_tx is empty
        {
            let conn = recover_lock(db.conn.lock());
            conn.execute(
                "UPDATE notes SET is_spent = 1, spent_in_tx = '' WHERE cmu = ?1",
                params![cmu.as_slice()],
            )
            .unwrap();
        }

        // Total should include this orphan
        assert_eq!(db.get_total_unspent_balance(0).unwrap(), 50000);
        // Spendable should not (no witness anyway)
        assert_eq!(db.get_balance(0).unwrap(), 0);
    }

    #[test]
    fn test_notes_without_witnesses() {
        let db = test_db();
        let cmu1 = [0xAAu8; 32];
        let cmu2 = [0xBBu8; 32];
        db.insert_note(
            0, 100, &cmu1, 50000, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            200,
            &cmu2,
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();

        let (count, value, min_h) = db.get_notes_without_witnesses(0).unwrap();
        assert_eq!(count, 1);
        assert_eq!(value, 50000);
        assert_eq!(min_h, 100);
    }

    #[test]
    fn test_insert_transaction() {
        let db = test_db();
        db.insert_transaction(
            "tx_abc",
            100,
            Some(1000),
            TxType::Sent,
            50000,
            10000,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        let tx = db.get_transaction_by_txid("tx_abc").unwrap().unwrap();
        assert_eq!(tx.amount, 50000);
        assert_eq!(tx.fee, 10000);
        assert_eq!(tx.tx_type, TxType::Sent);
        assert_eq!(tx.status, TxStatus::Confirmed);
        assert_eq!(tx.height, 100);
    }

    #[test]
    fn test_transaction_history_pagination() {
        let db = test_db();
        for i in 0..10 {
            db.insert_transaction(
                &format!("tx_{i}"),
                i * 10,
                None,
                TxType::Received,
                1000 * (i + 1),
                10000,
                None,
                None,
                TxStatus::Confirmed,
            )
            .unwrap();
        }

        let page1 = db.get_transaction_history(3, 0).unwrap();
        assert_eq!(page1.len(), 3);
        assert!(page1[0].height >= page1[1].height); // DESC order

        let page2 = db.get_transaction_history(3, 3).unwrap();
        assert_eq!(page2.len(), 3);
    }

    #[test]
    fn test_pending_transactions() {
        let db = test_db();
        db.insert_transaction(
            "tx_a",
            0,
            None,
            TxType::Sent,
            50000,
            10000,
            None,
            None,
            TxStatus::Pending,
        )
        .unwrap();
        db.insert_transaction(
            "tx_b",
            100,
            None,
            TxType::Received,
            30000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        let pending = db.get_pending_transactions().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].txid, "tx_a");
    }

    #[test]
    fn test_update_transaction_status() {
        let db = test_db();
        db.insert_transaction(
            "tx_a",
            0,
            None,
            TxType::Sent,
            50000,
            10000,
            None,
            None,
            TxStatus::Pending,
        )
        .unwrap();

        let updated = db
            .update_transaction_status("tx_a", TxStatus::Confirmed, Some(500), Some(12345))
            .unwrap();
        assert!(updated);

        let tx = db.get_transaction_by_txid("tx_a").unwrap().unwrap();
        assert_eq!(tx.status, TxStatus::Confirmed);
        assert_eq!(tx.height, 500);
        assert_eq!(tx.timestamp, Some(12345));
    }

    #[test]
    fn test_delete_phantom_transaction() {
        let db = test_db();
        db.insert_transaction(
            "phantom_tx",
            100,
            None,
            TxType::Sent,
            99000,
            10000,
            None,
            None,
            TxStatus::Phantom,
        )
        .unwrap();

        let amount = db.delete_phantom_transaction("phantom_tx").unwrap();
        assert_eq!(amount, Some(99000));
        assert!(db.get_transaction_by_txid("phantom_tx").unwrap().is_none());
    }

    #[test]
    fn test_update_all_confirmations() {
        let db = test_db();
        db.insert_transaction(
            "tx_a",
            100,
            None,
            TxType::Received,
            1000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.insert_transaction(
            "tx_b",
            200,
            None,
            TxType::Received,
            2000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        db.update_all_confirmations(300).unwrap();

        let a = db.get_transaction_by_txid("tx_a").unwrap().unwrap();
        let b = db.get_transaction_by_txid("tx_b").unwrap().unwrap();
        assert_eq!(a.confirmations, 201); // 300 - 100 + 1
        assert_eq!(b.confirmations, 101); // 300 - 200 + 1
    }

    #[test]
    fn test_record_sent_transaction_atomic() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let raw_nf = [0xBBu8; 32];
        // insert_note hashes the raw nullifier internally
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            Some(&raw_nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        // record_sent_transaction_atomic now accepts the RAW nullifier
        // and hashes it internally before comparing against the DB
        let history_id = db
            .record_sent_transaction_atomic(&raw_nf, "atomic_tx", 200, 50000, 10000, None)
            .unwrap();
        assert!(history_id > 0);

        // Note should be spent
        let note = db.get_note_by_nullifier(&raw_nf).unwrap().unwrap();
        assert!(note.is_spent);
        assert_eq!(note.spent_in_tx, Some("atomic_tx".into()));

        // TX should exist
        let tx = db.get_transaction_by_txid("atomic_tx").unwrap().unwrap();
        assert_eq!(tx.amount, 50000);
        assert_eq!(tx.status, TxStatus::Confirmed);
    }

    #[test]
    fn test_sync_state_roundtrip() {
        let db = test_db();
        db.update_last_scanned_height(12345).unwrap();
        let state = db.get_sync_state().unwrap();
        assert_eq!(state.last_scanned_height, 12345);
    }

    #[test]
    fn test_tree_state_roundtrip() {
        let db = test_db();
        let tree_data = vec![0x01, 0x02, 0x03, 0x04];
        db.save_tree_state(&tree_data, 500).unwrap();

        assert_eq!(db.get_tree_state().unwrap(), Some(tree_data));
        assert_eq!(db.get_tree_height().unwrap(), 500);
    }

    #[test]
    fn test_clear_tree_state_only() {
        let db = test_db();
        // Insert note with witness
        let cmu = [0xAAu8; 32];
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();
        db.save_tree_state(&[0x01], 100).unwrap();

        db.clear_tree_state_only().unwrap();

        // Tree state should be cleared
        assert!(db.get_tree_state().unwrap().is_none());
        assert_eq!(db.get_tree_height().unwrap(), 0);
        // Witnesses should be preserved (FIX #1210)
        let notes = db.get_all_unspent_notes(0).unwrap();
        assert!(notes[0].witness.is_some());
    }

    #[test]
    fn test_clear_tree_state_for_rebuild() {
        let db = test_db();
        let cmu = [0xAAu8; 32];
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            100,
            &cmu,
            50000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();
        db.save_tree_state(&[0x01], 100).unwrap();

        db.clear_tree_state_for_rebuild().unwrap();

        assert!(db.get_tree_state().unwrap().is_none());
        // Witnesses should also be cleared
        let notes = db.get_all_unspent_notes(0).unwrap();
        assert!(notes[0].witness.is_none());
    }

    #[test]
    fn test_delta_bundle_verified_flag() {
        let db = test_db();
        assert!(!db.get_delta_bundle_verified().unwrap());
        db.set_delta_bundle_verified(true).unwrap();
        assert!(db.get_delta_bundle_verified().unwrap());
        db.set_delta_bundle_verified(false).unwrap();
        assert!(!db.get_delta_bundle_verified().unwrap());
    }

    #[test]
    fn test_execute_in_transaction() {
        let db = test_db();
        let result = db.execute_in_transaction(|tx| {
            tx.execute(
                "INSERT INTO notes (account_id, height, cmu, value) VALUES (0, 1, ?1, 100)",
                params![[0xAAu8; 32].as_slice()],
            )?;
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(db.count_unspent_notes(0).unwrap(), 1);
    }

    #[test]
    fn test_get_all_unspent_notes() {
        let db = test_db();
        let cmu1 = [0xAAu8; 32];
        let cmu2 = [0xBBu8; 32];
        let nf = [0xCCu8; 32];
        db.insert_note(
            0,
            100,
            &cmu1,
            50000,
            Some(&nf),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        db.insert_note(
            0, 200, &cmu2, 30000, None, None, None, None, None, None, None, None, None,
        )
        .unwrap();

        // Mark first as spent
        db.mark_note_spent(&nf, "tx1", 150).unwrap();

        let unspent = db.get_all_unspent_notes(0).unwrap();
        assert_eq!(unspent.len(), 1);
        assert_eq!(unspent[0].value, 30000);
    }

    #[test]
    fn test_clear_all_witnesses() {
        let db = test_db();
        let witness = vec![0x01u8; 200];
        db.insert_note(
            0,
            100,
            &[0xAAu8; 32],
            50000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();
        db.insert_note(
            0,
            200,
            &[0xBBu8; 32],
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&witness),
            None,
            None,
        )
        .unwrap();

        let count = db.clear_all_witnesses().unwrap();
        assert_eq!(count, 2);
        assert_eq!(db.get_balance(0).unwrap(), 0); // No witnesses = 0 spendable
    }

    #[test]
    fn test_transaction_count() {
        let db = test_db();
        assert_eq!(db.get_transaction_count().unwrap(), 0);
        db.insert_transaction(
            "tx1",
            100,
            None,
            TxType::Sent,
            1000,
            10000,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.insert_transaction(
            "tx2",
            200,
            None,
            TxType::Received,
            2000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        assert_eq!(db.get_transaction_count().unwrap(), 2);
    }

    #[test]
    fn test_verified_checkpoint_height() {
        let db = test_db();
        assert_eq!(db.get_verified_checkpoint_height().unwrap(), 0);
        db.update_verified_checkpoint_height(5000).unwrap();
        assert_eq!(db.get_verified_checkpoint_height().unwrap(), 5000);
    }

    #[test]
    fn test_clear_notes_and_history() {
        let db = test_db();

        // Insert some notes and transactions
        db.insert_note(
            0,
            100,
            &[0xAAu8; 32],
            50000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("tx1"),
            None,
        )
        .unwrap();
        db.insert_note(
            0,
            200,
            &[0xBBu8; 32],
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("tx2"),
            None,
        )
        .unwrap();
        db.insert_transaction(
            "tx1",
            100,
            None,
            TxType::Received,
            50000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.insert_transaction(
            "tx2",
            200,
            None,
            TxType::Received,
            30000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.save_tree_state(&[0x01, 0x02], 500).unwrap();

        assert_eq!(db.count_unspent_notes(0).unwrap(), 2);
        assert_eq!(db.get_transaction_count().unwrap(), 2);

        // Clear everything
        db.clear_notes_and_history().unwrap();

        assert_eq!(db.count_unspent_notes(0).unwrap(), 0);
        assert_eq!(db.get_transaction_count().unwrap(), 0);
        assert!(db.get_tree_state().unwrap().is_none());
        assert_eq!(db.get_tree_height().unwrap(), 0);
    }

    #[test]
    fn test_fix_zero_txid_notes() {
        let db = test_db();
        let zero_txid = "0000000000000000000000000000000000000000000000000000000000000000";

        // Insert notes with zero txid (simulating pre-fix scan)
        db.insert_note(
            0,
            100,
            &[0xAAu8; 32],
            50000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(zero_txid),
            None,
        )
        .unwrap();
        db.insert_note(
            0,
            200,
            &[0xBBu8; 32],
            30000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(zero_txid),
            None,
        )
        .unwrap();
        // One note with proper txid — should NOT be deleted
        db.insert_note(
            0,
            300,
            &[0xCCu8; 32],
            10000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("abc123"),
            None,
        )
        .unwrap();

        // Add TX history for zero txid
        db.insert_transaction(
            zero_txid,
            100,
            None,
            TxType::Received,
            80000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.insert_transaction(
            "abc123",
            300,
            None,
            TxType::Received,
            10000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        // Set tree state so we can verify it gets reset
        db.save_tree_state(&[0x01, 0x02], 200).unwrap();

        // Run migration
        let cleaned = db.fix_zero_txid_notes().unwrap();
        assert_eq!(cleaned, 2);

        // Zero-txid notes should be gone
        assert_eq!(db.count_unspent_notes(0).unwrap(), 1);

        // TX history for zero txid should be gone
        assert!(db.get_transaction_by_txid(zero_txid).unwrap().is_none());

        // Good tx history should remain
        assert!(db.get_transaction_by_txid("abc123").unwrap().is_some());

        // Tree state should be reset
        assert!(db.get_tree_state().unwrap().is_none());
        assert_eq!(db.get_tree_height().unwrap(), 0);

        // Running again should be a no-op
        assert_eq!(db.fix_zero_txid_notes().unwrap(), 0);
    }

    #[test]
    fn test_change_output_not_shown_as_sent() {
        // Regression test: when a TX creates multiple outputs to us (send-to-self),
        // UNIQUE(txid, tx_type) drops the second "received" entry. The old code used
        // transaction_history for change detection → wrong net amount. The fix uses
        // the notes table (which has ALL notes) for authoritative change computation.
        // Send-to-self TXs should appear as SelfTransfer (not hidden).
        let db = test_db();
        let spend_txid = "53cec586d7950e2c00000000000000000000000000000000000000000000dead";

        // Input note: 92,934,999 zatoshis (spent in this TX)
        let input_cmu = [0x01u8; 32];
        let input_nf = [0x02u8; 32];
        db.insert_note(
            0,
            3023200,
            &input_cmu,
            92_934_999,
            Some(&input_nf),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("prev_tx"),
            None,
        )
        .unwrap();
        db.mark_note_spent(&input_nf, spend_txid, 3023247).unwrap();

        // Output note 1: 119,999 (sent to self — recipient address is ours)
        let out1_cmu = [0x03u8; 32];
        db.insert_note(
            0,
            3023247,
            &out1_cmu,
            119_999,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(spend_txid),
            None,
        )
        .unwrap();

        // Output note 2: 92,805,000 (change back to us)
        let out2_cmu = [0x04u8; 32];
        db.insert_note(
            0,
            3023247,
            &out2_cmu,
            92_805_000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(spend_txid),
            None,
        )
        .unwrap();

        // Scanner creates "received" entry (first note wins due to UNIQUE(txid, tx_type))
        db.insert_transaction(
            spend_txid,
            3023247,
            None,
            TxType::Received,
            119_999,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        // Second "received" entry is dropped by INSERT OR IGNORE
        let _ = db.insert_transaction(
            spend_txid,
            3023247,
            None,
            TxType::Received,
            92_805_000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        );

        // Scanner creates "sent" entry with raw total input
        db.insert_transaction(
            spend_txid,
            3023247,
            None,
            TxType::Sent,
            92_934_999,
            10_000,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        // Query history — should show as SelfTransfer
        let history = db.get_transaction_history(50, 0).unwrap();

        // For send-to-self: net = 92,934,999 - (119,999 + 92,805,000) - 10,000 = 0
        // This should appear as SelfTransfer with amount = fee
        let self_entries: Vec<_> = history
            .iter()
            .filter(|r| r.tx_type == TxType::SelfTransfer)
            .collect();
        assert_eq!(
            self_entries.len(),
            1,
            "Send-to-self TX should appear as SelfTransfer, got history: {:?}",
            history
                .iter()
                .map(|r| (&r.tx_type, r.amount))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            self_entries[0].amount, 10_000,
            "SelfTransfer amount should be the fee"
        );

        // No "sent" or "received" entries — only the SelfTransfer
        let sent_entries: Vec<_> = history
            .iter()
            .filter(|r| r.tx_type == TxType::Sent)
            .collect();
        assert!(
            sent_entries.is_empty(),
            "Should have no 'sent' entries for send-to-self"
        );

        let recv_entries: Vec<_> = history
            .iter()
            .filter(|r| r.tx_type == TxType::Received && r.txid == spend_txid)
            .collect();
        assert!(
            recv_entries.is_empty(),
            "Should have no 'received' entries for send-to-self"
        );
    }

    #[test]
    fn test_send_to_other_shows_correct_net() {
        // Normal send: only change output is in our wallet, recipient's output is not.
        let db = test_db();
        let spend_txid = "aabbccdd00000000000000000000000000000000000000000000000000001234";

        // Input note: 92,934,999 zatoshis (spent)
        let input_cmu = [0x11u8; 32];
        let input_nf = [0x12u8; 32];
        db.insert_note(
            0,
            3023200,
            &input_cmu,
            92_934_999,
            Some(&input_nf),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("prev_tx2"),
            None,
        )
        .unwrap();
        db.mark_note_spent(&input_nf, spend_txid, 3023247).unwrap();

        // Only change output in our wallet: 92,805,000
        let change_cmu = [0x13u8; 32];
        db.insert_note(
            0,
            3023247,
            &change_cmu,
            92_805_000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(spend_txid),
            None,
        )
        .unwrap();

        // Transaction history entries
        db.insert_transaction(
            spend_txid,
            3023247,
            None,
            TxType::Received,
            92_805_000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();
        db.insert_transaction(
            spend_txid,
            3023247,
            None,
            TxType::Sent,
            92_934_999,
            10_000,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        let history = db.get_transaction_history(50, 0).unwrap();

        // net = 92,934,999 - 92,805,000 - 10,000 = 119,999
        let sent_entries: Vec<_> = history
            .iter()
            .filter(|r| r.tx_type == TxType::Sent)
            .collect();
        assert_eq!(sent_entries.len(), 1);
        assert_eq!(
            sent_entries[0].amount, 119_999,
            "Sent amount should be net (input - change - fee), got {}",
            sent_entries[0].amount,
        );

        // "received" entry should be hidden (it's change)
        let recv_entries: Vec<_> = history
            .iter()
            .filter(|r| r.txid == spend_txid && r.tx_type == TxType::Received)
            .collect();
        assert!(
            recv_entries.is_empty(),
            "Change output should be hidden from history"
        );
    }

    #[test]
    fn test_synthesized_sent_uses_notes_for_change() {
        // When there's no explicit "sent" entry, the synthesis path should also
        // use the notes table for change detection.
        let db = test_db();
        let spend_txid = "synthtest0000000000000000000000000000000000000000000000000000abcd";

        // Input note: 100,000 (spent)
        let input_cmu = [0x21u8; 32];
        let input_nf = [0x22u8; 32];
        db.insert_note(
            0,
            500,
            &input_cmu,
            100_000,
            Some(&input_nf),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("prev_tx3"),
            None,
        )
        .unwrap();
        db.mark_note_spent(&input_nf, spend_txid, 600).unwrap();

        // Change output: 60,000
        let change_cmu = [0x23u8; 32];
        db.insert_note(
            0,
            600,
            &change_cmu,
            60_000,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(spend_txid),
            None,
        )
        .unwrap();

        // Only a "received" entry — NO "sent" entry (triggers synthesis)
        db.insert_transaction(
            spend_txid,
            600,
            None,
            TxType::Received,
            60_000,
            0,
            None,
            None,
            TxStatus::Confirmed,
        )
        .unwrap();

        let history = db.get_transaction_history(50, 0).unwrap();

        // Synthesized: net = 100,000 - 60,000 - 10,000 = 30,000
        let sent_entries: Vec<_> = history
            .iter()
            .filter(|r| r.tx_type == TxType::Sent)
            .collect();
        assert_eq!(sent_entries.len(), 1);
        assert_eq!(
            sent_entries[0].amount, 30_000,
            "Synthesized sent amount should be net, got {}",
            sent_entries[0].amount,
        );
    }

    #[test]
    fn test_transparent_utxo_is_imported() {
        let db = test_db();
        db.insert_transparent_utxo(
            100,
            "tx_imported",
            0,
            &[0x76, 0xa9],
            "t1ImportedAddr",
            50000,
            false,
            0,
            true,
        )
        .unwrap();
        let utxos = db.get_unspent_transparent_utxos().unwrap();
        assert_eq!(utxos.len(), 1);
        assert!(utxos[0].is_imported);
    }

    #[test]
    fn test_store_and_load_imported_transparent_key() {
        let db = test_db();
        let fake_encrypted = vec![0xAA; 48];
        db.store_imported_transparent_key("t1TestAddr123", &fake_encrypted)
            .unwrap();

        let addrs = db.get_imported_transparent_addresses().unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].1, "t1TestAddr123");

        let loaded = db
            .get_imported_transparent_secret("t1TestAddr123")
            .unwrap();
        assert_eq!(loaded, Some(fake_encrypted));

        let missing = db
            .get_imported_transparent_secret("t1NoSuchAddr")
            .unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn test_imported_key_count() {
        let db = test_db();
        assert_eq!(db.get_imported_key_count().unwrap(), 0);
        db.store_imported_transparent_key("t1Addr1", &[0; 48])
            .unwrap();
        db.store_imported_transparent_key("t1Addr2", &[1; 48])
            .unwrap();
        assert_eq!(db.get_imported_key_count().unwrap(), 2);
    }

    #[test]
    fn test_get_funded_transparent_addresses() {
        let db = test_db();
        // Two UTXOs for same address (should aggregate)
        db.insert_transparent_utxo(100, "tx1", 0, &[], "t1AddrA", 10000, false, 0, false)
            .unwrap();
        db.insert_transparent_utxo(101, "tx2", 0, &[], "t1AddrA", 20000, false, 0, false)
            .unwrap();
        // Change address
        db.insert_transparent_utxo(102, "tx3", 1, &[], "t1AddrB", 5000, true, 1, false)
            .unwrap();
        // Spent UTXO (should not appear)
        db.insert_transparent_utxo(103, "tx4", 0, &[], "t1AddrC", 99999, false, 2, false)
            .unwrap();
        db.mark_transparent_utxo_spent("tx4", 0, "txSpend", 104)
            .unwrap();

        let funded = db.get_funded_transparent_addresses().unwrap();
        assert_eq!(funded.len(), 2);

        let a = funded.iter().find(|f| f.address == "t1AddrA").unwrap();
        assert_eq!(a.balance, 30000);
        assert!(!a.is_change);
        assert!(!a.is_imported);

        let b = funded.iter().find(|f| f.address == "t1AddrB").unwrap();
        assert_eq!(b.balance, 5000);
        assert!(b.is_change);
    }
}
