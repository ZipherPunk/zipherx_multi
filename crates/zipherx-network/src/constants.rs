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

/// Maximum known addresses to store.
pub const MAX_KNOWN_ADDRESSES: usize = 1000;

/// Maximum addresses accepted per peer per discovery cycle (NET-005).
pub const MAX_ADDRESSES_PER_PEER: usize = 100;

/// Inventory types.
pub const MSG_TX: u32 = 1;
pub const MSG_BLOCK: u32 = 2;
pub const MSG_FILTERED_BLOCK: u32 = 3;

/// RN-5: Known checkpoints — (height, block_hash_hex_le).
/// Headers at these heights MUST match the expected hash.
/// Block hashes are in internal (little-endian) byte order.
/// The genesis hash and subsequent checkpoint hashes anchor the chain
/// and prevent an attacker from feeding an entirely fabricated chain.
///
/// SECURITY WARNING: 5 of 6 checkpoints have empty hashes, making them
/// ineffective. An attacker who controls all connected peers could feed
/// a fabricated chain that only needs to match the genesis hash. Until
/// real hashes are populated, header sync relies primarily on Equihash
/// PoW verification and nBits range checks for chain validity.
///
/// TODO: Replace placeholder hashes with verified mainnet block hashes.
/// Run `zclassic-cli getblockhash <height>` to obtain the real hashes.
pub const CHECKPOINTS: &[(u64, &str)] = &[
    // Genesis block (height 0) — Zclassic genesis
    (0, "0007104ccda289427919efc39dc9e4d499804b7bebc22df55f8b834301571b40"),
    // Height 100,000
    (100_000, ""),  // TODO: populate with `zclassic-cli getblockhash 100000`
    // Height 250,000
    (250_000, ""),  // TODO: populate with `zclassic-cli getblockhash 250000`
    // Sapling activation (height 476969)
    (476_969, ""),  // TODO: populate with `zclassic-cli getblockhash 476969`
    // Height 600,000
    (600_000, ""),  // TODO: populate with `zclassic-cli getblockhash 600000`
    // Buttercup activation (height 707000)
    (707_000, ""),  // TODO: populate with `zclassic-cli getblockhash 707000`
];

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
