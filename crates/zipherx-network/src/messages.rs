//! P2P message types: serialization and deserialization.
//!
//! Each message type corresponds to a Bitcoin-derived P2P command.
//! All multi-byte integers are little-endian unless noted (port = big-endian).

use crate::constants::*;
use crate::types::{InvVector, InvType, NetworkAddress, RejectCode, AddrV2Network};

// ---------------------------------------------------------------------------
// CompactSize (varint) encoding — Bitcoin protocol
// ---------------------------------------------------------------------------

/// Write a CompactSize (varint) value to a buffer.
pub fn write_compact_size(buf: &mut Vec<u8>, value: u64) {
    if value < 0xFD {
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(0xFD);
        buf.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= 0xFFFF_FFFF {
        buf.push(0xFE);
        buf.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        buf.push(0xFF);
        buf.extend_from_slice(&value.to_le_bytes());
    }
}

/// Read a CompactSize (varint) value from data at offset.
/// Returns (value, bytes_consumed) or None if insufficient data.
pub fn read_compact_size(data: &[u8], offset: usize) -> Option<(u64, usize)> {
    if offset >= data.len() {
        return None;
    }
    let first = data[offset];
    if first < 0xFD {
        Some((first as u64, 1))
    } else if first == 0xFD {
        if offset + 3 > data.len() { return None; }
        let val = u16::from_le_bytes([data[offset + 1], data[offset + 2]]);
        Some((val as u64, 3))
    } else if first == 0xFE {
        if offset + 5 > data.len() { return None; }
        let val = u32::from_le_bytes([
            data[offset + 1], data[offset + 2],
            data[offset + 3], data[offset + 4],
        ]);
        Some((val as u64, 5))
    } else {
        if offset + 9 > data.len() { return None; }
        let val = u64::from_le_bytes([
            data[offset + 1], data[offset + 2],
            data[offset + 3], data[offset + 4],
            data[offset + 5], data[offset + 6],
            data[offset + 7], data[offset + 8],
        ]);
        Some((val, 9))
    }
}

/// Write a varint-prefixed string.
pub fn write_var_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_compact_size(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

/// Read a varint-prefixed string from data at offset.
/// Returns (string, bytes_consumed) or None.
///
/// Uses `from_utf8_lossy` intentionally: P2P user-agent strings from real-world
/// nodes occasionally contain non-UTF-8 bytes. Rejecting messages over encoding
/// errors would cause unnecessary peer disconnects. Lossy replacement (U+FFFD)
/// preserves message processing while safely handling malformed strings.
pub fn read_var_string(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let (len, varint_size) = read_compact_size(data, offset)?;
    // NET-004: Guard against u64→usize truncation on 32-bit platforms
    if len > data.len() as u64 { return None; }
    let str_start = offset + varint_size;
    let str_end = str_start + len as usize;
    if str_end > data.len() { return None; }
    let s = String::from_utf8_lossy(&data[str_start..str_end]).to_string();
    Some((s, varint_size + len as usize))
}

// ---------------------------------------------------------------------------
// Version message
// ---------------------------------------------------------------------------

/// P2P version message — initiates handshake.
#[derive(Debug, Clone)]
pub struct VersionMessage {
    /// Protocol version (e.g., 170012).
    pub version: u32,
    /// Services offered by this node.
    pub services: u64,
    /// Unix timestamp.
    pub timestamp: i64,
    /// Recipient network address (26 bytes).
    pub addr_recv: NetworkAddress,
    /// Sender network address (26 bytes).
    pub addr_from: NetworkAddress,
    /// Random nonce for connection dedup.
    pub nonce: u64,
    /// User agent string (e.g., "/MagicBean:2.1.2/").
    pub user_agent: String,
    /// Sender's best block height.
    pub start_height: i32,
    /// Whether to relay transactions.
    pub relay: bool,
}

impl VersionMessage {
    /// Serialize to wire format payload.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(86 + self.user_agent.len());

        // Protocol version (4 bytes LE)
        buf.extend_from_slice(&self.version.to_le_bytes());
        // Services (8 bytes LE)
        buf.extend_from_slice(&self.services.to_le_bytes());
        // Timestamp (8 bytes LE)
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        // Recipient address (26 bytes)
        buf.extend_from_slice(&self.addr_recv.serialize());
        // Sender address (26 bytes)
        buf.extend_from_slice(&self.addr_from.serialize());
        // Nonce (8 bytes LE)
        buf.extend_from_slice(&self.nonce.to_le_bytes());
        // User agent (varint string)
        write_var_string(&mut buf, &self.user_agent);
        // Start height (4 bytes LE)
        buf.extend_from_slice(&self.start_height.to_le_bytes());
        // Relay flag (1 byte)
        buf.push(if self.relay { 0x01 } else { 0x00 });

        buf
    }

    /// Deserialize from wire format payload.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        // Minimum: 4+8+8+26+26+8 = 80 bytes before varint string
        if data.len() < 80 {
            return None;
        }

        let version = u32::from_le_bytes(data[0..4].try_into().ok()?);
        let services = u64::from_le_bytes(data[4..12].try_into().ok()?);
        let timestamp = i64::from_le_bytes(data[12..20].try_into().ok()?);
        let addr_recv = NetworkAddress::deserialize(data[20..46].try_into().ok()?);
        let addr_from = NetworkAddress::deserialize(data[46..72].try_into().ok()?);
        let nonce = u64::from_le_bytes(data[72..80].try_into().ok()?);

        // User agent (varint string)
        let (user_agent, ua_size) = read_var_string(data, 80)?;
        let offset = 80 + ua_size;

        // Start height (4 bytes)
        if offset + 4 > data.len() { return None; }
        let start_height = i32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);

        // Relay flag (1 byte, optional — defaults to true)
        let relay = if offset + 5 <= data.len() {
            data[offset + 4] != 0
        } else {
            true
        };

        Some(Self {
            version,
            services,
            timestamp,
            addr_recv,
            addr_from,
            nonce,
            user_agent,
            start_height,
            relay,
        })
    }
}

// ---------------------------------------------------------------------------
// Inventory messages (inv, getdata)
// ---------------------------------------------------------------------------

/// Serialize inventory vectors to wire format.
pub fn serialize_inv(items: &[InvVector]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + items.len() * 36);
    write_compact_size(&mut buf, items.len() as u64);
    for item in items {
        buf.extend_from_slice(&(item.inv_type as u32).to_le_bytes());
        buf.extend_from_slice(&item.hash);
    }
    buf
}

/// Deserialize inventory vectors from wire format.
pub fn deserialize_inv(data: &[u8]) -> Option<Vec<InvVector>> {
    let (count, varint_size) = read_compact_size(data, 0)?;
    // Guard against unbounded allocation from malicious peers
    if count > 50_000 { return None; }
    let count = count as usize;
    let mut offset = varint_size;
    let mut items = Vec::with_capacity(count);

    for _ in 0..count {
        if offset + 36 > data.len() { return None; }
        let type_val = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
        let inv_type = InvType::from_u32(type_val)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&data[offset + 4..offset + 36]);
        items.push(InvVector { inv_type, hash });
        offset += 36;
    }

    Some(items)
}

// ---------------------------------------------------------------------------
// GetHeaders message
// ---------------------------------------------------------------------------

/// GetHeaders request — ask for block headers starting from locator hashes.
#[derive(Debug, Clone)]
pub struct GetHeadersMessage {
    /// Protocol version.
    pub version: u32,
    /// Block locator hashes (newest first, in wire format = reversed).
    pub locator_hashes: Vec<[u8; 32]>,
    /// Stop hash (all zeros = get maximum headers).
    pub stop_hash: [u8; 32],
}

impl GetHeadersMessage {
    /// Serialize to wire format payload.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + 1 + self.locator_hashes.len() * 32 + 32);

        // Protocol version
        buf.extend_from_slice(&self.version.to_le_bytes());
        // Locator hash count
        write_compact_size(&mut buf, self.locator_hashes.len() as u64);
        // Locator hashes
        for hash in &self.locator_hashes {
            buf.extend_from_slice(hash);
        }
        // Stop hash
        buf.extend_from_slice(&self.stop_hash);

        buf
    }

    /// Deserialize from wire format payload.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 4 { return None; }
        let version = u32::from_le_bytes(data[0..4].try_into().ok()?);

        let (count, varint_size) = read_compact_size(data, 4)?;
        // NET-001: Cap locator hash count (Bitcoin Core caps at 101)
        if count > 101 { return None; }
        let count = count as usize;
        let mut offset = 4 + varint_size;
        let mut locator_hashes = Vec::with_capacity(count);

        for _ in 0..count {
            if offset + 32 > data.len() { return None; }
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&data[offset..offset + 32]);
            locator_hashes.push(hash);
            offset += 32;
        }

        if offset + 32 > data.len() { return None; }
        let mut stop_hash = [0u8; 32];
        stop_hash.copy_from_slice(&data[offset..offset + 32]);

        Some(Self { version, locator_hashes, stop_hash })
    }
}

// ---------------------------------------------------------------------------
// Block header (as received in headers response)
// ---------------------------------------------------------------------------

/// Zclassic block header (Sapling-era, Equihash(192,7)).
#[derive(Debug, Clone)]
pub struct BlockHeader {
    /// Block version.
    pub version: i32,
    /// Previous block hash (32 bytes).
    pub prev_hash: [u8; 32],
    /// Merkle root of transactions (32 bytes).
    pub merkle_root: [u8; 32],
    /// Final Sapling root (commitment tree root, 32 bytes).
    pub final_sapling_root: [u8; 32],
    /// Block timestamp (seconds since epoch).
    pub timestamp: u32,
    /// Difficulty bits.
    pub bits: u32,
    /// Equihash nonce (32 bytes).
    pub nonce: [u8; 32],
    /// Equihash solution (400 bytes for post-Bubbles (192,7)).
    pub solution: Vec<u8>,
}

impl BlockHeader {
    /// Base header size before solution: 4+32+32+32+4+4+32 = 140 bytes.
    pub const BASE_SIZE: usize = 140;

    /// Serialize the base header (140 bytes, without solution).
    pub fn serialize_base(&self) -> [u8; 140] {
        let mut buf = [0u8; 140];
        buf[0..4].copy_from_slice(&self.version.to_le_bytes());
        buf[4..36].copy_from_slice(&self.prev_hash);
        buf[36..68].copy_from_slice(&self.merkle_root);
        buf[68..100].copy_from_slice(&self.final_sapling_root);
        buf[100..104].copy_from_slice(&self.timestamp.to_le_bytes());
        buf[104..108].copy_from_slice(&self.bits.to_le_bytes());
        buf[108..140].copy_from_slice(&self.nonce);
        buf
    }

    /// Serialize complete header (base + varint solution length + solution).
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::BASE_SIZE + 3 + self.solution.len());
        buf.extend_from_slice(&self.serialize_base());
        write_compact_size(&mut buf, self.solution.len() as u64);
        buf.extend_from_slice(&self.solution);
        buf
    }

    /// Deserialize from wire format.
    pub fn deserialize(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < Self::BASE_SIZE { return None; }

        let version = i32::from_le_bytes(data[0..4].try_into().ok()?);
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(&data[4..36]);
        let mut merkle_root = [0u8; 32];
        merkle_root.copy_from_slice(&data[36..68]);
        let mut final_sapling_root = [0u8; 32];
        final_sapling_root.copy_from_slice(&data[68..100]);
        let timestamp = u32::from_le_bytes(data[100..104].try_into().ok()?);
        let bits = u32::from_le_bytes(data[104..108].try_into().ok()?);
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&data[108..140]);

        // Solution: varint length + solution bytes
        let (sol_len, varint_size) = read_compact_size(data, Self::BASE_SIZE)?;
        let sol_start = Self::BASE_SIZE + varint_size;
        let sol_end = sol_start + sol_len as usize;
        if sol_end > data.len() { return None; }
        let solution = data[sol_start..sol_end].to_vec();

        let total_consumed = sol_end;

        Some((Self {
            version,
            prev_hash,
            merkle_root,
            final_sapling_root,
            timestamp,
            bits,
            nonce,
            solution,
        }, total_consumed))
    }
}

/// Deserialize a headers response (multiple block headers).
/// Each header in the response is followed by a 1-byte tx_count (always 0).
/// Maximum headers per response (Bitcoin protocol limit is 2000).
const MAX_HEADERS_PER_RESPONSE: u64 = 2000;

pub fn deserialize_headers(data: &[u8]) -> Option<Vec<BlockHeader>> {
    let (count, varint_size) = read_compact_size(data, 0)?;
    // Guard against unbounded allocation from malicious peers
    if count > MAX_HEADERS_PER_RESPONSE { return None; }
    let count = count as usize;
    let mut offset = varint_size;
    let mut headers = Vec::with_capacity(count);

    for _ in 0..count {
        let (header, consumed) = BlockHeader::deserialize(&data[offset..])?;
        offset += consumed;
        // Skip tx_count byte (always 0 in headers message)
        if offset >= data.len() { return None; }
        offset += 1; // tx_count
        headers.push(header);
    }

    Some(headers)
}

// ---------------------------------------------------------------------------
// Ping / Pong
// ---------------------------------------------------------------------------

/// Serialize a ping or pong payload (8-byte nonce).
pub fn serialize_ping(nonce: u64) -> [u8; 8] {
    nonce.to_le_bytes()
}

/// Deserialize a ping or pong payload.
pub fn deserialize_ping(data: &[u8]) -> Option<u64> {
    if data.len() < 8 { return None; }
    Some(u64::from_le_bytes(data[0..8].try_into().ok()?))
}

// ---------------------------------------------------------------------------
// Reject message
// ---------------------------------------------------------------------------

/// Parsed reject message from a peer.
#[derive(Debug, Clone)]
pub struct RejectMessage {
    /// The message type that was rejected (e.g., "tx").
    pub message: String,
    /// Reject reason code.
    pub code: RejectCode,
    /// Human-readable reason.
    pub reason: String,
    /// Extra data (e.g., txid for rejected transactions).
    pub data: Vec<u8>,
}

impl RejectMessage {
    /// Deserialize from wire format payload.
    pub fn deserialize(payload: &[u8]) -> Option<Self> {
        let mut offset = 0;

        // Message type (varint string)
        let (message, msg_size) = read_var_string(payload, offset)?;
        offset += msg_size;

        // Reject code (1 byte)
        if offset >= payload.len() { return None; }
        let code = RejectCode::from_u8(payload[offset])?;
        offset += 1;

        // Reason string (varint string)
        let (reason, reason_size) = read_var_string(payload, offset)?;
        offset += reason_size;

        // Extra data (remaining bytes, often a 32-byte hash)
        let data = if offset < payload.len() {
            payload[offset..].to_vec()
        } else {
            Vec::new()
        };

        Some(Self { message, code, reason, data })
    }
}

// ---------------------------------------------------------------------------
// Addr message (legacy)
// ---------------------------------------------------------------------------

/// Network address with timestamp (from addr message).
#[derive(Debug, Clone)]
pub struct TimestampedAddress {
    /// Time last seen (Unix timestamp).
    pub timestamp: u32,
    /// Network address.
    pub address: NetworkAddress,
}

/// Deserialize an addr message payload.
pub fn deserialize_addr(data: &[u8]) -> Option<Vec<TimestampedAddress>> {
    let (count, varint_size) = read_compact_size(data, 0)?;
    let count = count as usize;
    // Cap at MAX_ADDRESSES_PER_PEER to prevent abuse (NET-005)
    if count > MAX_ADDRESSES_PER_PEER { return None; }

    let mut offset = varint_size;
    let mut addrs = Vec::with_capacity(count);

    for _ in 0..count {
        // timestamp (4) + address (26) = 30 bytes
        if offset + 30 > data.len() { return None; }
        let timestamp = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
        let address = NetworkAddress::deserialize(data[offset + 4..offset + 30].try_into().ok()?);
        addrs.push(TimestampedAddress { timestamp, address });
        offset += 30;
    }

    Some(addrs)
}

// ---------------------------------------------------------------------------
// AddrV2 message (BIP 155)
// ---------------------------------------------------------------------------

/// Network address from addrv2 message.
#[derive(Debug, Clone)]
pub struct AddrV2Entry {
    /// Time last seen.
    pub timestamp: u32,
    /// Services (varint).
    pub services: u64,
    /// Network type.
    pub network: AddrV2Network,
    /// Address bytes (length depends on network type).
    pub address: Vec<u8>,
    /// Port number.
    pub port: u16,
}

/// Deserialize an addrv2 message payload.
pub fn deserialize_addrv2(data: &[u8]) -> Option<Vec<AddrV2Entry>> {
    let (count, varint_size) = read_compact_size(data, 0)?;
    let count = count as usize;
    if count > MAX_ADDRESSES_PER_PEER { return None; }

    let mut offset = varint_size;
    let mut addrs = Vec::with_capacity(count);

    for _ in 0..count {
        // Timestamp (4 bytes)
        if offset + 4 > data.len() { return None; }
        let timestamp = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
        offset += 4;

        // Services (varint)
        let (services, svc_size) = read_compact_size(data, offset)?;
        offset += svc_size;

        // Network ID (1 byte)
        if offset >= data.len() { return None; }
        let network = AddrV2Network::from_u8(data[offset])?;
        offset += 1;

        // Address length (varint) + address bytes
        let (addr_len, addr_varint_size) = read_compact_size(data, offset)?;
        offset += addr_varint_size;
        let addr_end = offset + addr_len as usize;
        if addr_end > data.len() { return None; }
        let address = data[offset..addr_end].to_vec();
        offset = addr_end;

        // Port (2 bytes, big-endian)
        if offset + 2 > data.len() { return None; }
        let port = u16::from_be_bytes([data[offset], data[offset + 1]]);
        offset += 2;

        addrs.push(AddrV2Entry {
            timestamp,
            services,
            network,
            address,
            port,
        });
    }

    Some(addrs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_size_roundtrip() {
        for value in [0u64, 1, 252, 253, 0xFFFE, 0xFFFF, 0x10000, 0xFFFF_FFFF, 0x1_0000_0000] {
            let mut buf = Vec::new();
            write_compact_size(&mut buf, value);
            let (decoded, _) = read_compact_size(&buf, 0).unwrap();
            assert_eq!(decoded, value, "CompactSize roundtrip failed for {value}");
        }
    }

    #[test]
    fn test_var_string_roundtrip() {
        let test_str = "/MagicBean:2.1.2/";
        let mut buf = Vec::new();
        write_var_string(&mut buf, test_str);
        let (decoded, consumed) = read_var_string(&buf, 0).unwrap();
        assert_eq!(decoded, test_str);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn test_version_message_roundtrip() {
        let msg = VersionMessage {
            version: PROTOCOL_VERSION,
            services: SERVICES_NODE_NETWORK,
            timestamp: 1700000000,
            addr_recv: NetworkAddress::empty(),
            addr_from: NetworkAddress::empty(),
            nonce: 0xDEADBEEF,
            user_agent: USER_AGENT.to_string(),
            start_height: 2_951_853,
            relay: true,
        };

        let serialized = msg.serialize();
        let deserialized = VersionMessage::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.version, PROTOCOL_VERSION);
        assert_eq!(deserialized.services, SERVICES_NODE_NETWORK);
        assert_eq!(deserialized.timestamp, 1700000000);
        assert_eq!(deserialized.nonce, 0xDEADBEEF);
        assert_eq!(deserialized.user_agent, USER_AGENT);
        assert_eq!(deserialized.start_height, 2_951_853);
        assert!(deserialized.relay);
    }

    #[test]
    fn test_inv_roundtrip() {
        let items = vec![
            InvVector {
                inv_type: InvType::Tx,
                hash: [0xAA; 32],
            },
            InvVector {
                inv_type: InvType::Block,
                hash: [0xBB; 32],
            },
        ];

        let serialized = serialize_inv(&items);
        let deserialized = deserialize_inv(&serialized).unwrap();

        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized[0].inv_type, InvType::Tx);
        assert_eq!(deserialized[0].hash, [0xAA; 32]);
        assert_eq!(deserialized[1].inv_type, InvType::Block);
        assert_eq!(deserialized[1].hash, [0xBB; 32]);
    }

    #[test]
    fn test_getheaders_roundtrip() {
        let msg = GetHeadersMessage {
            version: PROTOCOL_VERSION,
            locator_hashes: vec![[0x11; 32], [0x22; 32]],
            stop_hash: [0x00; 32],
        };

        let serialized = msg.serialize();
        let deserialized = GetHeadersMessage::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.version, PROTOCOL_VERSION);
        assert_eq!(deserialized.locator_hashes.len(), 2);
        assert_eq!(deserialized.locator_hashes[0], [0x11; 32]);
        assert_eq!(deserialized.stop_hash, [0x00; 32]);
    }

    #[test]
    fn test_ping_pong() {
        let nonce = 0x1234567890ABCDEFu64;
        let data = serialize_ping(nonce);
        let decoded = deserialize_ping(&data).unwrap();
        assert_eq!(decoded, nonce);
    }

    #[test]
    fn test_reject_duplicate_is_success() {
        assert!(RejectCode::Duplicate.is_success());
        assert!(!RejectCode::Invalid.is_success());
        assert!(!RejectCode::Malformed.is_success());
    }

    #[test]
    fn test_network_address_roundtrip() {
        let addr = NetworkAddress {
            services: SERVICES_NODE_NETWORK,
            ip: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 192, 168, 1, 1],
            port: 8233,
        };
        let serialized = addr.serialize();
        let deserialized = NetworkAddress::deserialize(&serialized);
        assert_eq!(deserialized.services, SERVICES_NODE_NETWORK);
        assert_eq!(deserialized.port, 8233);
        assert_eq!(deserialized.ip[12..16], [192, 168, 1, 1]);
    }

    #[test]
    fn test_block_header_serialize_base() {
        let header = BlockHeader {
            version: 4,
            prev_hash: [0x11; 32],
            merkle_root: [0x22; 32],
            final_sapling_root: [0x33; 32],
            timestamp: 1700000000,
            bits: 0x2007FFFF,
            nonce: [0x44; 32],
            solution: vec![0x55; 400],
        };

        let base = header.serialize_base();
        assert_eq!(base.len(), 140);

        let full = header.serialize();
        // 140 + varint(400 = 0xFD 0x90 0x01 = 3 bytes) + 400 = 543
        assert_eq!(full.len(), 543);
    }
}
