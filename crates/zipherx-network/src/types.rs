//! Network error types, peer info, and protocol enums.

use thiserror::Error;

/// P2P protocol errors.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Invalid magic bytes: expected {expected:02x?}, got {got:02x?}")]
    InvalidMagicBytes {
        expected: [u8; 4],
        got: [u8; 4],
    },

    #[error("Payload too large: {size} bytes (max {max})")]
    PayloadTooLarge {
        size: u32,
        max: u32,
    },

    #[error("Invalid checksum")]
    InvalidChecksum,

    #[error("Invalid command: {0}")]
    InvalidCommand(String),

    #[error("Insufficient data: need {needed} bytes, have {available}")]
    InsufficientData {
        needed: usize,
        available: usize,
    },

    #[error("Malformed message: {0}")]
    Malformed(String),

    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u32),
}

/// Network-level errors (connection, peer management).
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Connection timeout after {0}ms")]
    ConnectionTimeout(u64),

    #[error("Peer disconnected: {0}")]
    PeerDisconnected(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("DNS resolution failed: {0}")]
    DnsResolutionFailed(String),

    #[error("No peers available")]
    NoPeersAvailable,

    #[error("Consensus threshold not met: have {have}, need {need}")]
    ConsensusNotMet {
        have: usize,
        need: usize,
    },

    #[error("Peer banned: {address} (reason: {reason})")]
    PeerBanned {
        address: String,
        reason: String,
    },

    #[error("Broadcast failed: {0}")]
    BroadcastFailed(String),

    #[error("Block fetch failed: received {received}/{expected} blocks")]
    BlockFetchFailed {
        received: usize,
        expected: usize,
    },

    #[error("Header sync failed: {0}")]
    HeaderSyncFailed(String),

    #[error("SOCKS5 proxy error: {0}")]
    Socks5Error(String),

    #[error("Wrong chain: peer {0} is on Zcash, not Zclassic")]
    WrongChain(String),

    #[error("Stream desync: too many resyncs on peer {0}")]
    StreamDesync(String),

    #[error("Dispatcher not active")]
    DispatcherInactive,

    #[error("Response timeout")]
    ResponseTimeout,

    #[error("Peer not connected")]
    NotConnected,
}

/// Peer connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// TCP connection established, no handshake yet.
    Connecting,
    /// Version message sent, awaiting verack.
    HandshakeSent,
    /// Handshake complete, peer is active.
    Connected,
    /// Peer is being disconnected.
    Disconnecting,
    /// Peer has been disconnected.
    Disconnected,
    /// Peer has been banned.
    Banned,
}

/// Peer scoring — tracks reliability.
#[derive(Debug, Clone)]
pub struct PeerScore {
    /// Successful message exchanges.
    pub successes: u32,
    /// Failed or timed-out requests.
    pub failures: u32,
    /// Total bytes received from this peer.
    pub bytes_received: u64,
    /// Average response latency in milliseconds.
    pub avg_latency_ms: u64,
    /// Number of valid blocks received.
    pub blocks_received: u32,
    /// Number of invalid blocks received.
    pub invalid_blocks: u32,
}

impl Default for PeerScore {
    fn default() -> Self {
        Self {
            successes: 0,
            failures: 0,
            bytes_received: 0,
            avg_latency_ms: 0,
            blocks_received: 0,
            invalid_blocks: 0,
        }
    }
}

/// Information about a connected peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer's network address (ip:port).
    pub address: String,
    /// Peer's self-reported protocol version.
    pub protocol_version: u32,
    /// Peer's self-reported user agent.
    pub user_agent: String,
    /// Peer's self-reported best block height.
    pub start_height: u32,
    /// Peer's advertised services (bitfield).
    pub services: u64,
    /// Current connection state.
    pub state: PeerState,
    /// Reliability score.
    pub score: PeerScore,
    /// Unix timestamp of last activity.
    pub last_activity: u64,
    /// Whether this peer relays transactions.
    pub relay: bool,
}

/// Inventory vector types (Bitcoin protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum InvType {
    /// Transaction.
    Tx = 1,
    /// Block.
    Block = 2,
    /// Filtered block (Bloom filter).
    FilteredBlock = 3,
}

impl InvType {
    /// Parse from wire format u32.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Tx),
            2 => Some(Self::Block),
            3 => Some(Self::FilteredBlock),
            _ => None,
        }
    }
}

/// Inventory vector — references a TX or block by type + hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvVector {
    /// Type of object.
    pub inv_type: InvType,
    /// 32-byte hash (block hash or txid).
    pub hash: [u8; 32],
}

/// Reject message reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectCode {
    Malformed = 0x01,
    Invalid = 0x10,
    Obsolete = 0x11,
    Duplicate = 0x12,
    Nonstandard = 0x40,
    Dust = 0x41,
    InsufficientFee = 0x42,
    Checkpoint = 0x43,
}

impl RejectCode {
    /// Parse from wire format byte.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Malformed),
            0x10 => Some(Self::Invalid),
            0x11 => Some(Self::Obsolete),
            0x12 => Some(Self::Duplicate),
            0x40 => Some(Self::Nonstandard),
            0x41 => Some(Self::Dust),
            0x42 => Some(Self::InsufficientFee),
            0x43 => Some(Self::Checkpoint),
            _ => None,
        }
    }

    /// Whether this reject code indicates the TX is already accepted.
    /// DUPLICATE means the TX is already in the peer's mempool = success.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Duplicate)
    }
}

/// Network address in P2P protocol (26 bytes on wire).
#[derive(Debug, Clone)]
pub struct NetworkAddress {
    /// Services bitfield.
    pub services: u64,
    /// IPv6 address (IPv4 mapped as ::ffff:x.x.x.x).
    pub ip: [u8; 16],
    /// Port number (big-endian on wire).
    pub port: u16,
}

impl NetworkAddress {
    /// Create an empty (all-zeros) network address.
    pub fn empty() -> Self {
        Self {
            services: 0,
            ip: [0u8; 16],
            port: 0,
        }
    }

    /// Serialize to 26 bytes (wire format).
    pub fn serialize(&self) -> [u8; 26] {
        let mut buf = [0u8; 26];
        buf[0..8].copy_from_slice(&self.services.to_le_bytes());
        buf[8..24].copy_from_slice(&self.ip);
        buf[24..26].copy_from_slice(&self.port.to_be_bytes());
        buf
    }

    /// Deserialize from 26 bytes.
    pub fn deserialize(data: &[u8; 26]) -> Self {
        let services = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let mut ip = [0u8; 16];
        ip.copy_from_slice(&data[8..24]);
        let port = u16::from_be_bytes([data[24], data[25]]);
        Self { services, ip, port }
    }
}

/// BIP155 address network types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AddrV2Network {
    IPv4 = 0x01,
    IPv6 = 0x02,
    TorV2 = 0x03,
    TorV3 = 0x04,
    I2P = 0x05,
    CJDNS = 0x06,
}

impl AddrV2Network {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::IPv4),
            0x02 => Some(Self::IPv6),
            0x03 => Some(Self::TorV2),
            0x04 => Some(Self::TorV3),
            0x05 => Some(Self::I2P),
            0x06 => Some(Self::CJDNS),
            _ => None,
        }
    }

    /// Expected address length for this network type.
    pub fn address_length(&self) -> usize {
        match self {
            Self::IPv4 => 4,
            Self::IPv6 => 16,
            Self::TorV2 => 10,
            Self::TorV3 => 32,
            Self::I2P => 32,
            Self::CJDNS => 16,
        }
    }
}

/// Result of a ping/pong exchange.
#[derive(Debug, Clone)]
pub struct PingResult {
    /// Round-trip latency in milliseconds.
    pub latency_ms: u64,
    /// The nonce used in the ping.
    pub nonce: u64,
    /// Whether the pong was received (vs timeout).
    pub success: bool,
}
