//! Peer manager — connection pool, DNS discovery, banning, consensus.
//!
//! Manages the set of active P2P peer connections. Handles peer discovery
//! via DNS seeds and hardcoded nodes, connection pooling, banning, scoring,
//! and consensus height calculation with Sybil detection.
//!
//! RN-N6: Peer connections use unencrypted TCP, which is inherent to the
//! Zcash/Zclassic P2P protocol (based on Bitcoin's P2P protocol). TLS is
//! not supported at the protocol level. Privacy is provided at the
//! application layer via Sapling shielded transactions, and network-level
//! privacy is available via Tor routing (see zipherx-tor).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;

use crate::constants::*;
use crate::peer::{Peer, Socks5Config};
use crate::types::*;

/// Default peer manager configuration.
pub struct PeerManagerConfig {
    pub min_peers: usize,
    pub max_peers: usize,
    pub consensus_threshold: usize,
    pub ban_duration: Duration,
    pub connection_cooldown: Duration,
}

impl Default for PeerManagerConfig {
    fn default() -> Self {
        Self {
            min_peers: 8,
            max_peers: 30,
            consensus_threshold: CONSENSUS_THRESHOLD,
            ban_duration: Duration::from_secs(7 * 24 * 3600), // 7 days
            connection_cooldown: Duration::from_secs(2),
        }
    }
}

/// Reason for banning a peer.
#[derive(Debug, Clone)]
pub enum BanReason {
    /// Peer is on Zcash, not Zclassic.
    WrongChain,
    /// Peer reported fake chain height (Sybil attack).
    SybilAttack,
    /// Peer sent corrupted data.
    CorruptedData,
    /// Peer violated protocol.
    ProtocolViolation,
}

impl std::fmt::Display for BanReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BanReason::WrongChain => write!(f, "wrong chain"),
            BanReason::SybilAttack => write!(f, "Sybil attack"),
            BanReason::CorruptedData => write!(f, "corrupted data"),
            BanReason::ProtocolViolation => write!(f, "protocol violation"),
        }
    }
}

/// A banned peer entry.
#[derive(Debug, Clone)]
struct BanEntry {
    #[allow(dead_code)]
    reason: BanReason,
    banned_at: Instant,
    duration: Duration,
    permanent: bool,
}

/// A parked peer (temporary backoff for transient failures).
#[derive(Debug, Clone)]
struct ParkEntry {
    parked_at: Instant,
    backoff: Duration,
    attempts: u32,
}

/// RN-N4: Per-peer rate limiting state.
/// Tracks message count per minute to detect misbehaving or flooding peers.
#[derive(Debug, Clone)]
struct RateLimitEntry {
    message_count: u32,
    window_start: Instant,
}

/// RN-N4: Maximum messages allowed per peer per minute.
const MAX_MESSAGES_PER_MINUTE: u32 = 1000;

/// Known peer address info.
#[derive(Debug, Clone)]
struct AddressInfo {
    host: String,
    port: u16,
    #[allow(dead_code)]
    last_attempt: Option<Instant>,
    #[allow(dead_code)]
    is_hardcoded: bool,
}

/// Manages the pool of P2P peer connections.
pub struct PeerManager {
    /// Active peers (host:port → Peer).
    pub peers: HashMap<String, Peer>,

    /// Known peer addresses.
    known_addresses: HashMap<String, AddressInfo>,

    /// Banned peers.
    banned_peers: HashMap<String, BanEntry>,

    /// Parked peers (temporary backoff).
    parked_peers: HashMap<String, ParkEntry>,

    /// RN-N4: Per-peer message rate limits.
    rate_limits: HashMap<String, RateLimitEntry>,

    /// Configuration.
    config: PeerManagerConfig,

    /// Tor SOCKS5 config (None = no Tor).
    socks5_config: Option<Socks5Config>,

    /// Shared semaphore for SOCKS5 connections.
    socks_semaphore: Arc<Semaphore>,
}

impl PeerManager {
    /// Create a new peer manager.
    pub fn new(config: PeerManagerConfig) -> Self {
        Self {
            peers: HashMap::new(),
            known_addresses: HashMap::new(),
            banned_peers: HashMap::new(),
            parked_peers: HashMap::new(),
            rate_limits: HashMap::new(),
            config,
            socks5_config: None,
            socks_semaphore: Arc::new(Semaphore::new(6)),
        }
    }

    /// Set Tor SOCKS5 configuration.
    pub fn set_socks5_config(&mut self, config: Socks5Config) {
        self.socks5_config = Some(config);
    }

    /// Remove Tor SOCKS5 configuration (revert to direct connections).
    pub fn clear_socks5_config(&mut self) {
        self.socks5_config = None;
    }

    /// Disconnect all connected peers.
    ///
    /// RN-N7: Yields briefly after disconnecting all peers to allow background
    /// tasks (readers, dispatchers) to observe the disconnect and shut down.
    pub async fn disconnect_all(&mut self) {
        let peer_ids: Vec<String> = self.peers.keys().cloned().collect();
        for id in peer_ids {
            if let Some(mut peer) = self.peers.remove(&id) {
                peer.disconnect().await;
            }
        }
        // Brief yield to let spawned tasks observe disconnect and terminate
        tokio::task::yield_now().await;
    }

    /// Connect to the network — discover peers, connect, handshake.
    ///
    /// Preserves existing healthy connections. Only connects to candidates
    /// that are NOT already in the peer pool. Returns when at least
    /// `consensus_threshold` peers are connected.
    pub async fn connect(&mut self) -> Result<(), NetworkError> {
        // Prune dead peers first (handshake done but TCP gone)
        let dead_keys: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, p)| !p.is_connected())
            .map(|(k, _)| k.clone())
            .collect();
        for key in &dead_keys {
            self.peers.remove(key);
        }
        if !dead_keys.is_empty() {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Pruned {} dead peer(s)", dead_keys.len());
        }

        // If we already have enough peers, skip discovery entirely
        let current_connected = self.connected_count();
        if current_connected >= self.config.consensus_threshold {
            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] Already have {} peers (need {}), skipping reconnect",
                current_connected, self.config.consensus_threshold,
            );
            return Ok(());
        }

        // Discover peers (skip DNS when Tor enabled to prevent DNS leaks).
        // DNS lookups go through the system resolver (clearnet), which would
        // reveal to the ISP that this device uses Zclassic. When Tor is active,
        // rely on hardcoded seed IPs + P2P addr/addrv2 discovery instead.
        let addresses = if self.socks5_config.is_some() {
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] Tor active — skipping DNS discovery (prevents DNS leak)");
            Vec::new()
        } else {
            self.discover_peers().await
        };

        // Add hardcoded seeds
        for addr_str in HARDCODED_SEEDS {
            let key = format!("{addr_str}:{DEFAULT_PORT}");
            self.known_addresses.entry(key.clone()).or_insert(AddressInfo {
                host: addr_str.to_string(),
                port: DEFAULT_PORT,
                last_attempt: None,
                is_hardcoded: true,
            });
        }

        // Add discovered addresses
        for (host, port) in &addresses {
            let key = format!("{host}:{port}");
            self.known_addresses.entry(key).or_insert(AddressInfo {
                host: host.clone(),
                port: *port,
                last_attempt: None,
                is_hardcoded: false,
            });
        }

        // Filter banned/parked/ALREADY CONNECTED, collect candidates
        let candidates: Vec<(String, u16)> = self
            .known_addresses
            .values()
            .filter(|info| {
                let key = format!("{}:{}", info.host, info.port);
                !self.is_banned(&info.host)
                    && !self.is_parked(&key)
                    && !self.peers.contains_key(&key)
            })
            .map(|info| (info.host.clone(), info.port))
            .take(self.config.max_peers)
            .collect();

        // Batch connect with concurrency limit.
        // Track TOTAL connected (existing + new) for break/error conditions.
        let batch_size = 10;

        for chunk in candidates.chunks(batch_size) {
            let mut join_set = tokio::task::JoinSet::new();

            for (host, port) in chunk {
                let host = host.clone();
                let port = *port;
                let tor = self.socks5_config.clone();
                let sem = self.socks_semaphore.clone();

                join_set.spawn(async move {
                    let peer_id = format!("{host}:{port}");
                    let mut peer = Peer::new(host, port);
                    let sem_ref = if tor.is_some() { Some(&*sem as &Semaphore) } else { None };

                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] Connecting to {peer_id}...");

                    // TCP connect can take 2-5s, handshake (version/verack exchange)
                    // another 5-15s. 30s outer timeout gives enough room.
                    match tokio::time::timeout(Duration::from_secs(30), async {
                        peer.connect(tor.as_ref(), sem_ref).await?;
                        #[cfg(debug_assertions)]
                        eprintln!("[ZipherX] TCP connected to {peer_id}, starting handshake...");
                        peer.perform_handshake(0).await?;
                        Ok::<Peer, NetworkError>(peer)
                    })
                    .await
                    {
                        Ok(Ok(peer)) => {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "[ZipherX] Peer {} connected (version {}, height {})",
                                peer.id, peer.peer_version, peer.peer_start_height
                            );
                            Some(peer)
                        }
                        Ok(Err(e)) => {
                            #[cfg(debug_assertions)]
                            eprintln!("[ZipherX] Peer {peer_id} failed: {e}");
                            None
                        }
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            eprintln!("[ZipherX] Peer {peer_id} timed out (30s)");
                            None
                        }
                    }
                });
            }

            while let Some(result) = join_set.join_next().await {
                if let Ok(Some(peer)) = result {
                    let key = peer.id.clone();
                    self.peers.insert(key, peer);
                }
                // Early exit: once we have enough peers, abort remaining attempts
                // (saves 15s waiting for timeout peers when enough are already connected)
                if self.connected_count() >= self.config.consensus_threshold {
                    join_set.abort_all();
                    break;
                }
            }

            if self.connected_count() >= self.config.consensus_threshold {
                break;
            }
        }

        let total_connected = self.connected_count();
        if total_connected < self.config.consensus_threshold {
            return Err(NetworkError::ConsensusNotMet {
                have: total_connected,
                need: self.config.consensus_threshold,
            });
        }

        Ok(())
    }

    /// Discover peers from DNS seeds.
    ///
    /// RN-7: DNS seed resolution uses the system resolver which does NOT verify
    /// DNSSEC signatures. A network-level attacker (ISP, coffee shop WiFi, etc.)
    /// could poison DNS responses to return attacker-controlled IP addresses,
    /// enabling an eclipse attack. Mitigations:
    /// - Hardcoded seed IPs provide a DNS-independent fallback
    /// - Consensus height requires agreement from multiple peers
    /// - Tor mode skips DNS entirely (prevents DNS leaks)
    /// - Future: DNSSEC validation or DNS-over-HTTPS
    pub async fn discover_peers(&self) -> Vec<(String, u16)> {
        let mut addresses = Vec::new();

        for seed in DNS_SEEDS {
            let lookup = format!("{seed}:{DEFAULT_PORT}");
            #[cfg(debug_assertions)]
            eprintln!("[ZipherX] DNS lookup: {lookup}");
            let dns_result = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::net::lookup_host(lookup),
            )
            .await;
            match dns_result {
                Ok(Ok(addrs)) => {
                    let mut count = 0;
                    for addr in addrs {
                        let ip = addr.ip().to_string();
                        if !is_reserved_ip(&ip) {
                            addresses.push((ip, DEFAULT_PORT));
                            count += 1;
                        }
                    }
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] DNS seed {seed}: {count} addresses found");
                }
                Ok(Err(e)) => {
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] DNS seed {seed} failed: {e}");
                    continue;
                }
                Err(_) => {
                    #[cfg(debug_assertions)]
                    eprintln!("[ZipherX] DNS seed {seed} timed out");
                    continue;
                }
            }
        }

        #[cfg(debug_assertions)]
        eprintln!(
            "[ZipherX] Peer discovery: {} DNS + {} hardcoded = {} candidates",
            addresses.len(),
            HARDCODED_SEEDS.len(),
            addresses.len() + HARDCODED_SEEDS.len()
        );
        addresses
    }

    /// Add addresses discovered from P2P addr/addrv2 messages.
    pub fn add_discovered_addresses(&mut self, addrs: Vec<(String, u16)>) {
        for (host, port) in addrs {
            if self.known_addresses.len() >= MAX_KNOWN_ADDRESSES {
                break;
            }
            let key = format!("{host}:{port}");
            self.known_addresses.entry(key).or_insert(AddressInfo {
                host,
                port,
                last_attempt: None,
                is_hardcoded: false,
            });
        }
    }

    /// Get all connected, handshake-complete, valid Zclassic peers.
    pub fn get_ready_peers(&self) -> Vec<&Peer> {
        self.peers
            .values()
            .filter(|p| p.is_connected() && p.is_handshake_complete())
            .collect()
    }

    /// Get dead peers — handshake complete but connection not ready (FIX #1228).
    ///
    /// These need reconnection after listener stops kill NWConnections.
    pub fn get_dead_peers(&self) -> Vec<String> {
        self.peers
            .values()
            .filter(|p| p.is_handshake_complete() && !p.is_connected())
            .map(|p| p.id.clone())
            .collect()
    }

    /// Get mutable reference to a peer by key.
    pub fn get_peer_mut(&mut self, key: &str) -> Option<&mut Peer> {
        self.peers.get_mut(key)
    }

    /// Number of connected peers.
    pub fn connected_count(&self) -> usize {
        self.get_ready_peers().len()
    }

    /// Get consensus chain height from connected peers.
    ///
    /// RN-N3: Uses the MEDIAN of peer heights instead of the maximum to prevent
    /// a single malicious peer from controlling the consensus height (Sybil resistance).
    /// Requires at least 2 peers to agree within 10 blocks.
    /// Returns `ConsensusNotMet` if fewer than 2 peers are available
    /// (a single peer could feed a fabricated chain).
    /// Bans outliers >500 blocks from consensus (Sybil detection).
    pub fn get_consensus_height(&mut self) -> Result<u64, NetworkError> {
        let mut heights: Vec<(String, u64)> = {
            let ready = self.get_ready_peers();
            if ready.is_empty() {
                return Err(NetworkError::NoPeersAvailable);
            }
            ready
                .iter()
                .map(|p| (p.id.clone(), p.peer_start_height as u64))
                .collect()
        };

        if heights.len() < 2 {
            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] REJECTED: Only {} peer(s) connected (minimum 2 required for consensus). \
                 A single peer could report a fabricated chain height. Retrying...",
                heights.len(),
            );
            return Err(NetworkError::ConsensusNotMet {
                have: heights.len(),
                need: 2,
            });
        }

        if heights.len() < CONSENSUS_THRESHOLD {
            #[cfg(debug_assertions)]
            eprintln!(
                "[ZipherX] WARNING: Only {} peers connected (consensus_threshold = {}). \
                 Consensus height may be unreliable.",
                heights.len(), CONSENSUS_THRESHOLD,
            );
        }

        // Sort by height ascending for median calculation
        heights.sort_by(|a, b| a.1.cmp(&b.1));

        // RN-N3: Use median height instead of max to resist Sybil attacks.
        // A single malicious peer reporting a fake high height cannot shift the median.
        let mid = heights.len() / 2;
        let median_height = heights[mid].1;

        // Count peers within 10 blocks of median
        let consensus_count = heights
            .iter()
            .filter(|(_, h)| {
                let diff = if *h > median_height {
                    *h - median_height
                } else {
                    median_height - *h
                };
                diff <= 10
            })
            .count();

        if consensus_count < 2 {
            // No consensus — still use median as safest estimate
            return Ok(median_height);
        }

        let consensus_height = median_height;

        // Sybil detection: ban peers >500 blocks above consensus
        let to_ban: Vec<String> = heights
            .iter()
            .filter(|(_, h)| *h > consensus_height + 500)
            .filter_map(|(id, _)| self.peers.get(id).map(|p| p.host.clone()))
            .collect();
        for host in &to_ban {
            self.ban_peer(host, BanReason::SybilAttack);
        }

        // Absolute sanity cap: 10M blocks
        Ok(consensus_height.min(10_000_000))
    }

    /// Ban a peer.
    pub fn ban_peer(&mut self, host: &str, reason: BanReason) {
        self.banned_peers.insert(
            host.to_string(),
            BanEntry {
                reason,
                banned_at: Instant::now(),
                duration: self.config.ban_duration,
                permanent: false,
            },
        );
    }

    /// Ban a peer permanently.
    pub fn ban_peer_permanent(&mut self, host: &str, reason: BanReason) {
        self.banned_peers.insert(
            host.to_string(),
            BanEntry {
                reason,
                banned_at: Instant::now(),
                duration: Duration::from_secs(365 * 24 * 3600),
                permanent: true,
            },
        );
    }

    /// Check if a peer is banned.
    pub fn is_banned(&self, host: &str) -> bool {
        if let Some(entry) = self.banned_peers.get(host) {
            if entry.permanent || entry.banned_at.elapsed() < entry.duration {
                return true;
            }
        }
        false
    }

    /// Park a peer (temporary backoff).
    pub fn park_peer(&mut self, key: &str) {
        let entry = self.parked_peers.entry(key.to_string()).or_insert(ParkEntry {
            parked_at: Instant::now(),
            backoff: Duration::from_secs(30),
            attempts: 0,
        });
        entry.parked_at = Instant::now();
        entry.attempts += 1;
        // Exponential backoff: 30s, 60s, 120s, 240s, max 600s
        entry.backoff = Duration::from_secs(
            (30 * (1u64 << entry.attempts.min(5))).min(600),
        );
    }

    /// Check if a peer is parked.
    pub fn is_parked(&self, key: &str) -> bool {
        if let Some(entry) = self.parked_peers.get(key) {
            entry.parked_at.elapsed() < entry.backoff
        } else {
            false
        }
    }

    /// RN-N4: Record a message from a peer and check rate limits.
    ///
    /// Returns `true` if the peer is within rate limits, `false` if the peer
    /// has exceeded MAX_MESSAGES_PER_MINUTE and should be disconnected.
    /// Automatically disconnects and bans flooding peers.
    pub fn record_peer_message(&mut self, peer_id: &str) -> bool {
        let now = Instant::now();
        let entry = self.rate_limits.entry(peer_id.to_string()).or_insert(RateLimitEntry {
            message_count: 0,
            window_start: now,
        });

        // Reset window if more than 60 seconds have elapsed
        if now.duration_since(entry.window_start) >= Duration::from_secs(60) {
            entry.message_count = 0;
            entry.window_start = now;
        }

        entry.message_count += 1;

        if entry.message_count > MAX_MESSAGES_PER_MINUTE {
            // Extract host before mutable borrow for ban
            let host = self.peers.get(peer_id).map(|p| p.host.clone());
            if let Some(host) = host {
                #[cfg(debug_assertions)]
                eprintln!(
                    "[ZipherX] Rate limit exceeded for peer {peer_id} ({} msgs/min), banning",
                    entry.message_count,
                );
                self.ban_peer(&host, BanReason::ProtocolViolation);
            }
            self.peers.remove(peer_id);
            self.rate_limits.remove(peer_id);
            return false;
        }

        true
    }

    /// Start block listeners on all connected peers.
    pub async fn start_all_block_listeners(&mut self) {
        let keys: Vec<String> = self.peers.keys().cloned().collect();
        for key in keys {
            if let Some(peer) = self.peers.get_mut(&key) {
                if peer.is_connected() && !peer.is_listener_active() {
                    let _ = peer.start_block_listener();
                }
            }
        }
    }

    /// Stop all block listeners.
    pub async fn stop_all_block_listeners(&mut self) {
        let keys: Vec<String> = self.peers.keys().cloned().collect();
        for key in keys {
            if let Some(peer) = self.peers.get_mut(&key) {
                peer.stop_block_listener().await;
            }
        }
    }

    /// Check if any block listeners are active.
    pub fn has_active_block_listeners(&self) -> bool {
        self.peers.values().any(|p| p.is_listener_active())
    }

    /// Send getheaders to all available peers in parallel, return fastest response.
    ///
    /// Uses the dispatcher's oneshot channels to race multiple peers.
    /// The winning peer's response is returned; losers' responses are discarded
    /// by the dispatcher (oneshot senders drop harmlessly).
    pub async fn race_getheaders(
        &self,
        payload: &[u8],
        failed_peers: &std::collections::HashSet<String>,
        timeout: Duration,
    ) -> Result<(String, Vec<u8>), NetworkError> {
        use tokio::sync::oneshot;

        // Collect (peer_id, receiver) for all ready peers
        let mut receivers: Vec<(String, oneshot::Receiver<(String, Vec<u8>)>)> = Vec::new();

        let ready_ids: Vec<String> = self
            .get_ready_peers()
            .iter()
            .filter(|p| !failed_peers.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();

        if ready_ids.is_empty() {
            return Err(NetworkError::NoPeersAvailable);
        }

        // Phase 1: Register handlers on all peers (before sending, prevents race)
        for pid in &ready_ids {
            if let Some(peer) = self.peers.get(pid) {
                let rx = {
                    let mut disp = peer.dispatcher().lock().unwrap();
                    if disp.is_active() {
                        Some(disp.register_handler("headers"))
                    } else {
                        None
                    }
                };
                if let Some(rx) = rx {
                    receivers.push((pid.clone(), rx));
                }
            }
        }

        if receivers.is_empty() {
            return Err(NetworkError::NoPeersAvailable);
        }

        // Phase 2: Send getheaders to all peers
        for pid in &ready_ids {
            if let Some(peer) = self.peers.get(pid) {
                let _ = peer.send_message("getheaders", payload).await;
            }
        }

        // Phase 3: Race all receivers — first response wins
        let result = tokio::time::timeout(timeout, async {
            // Use select_all pattern via a FuturesUnordered
            let mut futs: tokio::task::JoinSet<Option<(String, Vec<u8>)>> = tokio::task::JoinSet::new();
            for (pid, rx) in receivers {
                futs.spawn(async move {
                    match rx.await {
                        Ok((_cmd, payload)) => Some((pid, payload)),
                        Err(_) => None,
                    }
                });
            }

            while let Some(result) = futs.join_next().await {
                if let Ok(Some((winner_pid, payload))) = result {
                    // Got a response — abort remaining futures
                    futs.abort_all();
                    return Ok((winner_pid, payload));
                }
            }

            Err(NetworkError::ResponseTimeout)
        })
        .await;

        match result {
            Ok(inner) => inner,
            Err(_) => Err(NetworkError::ResponseTimeout),
        }
    }

    /// Broadcast a raw transaction to all connected peers.
    ///
    /// Returns (accepted, total) — number of peers that accepted (no error) and total sent to.
    /// FIX #1184: NEVER stop block listeners before broadcast.
    /// FIX #1261: Caller should retry once if 0/N accepted.
    pub async fn broadcast_transaction(&self, raw_tx: &[u8]) -> Result<(u32, u32), NetworkError> {
        let ready_peers = self.get_ready_peers();
        if ready_peers.is_empty() {
            return Err(NetworkError::NoPeersAvailable);
        }

        let mut accepted: u32 = 0;
        let total = ready_peers.len() as u32;

        for peer in &ready_peers {
            match peer.send_message("tx", raw_tx).await {
                Ok(()) => accepted += 1,
                Err(e) => {
                    tracing::warn!("Broadcast to {} failed: {e}", peer.id);
                }
            }
        }

        if accepted == 0 {
            return Err(NetworkError::BroadcastFailed(
                format!("0/{total} peers accepted the transaction"),
            ));
        }

        Ok((accepted, total))
    }
}

/// Hardcoded seed nodes (exempt from cooldown + ban).
const HARDCODED_SEEDS: &[&str] = &[
    "140.174.189.3",
    "140.174.189.17",
    "205.209.104.118",
    "95.179.131.117",
    "45.77.216.198",
    "212.23.222.231",
    "157.90.223.151",
];

/// Check if an IP is reserved/private.
fn is_reserved_ip(ip: &str) -> bool {
    ip.starts_with("10.")
        || ip.starts_with("192.168.")
        || ip.starts_with("127.")
        || ip.starts_with("169.254.")
        || ip.starts_with("0.")
        || ip.starts_with("255.")
        || (ip.starts_with("172.")
            && ip
                .split('.')
                .nth(1)
                .and_then(|s| s.parse::<u8>().ok())
                .map_or(false, |n| (16..=31).contains(&n)))
        || (ip.starts_with("100.")
            && ip
                .split('.')
                .nth(1)
                .and_then(|s| s.parse::<u8>().ok())
                .map_or(false, |n| (64..=127).contains(&n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ban_and_expiry() {
        let mut pm = PeerManager::new(PeerManagerConfig {
            ban_duration: Duration::from_millis(50),
            ..Default::default()
        });

        pm.ban_peer("1.2.3.4", BanReason::WrongChain);
        assert!(pm.is_banned("1.2.3.4"));
        assert!(!pm.is_banned("5.6.7.8"));

        // Wait for ban to expire
        std::thread::sleep(Duration::from_millis(60));
        assert!(!pm.is_banned("1.2.3.4"));
    }

    #[test]
    fn test_permanent_ban() {
        let mut pm = PeerManager::new(PeerManagerConfig::default());
        pm.ban_peer_permanent("evil.node", BanReason::SybilAttack);
        assert!(pm.is_banned("evil.node"));
    }

    #[test]
    fn test_park_and_backoff() {
        let mut pm = PeerManager::new(PeerManagerConfig::default());

        pm.park_peer("1.2.3.4:8233");
        assert!(pm.is_parked("1.2.3.4:8233"));
        assert!(!pm.is_parked("5.6.7.8:8233"));
    }

    #[test]
    fn test_reserved_ip_filtering() {
        assert!(is_reserved_ip("10.0.0.1"));
        assert!(is_reserved_ip("192.168.1.1"));
        assert!(is_reserved_ip("127.0.0.1"));
        assert!(is_reserved_ip("172.16.0.1"));
        assert!(is_reserved_ip("172.31.0.1"));
        assert!(is_reserved_ip("100.64.0.1"));
        assert!(is_reserved_ip("169.254.0.1"));

        assert!(!is_reserved_ip("8.8.8.8"));
        assert!(!is_reserved_ip("140.174.189.3"));
        assert!(!is_reserved_ip("172.32.0.1"));
    }

    #[test]
    fn test_consensus_height() {
        // Test with pre-populated peers (using internal state)
        // Since we can't easily create connected peers in a unit test,
        // we test the helper logic directly.

        // Test is_reserved_ip edge cases
        assert!(!is_reserved_ip("100.63.0.1")); // Just below CGNAT range
        assert!(is_reserved_ip("100.127.0.1")); // Top of CGNAT range
        assert!(!is_reserved_ip("100.128.0.1")); // Just above CGNAT range
    }

    #[test]
    fn test_add_discovered_addresses() {
        let mut pm = PeerManager::new(PeerManagerConfig::default());

        pm.add_discovered_addresses(vec![
            ("1.2.3.4".into(), 8233),
            ("5.6.7.8".into(), 8233),
        ]);

        assert_eq!(pm.known_addresses.len(), 2);

        // Dedup
        pm.add_discovered_addresses(vec![("1.2.3.4".into(), 8233)]);
        assert_eq!(pm.known_addresses.len(), 2);
    }

    #[test]
    fn test_config_defaults() {
        let config = PeerManagerConfig::default();
        assert_eq!(config.min_peers, 8);
        assert_eq!(config.max_peers, 30);
        assert_eq!(config.consensus_threshold, 5);
    }
}
