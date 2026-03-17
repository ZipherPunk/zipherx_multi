//! Storage data types — Rust-native structs for database records.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::encryption::EncryptionError;

/// Storage layer error types.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database not opened")]
    NotOpened,

    #[error("Database open failed: {0}")]
    OpenFailed(String),

    #[error("Schema creation failed: {0}")]
    SchemaFailed(String),

    #[error("Insert failed: {0}")]
    InsertFailed(String),

    #[error("Update failed: {0}")]
    UpdateFailed(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Record not found")]
    NotFound,

    #[error("Encryption error: {0}")]
    Encryption(#[from] EncryptionError),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Delta bundle is verified — clear blocked (use force=true)")]
    DeltaBundleVerified,
}

/// A shielded note (received ZCL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// Database primary key.
    pub id: i64,
    /// Account index.
    pub account_id: i64,
    /// Block height where this note was received.
    pub height: u64,
    /// Note commitment (32 bytes).
    pub cmu: Vec<u8>,
    /// Ephemeral public key (32 bytes).
    pub epk: Option<Vec<u8>>,
    /// Encrypted ciphertext (580 bytes).
    pub ciphertext: Option<Vec<u8>>,
    /// Note value in zatoshis.
    pub value: u64,
    /// Randomness for commitment (32 bytes).
    pub rcm: Option<Vec<u8>>,
    /// Nullifier (32 bytes) — spent-flag for blockchain.
    pub nullifier: Option<Vec<u8>>,
    /// Incremental witness (serialized merkle path).
    pub witness: Option<Vec<u8>>,
    /// Anchor (tree root when note was created).
    pub anchor: Option<Vec<u8>>,
    /// Whether this note has been spent.
    pub is_spent: bool,
    /// Transaction ID that spent this note.
    pub spent_in_tx: Option<String>,
    /// Block height where this note was spent.
    pub spent_height: Option<u64>,
    /// Memo field (UTF-8 text from 512-byte memo field).
    pub memo: Option<String>,
    /// Diversifier (11 bytes).
    pub diversifier: Option<Vec<u8>>,
    /// Transaction ID that created this note.
    pub received_txid: Option<String>,
    /// Position in the commitment tree.
    pub position: Option<u64>,
}

/// A transparent UTXO (unspent transaction output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparentUtxo {
    /// Database primary key.
    pub id: i64,
    /// Block height where this UTXO was created.
    pub height: u64,
    /// Transaction ID that created this output.
    pub txid: String,
    /// Output index within the transaction.
    pub output_index: u32,
    /// Raw scriptPubKey bytes.
    pub script_pubkey: Vec<u8>,
    /// Encoded transparent address (t1...).
    pub address: String,
    /// Value in zatoshis.
    pub value: u64,
    /// Whether this is a change output (internal chain).
    pub is_change: bool,
    /// BIP-44 child index used to derive the address.
    pub child_index: u32,
    /// Whether this UTXO belongs to an imported (WIF) key rather than a derived key.
    pub is_imported: bool,
}

/// Transaction type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxType {
    /// Sent transaction (alpha).
    Sent,
    /// Received transaction (beta).
    Received,
    /// Change output.
    Change,
    /// Send-to-self (user sent to their own address).
    SelfTransfer,
    /// Self-send: shielded → transparent (z→t).
    SelfZ2T,
    /// Self-send: transparent → shielded (t→z).
    SelfT2Z,
}

impl TxType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Received => "received",
            Self::Change => "change",
            Self::SelfTransfer => "self",
            Self::SelfZ2T => "self_z2t",
            Self::SelfT2Z => "self_t2z",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sent" => Some(Self::Sent),
            "received" => Some(Self::Received),
            "change" => Some(Self::Change),
            "self" => Some(Self::SelfTransfer),
            "self_z2t" => Some(Self::SelfZ2T),
            "self_t2z" => Some(Self::SelfT2Z),
            _ => None,
        }
    }
}

/// Transaction status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxStatus {
    /// Broadcast to mempool, awaiting confirmation.
    Pending,
    /// Included in a block.
    Confirmed,
    /// Rejected by peers.
    Rejected,
    /// Phantom — detected as invalid, notes restored.
    Phantom,
}

impl TxStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
            Self::Phantom => "phantom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "confirmed" => Some(Self::Confirmed),
            "rejected" => Some(Self::Rejected),
            "phantom" => Some(Self::Phantom),
            _ => None,
        }
    }
}

/// A transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Database primary key.
    pub id: i64,
    /// Transaction ID (hex string, display format).
    pub txid: String,
    /// Transaction type.
    pub tx_type: TxType,
    /// Amount in zatoshis.
    pub amount: u64,
    /// Fee in zatoshis.
    pub fee: u64,
    /// Destination address (for sent) or source (for received).
    pub address: Option<String>,
    /// Memo text.
    pub memo: Option<String>,
    /// Number of confirmations.
    pub confirmations: u32,
    /// Unix timestamp.
    pub timestamp: Option<u64>,
    /// Transaction status.
    pub status: TxStatus,
    /// Block height (0 = unconfirmed).
    pub height: u64,
}

/// Sync state checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Last scanned block height.
    pub last_scanned_height: u64,
    /// Last fully verified checkpoint height.
    pub verified_checkpoint_height: u64,
    /// Serialized commitment tree state.
    pub tree_state: Option<Vec<u8>>,
    /// Current tree height (number of appended CMUs).
    pub tree_height: u64,
    /// Boost file height used for initial sync.
    pub boost_file_height: u64,
    /// Number of CMUs from boost file.
    pub boost_cmu_count: u64,
    /// Whether delta bundle has been verified.
    pub delta_bundle_verified: bool,
}

/// Delta CMU bundle manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaManifest {
    /// Start height of delta range.
    pub start_height: u64,
    /// End height of delta range.
    pub end_height: u64,
    /// Number of CMUs in the delta bundle.
    pub cmu_count: u64,
    /// Whether the bundle has been verified against blockchain.
    pub verified: bool,
    /// Unix timestamp of last update.
    pub last_updated: u64,
}

/// Block header record (for HeaderStore).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBlockHeader {
    /// Block height.
    pub height: u64,
    /// Block hash (32 bytes).
    pub hash: Vec<u8>,
    /// Previous block hash (32 bytes).
    pub prev_hash: Vec<u8>,
    /// Final Sapling root (32 bytes).
    pub final_sapling_root: Option<Vec<u8>>,
    /// Block timestamp.
    pub timestamp: u64,
    /// Difficulty bits.
    pub bits: u32,
    /// Block version.
    pub version: i32,
}
