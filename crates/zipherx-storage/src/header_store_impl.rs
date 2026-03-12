//! SqliteHeaderStore — persistent HeaderStore backed by SQLite.
//!
//! Implements `zipherx_network::header_sync::HeaderStore` trait with
//! sapling root caching and both-byte-order lookups (FIX #1230).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use rusqlite::params;

use zipherx_network::header_sync::{HeaderStore, StoredHeader};
use zipherx_network::types::NetworkError;

/// SQLite-backed HeaderStore with in-memory sapling root cache.
pub struct SqliteHeaderStore {
    conn: Mutex<rusqlite::Connection>,
    /// FIX #1253: In-memory cache of delta sapling roots.
    /// Checked before SQL for fast lookups.
    delta_sapling_roots: Mutex<HashSet<Vec<u8>>>,
    /// Rows inserted since last WAL checkpoint (for bulk import disk pressure management).
    bulk_insert_count: AtomicU64,
}

impl SqliteHeaderStore {
    /// Open a file-backed header store with optional SQLCipher encryption (M-3).
    ///
    /// If `encryption_key` is provided, the database is encrypted with SQLCipher.
    /// Pass `None` for an unencrypted header store (backward-compatible default).
    pub fn open(path: &str) -> Result<Self, NetworkError> {
        Self::open_with_key(path, None)
    }

    /// Open a file-backed header store with an explicit encryption key.
    pub fn open_with_key(path: &str, encryption_key: Option<&[u8]>) -> Result<Self, NetworkError> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| NetworkError::HeaderSyncFailed(format!("Open: {e}")))?;

        if let Some(key) = encryption_key {
            let mut hex_key = hex::encode(key);
            // SAFETY: hex::encode only produces ASCII hex digits [0-9a-f].
            assert!(
                hex_key.chars().all(|c| c.is_ascii_hexdigit()),
                "Invalid hex key"
            );
            let pragma_result = conn
                .execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))
                .map_err(|e| NetworkError::HeaderSyncFailed(format!("PRAGMA key: {e}")));
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
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
                .map_err(|e| NetworkError::HeaderSyncFailed(format!("Key verification: {e}")))?;
        }

        Self::setup(conn)
    }

    /// Open an in-memory header store (for testing).
    pub fn open_in_memory() -> Result<Self, NetworkError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| NetworkError::HeaderSyncFailed(format!("Open: {e}")))?;
        Self::setup(conn)
    }

    fn setup(conn: rusqlite::Connection) -> Result<Self, NetworkError> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;",
        )
        .map_err(|e| NetworkError::HeaderSyncFailed(format!("Pragmas: {e}")))?;

        conn.execute_batch(crate::schema::CREATE_HEADERS_TABLE)
            .map_err(|e| NetworkError::HeaderSyncFailed(format!("Schema: {e}")))?;
        conn.execute_batch(crate::schema::CREATE_SAPLING_ROOTS_TABLE)
            .map_err(|e| NetworkError::HeaderSyncFailed(format!("Schema: {e}")))?;
        // Create indexes
        for idx in &[
            "CREATE INDEX IF NOT EXISTS idx_headers_hash ON block_headers(hash)",
            "CREATE INDEX IF NOT EXISTS idx_sapling_roots_root ON sapling_roots(root)",
            "CREATE INDEX IF NOT EXISTS idx_sapling_roots_reversed ON sapling_roots(root_reversed)",
        ] {
            conn.execute_batch(idx)
                .map_err(|e| NetworkError::HeaderSyncFailed(format!("Index: {e}")))?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            delta_sapling_roots: Mutex::new(HashSet::new()),
            bulk_insert_count: AtomicU64::new(0),
        })
    }

    // ---------------------------------------------------------------
    // Bulk import mode — disable safety for 10-50x faster inserts
    // ---------------------------------------------------------------

    /// Enable bulk import mode: disables synchronous writes and drops indexes.
    /// Call `end_bulk_import` after to restore safety and rebuild indexes.
    pub fn begin_bulk_import(&self) -> Result<(), NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        // STOR-001: Use journal_mode=WAL instead of MEMORY to prevent database
        // corruption if the process crashes during bulk import. WAL mode still
        // provides good bulk import performance with synchronous=NORMAL.
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA journal_mode = WAL;
             PRAGMA cache_size = -64000;
             DROP INDEX IF EXISTS idx_headers_hash;
             DROP INDEX IF EXISTS idx_sapling_roots_root;
             DROP INDEX IF EXISTS idx_sapling_roots_reversed;",
        )
        .map_err(|e| NetworkError::HeaderSyncFailed(format!("begin_bulk_import: {e}")))?;
        self.bulk_insert_count.store(0, Ordering::Relaxed);
        eprintln!(
            "[ZipherX] HeaderStore: bulk import mode ON (synchronous=NORMAL, indexes dropped)"
        );
        Ok(())
    }

    /// End bulk import mode: rebuild indexes and restore safe pragmas.
    ///
    /// Rebuilds indexes one at a time with WAL checkpoints between each to
    /// prevent "database or disk is full" on space-constrained devices
    /// (e.g. Android emulators). Each checkpoint merges WAL back into the
    /// main DB file and truncates the WAL, freeing disk space for the next
    /// index build.
    pub fn end_bulk_import(&self) -> Result<(), NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

        // Checkpoint WAL before rebuilding indexes to reclaim space from bulk inserts
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");

        // Build indexes one at a time with checkpoints between each
        let indexes = [
            "CREATE INDEX IF NOT EXISTS idx_headers_hash ON block_headers(hash)",
            "CREATE INDEX IF NOT EXISTS idx_sapling_roots_root ON sapling_roots(root)",
            "CREATE INDEX IF NOT EXISTS idx_sapling_roots_reversed ON sapling_roots(root_reversed)",
        ];
        for idx_sql in &indexes {
            conn.execute_batch(idx_sql)
                .map_err(|e| NetworkError::HeaderSyncFailed(format!("end_bulk_import: {e}")))?;
            // Checkpoint after each index to free WAL space for the next one
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
        }

        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA journal_mode = WAL;
             PRAGMA cache_size = -8000;",
        )
        .map_err(|e| NetworkError::HeaderSyncFailed(format!("end_bulk_import: {e}")))?;
        eprintln!(
            "[ZipherX] HeaderStore: bulk import mode OFF (indexes rebuilt, synchronous=NORMAL)"
        );
        Ok(())
    }

    // ---------------------------------------------------------------
    // Sapling root queries (FIX #1230 / #1253)
    // ---------------------------------------------------------------

    /// Check if a sapling root exists (both byte orders, FIX #1230).
    ///
    /// Checks:
    /// 1. In-memory delta cache (FIX #1253)
    /// 2. sapling_roots table (both original + reversed)
    /// 3. block_headers.final_sapling_root
    pub fn contains_sapling_root(&self, anchor: &[u8]) -> Result<bool, NetworkError> {
        let reversed: Vec<u8> = anchor.iter().rev().copied().collect();

        // 1. Check in-memory cache
        {
            let cache = self
                .delta_sapling_roots
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if cache.contains(anchor) || cache.contains(&reversed) {
                return Ok(true);
            }
        }

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());

        // 2. Check sapling_roots table (both byte orders)
        let found: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sapling_roots
                 WHERE root = ?1 OR root_reversed = ?1 OR root = ?2 OR root_reversed = ?2",
                params![anchor, &reversed],
                |row| row.get(0),
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        if found {
            return Ok(true);
        }

        // 3. Check block_headers.final_sapling_root
        let found: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM block_headers
                 WHERE final_sapling_root = ?1 OR final_sapling_root = ?2",
                params![anchor, &reversed],
                |row| row.get(0),
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(found)
    }

    /// Get the sapling root for a specific height.
    pub fn get_sapling_root(&self, height: u64) -> Result<Option<Vec<u8>>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        // Try sapling_roots first
        let root: Option<Vec<u8>> = conn
            .query_row(
                "SELECT root FROM sapling_roots WHERE height = ?1",
                params![height as i64],
                |row| row.get(0),
            )
            .ok();
        if root.is_some() {
            return Ok(root);
        }
        // Fall back to block_headers
        let root: Option<Vec<u8>> = conn
            .query_row(
                "SELECT final_sapling_root FROM block_headers WHERE height = ?1",
                params![height as i64],
                |row| row.get(0),
            )
            .ok();
        Ok(root)
    }

    /// Get block timestamp for a height.
    pub fn get_block_time(&self, height: u64) -> Result<Option<u64>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let ts: Option<i64> = conn
            .query_row(
                "SELECT timestamp FROM block_headers WHERE height = ?1",
                params![height as i64],
                |row| row.get(0),
            )
            .ok();
        Ok(ts.map(|t| t as u64))
    }

    /// Load delta sapling roots into the in-memory cache (FIX #1253).
    ///
    /// Entries are (height, root) pairs. Both byte orders are cached.
    pub fn load_delta_sapling_roots(&self, entries: &[(u64, Vec<u8>)]) {
        let mut cache = self
            .delta_sapling_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_, root) in entries {
            let reversed: Vec<u8> = root.iter().rev().copied().collect();
            cache.insert(root.clone());
            cache.insert(reversed);
        }
    }

    /// Insert sapling roots into the database (both byte orders, FIX #1230).
    pub fn store_sapling_roots(&self, entries: &[(u64, Vec<u8>)]) -> Result<(), NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO sapling_roots (height, root, root_reversed)
                 VALUES (?1, ?2, ?3)",
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        for (height, root) in entries {
            let reversed: Vec<u8> = root.iter().rev().copied().collect();
            stmt.execute(params![*height as i64, root, &reversed])
                .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        }

        // Also add to in-memory cache
        let mut cache = self
            .delta_sapling_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_, root) in entries {
            let reversed: Vec<u8> = root.iter().rev().copied().collect();
            cache.insert(root.clone());
            cache.insert(reversed);
        }
        Ok(())
    }

    /// Find blocks where the Sapling tree root changed (blocks with new shielded outputs).
    ///
    /// Compares each block's `final_sapling_root` with the previous block's root.
    /// Blocks where the root differs contain new Sapling outputs (CMUs).
    /// Returns `(height, hash)` pairs sorted by height.
    ///
    /// Only checks heights >= `start_height` and <= `end_height`.
    /// Pre-Sapling blocks (root = all zeros) are excluded.
    pub fn get_blocks_with_new_outputs(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Result<Vec<(u64, [u8; 32])>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT h1.height, h1.hash
                 FROM block_headers h1
                 WHERE h1.height BETWEEN ?1 AND ?2
                   AND h1.final_sapling_root IS NOT NULL
                   AND length(h1.final_sapling_root) = 32
                   AND h1.final_sapling_root != X'0000000000000000000000000000000000000000000000000000000000000000'
                   AND h1.final_sapling_root != COALESCE(
                       (SELECT h2.final_sapling_root FROM block_headers h2
                        WHERE h2.height = h1.height - 1),
                       X'0000000000000000000000000000000000000000000000000000000000000000'
                   )
                 ORDER BY h1.height",
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let rows = stmt
            .query_map(params![start_height as i64, end_height as i64], |row| {
                let height: i64 = row.get(0)?;
                let hash_blob: Vec<u8> = row.get(1)?;
                Ok((height as u64, blob_to_array32(&hash_blob)))
            })
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let mut blocks = Vec::new();
        for r in rows {
            blocks.push(r.map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?);
        }
        Ok(blocks)
    }

    /// Get ALL block hashes in a height range (for full-block nullifier scan).
    ///
    /// Returns `(height, hash)` pairs for every block in `[start_height, end_height]`,
    /// sorted by height. Unlike `get_blocks_with_new_outputs()`, this includes blocks
    /// that do NOT have new Sapling outputs (spend-only blocks).
    pub fn get_all_block_hashes_in_range(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Result<Vec<(u64, [u8; 32])>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT height, hash FROM block_headers
                 WHERE height BETWEEN ?1 AND ?2
                 ORDER BY height",
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let rows = stmt
            .query_map(params![start_height as i64, end_height as i64], |row| {
                let height: i64 = row.get(0)?;
                let hash_blob: Vec<u8> = row.get(1)?;
                Ok((height as u64, blob_to_array32(&hash_blob)))
            })
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let mut blocks = Vec::new();
        for r in rows {
            blocks.push(r.map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?);
        }
        Ok(blocks)
    }

    /// Count block headers in a height range without loading any data.
    pub fn count_block_hashes_in_range(
        &self,
        start_height: u64,
        end_height: u64,
    ) -> Result<usize, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM block_headers WHERE height BETWEEN ?1 AND ?2",
                params![start_height as i64, end_height as i64],
                |row| row.get(0),
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(count as usize)
    }

    /// Get a page of block hashes in a height range using LIMIT/OFFSET.
    pub fn get_block_hashes_in_range_paged(
        &self,
        start_height: u64,
        end_height: u64,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(u64, [u8; 32])>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare(
                "SELECT height, hash FROM block_headers
                 WHERE height BETWEEN ?1 AND ?2
                 ORDER BY height
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let rows = stmt
            .query_map(
                params![
                    start_height as i64,
                    end_height as i64,
                    limit as i64,
                    offset as i64,
                ],
                |row| {
                    let height: i64 = row.get(0)?;
                    let hash_blob: Vec<u8> = row.get(1)?;
                    Ok((height as u64, blob_to_array32(&hash_blob)))
                },
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        let mut blocks = Vec::new();
        for r in rows {
            blocks.push(r.map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?);
        }
        Ok(blocks)
    }

    /// Get the number of stored headers.
    pub fn header_count(&self) -> Result<usize, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM block_headers", [], |row| row.get(0))
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(count as usize)
    }

    /// RS-N2: Run WAL checkpoint to reclaim disk space and prevent unbounded
    /// WAL file growth. Call after sync completion or bulk operations.
    pub fn checkpoint_wal(&self) -> Result<(), NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .map_err(|e| NetworkError::HeaderSyncFailed(format!("WAL checkpoint: {e}")))?;
        Ok(())
    }

    /// Delete headers above a certain height (for chain reorganization).
    pub fn truncate_above(&self, height: u64) -> Result<usize, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let deleted = conn
            .execute(
                "DELETE FROM block_headers WHERE height > ?1",
                params![height as i64],
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(deleted)
    }
}

/// Convert a BLOB to `[u8; 32]`, padding or truncating as needed.
fn blob_to_array32(blob: &[u8]) -> [u8; 32] {
    let mut arr = [0u8; 32];
    let len = blob.len().min(32);
    arr[..len].copy_from_slice(&blob[..len]);
    arr
}

impl HeaderStore for SqliteHeaderStore {
    fn get_latest_height(&self) -> Result<Option<u64>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let height: Option<i64> = conn
            .query_row("SELECT MAX(height) FROM block_headers", [], |row| {
                row.get(0)
            })
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(height.map(|h| h as u64))
    }

    fn get_header(&self, height: u64) -> Result<Option<StoredHeader>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let result = conn.query_row(
            "SELECT hash, prev_hash, final_sapling_root, timestamp, bits
             FROM block_headers WHERE height = ?1",
            params![height as i64],
            |row| {
                let hash_blob: Vec<u8> = row.get(0)?;
                let prev_blob: Vec<u8> = row.get(1)?;
                let root_blob: Option<Vec<u8>> = row.get(2)?;
                let timestamp: i64 = row.get(3)?;
                let bits: i64 = row.get(4)?;
                Ok(StoredHeader {
                    hash: blob_to_array32(&hash_blob),
                    prev_hash: blob_to_array32(&prev_blob),
                    final_sapling_root: root_blob.map(|b| blob_to_array32(&b)).unwrap_or([0u8; 32]),
                    timestamp: timestamp as u32,
                    bits: bits as u32,
                })
            },
        );
        match result {
            Ok(header) => Ok(Some(header)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(NetworkError::HeaderSyncFailed(e.to_string())),
        }
    }

    fn get_header_hash(&self, height: u64) -> Result<Option<[u8; 32]>, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let result = conn.query_row(
            "SELECT hash FROM block_headers WHERE height = ?1",
            params![height as i64],
            |row| {
                let blob: Vec<u8> = row.get(0)?;
                Ok(blob_to_array32(&blob))
            },
        );
        match result {
            Ok(hash) => Ok(Some(hash)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(NetworkError::HeaderSyncFailed(e.to_string())),
        }
    }

    fn store_headers(&self, headers: Vec<(u64, StoredHeader)>) -> Result<(), NetworkError> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let tx = conn
            .transaction()
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO block_headers
                     (height, hash, prev_hash, final_sapling_root, timestamp, bits)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

            let mut root_stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO sapling_roots (height, root, root_reversed)
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

            for (height, header) in &headers {
                stmt.execute(params![
                    *height as i64,
                    header.hash.as_slice(),
                    header.prev_hash.as_slice(),
                    header.final_sapling_root.as_slice(),
                    header.timestamp as i64,
                    header.bits as i64,
                ])
                .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

                // Store sapling root in both byte orders (FIX #1230)
                if header.final_sapling_root != [0u8; 32] {
                    let reversed: Vec<u8> =
                        header.final_sapling_root.iter().rev().copied().collect();
                    root_stmt
                        .execute(params![
                            *height as i64,
                            header.final_sapling_root.as_slice(),
                            &reversed,
                        ])
                        .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
                }
            }
        }

        tx.commit()
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;

        // Periodic WAL checkpoint during bulk import to prevent disk exhaustion.
        // Every 200k rows (~every 4 batches of 50k), checkpoint to merge WAL
        // back into the main DB file. This prevents the WAL from growing to
        // hundreds of MB on space-constrained devices (Android emulators).
        let prev = self
            .bulk_insert_count
            .fetch_add(headers.len() as u64, Ordering::Relaxed);
        if prev / 200_000 != (prev + headers.len() as u64) / 200_000 {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
        }

        // Update in-memory cache with new roots
        let mut cache = self
            .delta_sapling_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_, header) in &headers {
            if header.final_sapling_root != [0u8; 32] {
                let reversed: Vec<u8> = header.final_sapling_root.iter().rev().copied().collect();
                cache.insert(header.final_sapling_root.to_vec());
                cache.insert(reversed);
            }
        }

        Ok(())
    }

    fn count_headers_in_range(&self, from: u64, to: u64) -> Result<usize, NetworkError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM block_headers WHERE height >= ?1 AND height <= ?2",
                params![from as i64, to as i64],
                |row| row.get(0),
            )
            .map_err(|e| NetworkError::HeaderSyncFailed(e.to_string()))?;
        Ok(count as usize)
    }

    fn truncate_above(&self, height: u64) -> Result<(), NetworkError> {
        SqliteHeaderStore::truncate_above(self, height).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> SqliteHeaderStore {
        SqliteHeaderStore::open_in_memory().unwrap()
    }

    fn make_header(hash_byte: u8, prev_byte: u8, root_byte: u8) -> StoredHeader {
        let mut hash = [0u8; 32];
        hash[0] = hash_byte;
        let mut prev = [0u8; 32];
        prev[0] = prev_byte;
        let mut root = [0u8; 32];
        root[0] = root_byte;
        StoredHeader {
            hash,
            prev_hash: prev,
            final_sapling_root: root,
            timestamp: 1700000000,
            bits: 0x2007ffff,
        }
    }

    #[test]
    fn test_open_in_memory() {
        let store = test_store();
        assert_eq!(store.get_latest_height().unwrap(), None);
    }

    #[test]
    fn test_store_and_retrieve_header() {
        let store = test_store();
        let header = make_header(0xAA, 0xBB, 0xCC);
        store.store_headers(vec![(100, header.clone())]).unwrap();

        let retrieved = store.get_header(100).unwrap().unwrap();
        assert_eq!(retrieved.hash, header.hash);
        assert_eq!(retrieved.prev_hash, header.prev_hash);
        assert_eq!(retrieved.final_sapling_root, header.final_sapling_root);
        assert_eq!(retrieved.timestamp, header.timestamp);
        assert_eq!(retrieved.bits, header.bits);
    }

    #[test]
    fn test_get_latest_height() {
        let store = test_store();
        store
            .store_headers(vec![
                (100, make_header(1, 0, 0xAA)),
                (200, make_header(2, 1, 0xBB)),
                (150, make_header(3, 0, 0xCC)),
            ])
            .unwrap();

        assert_eq!(store.get_latest_height().unwrap(), Some(200));
    }

    #[test]
    fn test_get_header_hash() {
        let store = test_store();
        let header = make_header(0xAA, 0xBB, 0xCC);
        store.store_headers(vec![(100, header.clone())]).unwrap();

        let hash = store.get_header_hash(100).unwrap().unwrap();
        assert_eq!(hash, header.hash);

        assert!(store.get_header_hash(999).unwrap().is_none());
    }

    #[test]
    fn test_count_headers_in_range() {
        let store = test_store();
        store
            .store_headers(vec![
                (100, make_header(1, 0, 0xAA)),
                (200, make_header(2, 1, 0xBB)),
                (300, make_header(3, 2, 0xCC)),
            ])
            .unwrap();

        assert_eq!(store.count_headers_in_range(100, 300).unwrap(), 3);
        assert_eq!(store.count_headers_in_range(100, 200).unwrap(), 2);
        assert_eq!(store.count_headers_in_range(150, 250).unwrap(), 1);
        assert_eq!(store.count_headers_in_range(400, 500).unwrap(), 0);
    }

    #[test]
    fn test_contains_sapling_root_from_headers() {
        let store = test_store();
        let mut root = [0u8; 32];
        root[0] = 0xAA;
        store
            .store_headers(vec![(100, make_header(1, 0, 0xAA))])
            .unwrap();

        // Direct match
        assert!(store.contains_sapling_root(&root).unwrap());

        // Reversed match (FIX #1230)
        let reversed: Vec<u8> = root.iter().rev().copied().collect();
        assert!(store.contains_sapling_root(&reversed).unwrap());

        // Non-existent
        assert!(!store.contains_sapling_root(&[0xFFu8; 32]).unwrap());
    }

    #[test]
    fn test_contains_sapling_root_from_cache() {
        let store = test_store();
        let root = vec![0xDD; 32];
        store.load_delta_sapling_roots(&[(500, root.clone())]);

        // Should find in cache
        assert!(store.contains_sapling_root(&root).unwrap());

        // Reversed should also match
        let reversed: Vec<u8> = root.iter().rev().copied().collect();
        assert!(store.contains_sapling_root(&reversed).unwrap());
    }

    #[test]
    fn test_store_sapling_roots() {
        let store = test_store();
        let root = vec![0xEE; 32];
        store.store_sapling_roots(&[(600, root.clone())]).unwrap();

        // Should find via SQL
        assert!(store.contains_sapling_root(&root).unwrap());

        // And in cache
        let reversed: Vec<u8> = root.iter().rev().copied().collect();
        assert!(store.contains_sapling_root(&reversed).unwrap());
    }

    #[test]
    fn test_get_sapling_root() {
        let store = test_store();
        let header = make_header(1, 0, 0xAA);
        store.store_headers(vec![(100, header)]).unwrap();

        let root = store.get_sapling_root(100).unwrap().unwrap();
        assert_eq!(root[0], 0xAA);

        assert!(store.get_sapling_root(999).unwrap().is_none());
    }

    #[test]
    fn test_get_block_time() {
        let store = test_store();
        let header = make_header(1, 0, 0xAA);
        store.store_headers(vec![(100, header)]).unwrap();

        let ts = store.get_block_time(100).unwrap().unwrap();
        assert_eq!(ts, 1700000000);

        assert!(store.get_block_time(999).unwrap().is_none());
    }

    #[test]
    fn test_header_count() {
        let store = test_store();
        assert_eq!(store.header_count().unwrap(), 0);

        store
            .store_headers(vec![
                (100, make_header(1, 0, 0xAA)),
                (200, make_header(2, 1, 0xBB)),
            ])
            .unwrap();
        assert_eq!(store.header_count().unwrap(), 2);
    }

    #[test]
    fn test_get_blocks_with_new_outputs() {
        let store = test_store();
        // Heights 100-103, same root = only first has a "new" root
        // Height 104 changes root = new output block
        store
            .store_headers(vec![
                (100, make_header(1, 0, 0xAA)),
                (101, make_header(2, 1, 0xAA)), // same root as 100
                (102, make_header(3, 2, 0xAA)), // same root
                (103, make_header(4, 3, 0xBB)), // root changed!
                (104, make_header(5, 4, 0xBB)), // same root as 103
                (105, make_header(6, 5, 0xCC)), // root changed!
            ])
            .unwrap();

        let blocks = store.get_blocks_with_new_outputs(100, 105).unwrap();
        // Block 100: root 0xAA, prev doesn't exist → differs from zero → new
        // Block 101-102: same root as previous → no new outputs
        // Block 103: root 0xBB, prev was 0xAA → new
        // Block 104: same root as 103 → no
        // Block 105: root 0xCC, prev was 0xBB → new
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].0, 100);
        assert_eq!(blocks[1].0, 103);
        assert_eq!(blocks[2].0, 105);
    }

    #[test]
    fn test_get_blocks_with_new_outputs_zero_root_excluded() {
        let store = test_store();
        // Pre-Sapling blocks have zero root — should be excluded
        store
            .store_headers(vec![
                (100, make_header(1, 0, 0)),    // zero root
                (101, make_header(2, 1, 0)),    // zero root
                (102, make_header(3, 2, 0xAA)), // first non-zero = new
            ])
            .unwrap();

        let blocks = store.get_blocks_with_new_outputs(100, 102).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, 102);
    }

    #[test]
    fn test_truncate_above() {
        let store = test_store();
        store
            .store_headers(vec![
                (100, make_header(1, 0, 0xAA)),
                (200, make_header(2, 1, 0xBB)),
                (300, make_header(3, 2, 0xCC)),
            ])
            .unwrap();

        let deleted = store.truncate_above(150).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.get_latest_height().unwrap(), Some(100));
    }
}
