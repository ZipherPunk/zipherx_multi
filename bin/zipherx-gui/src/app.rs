//! ZipherX GUI application state.
//!
//! `ZipherXApp` is the single source of truth for the entire GUI.
//! All sensitive fields are zeroized on Drop to prevent key leakage.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use zeroize::Zeroize;
use zipherx_platform::{PlatformInfo, SecureStorage};

use crate::effects::confetti::Particle;
use crate::sync::SharedState;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Top-level navigation tab.
#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    #[default]
    Wallet,
    History,
    Send,
    Node,
    Settings,
}

/// Application lifecycle phase.
#[derive(PartialEq, Eq)]
pub enum Phase {
    /// Disclaimer — must scroll to bottom and accept.
    Disclaimer,
    /// Wallet is locked — waiting for password.
    Locked,
    /// Setup — create, restore, or import wallet.
    #[allow(dead_code)]
    Setup,
    /// Fully operational.
    Ready,
}

/// Wallet setup mode.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SetupMode {
    Create,
    Restore,
    Import,
}

/// Filter for the history view.
#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum HistoryFilter {
    #[default]
    All,
    Received,
    Sent,
}

/// Sync task status for progress display.
#[derive(PartialEq, Eq, Clone, Copy)]
#[allow(dead_code)]
pub enum SyncTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Individual sync task for progress tracking.
#[derive(Clone)]
#[allow(dead_code)]
pub struct SyncTask {
    pub id: String,
    pub title: String,
    pub status: SyncTaskStatus,
    pub detail: String,
    pub progress: f32,
    pub start_time: Option<Instant>,
    pub end_time: Option<Instant>,
}

/// Transaction record for display.
#[derive(Clone)]
pub struct TransactionRecord {
    pub txid: String,
    pub tx_type: String, // "sent", "received", "self"
    pub amount: u64,
    pub fee: u64,
    pub address: Option<String>,
    pub memo: Option<String>,
    pub confirmations: u64,
    pub height: u64,
    pub timestamp: u64,
}

/// Balance info.
#[derive(Clone, Default)]
pub struct BalanceDisplay {
    pub total: u64,
    pub spendable: u64,
    pub note_count: usize,
    pub spendable_note_count: usize,
}

// ---------------------------------------------------------------------------
// ZipherXApp
// ---------------------------------------------------------------------------

/// Root application state for the ZipherX desktop wallet.
pub struct ZipherXApp {
    // -- lifecycle --
    pub phase: Phase,
    pub tab: Tab,

    // -- disclaimer --
    #[allow(dead_code)]
    pub disclaimer_scroll_offset: f32,
    pub disclaimer_scrolled_to_bottom: bool,

    // -- auth --
    pub password_input: String,
    pub password_confirm: String,
    pub password_error: Option<String>,

    // -- setup --
    pub setup_mode: Option<SetupMode>,
    pub mnemonic_words: Vec<String>,
    pub mnemonic_input: String,
    pub import_key_input: String,
    pub setup_error: Option<String>,

    // -- wallet --
    pub sk_bytes: Option<Vec<u8>>,
    pub address: Option<String>,
    pub balance: BalanceDisplay,

    // -- network --
    pub block_height: u64,
    pub peer_count: u32,
    pub tor_enabled: bool,
    pub tor_state: String,
    pub onion_address: Option<String>,

    // -- sync --
    pub is_syncing: bool,
    pub sync_phase: String,
    pub sync_progress: f32,
    pub sync_tasks: Vec<SyncTask>,
    pub overall_progress: f32,
    pub sync_start_time: Option<Instant>,
    pub sync_error: Option<String>,

    // -- send --
    pub send_address: String,
    pub send_amount: String,
    pub send_memo: String,
    pub send_fee: u64,
    pub send_error: Option<String>,
    pub send_in_progress: bool,
    pub send_phase: String,
    pub send_phase_current: u32,
    pub send_phase_total: u32,
    pub show_send_confirm: bool,
    pub show_send_reauth: bool,
    pub reauth_password: String,

    // -- send lifecycle (clearing + settlement) --
    pub mempool_accepted: bool,
    pub mempool_peer_status: Option<String>,
    pub pending_confirmation_txid: Option<String>,
    pub pending_settlement_message: Option<String>,
    pub mempool_tx_notification: Option<(String, u64)>,
    pub clearing_celebration: Option<String>,
    pub clearing_duration: Option<String>,
    pub settlement_celebration: Option<String>,
    pub settlement_duration: Option<String>,
    pub settlement_txid: Option<String>,
    pub send_timestamp: Option<Instant>,
    #[allow(dead_code)]
    pub mempool_timestamp: Option<Instant>,
    pub confirmed_sent_count_at_send: usize,
    pub pending_resync_timer: Option<std::time::Instant>,
    pub pending_resync_count: u32,
    pub last_bg_sync: Option<std::time::Instant>,
    pub initial_sync_done: bool,

    // -- receive --
    pub show_receive: bool,
    pub qr_texture: Option<egui::TextureHandle>,

    // -- receive notifications --
    pub receive_celebration: Option<String>,
    pub receive_amount: Option<u64>,
    pub receive_txid: Option<String>,
    pub known_received_txids: std::collections::HashSet<String>,
    pub receive_txids_seeded: bool,
    pub mempool_notification_time: Option<Instant>,

    // -- incoming TX confirmation tracking --
    pub pending_incoming_txid: Option<String>,
    pub pending_incoming_amount: Option<u64>,
    pub pending_incoming_message: Option<String>,
    pub pending_incoming_resync_timer: Option<Instant>,
    pub pending_incoming_resync_count: u32,

    // -- history --
    pub transactions: Vec<TransactionRecord>,
    pub history_filter: HistoryFilter,
    pub history_expanded: Option<usize>,

    // -- export key --
    pub show_export: bool,
    pub show_export_confirm: bool,
    pub export_password: String,
    pub export_key_display: String,
    pub export_auto_dismiss: Option<Instant>,

    // -- logo --
    pub logo_texture: Option<egui::TextureHandle>,

    // -- settings --
    pub version: String,

    // -- peer management --
    pub peer_section_expanded: bool,
    pub peer_infos: Vec<crate::sync::PeerDisplayInfo>,
    pub custom_peer_host: String,
    pub custom_peer_port: String,
    pub peer_action_result: Option<String>,

    // -- maintenance --
    pub maintenance_in_progress: bool,
    pub maintenance_status: Option<String>,
    pub show_repair_confirm: bool,
    pub show_rescan_confirm: bool,

    // -- clipboard auto-clear --
    pub clipboard_clear_at: Option<Instant>,

    // -- celebrations --
    pub confetti_particles: Vec<Particle>,
    pub firework_particles: Vec<Particle>,

    // -- auto-lock --
    pub last_interaction: Instant,
    pub auto_lock_secs: u64,

    // -- full node --
    pub fullnode_enabled: bool,
    pub show_fullnode_confirm: bool,
    pub node_daemon_status: crate::fullnode::manager::DaemonStatus,
    pub node_daemon_pid: Option<u32>,
    pub node_daemon_path: Option<PathBuf>,
    pub node_daemon_path_input: String,
    pub node_data_dir: PathBuf,
    pub node_rpc_port: u16,
    pub node_rpc_user: String,
    pub node_rpc_password: String,
    pub node_chain_info: Option<crate::fullnode::manager::ChainInfo>,
    pub node_network_info: Option<crate::fullnode::manager::NetworkInfo>,
    pub node_mempool_info: Option<serde_json::Value>,
    pub node_error: Option<String>,
    pub node_log_lines: Vec<String>,
    pub node_manager: Option<crate::fullnode::manager::SharedNodeManager>,
    pub node_poll_interval: std::time::Instant,

    // -- error --
    pub error: Option<String>,

    // -- background sync --
    pub shared_state: Option<Arc<Mutex<SharedState>>>,

    // -- platform --
    pub data_dir: PathBuf,
    pub storage: Arc<crate::platform::GuiSecureStorage>,
}

impl Default for ZipherXApp {
    fn default() -> Self {
        let info = crate::platform::GuiPlatformInfo::new();
        let data_dir = info.data_directory();
        let storage = Arc::new(crate::platform::GuiSecureStorage::new(&data_dir));

        let has_wallet = storage.has_key("spending_key");
        let accepted_disclaimer = data_dir.join(".disclaimer_accepted").exists();

        let phase = if !accepted_disclaimer {
            Phase::Disclaimer
        } else if has_wallet {
            Phase::Locked
        } else {
            Phase::Locked // Need password first, then setup
        };

        Self {
            phase,
            tab: Tab::default(),

            disclaimer_scroll_offset: 0.0,
            disclaimer_scrolled_to_bottom: false,

            password_input: String::new(),
            password_confirm: String::new(),
            password_error: None,

            setup_mode: None,
            mnemonic_words: Vec::new(),
            mnemonic_input: String::new(),
            import_key_input: String::new(),
            setup_error: None,

            sk_bytes: None,
            address: None,
            balance: BalanceDisplay::default(),

            block_height: 0,
            peer_count: 0,
            tor_enabled: false,
            tor_state: "Disconnected".to_string(),
            onion_address: None,

            is_syncing: false,
            sync_phase: "Idle".to_string(),
            sync_progress: 0.0,
            sync_tasks: Vec::new(),
            overall_progress: 0.0,
            sync_start_time: None,
            sync_error: None,

            send_address: String::new(),
            send_amount: String::new(),
            send_memo: String::new(),
            send_fee: 10_000, // 0.00010000 ZCL
            send_error: None,
            send_in_progress: false,
            send_phase: String::new(),
            send_phase_current: 0,
            send_phase_total: 0,
            show_send_confirm: false,
            show_send_reauth: false,
            reauth_password: String::new(),

            mempool_accepted: false,
            mempool_peer_status: None,
            pending_confirmation_txid: None,
            pending_settlement_message: None,
            mempool_tx_notification: None,
            clearing_celebration: None,
            clearing_duration: None,
            settlement_celebration: None,
            settlement_duration: None,
            settlement_txid: None,
            send_timestamp: None,
            mempool_timestamp: None,
            confirmed_sent_count_at_send: 0,
            pending_resync_timer: None,
            pending_resync_count: 0,
            last_bg_sync: None,
            initial_sync_done: false,

            show_receive: false,
            qr_texture: None,

            receive_celebration: None,
            receive_amount: None,
            receive_txid: None,
            known_received_txids: std::collections::HashSet::new(),
            receive_txids_seeded: false,
            mempool_notification_time: None,

            pending_incoming_txid: None,
            pending_incoming_amount: None,
            pending_incoming_message: None,
            pending_incoming_resync_timer: None,
            pending_incoming_resync_count: 0,

            transactions: Vec::new(),
            history_filter: HistoryFilter::default(),
            history_expanded: None,

            show_export: false,
            show_export_confirm: false,
            export_password: String::new(),
            export_key_display: String::new(),
            export_auto_dismiss: None,

            logo_texture: None,

            version: env!("CARGO_PKG_VERSION").to_string(),

            peer_section_expanded: false,
            peer_infos: Vec::new(),
            custom_peer_host: String::new(),
            custom_peer_port: "8033".to_string(),
            peer_action_result: None,

            maintenance_in_progress: false,
            maintenance_status: None,
            show_repair_confirm: false,
            show_rescan_confirm: false,

            clipboard_clear_at: None,

            confetti_particles: Vec::new(),
            firework_particles: Vec::new(),

            last_interaction: Instant::now(),
            auto_lock_secs: 300,

            fullnode_enabled: false,
            show_fullnode_confirm: false,
            node_daemon_status: crate::fullnode::manager::DaemonStatus::Stopped,
            node_daemon_pid: None,
            node_daemon_path: crate::fullnode::manager::FullNodeManager::find_daemon(),
            node_daemon_path_input: String::new(),
            node_data_dir: crate::fullnode::manager::FullNodeManager::default_data_dir(),
            node_rpc_port: 8023,
            node_rpc_user: String::new(),
            node_rpc_password: String::new(),
            node_chain_info: None,
            node_network_info: None,
            node_mempool_info: None,
            node_error: None,
            node_log_lines: Vec::new(),
            node_manager: None,
            node_poll_interval: Instant::now(),

            error: None,

            shared_state: None,

            data_dir,
            storage,
        }
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Format zatoshis as ZCL string (8 decimal places, trailing zeros trimmed).
pub fn fmt_zcl(sats: u64) -> String {
    let whole = sats / 100_000_000;
    let frac = sats % 100_000_000;
    if frac == 0 {
        format!("{}.0", whole)
    } else {
        let s = format!("{}.{:08}", whole, frac);
        s.trim_end_matches('0').to_string()
    }
}

/// Parse a ZCL string to zatoshis.
pub fn parse_zcl(input: &str) -> Option<u64> {
    let parts: Vec<&str> = input.split('.').collect();
    match parts.len() {
        1 => {
            let whole: u64 = parts[0].parse().ok()?;
            Some(whole * 100_000_000)
        }
        2 => {
            let whole: u64 = parts[0].parse().ok()?;
            let frac_str = format!("{:0<8}", parts[1]);
            if frac_str.len() > 8 {
                return None;
            }
            let frac: u64 = frac_str.parse().ok()?;
            Some(whole * 100_000_000 + frac)
        }
        _ => None,
    }
}

/// Cypherpunk quotes for the wallet screen.
#[allow(dead_code)]
pub fn random_quote() -> &'static str {
    const QUOTES: &[&str] = &[
        "Privacy is the power to selectively reveal oneself to the world.",
        "Cypherpunks write code.",
        "We must defend our own privacy if we expect to have any.",
        "Privacy in an open society requires anonymous transaction systems.",
        "We the Cypherpunks are dedicated to building anonymous systems.",
        "Cypherpunks deploy cryptography.",
        "The future is already here — it's just not evenly distributed.",
        "A cypherpunk's work is never done.",
        "Trust math, not middlemen.",
        "Code is speech. Encryption is armor.",
    ];
    QUOTES[rand::random::<usize>() % QUOTES.len()]
}

/// Random pending settlement message (waiting for block after mempool).
pub fn random_pending_settlement_message() -> &'static str {
    const MSGS: &[&str] = &[
        "Your proof floats in the mempool.\nMiners compete to etch it into the next block.\nPatience — privacy takes time.",
        "The zero-knowledge proof is verified.\nNow the chain must seal it.\nNo one knows what you sent. Not even the miners.",
        "Cypherpunks wait for blocks, not banks.\nYour shielded TX is queued.\nThe math is done. The mining continues.",
        "Your transaction is invisible to surveillance.\nA miner will lock it into stone shortly.\nTrust the protocol.",
        "Mempool accepted. Block pending.\nThe network validates without seeing.\nThis is what financial privacy looks like.",
        "Shielded and waiting.\nNo address. No amount. No trace.\nJust a proof waiting for its block.",
    ];
    MSGS[rand::random::<usize>() % MSGS.len()]
}

/// Random cypherpunk message while awaiting incoming TX block confirmation.
pub fn random_pending_incoming_message() -> &'static str {
    const MSGS: &[&str] = &[
        "The shadows carry your wealth in silence.\nA miner will carve this proof into stone.",
        "Zero knowledge. Full sovereignty.\nYour incoming transfer awaits its block.",
        "Someone sent you shielded ZCL.\nThe mempool holds the proof. The chain will seal it.",
        "Privacy in transit. No addresses exposed.\nWaiting for the next block to finalize.",
        "A shielded note drifts through the mempool.\nInvisible to all but you. Block confirmation pending.",
        "The network validates without seeing.\nYour incoming funds are queued for the ledger.",
    ];
    MSGS[rand::random::<usize>() % MSGS.len()]
}

/// Random clearing (mempool) celebration message.
pub fn random_clearing_message() -> &'static str {
    const MSGS: &[&str] = &[
        "Transaction accepted by the network mempool.\nYour zero-knowledge proof passed validation.",
        "Peers accepted your shielded transaction.\nWaiting for a miner to seal it into a block.",
        "Mempool cleared. Your TX is queued for the next block.\nThe network validates. Trust the math.",
        "Proof verified by peers. Transaction is in the mempool.\nNo identity revealed. Awaiting block inclusion.",
        "Network nodes accepted your transaction.\nShielded, validated, waiting for settlement.",
    ];
    MSGS[rand::random::<usize>() % MSGS.len()]
}

/// Random receive celebration message.
pub fn random_receive_message() -> &'static str {
    const MSGS: &[&str] = &[
        "Incoming shielded transaction detected.\nYour privacy is intact. The sender is unknown.",
        "Funds received. Zero-knowledge proof verified.\nNo trace. No identity. Just math.",
        "Someone sent you ZCL through the privacy layer.\nShielded. Verified. Yours.",
        "New shielded payment received.\nThe chain delivers. The world sees nothing.",
        "Incoming transfer confirmed on-chain.\nPrivacy preserved on both ends.",
    ];
    MSGS[rand::random::<usize>() % MSGS.len()]
}

/// Random settlement (block confirmation) celebration message.
pub fn random_settlement_message() -> &'static str {
    const MSGS: &[&str] = &[
        "Your transaction is now etched into the chain.\nPrivacy preserved. No trace left behind.",
        "The miners have spoken.\nYour shielded TX is sealed in cryptographic stone forever.",
        "Zero-knowledge proof verified.\nAnother private transaction joins the immutable ledger.",
        "Confirmation received.\nYour funds moved without leaving a trace.\nThe chain remembers. The world does not.",
        "Block mined. Cypherpunks write code.\nMiners write history.\nYour privacy is now permanent.",
        "Trust math, not middlemen.\nYour transaction is confirmed and irreversible.",
        "The proof is in the block.\nShielded, verified, sealed.\nThis is financial sovereignty.",
        "Another block, another victory for privacy.\nNo KYC. No surveillance. Just math.",
        "Your transaction joined the longest chain.\nCensorship-resistant. Permissionless. Private.",
        "Confirmed. The network accepted your proof.\nNo identity revealed. No trail to follow.",
    ];
    MSGS[rand::random::<usize>() % MSGS.len()]
}

// ---------------------------------------------------------------------------
// Drop — zeroize ALL secrets
// ---------------------------------------------------------------------------

impl Drop for ZipherXApp {
    fn drop(&mut self) {
        // Zeroize spending key bytes
        if let Some(ref mut sk) = self.sk_bytes {
            for byte in sk.iter_mut() {
                unsafe { std::ptr::write_volatile(byte, 0) };
            }
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        }

        // Zeroize all sensitive strings
        self.password_input.zeroize();
        self.password_confirm.zeroize();
        self.mnemonic_input.zeroize();
        self.import_key_input.zeroize();
        self.send_address.zeroize();
        self.send_amount.zeroize();
        self.send_memo.zeroize();
        self.reauth_password.zeroize();
        self.export_password.zeroize();
        self.export_key_display.zeroize();
        self.node_rpc_password.zeroize();

        // Zeroize mnemonic words
        for word in self.mnemonic_words.iter_mut() {
            word.zeroize();
        }

        // Lock storage (clears derived key + cache)
        self.storage.lock();
    }
}
