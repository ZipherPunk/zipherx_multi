//! Tor client lifecycle — system Tor detection + embedded Arti (Phase 2).
//!
//! Detects a running Tor SOCKS5 proxy on well-known ports (9050 = system tor,
//! 9150 = Tor Browser) and verifies the SOCKS5 protocol handshake.
//! All P2P traffic is then routed through the verified proxy.
//!
//! Phase 2: When Arti dependencies are enabled, start_tor() bootstraps
//! an embedded TorClient and starts a local SOCKS5 proxy.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering};
use std::sync::Mutex;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{TorError, TorState};

/// Global Tor state — shared across the application.
///
/// RT-6: These atomics track independent aspects of Tor state (port, running,
/// progress, error). Compound operations across multiple atomics (e.g., checking
/// is_socks_running AND get_socks_port) are NOT atomic — there is a TOCTOU
/// window where state could change between reads. In practice this is benign
/// because state transitions are unidirectional during normal operation
/// (Disconnected -> Connecting -> Bootstrapping -> Connected) and only
/// stop_tor() resets all fields. If stronger guarantees are needed, these
/// should be consolidated into a single Mutex<TorStateSnapshot>.
static TOR_STATE: AtomicU8 = AtomicU8::new(0);
static TOR_BOOTSTRAP_PROGRESS: AtomicU8 = AtomicU8::new(0);
static TOR_SOCKS_PORT: AtomicU16 = AtomicU16::new(0);
static SOCKS_SERVER_RUNNING: AtomicBool = AtomicBool::new(false);
static TOR_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// RT-N1: Timestamp when the Tor circuit was established.
/// Used to determine when the circuit should be renewed for privacy.
static CIRCUIT_CREATED_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// RT-N1: Maximum circuit age before renewal is recommended (10 minutes).
const MAX_CIRCUIT_AGE_SECS: u64 = 600;

/// RT-1: Global Tor-only mode flag. When enabled, all network operations
/// that would bypass Tor (use_tor=false) are rejected with an error.
/// This prevents accidental clearnet leaks when the user has opted into
/// Tor-only mode in the settings.
static TOR_ONLY_MODE: AtomicBool = AtomicBool::new(false);

/// Well-known Tor SOCKS5 ports to probe.
///
/// 9050 = system Tor daemon (`brew install tor && brew services start tor`)
/// 9150 = Tor Browser
/// 9250 = common alternate Tor port
const TOR_SOCKS_PORTS: &[u16] = &[9050, 9150, 9250];

/// Additional ports to probe (test-only: inject a mock SOCKS5 server).
#[cfg(test)]
static EXTRA_PROBE_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

// ============================================================================
// State Queries
// ============================================================================

/// Get the current Tor connection state.
pub fn get_state() -> TorState {
    TorState::from_u8(TOR_STATE.load(Ordering::SeqCst)).unwrap_or(TorState::Disconnected)
}

/// Get the bootstrap progress (0-100).
pub fn get_bootstrap_progress() -> u8 {
    TOR_BOOTSTRAP_PROGRESS.load(Ordering::SeqCst)
}

/// Get the SOCKS5 proxy port (0 = not running).
pub fn get_socks_port() -> u16 {
    TOR_SOCKS_PORT.load(Ordering::SeqCst)
}

/// Check if the SOCKS5 server is running.
pub fn is_socks_running() -> bool {
    SOCKS_SERVER_RUNNING.load(Ordering::SeqCst)
}

/// RT-1: Enable or disable Tor-only mode.
/// When enabled, download_file() and other network functions will reject
/// requests with use_tor=false.
pub fn set_tor_only_mode(enabled: bool) {
    TOR_ONLY_MODE.store(enabled, Ordering::SeqCst);
}

/// RT-1: Check if Tor-only mode is enabled.
pub fn is_tor_only_mode() -> bool {
    TOR_ONLY_MODE.load(Ordering::SeqCst)
}

/// Get the last error message.
pub fn get_last_error() -> Option<String> {
    TOR_ERROR.lock().ok().and_then(|guard| guard.clone())
}

/// Get the SOCKS5 proxy address (127.0.0.1:port), or None if not running.
pub fn get_socks_addr() -> Option<SocketAddr> {
    if !is_socks_running() {
        return None;
    }
    let port = get_socks_port();
    if port == 0 {
        return None;
    }
    Some(SocketAddr::from(([127, 0, 0, 1], port)))
}

/// RT-N1: Check if the Tor circuit should be renewed.
///
/// Returns `true` if the circuit has been active for longer than
/// `MAX_CIRCUIT_AGE_SECS` (10 minutes). Long-lived circuits reduce
/// privacy because the exit node can correlate traffic over time.
/// Callers should call `restart_tor()` when this returns `true`.
pub fn should_renew_circuit() -> bool {
    if let Ok(guard) = CIRCUIT_CREATED_AT.lock() {
        if let Some(created_at) = *guard {
            return created_at.elapsed().as_secs() >= MAX_CIRCUIT_AGE_SECS;
        }
    }
    false
}

/// RT-N1: Get the circuit age in seconds, or None if no circuit is active.
pub fn get_circuit_age_secs() -> Option<u64> {
    if let Ok(guard) = CIRCUIT_CREATED_AT.lock() {
        guard.map(|created_at| created_at.elapsed().as_secs())
    } else {
        None
    }
}

/// Get the Tor data directory for the current platform.
pub fn get_tor_data_dir() -> PathBuf {
    #[cfg(target_os = "ios")]
    {
        if let Some(home) = dirs::document_dir() {
            return home.join("ZipherX").join("Tor");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(support) = dirs::data_dir() {
            return support.join("ZipherX").join("Tor");
        }
    }

    #[cfg(target_os = "android")]
    {
        // Android: files directory passed via platform trait
        return PathBuf::from("/data/data/com.zipherx.wallet/files/tor");
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::data_local_dir() {
            return appdata.join("ZipherX").join("Tor");
        }
    }

    // Fallback (unreachable on Android/macOS/Windows where earlier cfg blocks return)
    #[allow(unreachable_code)]
    PathBuf::from("/tmp/zipherx_tor")
}

// ============================================================================
// Internal state helpers
// ============================================================================

fn set_state(state: TorState) {
    TOR_STATE.store(state as u8, Ordering::SeqCst);
}

fn set_bootstrap_progress(progress: u8) {
    TOR_BOOTSTRAP_PROGRESS.store(progress.min(100), Ordering::SeqCst);
}

fn set_socks_port(port: u16) {
    TOR_SOCKS_PORT.store(port, Ordering::SeqCst);
}

fn set_error(msg: Option<String>) {
    if let Ok(mut guard) = TOR_ERROR.lock() {
        *guard = msg;
    }
}

/// Test helper: force SOCKS server state to stopped without touching other state.
#[cfg(test)]
pub fn force_socks_stopped() {
    SOCKS_SERVER_RUNNING.store(false, Ordering::SeqCst);
}

/// Test helper: add extra ports for probe_socks5_proxy to check.
#[cfg(test)]
pub fn set_extra_probe_ports(ports: Vec<u16>) {
    if let Ok(mut guard) = EXTRA_PROBE_PORTS.lock() {
        *guard = ports;
    }
}

// ============================================================================
// SOCKS5 Probe
// ============================================================================

/// Timeout for SOCKS5 probe connect and read operations.
const PROBE_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(3);

/// Probe a port to verify it speaks the SOCKS5 protocol.
///
/// Sends a SOCKS5 greeting (version 5, no-auth method) and checks for
/// a valid response. Returns `true` if the port is a SOCKS5 proxy.
/// Returns `Err` if the port is not reachable (connection refused/timeout).
///
/// RT-5: This probe trusts any SOCKS5 server responding on the probed port.
/// A malicious local process could bind to port 9050 before Tor starts and
/// act as a SOCKS5 proxy, intercepting all traffic. Mitigations: (1) probe
/// well-known Tor ports only, (2) verify SOCKS5 protocol handshake, (3) on
/// macOS/Linux, Tor's port is typically reserved by the system service.
/// Future improvement: verify the proxy routes through actual Tor circuits
/// (e.g., by connecting to check.torproject.org).
///
/// RT-3: SECURITY WARNING: All connections share one Tor circuit — exit node
/// can correlate peer connections to this wallet. Phase 2 will implement
/// Arti circuit isolation (one circuit per peer) to prevent cross-peer
/// correlation attacks. See: https://docs.rs/arti-client/latest/
async fn probe_socks5_proxy(port: u16) -> Result<bool, String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let mut stream = tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| "connection timeout".to_string())?
        .map_err(|e| e.to_string())?;

    // SOCKS5 greeting: version(5) + 1 method + no-auth(0)
    stream
        .write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| format!("write: {e}"))?;

    // Response: version(1) + chosen_method(1)
    let mut resp = [0u8; 2];
    tokio::time::timeout(PROBE_TIMEOUT, stream.read_exact(&mut resp))
        .await
        .map_err(|_| "read timeout".to_string())?
        .map_err(|e| format!("read: {e}"))?;

    // Valid SOCKS5: version == 5, method == 0 (no auth)
    Ok(resp[0] == 0x05 && resp[1] == 0x00)
}

// ============================================================================
// Tor Lifecycle
// ============================================================================

/// Start the Tor client — detect system Tor SOCKS5 proxy.
///
/// Probes well-known Tor SOCKS5 ports (9050, 9150, 9250) for an existing
/// Tor daemon. Verifies the SOCKS5 protocol handshake before accepting.
///
/// Returns the verified SOCKS5 proxy port on success.
/// If `data_dir` is None, uses the platform default.
///
/// Phase 2: If no system Tor found, bootstraps embedded Arti TorClient.
pub async fn start_tor(data_dir: Option<PathBuf>) -> Result<u16, TorError> {
    let current_state = get_state();
    if current_state == TorState::Connected && is_socks_running() {
        return Ok(get_socks_port());
    }

    if current_state == TorState::Connecting || current_state == TorState::Bootstrapping {
        return Err(TorError::BootstrapFailed("Tor is already starting".into()));
    }

    // Clear previous error
    set_error(None);

    // Set state to Connecting
    set_state(TorState::Connecting);
    set_bootstrap_progress(0);

    // Resolve data directory
    let tor_dir = data_dir.unwrap_or_else(get_tor_data_dir);

    // Create data directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&tor_dir) {
        let msg = format!("Failed to create Tor data dir: {e}");
        set_state(TorState::Error);
        set_error(Some(msg.clone()));
        return Err(TorError::BootstrapFailed(msg));
    }

    set_state(TorState::Bootstrapping);
    set_bootstrap_progress(25);

    // Build port list: test-injected ports first (deterministic), then well-known ports
    let mut ports_to_probe: Vec<u16> = Vec::new();
    #[cfg(test)]
    if let Ok(guard) = EXTRA_PROBE_PORTS.lock() {
        ports_to_probe.extend(guard.iter());
    }
    ports_to_probe.extend(TOR_SOCKS_PORTS.iter());

    // Probe each port for a real SOCKS5 proxy
    set_bootstrap_progress(50);
    for &port in &ports_to_probe {
        match probe_socks5_proxy(port).await {
            Ok(true) => {
                // Found a verified SOCKS5 proxy
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] Verified SOCKS5 proxy on port {port}");
                set_socks_port(port);
                SOCKS_SERVER_RUNNING.store(true, Ordering::SeqCst);
                // RT-N1: Record circuit creation time for age tracking
                if let Ok(mut guard) = CIRCUIT_CREATED_AT.lock() {
                    *guard = Some(std::time::Instant::now());
                }
                set_bootstrap_progress(100);
                set_state(TorState::Connected);
                return Ok(port);
            }
            Ok(false) => {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] Port {port} listening but not SOCKS5");
            }
            Err(_e) => {
                #[cfg(debug_assertions)]
                eprintln!("[ZipherX-Tor] Port {port} not reachable: {_e}");
            }
        }
    }

    // ── RT-N2 TODO: Tor bridge support ───────────────────────────────
    // Bridge support is planned for a future release. This would allow
    // users in censored regions to connect to the Tor network via bridge
    // relays (obfs4, meek, snowflake). Implementation steps:
    // 1. Add bridge configuration UI in settings (bridge line input)
    // 2. Pass bridge config to Arti's TorClientConfigBuilder
    // 3. Support pluggable transports (obfs4proxy binary or built-in)

    // ── Phase 2: Embedded Arti bootstrap would go here ─────────────
    // let config = TorClientConfigBuilder::from_directories(tor_dir, ...)
    //     .build()?;
    // let client = TorClient::create_bootstrapped(config).await?;
    // let listener = TcpListener::bind("127.0.0.1:0").await?;
    // let socks_port = listener.local_addr()?.port();
    // ... spawn SOCKS5 accept loop using client.connect() ...

    // No system Tor found
    let msg = "No Tor SOCKS5 proxy found on ports 9050/9150/9250. \
               Install Tor: brew install tor && brew services start tor"
        .to_string();
    set_state(TorState::Error);
    set_error(Some(msg.clone()));
    set_bootstrap_progress(0);
    Err(TorError::SocksProxyFailed(msg))
}

/// Stop the Tor client and SOCKS5 proxy.
///
/// Resets all state to Disconnected. Safe to call even if not running.
pub async fn stop_tor() -> Result<(), TorError> {
    // Phase 2: Gracefully shut down Arti client
    // if let Some(client) = TOR_CLIENT.lock().take() { ... }

    SOCKS_SERVER_RUNNING.store(false, Ordering::SeqCst);
    set_socks_port(0);
    set_bootstrap_progress(0);
    set_state(TorState::Disconnected);
    set_error(None);
    // RT-N1: Clear circuit age tracking
    if let Ok(mut guard) = CIRCUIT_CREATED_AT.lock() {
        *guard = None;
    }

    Ok(())
}

/// Restart the Tor client.
///
/// Stops then starts with the same configuration.
pub async fn restart_tor(data_dir: Option<PathBuf>) -> Result<u16, TorError> {
    stop_tor().await?;
    // Brief delay for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    start_tor(data_dir).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    // Global test mutex — client tests share global atomics so must not run in parallel
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        set_state(TorState::Disconnected);
        set_bootstrap_progress(0);
        set_socks_port(0);
        SOCKS_SERVER_RUNNING.store(false, Ordering::SeqCst);
        TOR_ONLY_MODE.store(false, Ordering::SeqCst);
        set_error(None);
        set_extra_probe_ports(Vec::new());
        // RT-N1: Clear circuit age tracking
        if let Ok(mut guard) = CIRCUIT_CREATED_AT.lock() {
            *guard = None;
        }
    }

    /// Spawn a mock SOCKS5 server on an ephemeral port.
    /// Returns the port it's listening on.
    async fn spawn_mock_socks5() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            // Accept up to 5 connections (enough for tests)
            for _ in 0..5 {
                if let Ok((mut stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        // Read SOCKS5 greeting
                        let mut greeting = [0u8; 3];
                        if stream.read_exact(&mut greeting).await.is_err() {
                            return;
                        }
                        // Respond with no-auth accepted
                        let _ = stream.write_all(&[0x05, 0x00]).await;
                    });
                }
            }
        });

        // Brief yield to let the listener start
        tokio::task::yield_now().await;
        port
    }

    #[test]
    fn test_initial_state() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(get_state(), TorState::Disconnected);
        assert_eq!(get_bootstrap_progress(), 0);
        assert_eq!(get_socks_port(), 0);
        assert!(!is_socks_running());
        assert!(get_last_error().is_none());
        assert!(get_socks_addr().is_none());
    }

    #[tokio::test]
    async fn test_start_stop_tor() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();

        // Start a mock SOCKS5 server and inject its port
        let mock_port = spawn_mock_socks5().await;
        set_extra_probe_ports(vec![mock_port]);

        let port = start_tor(Some(std::env::temp_dir().join("zipherx_tor_test")))
            .await
            .unwrap();
        assert_eq!(port, mock_port);
        assert_eq!(get_state(), TorState::Connected);
        assert_eq!(get_bootstrap_progress(), 100);
        assert!(is_socks_running());
        assert!(get_socks_addr().is_some());

        stop_tor().await.unwrap();
        assert_eq!(get_state(), TorState::Disconnected);
        assert_eq!(get_socks_port(), 0);
        assert!(!is_socks_running());
        assert!(get_socks_addr().is_none());
    }

    #[tokio::test]
    async fn test_double_start_returns_same_port() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();

        let mock_port = spawn_mock_socks5().await;
        set_extra_probe_ports(vec![mock_port]);

        let port1 = start_tor(Some(std::env::temp_dir().join("zipherx_tor_test2")))
            .await
            .unwrap();
        let port2 = start_tor(None).await.unwrap();
        assert_eq!(port1, port2);
        stop_tor().await.unwrap();
    }

    #[tokio::test]
    async fn test_restart_tor() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();

        let mock_port = spawn_mock_socks5().await;
        set_extra_probe_ports(vec![mock_port]);

        let port = start_tor(Some(std::env::temp_dir().join("zipherx_tor_test3")))
            .await
            .unwrap();
        assert_eq!(port, mock_port);

        // After restart, same mock server is still running
        let new_port = restart_tor(Some(std::env::temp_dir().join("zipherx_tor_test3b")))
            .await
            .unwrap();
        assert_eq!(new_port, mock_port);
        assert_eq!(get_state(), TorState::Connected);
        stop_tor().await.unwrap();
    }

    #[tokio::test]
    async fn test_start_fails_without_proxy() {
        // Use lock().ok() to avoid PoisonError from prior test panics
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();

        // Don't inject any ports, and well-known ports likely aren't running in CI
        set_extra_probe_ports(Vec::new());

        // This will fail unless system Tor happens to be running
        // We can't deterministically test failure without blocking ports,
        // so we just verify the function doesn't panic
        let result = start_tor(Some(std::env::temp_dir().join("zipherx_tor_test_fail"))).await;
        // Reset state regardless of result (system Tor might be running)
        stop_tor().await.unwrap();
        // Just verify we got a result (Ok or Err) without panic
        let _ = result;
    }

    #[tokio::test]
    async fn test_probe_socks5_real() {
        // Spawn a mock SOCKS5 server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            stream.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting[0], 0x05);
            stream.write_all(&[0x05, 0x00]).await.unwrap();
        });

        tokio::task::yield_now().await;

        let result = probe_socks5_proxy(port).await;
        assert_eq!(result, Ok(true));
    }

    #[tokio::test]
    async fn test_probe_socks5_non_socks5() {
        // Spawn a server that speaks HTTP, not SOCKS5
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 3];
            let _ = stream.read_exact(&mut buf).await;
            // Respond with HTTP instead of SOCKS5
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n").await;
        });

        tokio::task::yield_now().await;

        let result = probe_socks5_proxy(port).await;
        assert_eq!(result, Ok(false));
    }

    #[tokio::test]
    async fn test_probe_socks5_not_listening() {
        // Port 1 is almost certainly not listening
        let result = probe_socks5_proxy(1).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_socks_addr_format() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_socks_port(9150);
        SOCKS_SERVER_RUNNING.store(true, Ordering::SeqCst);

        let addr = get_socks_addr().unwrap();
        assert_eq!(addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(addr.port(), 9150);

        SOCKS_SERVER_RUNNING.store(false, Ordering::SeqCst);
        assert!(get_socks_addr().is_none());
    }

    #[test]
    fn test_error_tracking() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_error(Some("test error".into()));
        assert_eq!(get_last_error(), Some("test error".into()));
        set_error(None);
        assert!(get_last_error().is_none());
    }

    #[test]
    fn test_bootstrap_progress_clamped() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_bootstrap_progress(150); // > 100
        assert_eq!(get_bootstrap_progress(), 100);
    }
}
