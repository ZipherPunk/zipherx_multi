//! Header chain sync — sync block headers with Equihash verification.
//!
//! Syncs block headers from the P2P network, verifies chain continuity
//! (each header's prev_hash matches the previous block's hash) and
//! proof-of-work (Equihash(192,7) verification via zipherx-crypto).
//!
//! Uses a trait for storage to decouple from zipherx-storage (Phase 3).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::constants::*;
use crate::messages::{self, GetHeadersMessage};
use crate::peer_manager::PeerManager;
use crate::types::*;

/// Trait for header storage — implemented by zipherx-storage in Phase 3.
///
/// For Phase 2 testing, an in-memory implementation is provided.
pub trait HeaderStore: Send + Sync {
    /// Get the latest stored header height.
    fn get_latest_height(&self) -> Result<Option<u64>, NetworkError>;

    /// Get a stored header by height.
    fn get_header(&self, height: u64) -> Result<Option<StoredHeader>, NetworkError>;

    /// Get a header hash by height.
    fn get_header_hash(&self, height: u64) -> Result<Option<[u8; 32]>, NetworkError>;

    /// Store multiple headers (height, header) in a batch.
    fn store_headers(&self, headers: Vec<(u64, StoredHeader)>) -> Result<(), NetworkError>;

    /// Count headers in a range.
    fn count_headers_in_range(&self, from: u64, to: u64) -> Result<usize, NetworkError>;

    /// Delete all headers above a given height (for recovery from stale data).
    fn truncate_above(&self, height: u64) -> Result<(), NetworkError>;
}

/// A stored block header (minimal fields for sync).
#[derive(Debug, Clone)]
pub struct StoredHeader {
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub final_sapling_root: [u8; 32],
    pub timestamp: u32,
    pub bits: u32,
}

/// Progress update during header sync.
#[derive(Debug, Clone)]
pub struct HeaderSyncProgress {
    pub current_height: u64,
    pub target_height: u64,
    pub headers_stored: u64,
}

/// Header chain sync engine.
pub struct HeaderSync<S: HeaderStore> {
    store: Arc<S>,
}

impl<S: HeaderStore> HeaderSync<S> {
    /// Create a new header sync engine.
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Sync headers from start_height to network tip.
    ///
    /// Steps:
    /// 1. Get consensus height from peer_manager
    /// 2. Stop block listeners (required for direct header requests)
    /// 3. Fetch headers in batches via getheaders/headers exchange
    /// 4. Verify chain continuity + Equihash PoW
    /// 5. Store verified headers
    /// 6. Restart block listeners
    pub async fn sync_headers(
        &self,
        peer_manager: &mut PeerManager,
        start_height: u64,
        max_headers: Option<u64>,
        progress_tx: Option<mpsc::Sender<HeaderSyncProgress>>,
    ) -> Result<u64, NetworkError> {
        // Get consensus height
        let chain_tip = peer_manager.get_consensus_height()?;

        // Determine effective range
        let effective_start = match self.store.get_latest_height()? {
            Some(h) if h >= start_height => h + 1,
            _ => start_height,
        };

        let effective_end = if let Some(max) = max_headers {
            (effective_start + max).min(chain_tip)
        } else {
            chain_tip
        };

        if effective_start > effective_end {
            return Ok(0); // Already synced
        }

        let total_headers = effective_end - effective_start + 1;
        if total_headers == 0 {
            return Ok(0);
        }

        // Ensure block listeners are active — send_and_wait uses the dispatcher
        // which requires an active listener. Do NOT stop listeners here: stopping
        // destroys the TCP reader (consumed by the listener task), and it can never
        // be recovered without a full reconnect (FIX: "No peers available" on 2nd sync).
        peer_manager.start_all_block_listeners().await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Sync headers
        self.sync_headers_simple(
            peer_manager,
            effective_start,
            effective_end,
            progress_tx.as_ref(),
        )
        .await
    }

    /// Batched header sync — single-peer with failover, accumulate in memory,
    /// flush to DB every BATCH_FLUSH_SIZE headers for maximum throughput.
    /// Falls back to next peer on error. Equihash verified every 100th header
    /// during bulk sync, every header in last 1000, and last header of each batch.
    async fn sync_headers_simple(
        &self,
        peer_manager: &mut PeerManager,
        start_height: u64,
        chain_tip: u64,
        progress_tx: Option<&mpsc::Sender<HeaderSyncProgress>>,
    ) -> Result<u64, NetworkError> {
        const BATCH_FLUSH_SIZE: usize = 10_000;

        let mut current_height = start_height;
        let mut headers_stored = 0u64;
        let mut failed_peers = std::collections::HashSet::new();
        let mut chain_discontinuity_retried = false;

        let mut pending_headers: Vec<(u64, StoredHeader)> = Vec::with_capacity(BATCH_FLUSH_SIZE);
        let mut last_known_hash: Option<[u8; 32]> = None;

        // Ensure all peers have listeners active before the tight loop.
        // If listeners died (idle timeout, TCP disconnect), the reader is gone
        // and start_block_listener silently fails. In that case, reconnect.
        {
            let ready = peer_manager.get_ready_peers();
            let peer_ids: Vec<String> = ready.iter().map(|p| p.id.clone()).collect();
            let mut started = 0;
            for pid in &peer_ids {
                if let Some(peer) = peer_manager.get_peer_mut(pid) {
                    if !peer.is_listener_active() {
                        if peer.start_block_listener().is_ok() {
                            started += 1;
                        }
                    } else {
                        started += 1; // Already active
                    }
                }
            }
            if started == 0 && !peer_ids.is_empty() {
                // All listeners dead (readers consumed) — must reconnect.
                // Disconnect zombie peers first so connect() doesn't short-circuit
                // with "already have enough peers" (zombies pass is_connected check).
                #[cfg(debug_assertions)]
                eprintln!(
                    "[ZipherX] Header sync: all {} peer listeners dead, disconnecting zombies and reconnecting...",
                    peer_ids.len(),
                );
                peer_manager.disconnect_all().await;
                match peer_manager.connect().await {
                    Ok(()) => {
                        peer_manager.start_all_block_listeners().await;
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(e) => {
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX] Header sync: reconnect failed: {e}");
                    }
                }
            } else if started > 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }

        // Get available peers — use first available as primary, rotate on failure
        let mut ranked_peers: Vec<String> = {
            let ready = peer_manager.get_ready_peers();
            ready.iter().map(|p| p.id.clone()).collect()
        };

        if ranked_peers.is_empty() {
            return Err(NetworkError::HeaderSyncFailed(
                "No peers available".into(),
            ));
        }
        #[cfg(debug_assertions)]
        eprintln!("[ZipherX] Header sync: {} peers available", ranked_peers.len());

        let start_time = std::time::Instant::now();
        let mut preferred_peer_idx = 0;
        let mut reconnect_attempts = 0u32;
        const MAX_RECONNECT_ATTEMPTS: u32 = 3;

        'sync_loop: while current_height <= chain_tip {
            // Pick best available peer
            let peer_key = match ranked_peers
                .iter()
                .skip(preferred_peer_idx)
                .chain(ranked_peers.iter().take(preferred_peer_idx))
                .find(|p| !failed_peers.contains(p.as_str()))
            {
                Some(p) => p.clone(),
                None => {
                    // All peers exhausted — flush progress and try to reconnect
                    if !pending_headers.is_empty() {
                        self.store.store_headers(std::mem::take(&mut pending_headers))?;
                        pending_headers = Vec::with_capacity(BATCH_FLUSH_SIZE);
                    }

                    if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                        return Err(NetworkError::HeaderSyncFailed(
                            format!(
                                "No peers available for header sync after {} reconnect attempts (synced to {})",
                                reconnect_attempts, current_height
                            ),
                        ));
                    }

                    reconnect_attempts += 1;
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[ZipherX] Header sync: all peers exhausted at height {}, disconnecting zombies and reconnecting (attempt {}/{})",
                        current_height, reconnect_attempts, MAX_RECONNECT_ATTEMPTS,
                    );

                    // Disconnect ALL peers — they've all failed or have dead listeners.
                    // Without this, connect() short-circuits because zombie peers still
                    // pass is_connected() (writer present, state Connected) even though
                    // their listeners are dead and readers consumed.
                    peer_manager.disconnect_all().await;

                    // Reconnect fresh
                    match peer_manager.connect().await {
                        Ok(()) => {
                            peer_manager.start_all_block_listeners().await;
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Err(e) => {
                            #[cfg(debug_assertions)]
                            eprintln!("[ZipherX] Header sync: reconnect failed: {e}");
                            return Err(NetworkError::HeaderSyncFailed(
                                format!(
                                    "Reconnect failed at height {}: {e}",
                                    current_height
                                ),
                            ));
                        }
                    }

                    // Refresh peer list and clear failed set (new peers)
                    ranked_peers = {
                        let ready = peer_manager.get_ready_peers();
                        ready.iter().map(|p| p.id.clone()).collect()
                    };
                    failed_peers.clear();
                    preferred_peer_idx = 0;

                    if ranked_peers.is_empty() {
                        return Err(NetworkError::HeaderSyncFailed(
                            "No peers available after reconnect".into(),
                        ));
                    }

                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[ZipherX] Header sync: reconnected with {} peers, resuming from height {}",
                        ranked_peers.len(), current_height,
                    );
                    continue 'sync_loop;
                }
            };

            // Build locator hash — in-memory fast path avoids DB reads
            let locator_hash = if let Some(hash) = last_known_hash {
                hash
            } else if current_height > 0 {
                match self.store.get_header_hash(current_height - 1)? {
                    Some(hash) => hash,
                    None => [0u8; 32],
                }
            } else {
                [0u8; 32]
            };

            // Also prepare reversed byte order — boost file may store hashes in
            // display order while P2P uses internal order (or vice versa).
            let locator_hash_reversed = {
                let mut rev = locator_hash;
                rev.reverse();
                rev
            };

            // Send both byte orders in the locator — peer will use whichever
            // it recognizes. This handles boost files that store hashes in
            // display order vs P2P internal order.
            let get_headers = GetHeadersMessage {
                version: PROTOCOL_VERSION,
                locator_hashes: if locator_hash == [0u8; 32] {
                    vec![locator_hash]
                } else {
                    vec![locator_hash, locator_hash_reversed]
                },
                stop_hash: [0u8; 32],
            };

            let peer = match peer_manager.get_peer_mut(&peer_key) {
                Some(p) => p,
                None => {
                    failed_peers.insert(peer_key);
                    continue;
                }
            };

            let result = peer
                .send_and_wait(
                    "getheaders",
                    &get_headers.serialize(),
                    "headers",
                    Duration::from_secs(15),
                )
                .await;

            let (_cmd, payload) = match result {
                Ok(r) => r,
                Err(e) => {
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] Header fetch failed from {peer_key}: {e}");
                    failed_peers.insert(peer_key);
                    continue;
                }
            };

            // Deserialize headers
            let headers = match messages::deserialize_headers(&payload) {
                Some(h) if !h.is_empty() => h,
                _ => break, // No headers = at tip
            };

            // Verify and accumulate
            let mut batch_verified = Vec::new();
            let mut prev_hash = locator_hash;
            let mut peer_bad = false;

            for (batch_idx, header) in headers.iter().enumerate() {
                let height = current_height + batch_verified.len() as u64;
                if height > chain_tip {
                    break;
                }

                // Verify chain continuity
                if header.prev_hash != prev_hash && prev_hash != [0u8; 32] {
                    // Try reversed byte order — boost file may store hashes in a
                    // different byte order than P2P wire format.
                    if batch_verified.is_empty() && header.prev_hash == locator_hash_reversed {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[ZipherX] Locator hash byte order mismatch at height {height} — using reversed order"
                        );
                        // Byte-order mismatch noted; fall through to process this header
                    } else if batch_verified.is_empty() && !chain_discontinuity_retried {
                        // Real discontinuity — but NEVER nuke boost headers.
                        // Truncate just the last 100 blocks and retry from there.
                        let truncate_to = current_height.saturating_sub(100);
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[ZipherX] Chain discontinuity at height {height} — truncating to {} and retrying (NOT from 0)",
                            truncate_to
                        );
                        pending_headers.clear();
                        last_known_hash = None;
                        self.store.truncate_above(truncate_to)?;
                        current_height = truncate_to + 1;
                        chain_discontinuity_retried = true;
                        failed_peers.clear();
                        preferred_peer_idx = 0;
                        continue 'sync_loop;
                    } else {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "[ZipherX] Chain discontinuity at height {height} from {peer_key}"
                        );
                        failed_peers.insert(peer_key.clone());
                        peer_bad = true;
                        break;
                    }
                }

                // RN-3: Reject headers with timestamps more than 2 hours in the future.
                // This matches Bitcoin's MAX_FUTURE_BLOCK_TIME and prevents peers from
                // feeding headers with far-future timestamps to manipulate difficulty.
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if (header.timestamp as u64) > now_secs + 7200 {
                    eprintln!(
                        "[ZipherX] Header timestamp {} too far in future at height {height} from {peer_key} \
                         (now={now_secs}, max={})",
                        header.timestamp, now_secs + 7200,
                    );
                    failed_peers.insert(peer_key.clone());
                    peer_bad = true;
                    break;
                }

                // RN-2: Basic nBits range validation to reject trivially fabricated
                // headers. This doesn't implement full LWMA3 difficulty adjustment
                // verification, but prevents headers with impossible difficulty values.
                const NBITS_MIN: u32 = 0x1900_0000; // Highest plausible difficulty
                const NBITS_MAX: u32 = 0x2100_FFFF; // Lowest plausible difficulty
                if header.bits < NBITS_MIN || header.bits > NBITS_MAX {
                    eprintln!(
                        "[ZipherX] Invalid nBits 0x{:08x} at height {height} from {peer_key} \
                         (valid range: 0x{:08x}..0x{:08x})",
                        header.bits, NBITS_MIN, NBITS_MAX,
                    );
                    failed_peers.insert(peer_key.clone());
                    peer_bad = true;
                    break;
                }

                let header_base = header.serialize_base();

                // Equihash verification: every 10th during bulk sync, every header
                // within the last 1000 blocks, and always the last header of each
                // batch (ensures chain continuity can't be broken at batch boundaries).
                // nBits/difficulty is implicitly checked via Equihash verification
                // (the solution must satisfy the difficulty target encoded in the header)
                let remaining = chain_tip.saturating_sub(current_height);
                let is_last_in_batch = batch_idx == headers.len() - 1;
                // RN-1: Verify every 10th header (10% coverage) instead of every
                // 100th (1%). This provides much better PoW verification coverage
                // with only a modest performance cost during bulk sync.
                let should_verify = remaining < 1000 || height % 10 == 0 || is_last_in_batch;

                if should_verify {
                    match zipherx_crypto::equihash::verify(&header_base, &header.solution) {
                        Ok(true) => {}
                        Ok(false) => {
                            eprintln!(
                                "[ZipherX] Equihash failed at height {height} from {peer_key}"
                            );
                            failed_peers.insert(peer_key.clone());
                            peer_bad = true;
                            break;
                        }
                        Err(e) => {
                            eprintln!(
                                "[ZipherX] Equihash error at height {height}: {e}"
                            );
                            failed_peers.insert(peer_key.clone());
                            peer_bad = true;
                            break;
                        }
                    }
                }

                let block_hash =
                    zipherx_crypto::equihash::compute_block_hash(&header_base, &header.solution);

                // RN-5: Checkpoint enforcement — verify that headers at known
                // checkpoint heights match the expected hash. This prevents an
                // attacker from feeding an entirely fabricated chain.
                for &(cp_height, cp_hash_hex) in CHECKPOINTS {
                    if height == cp_height && !cp_hash_hex.is_empty() {
                        if let Ok(expected_bytes) = hex::decode(cp_hash_hex) {
                            if expected_bytes.len() == 32 && block_hash[..] != expected_bytes[..] {
                                eprintln!(
                                    "[ZipherX] CHECKPOINT MISMATCH at height {height} from {peer_key}: \
                                     got {}, expected {cp_hash_hex}",
                                    hex::encode(block_hash),
                                );
                                failed_peers.insert(peer_key.clone());
                                peer_bad = true;
                                break;
                            }
                        }
                    }
                }
                if peer_bad {
                    break;
                }

                batch_verified.push((
                    height,
                    StoredHeader {
                        hash: block_hash,
                        prev_hash: header.prev_hash,
                        final_sapling_root: header.final_sapling_root,
                        timestamp: header.timestamp,
                        bits: header.bits,
                    },
                ));

                prev_hash = block_hash;
            }

            // If peer was bad, try next peer (don't exit the loop!)
            if peer_bad {
                continue 'sync_loop;
            }

            if batch_verified.is_empty() {
                break;
            }

            let count = batch_verified.len() as u64;
            last_known_hash = Some(batch_verified.last().unwrap().1.hash);
            current_height += count;
            headers_stored += count;
            pending_headers.extend(batch_verified);

            if let Some(peer) = peer_manager.get_peer_mut(&peer_key) {
                peer.record_success();
            }

            // Flush to DB when batch is large enough
            if pending_headers.len() >= BATCH_FLUSH_SIZE {
                let flush_count = pending_headers.len();
                self.store.store_headers(std::mem::take(&mut pending_headers))?;
                pending_headers = Vec::with_capacity(BATCH_FLUSH_SIZE);
                eprintln!("[ZipherX] Flushed {flush_count} headers to DB");
            }

            // Report progress every 10k headers
            if headers_stored % 10_000 < count {
                let pct = if chain_tip > start_height {
                    ((current_height - start_height) as f64
                        / (chain_tip - start_height) as f64
                        * 100.0) as u32
                } else {
                    100
                };
                let elapsed = start_time.elapsed().as_secs_f64();
                let rate = if elapsed > 0.0 {
                    headers_stored as f64 / elapsed
                } else {
                    0.0
                };
                eprintln!(
                    "[ZipherX] Header sync: {current_height}/{chain_tip} ({pct}%) — {headers_stored} stored [{rate:.0} hdr/s]"
                );

                if let Some(tx) = progress_tx {
                    let _ = tx
                        .send(HeaderSyncProgress {
                            current_height,
                            target_height: chain_tip,
                            headers_stored,
                        })
                        .await;
                }
            }
        }

        // Final flush
        if !pending_headers.is_empty() {
            let flush_count = pending_headers.len();
            self.store.store_headers(pending_headers)?;
            eprintln!("[ZipherX] Final flush: {flush_count} headers to DB");
        }

        // Final progress report
        if let Some(tx) = progress_tx {
            let _ = tx
                .send(HeaderSyncProgress {
                    current_height,
                    target_height: chain_tip,
                    headers_stored,
                })
                .await;
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 {
            headers_stored as f64 / elapsed
        } else {
            0.0
        };
        eprintln!(
            "[ZipherX] Header sync complete: {headers_stored} headers in {elapsed:.1}s ({rate:.0} hdr/s)"
        );

        Ok(headers_stored)
    }

}

/// In-memory header store for testing.
#[cfg(test)]
pub struct InMemoryHeaderStore {
    headers: std::sync::Mutex<std::collections::BTreeMap<u64, StoredHeader>>,
}

#[cfg(test)]
impl InMemoryHeaderStore {
    pub fn new() -> Self {
        Self {
            headers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

#[cfg(test)]
impl HeaderStore for InMemoryHeaderStore {
    fn get_latest_height(&self) -> Result<Option<u64>, NetworkError> {
        let headers = self.headers.lock().unwrap();
        Ok(headers.keys().last().copied())
    }

    fn get_header(&self, height: u64) -> Result<Option<StoredHeader>, NetworkError> {
        let headers = self.headers.lock().unwrap();
        Ok(headers.get(&height).cloned())
    }

    fn get_header_hash(&self, height: u64) -> Result<Option<[u8; 32]>, NetworkError> {
        let headers = self.headers.lock().unwrap();
        Ok(headers.get(&height).map(|h| h.hash))
    }

    fn store_headers(&self, new_headers: Vec<(u64, StoredHeader)>) -> Result<(), NetworkError> {
        let mut headers = self.headers.lock().unwrap();
        for (height, header) in new_headers {
            headers.insert(height, header);
        }
        Ok(())
    }

    fn count_headers_in_range(&self, from: u64, to: u64) -> Result<usize, NetworkError> {
        let headers = self.headers.lock().unwrap();
        Ok(headers.range(from..=to).count())
    }

    fn truncate_above(&self, height: u64) -> Result<(), NetworkError> {
        let mut headers = self.headers.lock().unwrap();
        let to_remove: Vec<u64> = headers.range((height + 1)..).map(|(k, _)| *k).collect();
        for k in to_remove {
            headers.remove(&k);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store() {
        let store = InMemoryHeaderStore::new();

        assert_eq!(store.get_latest_height().unwrap(), None);

        store
            .store_headers(vec![
                (100, StoredHeader {
                    hash: [1u8; 32],
                    prev_hash: [0u8; 32],
                    final_sapling_root: [0u8; 32],
                    timestamp: 1000,
                    bits: 0x2007ffff,
                }),
                (101, StoredHeader {
                    hash: [2u8; 32],
                    prev_hash: [1u8; 32],
                    final_sapling_root: [0u8; 32],
                    timestamp: 1060,
                    bits: 0x2007ffff,
                }),
            ])
            .unwrap();

        assert_eq!(store.get_latest_height().unwrap(), Some(101));
        assert_eq!(store.get_header_hash(100).unwrap(), Some([1u8; 32]));
        assert_eq!(store.count_headers_in_range(100, 101).unwrap(), 2);
        assert_eq!(store.count_headers_in_range(100, 100).unwrap(), 1);
        assert_eq!(store.count_headers_in_range(200, 300).unwrap(), 0);
    }

    #[test]
    fn test_stored_header_clone() {
        let header = StoredHeader {
            hash: [0xABu8; 32],
            prev_hash: [0xCDu8; 32],
            final_sapling_root: [0xEFu8; 32],
            timestamp: 12345,
            bits: 0x1d00ffff,
        };
        let cloned = header.clone();
        assert_eq!(cloned.hash, header.hash);
        assert_eq!(cloned.timestamp, header.timestamp);
    }

    #[test]
    fn test_progress_struct() {
        let progress = HeaderSyncProgress {
            current_height: 500,
            target_height: 1000,
            headers_stored: 100,
        };
        assert_eq!(progress.current_height, 500);
        assert_eq!(progress.target_height, 1000);
    }
}
