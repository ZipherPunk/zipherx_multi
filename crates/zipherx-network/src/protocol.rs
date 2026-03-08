//! P2P message framing: magic bytes, command, length, checksum.
//!
//! Bitcoin-derived message format:
//! [magic (4 bytes)] [command (12 bytes, null-padded)] [length (4 bytes LE)] [checksum (4 bytes)] [payload]

use sha2::{Sha256, Digest};
use crate::constants::*;
use crate::types::ProtocolError;

/// Frame a P2P message with header.
///
/// Returns the complete wire-format message: header + payload.
pub fn frame_message(command: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(MESSAGE_HEADER_SIZE + payload.len());

    // Magic bytes (4)
    message.extend_from_slice(&MAGIC_BYTES);

    // Command (12 bytes, null-padded)
    let mut cmd_bytes = [0u8; COMMAND_SIZE];
    let cmd = command.as_bytes();
    let len = cmd.len().min(COMMAND_SIZE);
    cmd_bytes[..len].copy_from_slice(&cmd[..len]);
    message.extend_from_slice(&cmd_bytes);

    // Payload length (4 bytes, little-endian)
    message.extend_from_slice(&(payload.len() as u32).to_le_bytes());

    // Checksum (first 4 bytes of double-SHA256 of payload)
    let checksum = compute_checksum(payload);
    message.extend_from_slice(&checksum);

    // Payload
    message.extend_from_slice(payload);

    message
}

/// Parse a P2P message header (24 bytes).
///
/// Returns (command, payload_length, checksum) or error.
pub fn parse_header(data: &[u8; MESSAGE_HEADER_SIZE]) -> Result<(String, u32, [u8; 4]), ProtocolError> {
    // Validate magic bytes
    if data[..4] != MAGIC_BYTES {
        return Err(ProtocolError::InvalidMagicBytes {
            expected: MAGIC_BYTES,
            got: [data[0], data[1], data[2], data[3]],
        });
    }

    // Extract command (strip null padding)
    let cmd_bytes = &data[4..16];
    let cmd_end = cmd_bytes.iter().position(|&b| b == 0).unwrap_or(COMMAND_SIZE);
    // RN-9: Validate that command bytes are ASCII-printable or NUL padding.
    // Non-ASCII bytes in a command name indicate a corrupted or malicious message.
    if !cmd_bytes.iter().all(|b| b.is_ascii_graphic() || *b == 0) {
        return Err(ProtocolError::Malformed(
            "Command contains non-ASCII bytes".into(),
        ));
    }
    let command = String::from_utf8_lossy(&cmd_bytes[..cmd_end]).to_string();

    // Payload length (little-endian)
    let length = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

    // Validate length
    if length > MAX_PAYLOAD_SIZE {
        return Err(ProtocolError::PayloadTooLarge {
            size: length,
            max: MAX_PAYLOAD_SIZE,
        });
    }

    // Checksum
    let checksum = [data[20], data[21], data[22], data[23]];

    Ok((command, length, checksum))
}

/// Verify payload checksum.
pub fn verify_checksum(payload: &[u8], expected: &[u8; 4]) -> bool {
    let actual = compute_checksum(payload);
    actual == *expected
}

/// Compute double-SHA256 checksum (first 4 bytes).
pub fn compute_checksum(data: &[u8]) -> [u8; 4] {
    let hash1 = Sha256::digest(data);
    let hash2 = Sha256::digest(hash1);
    [hash2[0], hash2[1], hash2[2], hash2[3]]
}

/// Scan a byte stream for the next valid magic byte sequence.
/// Returns the offset where magic bytes start, or None.
pub fn scan_for_magic(data: &[u8]) -> Option<usize> {
    if data.len() < 4 {
        return None;
    }
    for i in 0..=(data.len() - 4) {
        if data[i..i + 4] == MAGIC_BYTES {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_and_parse_roundtrip() {
        let payload = b"hello world";
        let message = frame_message("version", payload);

        // Parse header
        let header: [u8; MESSAGE_HEADER_SIZE] = message[..MESSAGE_HEADER_SIZE].try_into().unwrap();
        let (command, length, checksum) = parse_header(&header).unwrap();

        assert_eq!(command, "version");
        assert_eq!(length as usize, payload.len());
        assert!(verify_checksum(payload, &checksum));
    }

    #[test]
    fn test_empty_payload() {
        let message = frame_message("verack", &[]);
        let header: [u8; MESSAGE_HEADER_SIZE] = message[..MESSAGE_HEADER_SIZE].try_into().unwrap();
        let (command, length, checksum) = parse_header(&header).unwrap();

        assert_eq!(command, "verack");
        assert_eq!(length, 0);
        assert!(verify_checksum(&[], &checksum));
    }

    #[test]
    fn test_invalid_magic_bytes() {
        let mut header = [0u8; MESSAGE_HEADER_SIZE];
        header[0..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        let result = parse_header(&header);
        assert!(matches!(result, Err(ProtocolError::InvalidMagicBytes { .. })));
    }

    #[test]
    fn test_command_null_padding() {
        let message = frame_message("tx", &[1, 2, 3]);
        // Command should be "tx" followed by 10 null bytes
        assert_eq!(message[4], b't');
        assert_eq!(message[5], b'x');
        assert_eq!(message[6], 0);
        assert_eq!(message[15], 0);
    }

    #[test]
    fn test_scan_for_magic() {
        let mut data = vec![0xFF, 0xFF]; // garbage
        data.extend_from_slice(&MAGIC_BYTES);
        data.extend_from_slice(&[0x00; 20]); // rest of header

        assert_eq!(scan_for_magic(&data), Some(2));
    }

    #[test]
    fn test_scan_for_magic_not_found() {
        let data = vec![0xFF; 100];
        assert_eq!(scan_for_magic(&data), None);
    }

    #[test]
    fn test_payload_too_large() {
        let mut header = [0u8; MESSAGE_HEADER_SIZE];
        header[0..4].copy_from_slice(&MAGIC_BYTES);
        // Set length to 5 MB (exceeds MAX_PAYLOAD_SIZE)
        let huge_len = (5 * 1024 * 1024u32).to_le_bytes();
        header[16..20].copy_from_slice(&huge_len);
        let result = parse_header(&header);
        assert!(matches!(result, Err(ProtocolError::PayloadTooLarge { .. })));
    }
}
