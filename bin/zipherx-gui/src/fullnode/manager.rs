//! Full Node Manager — daemon lifecycle for `zclassicd`.
//!
//! Finds, starts, and monitors the Zclassic daemon process.
//! Does NOT kill processes directly — provides status and signals
//! so the user can manage shutdown.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

/// Daemon running status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatus {
    /// Not running, not started.
    Stopped,
    /// Starting up (process spawned, waiting for RPC).
    Starting,
    /// Running and accepting RPC connections.
    Running,
    /// Shutting down gracefully via `stop` RPC.
    Stopping,
    /// Error state with description.
    Error(String),
}

/// Configuration for the full node.
#[derive(Clone)]
pub struct FullNodeConfig {
    /// Path to the `zclassicd` binary.
    pub daemon_path: Option<PathBuf>,
    /// Data directory for the blockchain (default: platform-specific).
    pub data_dir: Option<PathBuf>,
    /// RPC port (default: 8023).
    pub rpc_port: u16,
    /// RPC username.
    pub rpc_user: String,
    /// RPC password.
    pub rpc_password: String,
    /// Extra arguments to pass to zclassicd.
    pub extra_args: Vec<String>,
}

impl Default for FullNodeConfig {
    fn default() -> Self {
        Self {
            daemon_path: None,
            data_dir: None,
            rpc_port: 8023,
            rpc_user: "zipherx".to_string(),
            rpc_password: generate_rpc_password(),
            extra_args: Vec::new(),
        }
    }
}

/// Manages the lifecycle of a `zclassicd` daemon process.
pub struct FullNodeManager {
    pub config: FullNodeConfig,
    pub status: DaemonStatus,
    child: Option<Child>,
    /// Recent log lines from daemon stdout/stderr.
    pub log_lines: Vec<String>,
    /// Blockchain info from RPC.
    pub chain_info: Option<ChainInfo>,
}

/// Blockchain info from `getblockchaininfo` RPC.
#[derive(Clone, Default)]
pub struct ChainInfo {
    pub blocks: u64,
    pub headers: u64,
    pub verification_progress: f64,
    pub size_on_disk: u64,
    pub pruned: bool,
    pub chain: String,
}

/// Network info from `getnetworkinfo` RPC.
#[derive(Clone, Default)]
pub struct NetworkInfo {
    pub version: u64,
    pub subversion: String,
    pub protocol_version: u64,
    pub connections: u32,
}

impl FullNodeManager {
    pub fn new(config: FullNodeConfig) -> Self {
        Self {
            config,
            status: DaemonStatus::Stopped,
            child: None,
            log_lines: Vec::new(),
            chain_info: None,
        }
    }

    /// Find the `zclassicd` binary on the system.
    pub fn find_daemon() -> Option<PathBuf> {
        let candidates = daemon_search_paths();
        for path in candidates {
            if path.exists() && path.is_file() {
                return Some(path);
            }
        }
        // Check PATH
        if let Ok(output) = Command::new("which").arg("zclassicd").output() {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path_str.is_empty() {
                    return Some(PathBuf::from(path_str));
                }
            }
        }
        None
    }

    /// Find the path of a running `zclassicd` process.
    pub fn find_running_daemon_path() -> Option<PathBuf> {
        // Try `pgrep -lf zclassicd` or `ps aux | grep zclassicd`
        #[cfg(unix)]
        {
            if let Ok(output) = Command::new("pgrep").args(["-a", "zclassicd"]).output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    // Format: "12345 /path/to/zclassicd -daemon ..."
                    for line in stdout.lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let candidate = PathBuf::from(parts[1]);
                            if candidate.exists() {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Get the default Zclassic data directory for the current platform.
    pub fn default_data_dir() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .unwrap_or_default()
                .join("Library/Application Support/Zclassic")
        }
        #[cfg(target_os = "linux")]
        {
            dirs::home_dir().unwrap_or_default().join(".zclassic")
        }
        #[cfg(target_os = "windows")]
        {
            dirs::data_dir().unwrap_or_default().join("Zclassic")
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            dirs::home_dir().unwrap_or_default().join(".zclassic")
        }
    }

    /// Read RPC credentials from zclassic.conf if available.
    pub fn read_conf_credentials(data_dir: &std::path::Path) -> Option<(String, String, u16)> {
        let conf_path = data_dir.join("zclassic.conf");
        let content = std::fs::read_to_string(conf_path).ok()?;
        let mut user = None;
        let mut pass = None;
        let mut port = 8023u16;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                match key.trim() {
                    "rpcuser" => user = Some(val.trim().to_string()),
                    "rpcpassword" => pass = Some(val.trim().to_string()),
                    "rpcport" => {
                        if let Ok(p) = val.trim().parse() {
                            port = p;
                        }
                    }
                    _ => {}
                }
            }
        }
        Some((user?, pass?, port))
    }

    /// Write RPC credentials to zclassic.conf.
    fn ensure_conf(&self) -> Result<(), String> {
        let data_dir = self
            .config
            .data_dir
            .clone()
            .unwrap_or_else(Self::default_data_dir);
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data dir: {}", e))?;

        let conf_path = data_dir.join("zclassic.conf");

        // If conf exists and has RPC creds, don't overwrite
        if conf_path.exists() {
            if Self::read_conf_credentials(&data_dir).is_some() {
                return Ok(());
            }
        }

        // Append RPC settings
        let conf_content = format!(
            "rpcuser={}\nrpcpassword={}\nrpcport={}\nserver=1\n",
            self.config.rpc_user, self.config.rpc_password, self.config.rpc_port
        );
        std::fs::write(&conf_path, conf_content)
            .map_err(|e| format!("Failed to write zclassic.conf: {}", e))?;
        Ok(())
    }

    /// Start the `zclassicd` daemon.
    pub fn start(&mut self) -> Result<(), String> {
        if self.status == DaemonStatus::Running || self.status == DaemonStatus::Starting {
            return Ok(());
        }

        let daemon_path = self
            .config
            .daemon_path
            .clone()
            .or_else(Self::find_daemon)
            .ok_or("zclassicd not found. Install Zclassic or set the path manually.")?;

        if !daemon_path.exists() {
            return Err(format!(
                "Daemon binary not found at: {}",
                daemon_path.display()
            ));
        }

        // Ensure zclassic.conf has RPC credentials
        self.ensure_conf()?;

        // Read back credentials (may differ from config if conf already existed)
        let data_dir = self
            .config
            .data_dir
            .clone()
            .unwrap_or_else(Self::default_data_dir);
        if let Some((user, pass, port)) = Self::read_conf_credentials(&data_dir) {
            self.config.rpc_user = user;
            self.config.rpc_password = pass;
            self.config.rpc_port = port;
        }

        let mut cmd = Command::new(&daemon_path);

        // Set data directory if custom
        if let Some(ref dir) = self.config.data_dir {
            cmd.arg(format!("-datadir={}", dir.display()));
        }

        // Add daemon flag to run in background
        cmd.arg("-daemon=0"); // Run in foreground so we can monitor

        // Extra args
        for arg in &self.config.extra_args {
            cmd.arg(arg);
        }

        // Redirect stdout/stderr for log capture
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        match cmd.spawn() {
            Ok(child) => {
                self.child = Some(child);
                self.status = DaemonStatus::Starting;
                self.log_lines.push(format!(
                    "[ZipherX] Started zclassicd (PID: {})",
                    self.child.as_ref().map(|c| c.id()).unwrap_or(0)
                ));
                Ok(())
            }
            Err(e) => {
                self.status = DaemonStatus::Error(e.to_string());
                Err(format!("Failed to start daemon: {}", e))
            }
        }
    }

    /// Request graceful shutdown via RPC `stop` command.
    pub fn request_stop(&mut self, rpc: &super::rpc::RpcClient) {
        self.status = DaemonStatus::Stopping;
        self.log_lines
            .push("[ZipherX] Requesting daemon shutdown...".to_string());
        match rpc.call("stop", &[]) {
            Ok(_) => {
                self.log_lines
                    .push("[ZipherX] Daemon stop command sent.".to_string());
            }
            Err(e) => {
                self.log_lines.push(format!(
                    "[ZipherX] Stop failed: {} — daemon may need manual shutdown",
                    e
                ));
            }
        }
    }

    /// Check if the daemon process is still running.
    pub fn check_process(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process exited
                    self.log_lines
                        .push(format!("[ZipherX] Daemon exited with status: {}", status));
                    self.child = None;
                    self.status = DaemonStatus::Stopped;
                    false
                }
                Ok(None) => true, // Still running
                Err(e) => {
                    self.log_lines
                        .push(format!("[ZipherX] Error checking process: {}", e));
                    false
                }
            }
        } else {
            false
        }
    }

    /// Check if daemon is externally running (not started by us) by trying RPC.
    pub fn is_daemon_running_external(&self) -> bool {
        let rpc = super::rpc::RpcClient::new(
            &format!("http://127.0.0.1:{}", self.config.rpc_port),
            &self.config.rpc_user,
            &self.config.rpc_password,
        );
        rpc.call("getinfo", &[]).is_ok()
    }

    /// Get the daemon PID if running.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    /// Trim log buffer to last N lines.
    pub fn trim_logs(&mut self, max_lines: usize) {
        if self.log_lines.len() > max_lines {
            let drain = self.log_lines.len() - max_lines;
            self.log_lines.drain(..drain);
        }
    }
}

/// Create a thread-safe wrapper.
pub type SharedNodeManager = Arc<Mutex<FullNodeManager>>;

pub fn new_shared(config: FullNodeConfig) -> SharedNodeManager {
    Arc::new(Mutex::new(FullNodeManager::new(config)))
}

/// Generate a random RPC password.
fn generate_rpc_password() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Platform-specific paths to search for `zclassicd`.
fn daemon_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/Zclassic.app/Contents/MacOS/zclassicd",
        ));
        paths.push(PathBuf::from("/usr/local/bin/zclassicd"));
        paths.push(home.join("bin/zclassicd"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/local/bin/zclassicd"));
        paths.push(PathBuf::from("/usr/bin/zclassicd"));
        paths.push(home.join("bin/zclassicd"));
        paths.push(home.join(".local/bin/zclassicd"));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(PathBuf::from("C:\\Program Files\\Zclassic\\zclassicd.exe"));
        paths.push(PathBuf::from(
            "C:\\Program Files (x86)\\Zclassic\\zclassicd.exe",
        ));
        paths.push(home.join("AppData\\Local\\Programs\\Zclassic\\zclassicd.exe"));
    }

    paths
}
