//! Zclassic P2P protocol constants.

/// Zclassic mainnet magic bytes (identifies network packets).
pub const MAGIC_BYTES: [u8; 4] = [0x24, 0xE9, 0x27, 0x64];

/// Protocol version (Bubbles + BIP155 addrv2 support).
pub const PROTOCOL_VERSION: u32 = 170012;

/// Minimum supported peer protocol version (Overwinter+).
pub const MIN_PEER_PROTOCOL_VERSION: u32 = 170002;

/// Maximum standard Zclassic protocol version.
pub const MAX_ZCLASSIC_PROTOCOL_VERSION: u32 = 170012;

/// Zclassic v2.x.x protocol version range.
pub const ZCLASSIC_V2_MIN_VERSION: u32 = 170100;
pub const ZCLASSIC_V2_MAX_VERSION: u32 = 170199;

/// Sapling activation height for Zclassic.
pub const SAPLING_ACTIVATION_HEIGHT: u64 = 476_969;

/// Buttercup activation height (branch ID upgrade).
pub const BUTTERCUP_ACTIVATION_HEIGHT: u64 = 707_000;

/// Default P2P port for Zclassic mainnet.
pub const DEFAULT_PORT: u16 = 8033;

/// Maximum blocks per getdata request (P2P protocol limit).
pub const MAX_BLOCKS_PER_REQUEST: usize = 128;

/// Maximum headers per getheaders response.
pub const MAX_HEADERS_PER_RESPONSE: usize = 160;

/// Minimum peers for consensus.
/// ZCL mainnet has very few active nodes (~4-6). A threshold of 5
/// causes connection failures when fewer nodes are reachable.
/// 3 peers is sufficient given median consensus height + Equihash PoW.
/// WARNING: With only 3 peers, a Sybil attacker controlling 2 peers
/// can influence the median. This is an accepted tradeoff for ZCL's
/// limited network topology. Consider increasing to 5 when network grows.
pub const CONSENSUS_THRESHOLD: usize = 3;

/// P2P message header size: magic(4) + command(12) + length(4) + checksum(4).
pub const MESSAGE_HEADER_SIZE: usize = 24;

/// Maximum P2P message payload size (4 MB).
pub const MAX_PAYLOAD_SIZE: u32 = 4 * 1024 * 1024;

/// Command name size in P2P header (null-padded).
pub const COMMAND_SIZE: usize = 12;

/// Services: NODE_NETWORK.
pub const SERVICES_NODE_NETWORK: u64 = 1;

/// User agent string (mimics full node to prevent fingerprinting).
pub const USER_AGENT: &str = "/MagicBean:2.1.2/";

/// Ping interval (2 minutes, matches Zcash PING_INTERVAL).
pub const PING_INTERVAL_SECS: u64 = 120;

/// Timeout interval (20 minutes, matches Zcash TIMEOUT_INTERVAL).
pub const TIMEOUT_INTERVAL_SECS: u64 = 1200;

/// Activity grace period before considering peer for timeout check.
pub const ACTIVITY_GRACE_PERIOD_SECS: u64 = 90;

/// Default fee in zatoshis.
pub const DEFAULT_FEE: u64 = 10_000;

/// RN-N4: Maximum messages allowed per peer per minute.
/// Peers exceeding this rate are disconnected to prevent flooding.
pub const MAX_MESSAGES_PER_MINUTE: u32 = 1000;

/// Maximum known addresses to store.
pub const MAX_KNOWN_ADDRESSES: usize = 1000;

/// Maximum addresses accepted per peer per discovery cycle (NET-005).
pub const MAX_ADDRESSES_PER_PEER: usize = 100;

/// Inventory types.
pub const MSG_TX: u32 = 1;
pub const MSG_BLOCK: u32 = 2;
pub const MSG_FILTERED_BLOCK: u32 = 3;

/// RN-5: Known checkpoints — (height, block_hash_hex).
/// Headers at these heights MUST match the expected hash.
///
/// Block hashes are in INTERNAL byte order (raw double-SHA256 output),
/// matching what `compute_block_hash()` returns. This is the REVERSE of
/// the display order shown by `getblockhash` RPC.
///
/// Note: genesis (height 0) is NOT included because `getheaders` with a
/// null locator returns headers starting from block 1, not genesis.
/// The genesis block is implicitly trusted by all nodes.
///
/// All hashes verified against a trusted Zclassic full node.
pub const CHECKPOINTS: &[(u64, &str)] = &[
    // Height 100,000
    (
        100_000,
        "4fe7e3180d63f55edbdf869ca35efdeaf9de8026a59a0745f9a9456801000000",
    ),
    // Height 250,000
    (
        250_000,
        "610b0b6500819a1d70159748b606903c49b343850869b74d4b18581e00000000",
    ),
    // Sapling activation (height 476,969)
    (
        476_969,
        "d3532b8a5603c8674654ba38297bcc9d8c4ef6f0f0c042c1fb5e244800000000",
    ),
    // Height 600,000
    (
        600_000,
        "d9a63208481288fb7bf1c3504c9215028932715d6876676f9cac86725d000000",
    ),
    // Buttercup activation (height 707,000)
    (
        707_000,
        "ab3d6b020e1fcfaf6bb6331e6673acad204638db1734dd3fcd2c9fb3ccf40000",
    ),
];

/// Returns true if checkpoint validation is incomplete (empty hashes present).
/// Header sync should log a warning when this returns true.
pub fn has_unpopulated_checkpoints() -> bool {
    CHECKPOINTS.iter().any(|(_, hash)| hash.is_empty())
}

/// DNS seeds for peer discovery.
///
/// RN-N5: Fallback strategy when DNS seeds are unreachable:
/// 1. HARDCODED_SEEDS in peer_manager.rs provides IP-based fallback
/// 2. Tor mode skips DNS entirely (uses hardcoded seeds + P2P addr discovery)
/// 3. P2P addr/addrv2 messages provide ongoing peer discovery from connected peers
/// 4. Previously discovered peers are cached in known_addresses for reconnection
pub const DNS_SEEDS: &[&str] = &[
    "dnsseed.zclassic.org",
    "dnsseed2.zclassic.org",
    "dnsseed.rotorproject.org",
    "dnsseed.zclnet.net",
];
