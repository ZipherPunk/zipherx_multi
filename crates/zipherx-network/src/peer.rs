//! P2P peer connection — TCP I/O, handshake, block listener.
//!
//! Replaces Swift's ~4,500 line Peer.swift with ~600 lines of Rust/tokio.
//! Key simplifications:
//! - `tokio::time::timeout()` replaces GCD workarounds
//! - Borrow checker prevents double-reads (no PeerMessageLock needed)
//! - `oneshot::Sender` replaces CheckedContinuation (no crash risk)
//! - `CancellationToken` replaces block listener state machine

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::constants::*;
use crate::dispatcher::Dispatcher;
use crate::messages::VersionMessage;
use crate::protocol;
use crate::socks5;
use crate::types::*;

/// Maximum magic byte resyncs before declaring stream desync.
const MAX_RESYNCS: usize = 5;
/// Maximum bytes to scan for magic bytes during resync.
///
/// RN-8: During resync, bytes between the current read position and the next
/// valid magic byte sequence are discarded. This means any partial or complete
/// P2P messages within those bytes are lost and will never be processed. In
/// practice this is acceptable because: (1) resyncs are rare (only on stream
/// corruption), (2) the lost messages are likely the corrupted ones, and
/// (3) higher-level protocols (header sync, block fetch) have timeout-based
/// retry logic that will re-request any missing data.
const MAX_RESYNC_SCAN: usize = 65536;
/// Block listener idle timeout (no data for this long = peer dead).
const LISTENER_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// Handshake message timeout.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Default send/receive timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// SOCKS5 proxy configuration for Tor routing.
#[derive(Debug, Clone)]
pub struct Socks5Config {
    pub proxy_addr: std::net::SocketAddr,
}

/// A P2P peer connection.
pub struct Peer {
    /// Unique identifier (host:port).
    pub id: String,
    /// Peer hostname or IP.
    pub host: String,
    /// Peer port.
    pub port: u16,

    /// Write half of the TCP stream (shared with block listener for pong).
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    /// Read half (held before block listener starts, then moved).
    reader: Option<BufReader<OwnedReadHalf>>,
    /// Message dispatcher (shared with block listener).
    dispatcher: Arc<Mutex<Dispatcher>>,

    /// Connection state.
    pub state: PeerState,
    /// Handshake results.
    pub peer_version: u32,
    pub peer_user_agent: String,
    pub peer_start_height: u32,
    pub supports_addrv2: bool,

    /// Scoring and health.
    pub score: PeerScore,
    pub consecutive_failures: u32,
    pub last_send: Option<Instant>,
    pub last_recv: Option<Instant>,

    /// Block listener.
    listener_cancel: Option<CancellationToken>,
    listener_handle: Option<JoinHandle<()>>,

    /// Whether connected via Tor.
    pub is_tor: bool,
}

impl Peer {
    /// Create a new peer (not yet connected).
    pub fn new(host: String, port: u16) -> Self {
        let id = format!("{host}:{port}");
        Self {
            id,
            host,
            port,
            writer: Arc::new(Mutex::new(None)),
            reader: None,
            dispatcher: Arc::new(Mutex::new(Dispatcher::new())),
            state: PeerState::Disconnected,
            peer_version: 0,
            peer_user_agent: String::new(),
            peer_start_height: 0,
            supports_addrv2: false,
            score: PeerScore::default(),
            consecutive_failures: 0,
            last_send: None,
            last_recv: None,
            listener_cancel: None,
            listener_handle: None,
            is_tor: false,
        }
    }

    /// Connect to the peer via TCP (or SOCKS5 for Tor).
    pub async fn connect(
        &mut self,
        tor_config: Option<&Socks5Config>,
        socks_semaphore: Option<&Semaphore>,
    ) -> Result<(), NetworkError> {
        if self.state == PeerState::Connected {
            return Ok(());
        }

        self.state = PeerState::Connecting;

        let stream = if let (Some(config), Some(sem)) = (tor_config, socks_semaphore) {
            self.is_tor = true;
            socks5::connect_via_socks5(
                config.proxy_addr,
                &self.host,
                self.port,
                sem,
                Duration::from_secs(20),
            )
            .await?
        } else {
            let addr = format!("{}:{}", self.host, self.port);
            tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(&addr))
                .await
                .map_err(|_| NetworkError::ConnectionTimeout(15000))?
                .map_err(|e| NetworkError::ConnectionFailed(format!("{}: {e}", self.id)))?
        };

        // Configure TCP options
        configure_tcp_stream(&stream)?;

        // Split into read/write halves
        let (read_half, write_half) = stream.into_split();
        self.reader = Some(BufReader::new(read_half));
        *self.writer.lock().unwrap() = Some(write_half);

        Ok(())
    }

    /// Perform version/verack handshake.
    pub async fn perform_handshake(&mut self, our_height: u32) -> Result<(), NetworkError> {
        self.state = PeerState::HandshakeSent;

        let writer = self.writer.clone();
        let mut reader = self.reader.take().ok_or(NetworkError::NotConnected)?;

        let result = do_handshake(&mut reader, &writer, &self.id, our_height).await;

        // Always put reader back
        self.reader = Some(reader);

        let hs = result?;
        self.peer_version = hs.version;
        self.peer_user_agent = hs.user_agent;
        self.peer_start_height = hs.start_height;
        self.supports_addrv2 = hs.supports_addrv2;
        self.state = PeerState::Connected;
        self.last_recv = Some(Instant::now());
        self.consecutive_failures = 0;
        self.score.successes += 1;

        Ok(())
    }

    /// Send a framed P2P message.
    pub async fn send_message(&self, command: &str, payload: &[u8]) -> Result<(), NetworkError> {
        self.send_via_writer(command, payload).await
    }

    /// Start the block listener (moves reader into a spawned task).
    ///
    /// After this, all message reads go through the dispatcher.
    pub fn start_block_listener(&mut self) -> Result<(), NetworkError> {
        let reader = self.reader.take().ok_or(NetworkError::NotConnected)?;
        let dispatcher = self.dispatcher.clone();
        let writer = self.writer.clone();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let peer_id = self.id.clone();

        let handle = tokio::spawn(async move {
            block_listener_loop(reader, writer, dispatcher, cancel_clone, peer_id).await;
        });

        self.listener_cancel = Some(cancel);
        self.listener_handle = Some(handle);
        Ok(())
    }

    /// Stop the block listener.
    pub async fn stop_block_listener(&mut self) {
        if let Some(cancel) = self.listener_cancel.take() {
            cancel.cancel();
        }
        if let Some(handle) = self.listener_handle.take() {
            // Wait up to 3 seconds for the listener to stop
            let _ = tokio::time::timeout(Duration::from_secs(3), handle).await;
        }
    }

    /// Send a message and wait for a specific response via the dispatcher.
    pub async fn send_and_wait(
        &self,
        command: &str,
        payload: &[u8],
        expected_response: &str,
        timeout: Duration,
    ) -> Result<(String, Vec<u8>), NetworkError> {
        // Register handler BEFORE sending (prevents race)
        let rx = {
            let mut disp = self.dispatcher.lock().unwrap();
            if !disp.is_active() {
                return Err(NetworkError::DispatcherInactive);
            }
            disp.register_handler(expected_response)
        };

        // Send the message
        self.send_via_writer(command, payload).await?;

        // Wait for response
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(NetworkError::PeerDisconnected(self.id.clone())),
            Err(_) => Err(NetworkError::ResponseTimeout),
        }
    }

    /// Send a message using the shared writer (for use after block listener starts).
    async fn send_via_writer(&self, command: &str, payload: &[u8]) -> Result<(), NetworkError> {
        let frame = protocol::frame_message(command, payload);

        // Take writer out, write, put back
        let mut writer_taken = {
            let mut guard = self.writer.lock().unwrap();
            guard.take().ok_or(NetworkError::NotConnected)?
        };

        let result = writer_taken.write_all(&frame).await;

        // Put writer back
        *self.writer.lock().unwrap() = Some(writer_taken);

        result.map_err(|e| NetworkError::ConnectionFailed(format!("Write: {e}")))
    }

    /// Disconnect from the peer.
    pub async fn disconnect(&mut self) {
        self.state = PeerState::Disconnecting;

        // Stop block listener
        self.stop_block_listener().await;

        // Drop writer (closes TCP connection)
        *self.writer.lock().unwrap() = None;
        self.reader = None;

        self.state = PeerState::Disconnected;
    }

    /// Check if the peer is connected and ready.
    pub fn is_connected(&self) -> bool {
        self.state == PeerState::Connected && self.writer.lock().unwrap().is_some()
    }

    /// Check if the handshake completed successfully.
    pub fn is_handshake_complete(&self) -> bool {
        self.peer_version > 0 && is_valid_zclassic_version(self.peer_version)
    }

    /// Check if this is a valid Zclassic peer.
    pub fn is_valid_zclassic_peer(&self) -> bool {
        is_valid_zclassic_version(self.peer_version)
    }

    /// Check if the block listener is active.
    pub fn is_listener_active(&self) -> bool {
        self.dispatcher.lock().unwrap().is_active()
    }

    /// Get a reference to the dispatcher.
    pub fn dispatcher(&self) -> &Arc<Mutex<Dispatcher>> {
        &self.dispatcher
    }

    /// Record a failure.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.score.failures += 1;
    }

    /// Record a success.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.score.successes += 1;
    }
}

/// Handshake result from a version/verack exchange.
struct HandshakeResult {
    version: u32,
    user_agent: String,
    start_height: u32,
    supports_addrv2: bool,
}

/// Send a framed P2P message using the shared writer.
///
/// Takes the writer out of the mutex, writes, puts it back.
/// This avoids holding a std::sync::Mutex across an await.
async fn send_frame(
    writer: &Arc<Mutex<Option<OwnedWriteHalf>>>,
    command: &str,
    payload: &[u8],
) -> Result<(), NetworkError> {
    let frame = protocol::frame_message(command, payload);
    let mut writer_taken = {
        let mut guard = writer.lock().unwrap();
        guard.take().ok_or(NetworkError::NotConnected)?
    };
    let result = writer_taken.write_all(&frame).await;
    *writer.lock().unwrap() = Some(writer_taken);
    result.map_err(|e| NetworkError::ConnectionFailed(format!("Write: {e}")))
}

/// Perform the version/verack handshake exchange.
///
/// Standalone function to avoid borrow conflicts with Peer fields.
async fn do_handshake(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &Arc<Mutex<Option<OwnedWriteHalf>>>,
    peer_id: &str,
    our_height: u32,
) -> Result<HandshakeResult, NetworkError> {
    // Build and send version message
    let version_msg = VersionMessage {
        version: PROTOCOL_VERSION,
        services: SERVICES_NODE_NETWORK,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        addr_recv: NetworkAddress::empty(),
        addr_from: NetworkAddress::empty(),
        nonce: rand::random(),
        user_agent: USER_AGENT.to_string(),
        start_height: our_height as i32,
        relay: true,
    };

    eprintln!("[ZipherX] Sending version to {peer_id} (v={PROTOCOL_VERSION}, ua={USER_AGENT})");
    send_frame(writer, "version", &version_msg.serialize()).await?;
    eprintln!("[ZipherX] Version sent to {peer_id}, waiting for response...");

    // Wait for version response
    let mut peer_version = 0u32;
    let mut peer_user_agent = String::new();
    let mut peer_start_height = 0u32;
    let mut received_version = false;

    for attempt in 0..5 {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, receive_message(reader)).await {
            Ok(Ok((cmd, payload))) => {
                eprintln!("[ZipherX] {peer_id}: received '{cmd}' ({} bytes) [attempt {attempt}]", payload.len());
                if cmd == "version" {
                    if let Some(ver) = VersionMessage::deserialize(&payload) {
                        peer_version = ver.version;
                        peer_user_agent = ver.user_agent.clone();
                        peer_start_height = ver.start_height as u32;

                        eprintln!("[ZipherX] {peer_id}: version={}, ua={}, height={}", ver.version, ver.user_agent, ver.start_height);

                        if !is_valid_zclassic_version(ver.version) {
                            eprintln!("[ZipherX] {peer_id}: WRONG CHAIN (version {})", ver.version);
                            return Err(NetworkError::WrongChain(peer_id.to_string()));
                        }

                        received_version = true;
                        break;
                    } else {
                        eprintln!("[ZipherX] {peer_id}: failed to deserialize version ({} bytes)", payload.len());
                    }
                } else if cmd == "reject" {
                    eprintln!("[ZipherX] {peer_id}: REJECTED");
                    return Err(NetworkError::HandshakeFailed(format!(
                        "Rejected by {peer_id}"
                    )));
                }
            }
            Ok(Err(e)) => {
                eprintln!("[ZipherX] {peer_id}: receive error: {e}");
                return Err(e);
            }
            Err(_) => {
                eprintln!("[ZipherX] {peer_id}: receive timeout (10s) [attempt {attempt}]");
                continue;
            }
        }
    }

    if !received_version {
        eprintln!("[ZipherX] {peer_id}: no version received after 5 attempts");
        return Err(NetworkError::HandshakeFailed(format!(
            "No version from {peer_id}"
        )));
    }

    // Send sendaddrv2 if peer supports BIP155
    if peer_version > 170011 {
        send_frame(writer, "sendaddrv2", &[]).await?;
    }

    // Send verack
    send_frame(writer, "verack", &[]).await?;

    // Wait for verack (and optionally sendaddrv2)
    let mut received_verack = false;
    let mut supports_addrv2 = false;

    for _ in 0..8 {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, receive_message(reader)).await {
            Ok(Ok((cmd, _payload))) => {
                match cmd.as_str() {
                    "verack" => received_verack = true,
                    "sendaddrv2" => supports_addrv2 = true,
                    _ => {}
                }
                if received_verack {
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => {
                if received_verack {
                    break;
                }
                continue;
            }
        }
    }

    if !received_verack {
        return Err(NetworkError::HandshakeFailed(format!(
            "No verack from {peer_id}"
        )));
    }

    Ok(HandshakeResult {
        version: peer_version,
        user_agent: peer_user_agent,
        start_height: peer_start_height,
        supports_addrv2,
    })
}

/// Check if a protocol version is valid Zclassic (not Zcash).
fn is_valid_zclassic_version(version: u32) -> bool {
    (version >= MIN_PEER_PROTOCOL_VERSION && version <= MAX_ZCLASSIC_PROTOCOL_VERSION)
        || (version >= ZCLASSIC_V2_MIN_VERSION && version <= ZCLASSIC_V2_MAX_VERSION)
}

/// Configure TCP keepalive and no-delay on a stream.
fn configure_tcp_stream(stream: &TcpStream) -> Result<(), NetworkError> {
    let socket = socket2::SockRef::from(stream);

    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(15))
        .with_interval(Duration::from_secs(15));

    socket.set_tcp_keepalive(&keepalive)
        .map_err(|e| NetworkError::ConnectionFailed(format!("Keepalive: {e}")))?;
    socket.set_nodelay(true)
        .map_err(|e| NetworkError::ConnectionFailed(format!("NoDelay: {e}")))?;

    Ok(())
}

/// Read a single P2P message from a buffered TCP reader.
///
/// Verifies magic bytes (with resync on mismatch), parses header,
/// reads payload, verifies checksum.
pub async fn receive_message(
    reader: &mut BufReader<OwnedReadHalf>,
) -> Result<(String, Vec<u8>), NetworkError> {
    let mut resync_count = 0;

    loop {
        // Read 24-byte message header
        let mut header_buf = [0u8; MESSAGE_HEADER_SIZE];
        tokio::time::timeout(DEFAULT_TIMEOUT, reader.read_exact(&mut header_buf))
            .await
            .map_err(|_| NetworkError::ResponseTimeout)?
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    NetworkError::PeerDisconnected("EOF".into())
                } else {
                    NetworkError::Io(e)
                }
            })?;

        // Parse header
        match protocol::parse_header(&header_buf) {
            Ok((command, payload_len, checksum)) => {
                // Read payload
                let len = payload_len as usize;
                let mut payload = vec![0u8; len];
                if len > 0 {
                    tokio::time::timeout(DEFAULT_TIMEOUT, reader.read_exact(&mut payload))
                        .await
                        .map_err(|_| NetworkError::ResponseTimeout)?
                        .map_err(|e| NetworkError::Io(e))?;
                }

                // Verify checksum
                if !protocol::verify_checksum(&payload, &checksum) {
                    return Err(NetworkError::Protocol(ProtocolError::InvalidChecksum));
                }

                return Ok((command, payload));
            }
            Err(ProtocolError::InvalidMagicBytes { .. }) => {
                // Attempt resync
                resync_count += 1;
                if resync_count > MAX_RESYNCS {
                    return Err(NetworkError::StreamDesync("Too many resyncs".into()));
                }

                // Scan for magic bytes in remaining header + up to 64KB
                let mut scan_buf = Vec::with_capacity(MAX_RESYNC_SCAN);
                scan_buf.extend_from_slice(&header_buf);

                let mut extra = vec![0u8; 4096];
                for _ in 0..16 {
                    match tokio::time::timeout(
                        Duration::from_secs(2),
                        reader.read(&mut extra),
                    )
                    .await
                    {
                        Ok(Ok(n)) if n > 0 => {
                            scan_buf.extend_from_slice(&extra[..n]);
                            if let Some(_offset) = protocol::scan_for_magic(&scan_buf) {
                                // Found magic — push remaining bytes back by reading from offset
                                // We can't "unread" in BufReader easily, so we'll just
                                // skip ahead and try again. The next loop iteration will
                                // try to read a fresh header.
                                break;
                            }
                        }
                        _ => break,
                    }

                    if scan_buf.len() >= MAX_RESYNC_SCAN {
                        return Err(NetworkError::StreamDesync(
                            "Magic not found in scan".into(),
                        ));
                    }
                }
                // Continue to next iteration — try reading a new header
                continue;
            }
            Err(e) => return Err(NetworkError::Protocol(e)),
        }
    }
}

/// Block listener loop — runs in a spawned task.
///
/// Reads messages from the TCP stream and routes them through the dispatcher.
/// Handles background messages (ping, inv) that no handler is waiting for.
async fn block_listener_loop(
    mut reader: BufReader<OwnedReadHalf>,
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    dispatcher: Arc<Mutex<Dispatcher>>,
    cancel: CancellationToken,
    peer_id: String,
) {
    dispatcher.lock().unwrap().set_active(true);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            result = tokio::time::timeout(LISTENER_IDLE_TIMEOUT, receive_message(&mut reader)) => {
                match result {
                    Ok(Ok((cmd, payload))) => {
                        let dispatched = dispatcher.lock().unwrap().dispatch(&cmd, payload.clone());
                        if !dispatched {
                            handle_background_message(&cmd, &payload, &writer, &peer_id).await;
                        }
                    }
                    Ok(Err(_)) => {
                        // Read error (disconnect, desync, etc.)
                        break;
                    }
                    Err(_) => {
                        // Idle timeout — peer is dead
                        break;
                    }
                }
            }
        }
    }

    let mut disp = dispatcher.lock().unwrap();
    disp.set_active(false);
    disp.cancel_all();
}

/// Handle a message that no dispatcher handler was waiting for.
async fn handle_background_message(
    command: &str,
    payload: &[u8],
    writer: &Arc<Mutex<Option<OwnedWriteHalf>>>,
    _peer_id: &str,
) {
    match command {
        "ping" => {
            // Respond with pong (same nonce)
            let frame = protocol::frame_message("pong", payload);
            let mut writer_taken = {
                let mut guard = writer.lock().unwrap();
                match guard.take() {
                    Some(w) => w,
                    None => return,
                }
            };
            let _ = writer_taken.write_all(&frame).await;
            *writer.lock().unwrap() = Some(writer_taken);
        }
        "inv" | "addr" | "addrv2" | "alert" | "getdata" | "notfound" | "mempool"
        | "getblocks" | "getheaders" => {
            // Silently ignore unsolicited messages
        }
        _ => {
            // Unknown command — ignore
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages;
    use tokio::net::TcpListener;

    /// Create a mock P2P server that performs a basic handshake.
    async fn mock_p2p_server(listener: TcpListener, our_height: i32) {
        let (stream, _) = listener.accept().await.unwrap();
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;

        // Read version message from client
        let (cmd, payload) = receive_message_raw(&mut reader).await.unwrap();
        assert_eq!(cmd, "version");

        let client_version = VersionMessage::deserialize(&payload).unwrap();
        assert_eq!(client_version.version, PROTOCOL_VERSION);

        // Send our version
        let version_msg = VersionMessage {
            version: PROTOCOL_VERSION,
            services: SERVICES_NODE_NETWORK,
            timestamp: 0,
            addr_recv: NetworkAddress::empty(),
            addr_from: NetworkAddress::empty(),
            nonce: rand::random(),
            user_agent: "/MockNode:1.0/".to_string(),
            start_height: our_height,
            relay: true,
        };
        let frame = protocol::frame_message("version", &version_msg.serialize());
        writer.write_all(&frame).await.unwrap();

        // Read sendaddrv2 + verack from client
        loop {
            let (cmd, _) = receive_message_raw(&mut reader).await.unwrap();
            if cmd == "verack" {
                break;
            }
        }

        // Send verack
        let frame = protocol::frame_message("verack", &[]);
        writer.write_all(&frame).await.unwrap();
    }

    /// Simplified receive for mock server (no resync, no timeout).
    async fn receive_message_raw(
        reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    ) -> Result<(String, Vec<u8>), NetworkError> {
        let mut header_buf = [0u8; MESSAGE_HEADER_SIZE];
        reader.read_exact(&mut header_buf).await
            .map_err(|e| NetworkError::Io(e))?;

        let (command, payload_len, checksum) = protocol::parse_header(&header_buf)?;
        let len = payload_len as usize;
        let mut payload = vec![0u8; len];
        if len > 0 {
            reader.read_exact(&mut payload).await
                .map_err(|e| NetworkError::Io(e))?;
        }
        assert!(protocol::verify_checksum(&payload, &checksum));
        Ok((command, payload))
    }

    #[tokio::test]
    async fn test_peer_connect_and_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(mock_p2p_server(listener, 1000));

        let mut peer = Peer::new(addr.ip().to_string(), addr.port());
        peer.connect(None, None).await.unwrap();
        peer.perform_handshake(500).await.unwrap();

        assert!(peer.is_connected());
        assert!(peer.is_handshake_complete());
        assert_eq!(peer.peer_version, PROTOCOL_VERSION);
        assert_eq!(peer.peer_start_height, 1000);

        peer.disconnect().await;
        assert!(!peer.is_connected());
    }

    #[tokio::test]
    async fn test_peer_wrong_chain_rejection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Mock server that sends Zcash version (170018)
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut writer = write_half;

            let _ = receive_message_raw(&mut reader).await.unwrap();

            let version_msg = VersionMessage {
                version: 170018, // Zcash version — should be rejected
                services: 1,
                timestamp: 0,
                addr_recv: NetworkAddress::empty(),
                addr_from: NetworkAddress::empty(),
                nonce: 0,
                user_agent: "/Zcash:5.0/".to_string(),
                start_height: 1000,
                relay: true,
            };
            let frame = protocol::frame_message("version", &version_msg.serialize());
            writer.write_all(&frame).await.unwrap();
        });

        let mut peer = Peer::new(addr.ip().to_string(), addr.port());
        peer.connect(None, None).await.unwrap();
        let result = peer.perform_handshake(500).await;

        assert!(matches!(result, Err(NetworkError::WrongChain(_))));
        peer.disconnect().await;
    }

    #[tokio::test]
    async fn test_peer_version_validation() {
        // Valid Zclassic versions
        assert!(is_valid_zclassic_version(170002));
        assert!(is_valid_zclassic_version(170009));
        assert!(is_valid_zclassic_version(170012));
        assert!(is_valid_zclassic_version(170100));
        assert!(is_valid_zclassic_version(170199));

        // Invalid (Zcash or too old)
        assert!(!is_valid_zclassic_version(170001));
        assert!(!is_valid_zclassic_version(170018));
        assert!(!is_valid_zclassic_version(170020));
        assert!(!is_valid_zclassic_version(0));
    }

    #[tokio::test]
    async fn test_send_and_receive_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Echo server — reads a message and sends it back
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut writer = write_half;

            let (cmd, payload) = receive_message_raw(&mut reader).await.unwrap();
            let frame = protocol::frame_message(&cmd, &payload);
            writer.write_all(&frame).await.unwrap();
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;

        // Send a ping
        let payload = messages::serialize_ping(42);
        let frame = protocol::frame_message("ping", &payload);
        writer.write_all(&frame).await.unwrap();

        // Receive echo
        let (cmd, data) = receive_message(&mut reader).await.unwrap();
        assert_eq!(cmd, "ping");
        assert_eq!(messages::deserialize_ping(&data), Some(42));
    }
}
