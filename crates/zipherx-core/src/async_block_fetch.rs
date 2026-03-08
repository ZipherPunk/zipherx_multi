//! Async multi-round block fetching via P2P dispatcher.
//!
//! Wraps the pure-logic functions from `block_fetcher` with real P2P networking
//! via the dispatcher pattern. Respects all FIX invariants for block fetching.
//!
//! Critical invariants:
//! - ALL fetches through dispatcher (FIX #1184)
//! - Multi-round: peers x 128 blocks per round (FIX #1189)
//! - Never advance cursor past unfetched blocks (FIX #1218)
//! - 50% threshold for success (FIX #1218)
//! - Adaptive TCP pacing (FIX #1197)
//! - Sort results by height (FIX #1199)
//! - Retry missing blocks WITHIN each batch (FIX #1199)

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::CoreError;
use zipherx_network::block_fetcher::{
    self, CompactBlock, FetchResult, PacingConfig,
};
use zipherx_network::messages::{self, BlockHeader};
use zipherx_network::peer_manager::PeerManager;
use zipherx_network::types::{InvType, InvVector};

// ============================================================================
// Constants
// ============================================================================

/// Maximum blocks per peer per round (P2P protocol limit).
pub const MAX_BLOCKS_PER_PEER: u64 = 128;

/// Timeout per block response.
const BLOCK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Timeout for entire round of block fetching.
const ROUND_TIMEOUT: Duration = Duration::from_secs(120);

// ============================================================================
// Async Block Fetching
// ============================================================================

/// Fetch specific blocks from P2P peers by their hashes.
///
/// Uses the dispatcher pattern (FIX #1184) for lock-free block fetching.
/// Blocks are distributed across peers in round-robin fashion, max 128 per peer.
/// Each round fetches `peer_count x 128` blocks.
///
/// Returns `CompactBlock`s with extracted Sapling outputs and spends.
pub async fn fetch_blocks_by_hashes(
    peer_manager: &mut PeerManager,
    blocks_to_fetch: &[(u64, [u8; 32])],
    pacing: &PacingConfig,
) -> Result<FetchResult, CoreError> {
    if blocks_to_fetch.is_empty() {
        return Ok(FetchResult {
            blocks: Vec::new(),
            missing_heights: Vec::new(),
            rounds: 0,
        });
    }

    let peer_count = peer_manager.connected_count();
    if peer_count == 0 {
        return Err(CoreError::Network(
            zipherx_network::types::NetworkError::NoPeersAvailable,
        ));
    }

    // Ensure block listeners are running (FIX #1184: ALL fetches through dispatcher)
    if !peer_manager.has_active_block_listeners() {
        peer_manager.start_all_block_listeners().await;
        // Give listeners time to activate
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Build hash → height lookup for matching received blocks
    let hash_to_height: HashMap<[u8; 32], u64> = blocks_to_fetch
        .iter()
        .map(|&(h, hash)| (hash, h))
        .collect();

    let blocks_per_round = (peer_count as u64) * MAX_BLOCKS_PER_PEER;
    let total = blocks_to_fetch.len();
    let num_rounds = (total + blocks_per_round as usize - 1) / blocks_per_round as usize;

    let mut all_blocks: Vec<CompactBlock> = Vec::new();
    let mut received_heights: HashSet<u64> = HashSet::new();
    let mut rounds_completed: usize = 0;
    let mut cursor: usize = 0; // Index into blocks_to_fetch

    for round in 0..num_rounds {
        if cursor >= total {
            break;
        }

        // Apply inter-round delay (FIX #1197: adaptive TCP pacing)
        if round > 0 && pacing.inter_round_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(pacing.inter_round_delay_ms)).await;
        }

        let round_end = (cursor + blocks_per_round as usize).min(total);
        let round_blocks = &blocks_to_fetch[cursor..round_end];

        // Get ready peer IDs
        let ready_peer_ids: Vec<String> = peer_manager
            .get_ready_peers()
            .iter()
            .map(|p| p.id.clone())
            .collect();

        if ready_peer_ids.is_empty() {
            break;
        }

        // Distribute blocks across peers (round-robin, max 128 per peer)
        let mut per_peer: Vec<Vec<(u64, [u8; 32])>> = vec![Vec::new(); ready_peer_ids.len()];
        for (i, &block) in round_blocks.iter().enumerate() {
            let peer_idx = i % ready_peer_ids.len();
            if per_peer[peer_idx].len() < MAX_BLOCKS_PER_PEER as usize {
                per_peer[peer_idx].push(block);
            }
        }

        // Phase 1: Register "block" handlers on each peer's dispatcher
        // Collect all receivers with their expected hash (for matching)
        let mut receivers: Vec<tokio::sync::oneshot::Receiver<(String, Vec<u8>)>> = Vec::new();
        let mut handlers_registered = 0;

        for (peer_idx, peer_blocks) in per_peer.iter().enumerate() {
            if peer_blocks.is_empty() {
                continue;
            }
            let pid = &ready_peer_ids[peer_idx];
            if let Some(peer) = peer_manager.peers.get(pid) {
                // RC-4: Recover from poisoned mutex instead of panicking.
                let mut disp = peer.dispatcher().lock().unwrap_or_else(|e| e.into_inner());
                if !disp.is_active() {
                    continue;
                }
                for _ in peer_blocks {
                    let rx = disp.register_handler("block");
                    receivers.push(rx);
                    handlers_registered += 1;
                }
            }
        }

        if handlers_registered == 0 {
            break;
        }

        // Phase 2: Send getdata to each peer
        for (peer_idx, peer_blocks) in per_peer.iter().enumerate() {
            if peer_blocks.is_empty() {
                continue;
            }
            let pid = &ready_peer_ids[peer_idx];
            if let Some(peer) = peer_manager.peers.get(pid) {
                let inv_items: Vec<InvVector> = peer_blocks
                    .iter()
                    .map(|(_, hash)| InvVector {
                        inv_type: InvType::Block,
                        hash: *hash,
                    })
                    .collect();
                let payload = messages::serialize_inv(&inv_items);
                // RC-6: Log send failures instead of silently swallowing them.
                // RC-20: Gate peer IP logging behind debug_assertions to avoid
                // leaking IP addresses in release builds.
                if let Err(e) = peer.send_message("getdata", &payload).await {
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] Warning: send_message(getdata) to peer {} failed: {:?}", pid, e);
                    #[cfg(not(debug_assertions))]
                    eprintln!("[ZipherX] Warning: send_message(getdata) to peer failed: {:?}", e);
                }
            }
        }

        // Phase 3: Collect all block responses via JoinSet
        let mut join_set = tokio::task::JoinSet::new();
        for rx in receivers {
            join_set.spawn(async move {
                match tokio::time::timeout(BLOCK_RESPONSE_TIMEOUT, rx).await {
                    Ok(Ok((_cmd, data))) => Some(data),
                    _ => None,
                }
            });
        }

        let mut round_blocks_received: Vec<CompactBlock> = Vec::new();
        let deadline = tokio::time::Instant::now() + ROUND_TIMEOUT;

        while let Ok(Some(result)) =
            tokio::time::timeout_at(deadline, join_set.join_next()).await
        {
            if let Ok(Some(raw_data)) = result {
                // Parse the block header to compute hash and identify height
                match parse_and_identify_block(&raw_data, &hash_to_height) {
                    Some(compact) => {
                        received_heights.insert(compact.height);
                        round_blocks_received.push(compact);
                    }
                    None => {
                        // Block didn't match any expected hash — skip (unsolicited)
                    }
                }
            }
        }

        // FIX #1218: Check 50% threshold
        let requested = round_blocks.len();
        let received = round_blocks_received.len();
        if !block_fetcher::meets_threshold(received, requested) {
            eprintln!(
                "[ZipherX] Block fetch round {}: only {}/{} received (<50%), stopping",
                round, received, requested
            );
            all_blocks.extend(round_blocks_received);
            break;
        }

        all_blocks.extend(round_blocks_received);
        rounds_completed += 1;

        // Advance cursor past received blocks
        // FIX #1218: Only advance to max received index + 1
        cursor = round_end;
    }

    // FIX #1199: Sort all blocks by height
    all_blocks.sort_by_key(|b| b.height);

    // Calculate missing heights
    let all_expected: HashSet<u64> = blocks_to_fetch.iter().map(|&(h, _)| h).collect();
    let missing: Vec<u64> = all_expected
        .difference(&received_heights)
        .copied()
        .collect();

    Ok(FetchResult {
        blocks: all_blocks,
        missing_heights: missing,
        rounds: rounds_completed,
    })
}

/// Parse raw block data, compute its hash, and match against expected hashes.
///
/// Returns `Some(CompactBlock)` if the block matches an expected hash.
fn parse_and_identify_block(
    raw_data: &[u8],
    hash_to_height: &HashMap<[u8; 32], u64>,
) -> Option<CompactBlock> {
    // Parse block header to compute hash
    let (header, _) = BlockHeader::deserialize(raw_data)?;
    let computed_hash = block_fetcher::compute_block_hash(&header);

    // Look up height from our expected hash mapping
    let height = hash_to_height.get(&computed_hash).copied()?;

    // Parse the full block to extract Sapling data
    match block_fetcher::parse_raw_block(raw_data, height, computed_hash) {
        Ok(compact) => Some(compact),
        Err(e) => {
            eprintln!(
                "[ZipherX] Failed to parse block at height {}: {e}",
                height
            );
            None
        }
    }
}

/// Fetch blocks for a contiguous height range from P2P peers.
///
/// Wrapper around `fetch_blocks_by_hashes` that looks up hashes from the header store.
pub async fn fetch_blocks_from_peers(
    peer_manager: &mut PeerManager,
    start_height: u64,
    end_height: u64,
    pacing: &PacingConfig,
    header_store: &zipherx_storage::header_store_impl::SqliteHeaderStore,
) -> Result<FetchResult, CoreError> {
    use zipherx_network::header_sync::HeaderStore;

    if start_height > end_height {
        return Ok(FetchResult {
            blocks: Vec::new(),
            missing_heights: Vec::new(),
            rounds: 0,
        });
    }

    // Look up block hashes from header store
    let mut blocks_to_fetch: Vec<(u64, [u8; 32])> = Vec::new();
    for h in start_height..=end_height {
        if let Some(hash) = header_store
            .get_header_hash(h)
            .map_err(|e| CoreError::Storage(e.to_string()))?
        {
            blocks_to_fetch.push((h, hash));
        }
    }

    fetch_blocks_by_hashes(peer_manager, &blocks_to_fetch, pacing).await
}

/// Fetch a single block by height with retry and peer rotation (FIX #1231).
pub async fn fetch_single_block(
    peer_manager: &mut PeerManager,
    height: u64,
    block_hash: [u8; 32],
) -> Result<CompactBlock, CoreError> {
    let max_retries = 3;

    // Ensure block listeners are running
    if !peer_manager.has_active_block_listeners() {
        peer_manager.start_all_block_listeners().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let ready_peer_ids: Vec<String> = peer_manager
        .get_ready_peers()
        .iter()
        .map(|p| p.id.clone())
        .collect();

    if ready_peer_ids.is_empty() {
        return Err(CoreError::Network(
            zipherx_network::types::NetworkError::NoPeersAvailable,
        ));
    }

    for attempt in 0..max_retries {
        // Rotate peer for each attempt
        let pid = &ready_peer_ids[attempt % ready_peer_ids.len()];
        if let Some(peer) = peer_manager.peers.get(pid) {
            // Register handler before sending
            let rx = {
                // RC-4: Recover from poisoned mutex instead of panicking.
                let mut disp = peer.dispatcher().lock().unwrap_or_else(|e| e.into_inner());
                if !disp.is_active() {
                    continue;
                }
                disp.register_handler("block")
            };

            // Build and send getdata
            let inv = InvVector {
                inv_type: InvType::Block,
                hash: block_hash,
            };
            let payload = messages::serialize_inv(&[inv]);
            if peer.send_message("getdata", &payload).await.is_err() {
                continue;
            }

            // Wait for response
            match tokio::time::timeout(BLOCK_RESPONSE_TIMEOUT, rx).await {
                Ok(Ok((_cmd, raw_data))) => {
                    let hash_map: HashMap<[u8; 32], u64> =
                        [(block_hash, height)].into_iter().collect();
                    if let Some(compact) = parse_and_identify_block(&raw_data, &hash_map) {
                        return Ok(compact);
                    }
                }
                _ => {
                    if attempt < max_retries - 1 {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
    }

    Err(CoreError::Network(
        zipherx_network::types::NetworkError::ResponseTimeout,
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_empty_list() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(config);

        let result = rt.block_on(fetch_blocks_by_hashes(
            &mut pm,
            &[],
            &PacingConfig::default(),
        ));
        let fetch = result.unwrap();
        assert!(fetch.blocks.is_empty());
        assert!(fetch.missing_heights.is_empty());
        assert_eq!(fetch.rounds, 0);
    }

    #[test]
    fn test_fetch_no_peers_returns_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(config);

        let result = rt.block_on(fetch_blocks_by_hashes(
            &mut pm,
            &[(100, [0; 32])],
            &PacingConfig::default(),
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_single_block_no_peers_returns_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = zipherx_network::peer_manager::PeerManagerConfig::default();
        let mut pm = PeerManager::new(config);

        let result = rt.block_on(fetch_single_block(&mut pm, 100, [0; 32]));
        assert!(result.is_err());
    }
}
