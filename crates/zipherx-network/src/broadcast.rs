//! Transaction broadcast and P2P mempool verification.
//!
//! CRITICAL INVARIANTS:
//! - NEVER stop block listeners before broadcast (FIX #1184)
//! - ALWAYS reverse txid bytes for P2P wire format (FIX #1200)
//! - DUPLICATE response = SUCCESS (peer already has TX in mempool)
//! - NEVER verify mined TXs via P2P mempool (FIX #1250) — height > 0 = skip
//! - NEVER confirm TX via P2P getdata — only block scanner (FIX #1259)
//! - Retry on 0/4 accept: reconnect peers, wait 0.5s, retry once (FIX #1261)

use crate::types::RejectCode;

// ============================================================================
// Types
// ============================================================================

/// Result of broadcasting a transaction via P2P.
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Transaction ID (display format — NOT reversed).
    pub txid: String,
    /// Peers that accepted the transaction.
    pub accepted_by: Vec<String>,
    /// Peers that rejected the transaction with a reason.
    pub rejected_by: Vec<(String, String)>,
    /// Peers that already had the transaction (DUPLICATE = success).
    pub duplicate_at: Vec<String>,
    /// Whether the broadcast is considered successful.
    pub success: bool,
}

impl BroadcastResult {
    /// Total peers that accepted (including duplicates).
    pub fn total_accepted(&self) -> usize {
        self.accepted_by.len() + self.duplicate_at.len()
    }

    /// Total peers that were attempted.
    pub fn total_attempted(&self) -> usize {
        self.accepted_by.len() + self.rejected_by.len() + self.duplicate_at.len()
    }
}

/// Result of verifying a transaction via P2P mempool check.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// Whether the TX was found in any peer's mempool.
    pub found_in_mempool: bool,
    /// Peers that confirmed the TX is in their mempool.
    pub confirming_peers: Vec<String>,
    /// Number of attempts made.
    pub attempts: u32,
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Reverse txid bytes for P2P wire format (FIX #1200).
///
/// Display format (hex string) is the reverse of wire format.
/// E.g., display "abcd...1234" → wire [0x34, 0x12, ..., 0xcd, 0xab].
pub fn reverse_txid_for_wire(txid_hex: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(txid_hex).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut reversed = [0u8; 32];
    for i in 0..32 {
        reversed[i] = bytes[31 - i];
    }
    Some(reversed)
}

/// Convert wire-format txid bytes back to display format hex string.
pub fn wire_txid_to_display(wire_txid: &[u8; 32]) -> String {
    let mut display = [0u8; 32];
    for i in 0..32 {
        display[i] = wire_txid[31 - i];
    }
    hex::encode(display)
}

/// Evaluate a reject message to determine if it's actually a success.
///
/// DUPLICATE (0x12) means the TX is already in the peer's mempool,
/// which is a success — the TX was previously accepted.
pub fn is_reject_actually_success(code: u8) -> bool {
    RejectCode::from_u8(code)
        .map(|c| c.is_success())
        .unwrap_or(false)
}

/// Determine if a broadcast result warrants a retry (FIX #1261).
///
/// Retry when 0 out of N peers accepted (possible network transient).
pub fn should_retry_broadcast(result: &BroadcastResult) -> bool {
    result.total_attempted() > 0 && result.total_accepted() == 0
}

/// Check if a transaction height indicates it's already mined (FIX #1250).
///
/// Mined TXs (height > 0) should NOT be verified via P2P mempool —
/// getdata only finds unconfirmed TXs.
pub fn is_mined(tx_height: u64) -> bool {
    tx_height > 0
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_txid_for_wire() {
        let txid = "0100000000000000000000000000000000000000000000000000000000000002";
        let wire = reverse_txid_for_wire(txid).unwrap();
        assert_eq!(wire[0], 0x02);
        assert_eq!(wire[31], 0x01);
    }

    #[test]
    fn test_reverse_txid_roundtrip() {
        let original = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let wire = reverse_txid_for_wire(original).unwrap();
        let display = wire_txid_to_display(&wire);
        assert_eq!(display, original);
    }

    #[test]
    fn test_reverse_txid_invalid() {
        assert!(reverse_txid_for_wire("tooshort").is_none());
        assert!(reverse_txid_for_wire("not_hex_at_all!").is_none());
    }

    #[test]
    fn test_is_reject_actually_success() {
        assert!(is_reject_actually_success(0x12)); // DUPLICATE
        assert!(!is_reject_actually_success(0x10)); // INVALID
        assert!(!is_reject_actually_success(0x01)); // MALFORMED
        assert!(!is_reject_actually_success(0xFF)); // Unknown
    }

    #[test]
    fn test_broadcast_result_counts() {
        let result = BroadcastResult {
            txid: "abc123".into(),
            accepted_by: vec!["peer1".into(), "peer2".into()],
            rejected_by: vec![("peer3".into(), "dust".into())],
            duplicate_at: vec!["peer4".into()],
            success: true,
        };
        assert_eq!(result.total_accepted(), 3); // 2 accepted + 1 duplicate
        assert_eq!(result.total_attempted(), 4);
    }

    #[test]
    fn test_should_retry_broadcast() {
        // 0/4 accepted → retry
        let bad_result = BroadcastResult {
            txid: "abc".into(),
            accepted_by: vec![],
            rejected_by: vec![("p1".into(), "err".into()), ("p2".into(), "err".into())],
            duplicate_at: vec![],
            success: false,
        };
        assert!(should_retry_broadcast(&bad_result));

        // 1/4 accepted → no retry
        let ok_result = BroadcastResult {
            txid: "abc".into(),
            accepted_by: vec!["p1".into()],
            rejected_by: vec![("p2".into(), "err".into())],
            duplicate_at: vec![],
            success: true,
        };
        assert!(!should_retry_broadcast(&ok_result));

        // 0/0 → no retry (no peers attempted)
        let empty_result = BroadcastResult {
            txid: "abc".into(),
            accepted_by: vec![],
            rejected_by: vec![],
            duplicate_at: vec![],
            success: false,
        };
        assert!(!should_retry_broadcast(&empty_result));
    }

    #[test]
    fn test_is_mined() {
        assert!(!is_mined(0)); // Unconfirmed
        assert!(is_mined(1)); // In block 1
        assert!(is_mined(500_000)); // In block 500K
    }

    #[test]
    fn test_verify_result() {
        let result = VerifyResult {
            found_in_mempool: true,
            confirming_peers: vec!["peer1".into(), "peer2".into()],
            attempts: 3,
        };
        assert!(result.found_in_mempool);
        assert_eq!(result.confirming_peers.len(), 2);
    }
}
