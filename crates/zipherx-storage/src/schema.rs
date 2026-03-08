//! Database schema definitions.
//!
//! All CREATE TABLE statements for the ZipherX wallet database.
//! Uses SQLCipher (AES-256 encrypted SQLite) for at-rest encryption.

/// Schema version — increment when tables change.
pub const SCHEMA_VERSION: u32 = 1;

/// Create the notes table (shielded notes / received ZCL).
pub const CREATE_NOTES_TABLE: &str = "
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL,
    cmu BLOB NOT NULL,
    epk BLOB,
    ciphertext BLOB,
    value INTEGER NOT NULL,
    rcm BLOB,
    nullifier BLOB,
    witness BLOB,
    anchor BLOB,
    is_spent INTEGER NOT NULL DEFAULT 0,
    spent_in_tx TEXT,
    spent_height INTEGER,
    memo TEXT,
    diversifier BLOB,
    received_txid TEXT,
    position INTEGER,
    UNIQUE(cmu)
)";

/// Create the transaction history table.
pub const CREATE_TX_HISTORY_TABLE: &str = "
CREATE TABLE IF NOT EXISTS transaction_history (
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
)";

/// Create the sync state table.
pub const CREATE_SYNC_STATE_TABLE: &str = "
CREATE TABLE IF NOT EXISTS sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_scanned_height INTEGER NOT NULL DEFAULT 0,
    verified_checkpoint_height INTEGER NOT NULL DEFAULT 0,
    tree_state BLOB,
    tree_height INTEGER NOT NULL DEFAULT 0,
    boost_file_height INTEGER NOT NULL DEFAULT 0,
    boost_cmu_count INTEGER NOT NULL DEFAULT 0,
    delta_bundle_verified INTEGER NOT NULL DEFAULT 0
)";

/// Create the block headers table (HeaderStore).
pub const CREATE_HEADERS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS block_headers (
    height INTEGER PRIMARY KEY,
    hash BLOB NOT NULL,
    prev_hash BLOB NOT NULL,
    final_sapling_root BLOB,
    timestamp INTEGER NOT NULL,
    bits INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 4,
    UNIQUE(hash)
)";

/// Create the delta CMU manifest table.
pub const CREATE_DELTA_MANIFEST_TABLE: &str = "
CREATE TABLE IF NOT EXISTS delta_manifest (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    start_height INTEGER NOT NULL DEFAULT 0,
    end_height INTEGER NOT NULL DEFAULT 0,
    cmu_count INTEGER NOT NULL DEFAULT 0,
    verified INTEGER NOT NULL DEFAULT 0,
    last_updated INTEGER NOT NULL DEFAULT 0
)";

/// Create the sapling roots lookup table.
pub const CREATE_SAPLING_ROOTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS sapling_roots (
    height INTEGER PRIMARY KEY,
    root BLOB NOT NULL,
    root_reversed BLOB NOT NULL
)";

/// Create indexes for fast lookups.
pub const CREATE_INDEXES: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_notes_nullifier ON notes(nullifier)",
    "CREATE INDEX IF NOT EXISTS idx_notes_spent ON notes(is_spent)",
    "CREATE INDEX IF NOT EXISTS idx_notes_cmu ON notes(cmu)",
    "CREATE INDEX IF NOT EXISTS idx_notes_height ON notes(height)",
    "CREATE INDEX IF NOT EXISTS idx_tx_history_txid ON transaction_history(txid)",
    "CREATE INDEX IF NOT EXISTS idx_tx_history_height ON transaction_history(height)",
    "CREATE INDEX IF NOT EXISTS idx_tx_history_status ON transaction_history(status)",
    "CREATE INDEX IF NOT EXISTS idx_headers_hash ON block_headers(hash)",
    "CREATE INDEX IF NOT EXISTS idx_sapling_roots_root ON sapling_roots(root)",
    "CREATE INDEX IF NOT EXISTS idx_sapling_roots_reversed ON sapling_roots(root_reversed)",
];

/// All schema creation statements in order.
pub fn all_create_statements() -> Vec<&'static str> {
    let mut stmts = vec![
        CREATE_NOTES_TABLE,
        CREATE_TX_HISTORY_TABLE,
        CREATE_SYNC_STATE_TABLE,
        CREATE_HEADERS_TABLE,
        CREATE_DELTA_MANIFEST_TABLE,
        CREATE_SAPLING_ROOTS_TABLE,
    ];
    stmts.extend_from_slice(CREATE_INDEXES);
    stmts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        // Verify all SQL statements are syntactically valid by parsing with rusqlite
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for stmt in all_create_statements() {
            conn.execute_batch(stmt).unwrap_or_else(|e| {
                panic!("Schema creation failed for:\n{stmt}\nError: {e}");
            });
        }
    }

    #[test]
    fn test_insert_and_query_note() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        for stmt in all_create_statements() {
            conn.execute_batch(stmt).unwrap();
        }

        conn.execute(
            "INSERT INTO notes (height, cmu, value) VALUES (?1, ?2, ?3)",
            rusqlite::params![100, vec![0xAAu8; 32], 50000i64],
        ).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sync_state_singleton() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SYNC_STATE_TABLE).unwrap();

        // Insert initial state
        conn.execute(
            "INSERT INTO sync_state (id, last_scanned_height) VALUES (1, 0)",
            [],
        ).unwrap();

        // Second insert should fail (CHECK constraint id = 1)
        let result = conn.execute(
            "INSERT INTO sync_state (id, last_scanned_height) VALUES (2, 100)",
            [],
        );
        assert!(result.is_err());
    }
}
