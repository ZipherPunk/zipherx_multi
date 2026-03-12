//! ZipherX CLI — Full interactive wallet for Linux, Windows, and macOS.
//!
//! Uses the same Rust core as the mobile apps (AsyncWallet, SQLCipher, P2P sync).
//!
//! Commands:
//!   create              Create a new wallet (24-word mnemonic)
//!   restore <words>     Restore wallet from mnemonic
//!   import <key>        Import private key (hex)
//!   address / addr      Show wallet shielded address
//!   balance / bal       Show current balance
//!   send <addr> <amt>   Send ZCL (with optional memo)
//!   sync                Sync to chain tip
//!   history / txs       Show recent transactions
//!   peers               Show connected peer count
//!   tor                 Show Tor status
//!   export              Export spending key (requires password)
//!   delete              Delete all wallet data
//!   repair              Run database repair
//!   version             Show version info
//!   quit                Exit

mod platform;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{atomic::Ordering, Arc};

use zipherx_core::async_wallet::AsyncWallet;
use zipherx_core::send::SendRequest;
use zipherx_core::sync::SyncStatus;
use zipherx_core::wallet::WalletConfig;
use zipherx_platform::{PlatformInfo, SecureStorage};

use platform::{CliPlatformInfo, CliSecureStorage};

// ============================================================================
// ANSI Colors
// ============================================================================

const GREEN: &str = "\x1b[32m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

// ============================================================================
// Banner
// ============================================================================

fn print_banner() {
    println!("{GREEN}╔══════════════════════════════════════════════════════════╗");
    println!("║  {BRIGHT_GREEN}███████╗██╗██████╗ ██╗  ██╗███████╗██████╗ ██╗  ██╗{GREEN}║");
    println!("║  {BRIGHT_GREEN}╚══███╔╝██║██╔══██╗██║  ██║██╔════╝██╔══██╗╚██╗██╔╝{GREEN}║");
    println!("║  {BRIGHT_GREEN}  ███╔╝ ██║██████╔╝███████║█████╗  ██████╔╝ ╚███╔╝ {GREEN}║");
    println!("║  {BRIGHT_GREEN} ███╔╝  ██║██╔═══╝ ██╔══██║██╔══╝  ██╔══██╗ ██╔██╗ {GREEN}║");
    println!("║  {BRIGHT_GREEN}███████╗██║██║     ██║  ██║███████╗██║  ██║██╔╝ ██╗{GREEN}║");
    println!("║  {BRIGHT_GREEN}╚══════╝╚═╝╚═╝     ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝{GREEN}║");
    println!(
        "║  {DIM}Privacy-first Zclassic wallet       v{}{GREEN}          ║",
        env!("CARGO_PKG_VERSION")
    );
    println!("║  {DIM}\"Privacy is the power to selectively reveal oneself\"{GREEN} ║");
    println!("╚══════════════════════════════════════════════════════════╝{RESET}");
    println!();
}

// ============================================================================
// Zatoshi formatting
// ============================================================================

fn format_zcl(zatoshis: u64) -> String {
    let whole = zatoshis / 100_000_000;
    let frac = zatoshis % 100_000_000;
    if frac == 0 {
        format!("{}.0", whole)
    } else {
        let s = format!("{}.{:08}", whole, frac);
        s.trim_end_matches('0').to_string()
    }
}

fn parse_zcl(input: &str) -> Option<u64> {
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

// ============================================================================
// Wallet State (held across REPL iterations)
// ============================================================================

struct WalletState {
    platform_info: CliPlatformInfo,
    storage: Arc<CliSecureStorage>,
    wallet: Option<AsyncWallet>,
    sk_bytes: Option<Vec<u8>>,
    runtime: tokio::runtime::Runtime,
    db_encryption_key: Option<Vec<u8>>,
}

impl WalletState {
    fn data_dir(&self) -> PathBuf {
        self.platform_info.data_directory()
    }

    fn is_wallet_loaded(&self) -> bool {
        self.wallet.is_some() && self.sk_bytes.is_some()
    }

    fn require_wallet(&self) -> bool {
        if !self.is_wallet_loaded() {
            println!(
                "{}No wallet loaded. Use 'create' or 'restore' first.{}",
                YELLOW, RESET
            );
            return false;
        }
        true
    }

    /// Initialize the AsyncWallet from config.
    fn init_wallet(&mut self) -> Result<(), String> {
        let data_dir = self.data_dir();

        // Ensure subdirectories exist
        let _ = std::fs::create_dir_all(data_dir.join("delta"));

        let config = WalletConfig {
            db_path: data_dir.join("wallet.db").to_string_lossy().into(),
            header_store_path: data_dir.join("headers.db").to_string_lossy().into(),
            delta_store_dir: data_dir.join("delta").to_string_lossy().into(),
            spend_params_path: data_dir
                .join("sapling-spend.params")
                .to_string_lossy()
                .into(),
            output_params_path: data_dir
                .join("sapling-output.params")
                .to_string_lossy()
                .into(),
            account_index: 0,
            db_encryption_key: self.db_encryption_key.clone(),
            boost_cache_dir: None,
        };

        let wallet = self
            .runtime
            .block_on(AsyncWallet::initialize(config))
            .map_err(|e| format!("Wallet init failed: {}", e))?;

        self.wallet = Some(wallet);
        Ok(())
    }

    /// Connect to the P2P network.
    fn connect_network(&self) {
        if let Some(ref wallet) = self.wallet {
            print!("{}[network]{} Connecting to peers...", DIM, RESET);
            let _ = io::stdout().flush();
            match self.runtime.block_on(wallet.connect_network()) {
                Ok(()) => {
                    let count = wallet.get_connected_peer_count();
                    println!(
                        "\r{}[network]{} Connected to {} peer(s)          ",
                        GREEN, RESET, count
                    );
                }
                Err(e) => {
                    println!(
                        "\r{}[network]{} Connection failed: {}          ",
                        RED, RESET, e
                    );
                    println!(
                        "{}Wallet will work offline. Use 'sync' to retry.{}",
                        DIM, RESET
                    );
                }
            }
        }
    }

    /// Generate or load the DB encryption key.
    fn ensure_db_key(&mut self) -> Result<(), String> {
        let db_key_id = "db_encryption_key";
        if self.storage.has_key(db_key_id) {
            let key = self
                .storage
                .load_key(db_key_id)
                .map_err(|e| format!("Failed to load DB key: {}", e))?;
            self.db_encryption_key = Some(key);
        } else {
            // Generate new 32-byte key
            let mut key = vec![0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
            self.storage
                .store_key(db_key_id, &key)
                .map_err(|e| format!("Failed to store DB key: {}", e))?;
            self.db_encryption_key = Some(key);
        }
        Ok(())
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    print_banner();

    // Initialize platform
    let platform_info = CliPlatformInfo::new();
    let data_dir = platform_info.data_directory();
    println!(
        "{}[init]{} Data directory: {}",
        DIM,
        RESET,
        data_dir.display()
    );
    println!(
        "{}[init]{} OS: {} / {}",
        DIM,
        RESET,
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let storage = Arc::new(platform::create_secure_storage(&data_dir));

    // Build tokio runtime
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!(
                "{}[fatal]{} Failed to create async runtime: {}",
                RED, RESET, e
            );
            return;
        }
    };

    let mut state = WalletState {
        platform_info,
        storage: storage.clone(),
        wallet: None,
        sk_bytes: None,
        runtime,
        db_encryption_key: None,
    };

    // Check if wallet already exists
    let wallet_exists = storage.has_key("spending_key");

    if wallet_exists {
        println!();
        println!("{}Existing wallet detected.{}", BRIGHT_GREEN, RESET);
        // Ask for password to unlock
        let password = prompt_password("Enter password to unlock: ");
        if password.is_empty() {
            println!("{}Password required. Exiting.{}", RED, RESET);
            return;
        }
        storage.set_password(&password);

        // Try to load the spending key to verify password
        match storage.load_key("spending_key") {
            Ok(sk) => {
                state.sk_bytes = Some(sk.clone());
                println!("{}[unlock]{} Wallet unlocked.", GREEN, RESET);

                // Derive and show address
                match zipherx_crypto::keys::derive_address(&sk, 0) {
                    Ok((addr_bytes, _)) => {
                        if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                            println!(
                                "{}[wallet]{} Address: {}{}{}",
                                DIM, RESET, BRIGHT_GREEN, addr, RESET
                            );
                        }
                    }
                    Err(_) => {}
                }

                // Initialize wallet subsystems
                if let Err(e) = state.ensure_db_key() {
                    println!("{}[error]{} {}", RED, RESET, e);
                    println!("{}Continuing without encrypted DB.{}", YELLOW, RESET);
                }
                match state.init_wallet() {
                    Ok(()) => {
                        println!("{}[init]{} Wallet engine initialized.", GREEN, RESET);
                        state.connect_network();
                    }
                    Err(e) => {
                        println!("{}[error]{} {}", RED, RESET, e);
                    }
                }
            }
            Err(_) => {
                println!(
                    "{}[error]{} Wrong password or corrupted key file.",
                    RED, RESET
                );
                println!(
                    "{}You can 'create' a new wallet or 'restore' from mnemonic.{}",
                    DIM, RESET
                );
            }
        }
    } else {
        println!();
        println!(
            "{}No wallet found.{} Use '{}create{}' or '{}restore{}' to get started.",
            YELLOW, RESET, BRIGHT_GREEN, RESET, BRIGHT_GREEN, RESET
        );
    }

    println!();
    println!(
        "Type '{}help{}' for available commands.",
        BRIGHT_GREEN, RESET
    );
    println!();

    // Interactive REPL
    let mut rl = match rustyline::DefaultEditor::new() {
        Ok(editor) => editor,
        Err(_) => {
            eprintln!("Failed to initialize readline, falling back to basic input");
            run_basic_repl(&mut state);
            return;
        }
    };

    loop {
        let prompt = if state.is_wallet_loaded() {
            format!("{}zipherx{}{} > {}", BRIGHT_GREEN, RESET, GREEN, RESET)
        } else {
            format!("{}zipherx{} > ", DIM, RESET)
        };

        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line);
                if !handle_command(line, &mut state) {
                    break;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("Use '{}quit{}' to exit.", BRIGHT_GREEN, RESET);
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                break;
            }
            Err(e) => {
                eprintln!("{}Input error: {}{}", RED, e, RESET);
                break;
            }
        }
    }

    // Clean shutdown
    if let Some(ref wallet) = state.wallet {
        wallet.stop_background_sync();
    }
    println!("{}Goodbye.{}", DIM, RESET);
}

// ============================================================================
// Command dispatch
// ============================================================================

fn handle_command(input: &str, state: &mut WalletState) -> bool {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = parts[0].to_lowercase();

    match cmd.as_str() {
        "help" | "?" => print_help(),
        "version" | "v" => cmd_version(),
        "create" => cmd_create(state),
        "restore" => cmd_restore(&parts[1..], state),
        "import" => cmd_import(&parts[1..], state),
        "address" | "addr" => cmd_address(state),
        "balance" | "bal" => cmd_balance(state),
        "send" => cmd_send(&parts[1..], state),
        "sync" => cmd_sync(state),
        "history" | "txs" => cmd_history(state),
        "peers" => cmd_peers(state),
        "tor" => cmd_tor(),
        "export" => cmd_export(state),
        "delete" => cmd_delete(state),
        "repair" => cmd_repair(state),
        "validate" => cmd_validate(&parts[1..]),
        "quit" | "exit" | "q" => return false,
        _ => println!(
            "{}Unknown command: '{}'{}.  Type 'help' for available commands.",
            YELLOW, cmd, RESET
        ),
    }

    true
}

fn print_help() {
    println!("{}Available commands:{}", BOLD, RESET);
    println!(
        "  {}create{}              Create a new wallet (24-word mnemonic)",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}restore{} <words>     Restore wallet from mnemonic",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}import{} <key>        Import private key (hex)",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}address{}             Show wallet shielded address",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}balance{}             Show balance (total, spendable, notes)",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}send{} <addr> <amt>   Send ZCL (e.g. send zs1... 1.5 [memo])",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}sync{}                Sync wallet to chain tip",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}history{}             Show recent transactions",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}peers{}               Show connected peer count",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}tor{}                 Show Tor status",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}export{}              Export spending key (requires password)",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}delete{}              Delete all wallet data",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}repair{}              Run database repair",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}validate{} <addr>     Validate a Zclassic address",
        BRIGHT_GREEN, RESET
    );
    println!(
        "  {}version{}             Show version info",
        BRIGHT_GREEN, RESET
    );
    println!("  {}quit{}                Exit", BRIGHT_GREEN, RESET);
}

fn cmd_version() {
    println!(
        "{}ZipherX CLI{} v{}",
        BOLD,
        RESET,
        env!("CARGO_PKG_VERSION")
    );
    println!("OS: {} / {}", std::env::consts::OS, std::env::consts::ARCH);
}

// ============================================================================
// Create wallet
// ============================================================================

fn cmd_create(state: &mut WalletState) {
    if state.is_wallet_loaded() {
        println!(
            "{}A wallet is already loaded.{} Use 'delete' first to create a new one.",
            YELLOW, RESET
        );
        return;
    }

    println!("{}Generating new wallet...{}", DIM, RESET);

    let phrase = match zipherx_crypto::mnemonic::generate() {
        Ok(p) => p,
        Err(e) => {
            println!("{}[error]{} Mnemonic generation failed: {}", RED, RESET, e);
            return;
        }
    };

    let words: Vec<&str> = phrase.split_whitespace().collect();
    println!();
    println!(
        "{}{}=== YOUR 24-WORD RECOVERY PHRASE ==={}",
        BOLD, YELLOW, RESET
    );
    println!("{}WRITE THESE DOWN AND KEEP THEM SAFE!{}", RED, RESET);
    println!();
    for (i, word) in words.iter().enumerate() {
        print!("  {}{:>2}.{} {:<14}", DIM, i + 1, RESET, word);
        if (i + 1) % 4 == 0 {
            println!();
        }
    }
    println!();
    println!("{}{}=== END OF RECOVERY PHRASE ==={}", BOLD, YELLOW, RESET);
    println!();

    // Ask for password
    let password = prompt_new_password();
    if password.is_empty() {
        println!(
            "{}Password is required to encrypt your wallet.{}",
            RED, RESET
        );
        return;
    }

    state.storage.set_password(&password);

    // Derive spending key
    let seed = match zipherx_crypto::mnemonic::to_seed(&phrase) {
        Ok(s) => s,
        Err(e) => {
            println!("{}[error]{} Seed derivation failed: {}", RED, RESET, e);
            return;
        }
    };

    let sk_bytes = match zipherx_crypto::keys::derive_spending_key(&seed, 0) {
        Ok(sk) => sk,
        Err(e) => {
            println!("{}[error]{} Key derivation failed: {}", RED, RESET, e);
            return;
        }
    };

    // Store encrypted spending key
    if let Err(e) = state.storage.store_key("spending_key", &sk_bytes) {
        println!("{}[error]{} Failed to store key: {}", RED, RESET, e);
        return;
    }

    // Show address
    match zipherx_crypto::keys::derive_address(&sk_bytes, 0) {
        Ok((addr_bytes, _)) => {
            if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                println!(
                    "{}[wallet]{} Address: {}{}{}",
                    GREEN, RESET, BRIGHT_GREEN, addr, RESET
                );
            }
        }
        Err(e) => println!("{}[warn]{} Address derivation: {}", YELLOW, RESET, e),
    }

    state.sk_bytes = Some(sk_bytes.to_vec());

    // Initialize wallet engine
    if let Err(e) = state.ensure_db_key() {
        println!("{}[error]{} DB key: {}", RED, RESET, e);
        return;
    }
    match state.init_wallet() {
        Ok(()) => {
            println!("{}[init]{} Wallet engine initialized.", GREEN, RESET);
            state.connect_network();
        }
        Err(e) => println!("{}[error]{} {}", RED, RESET, e),
    }

    println!();
    println!("{}Wallet created successfully!{}", BRIGHT_GREEN, RESET);
    println!(
        "{}Save your recovery phrase — it is the ONLY way to recover your funds.{}",
        YELLOW, RESET
    );
}

// ============================================================================
// Restore wallet
// ============================================================================

fn cmd_restore(args: &[&str], state: &mut WalletState) {
    if state.is_wallet_loaded() {
        println!(
            "{}A wallet is already loaded.{} Use 'delete' first.",
            YELLOW, RESET
        );
        return;
    }

    let words_str = if args.is_empty() {
        println!("Enter your 24-word mnemonic (space-separated):");
        print!("{}> {}", GREEN, RESET);
        let _ = io::stdout().flush();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap_or(0);
        input.trim().to_string()
    } else {
        args.join(" ")
    };

    let word_count = words_str.split_whitespace().count();
    if word_count != 24 {
        println!(
            "{}[error]{} Expected 24 words, got {}",
            RED, RESET, word_count
        );
        return;
    }

    if !zipherx_crypto::mnemonic::validate(&words_str) {
        println!("{}[error]{} Invalid mnemonic phrase", RED, RESET);
        return;
    }

    println!("{}[ok]{} Mnemonic validated.", GREEN, RESET);

    // Ask for password
    let password = prompt_new_password();
    if password.is_empty() {
        println!("{}Password is required.{}", RED, RESET);
        return;
    }

    state.storage.set_password(&password);

    let seed = match zipherx_crypto::mnemonic::to_seed(&words_str) {
        Ok(s) => s,
        Err(e) => {
            println!("{}[error]{} Seed derivation: {}", RED, RESET, e);
            return;
        }
    };

    let sk_bytes = match zipherx_crypto::keys::derive_spending_key(&seed, 0) {
        Ok(sk) => sk,
        Err(e) => {
            println!("{}[error]{} Key derivation: {}", RED, RESET, e);
            return;
        }
    };

    if let Err(e) = state.storage.store_key("spending_key", &sk_bytes) {
        println!("{}[error]{} Failed to store key: {}", RED, RESET, e);
        return;
    }

    match zipherx_crypto::keys::derive_address(&sk_bytes, 0) {
        Ok((addr_bytes, _)) => {
            if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                println!(
                    "{}[wallet]{} Address: {}{}{}",
                    GREEN, RESET, BRIGHT_GREEN, addr, RESET
                );
            }
        }
        Err(e) => println!("{}[warn]{} Address derivation: {}", YELLOW, RESET, e),
    }

    state.sk_bytes = Some(sk_bytes.to_vec());

    if let Err(e) = state.ensure_db_key() {
        println!("{}[error]{} DB key: {}", RED, RESET, e);
        return;
    }
    match state.init_wallet() {
        Ok(()) => {
            println!("{}[init]{} Wallet engine initialized.", GREEN, RESET);
            state.connect_network();
        }
        Err(e) => println!("{}[error]{} {}", RED, RESET, e),
    }

    println!(
        "{}Wallet restored. Run 'sync' to scan for your transactions.{}",
        BRIGHT_GREEN, RESET
    );
}

// ============================================================================
// Import private key
// ============================================================================

fn cmd_import(args: &[&str], state: &mut WalletState) {
    if state.is_wallet_loaded() {
        println!(
            "{}A wallet is already loaded.{} Use 'delete' first.",
            YELLOW, RESET
        );
        return;
    }

    if args.is_empty() {
        println!("Usage: import <spending_key_hex>");
        return;
    }

    let sk_hex = args[0];
    let sk_bytes = match hex::decode(sk_hex) {
        Ok(b) => b,
        Err(e) => {
            println!("{}[error]{} Invalid hex: {}", RED, RESET, e);
            return;
        }
    };

    if sk_bytes.len() != 32 {
        println!(
            "{}[error]{} Spending key must be 32 bytes (got {})",
            RED,
            RESET,
            sk_bytes.len()
        );
        return;
    }

    // Validate the key can derive an address
    match zipherx_crypto::keys::derive_address(&sk_bytes, 0) {
        Ok((addr_bytes, _)) => {
            if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                println!(
                    "{}[wallet]{} Address: {}{}{}",
                    GREEN, RESET, BRIGHT_GREEN, addr, RESET
                );
            }
        }
        Err(e) => {
            println!("{}[error]{} Invalid spending key: {}", RED, RESET, e);
            return;
        }
    }

    let password = prompt_new_password();
    if password.is_empty() {
        println!("{}Password is required.{}", RED, RESET);
        return;
    }

    state.storage.set_password(&password);

    if let Err(e) = state.storage.store_key("spending_key", &sk_bytes) {
        println!("{}[error]{} Failed to store key: {}", RED, RESET, e);
        return;
    }

    state.sk_bytes = Some(sk_bytes);

    if let Err(e) = state.ensure_db_key() {
        println!("{}[error]{} DB key: {}", RED, RESET, e);
        return;
    }
    match state.init_wallet() {
        Ok(()) => {
            println!("{}[init]{} Wallet engine initialized.", GREEN, RESET);
            state.connect_network();
        }
        Err(e) => println!("{}[error]{} {}", RED, RESET, e),
    }

    println!(
        "{}Key imported. Run 'sync' to scan for your transactions.{}",
        BRIGHT_GREEN, RESET
    );
}

// ============================================================================
// Address
// ============================================================================

fn cmd_address(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    let sk = state.sk_bytes.as_ref().unwrap();
    match zipherx_crypto::keys::derive_address(sk, 0) {
        Ok((addr_bytes, _)) => match zipherx_crypto::address::encode_address(&addr_bytes) {
            Ok(addr) => println!(
                "{}Address:{} {}{}{}",
                BOLD, RESET, BRIGHT_GREEN, addr, RESET
            ),
            Err(e) => println!("{}[error]{} Encoding: {}", RED, RESET, e),
        },
        Err(e) => println!("{}[error]{} Derivation: {}", RED, RESET, e),
    }
}

// ============================================================================
// Balance
// ============================================================================

fn cmd_balance(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    let wallet = state.wallet.as_ref().unwrap();
    match state.runtime.block_on(wallet.get_balance()) {
        Ok(balance) => {
            println!("{}Balance:{}", BOLD, RESET);
            println!(
                "  {}Total:{}     {} ZCL",
                BRIGHT_GREEN,
                RESET,
                format_zcl(balance.total)
            );
            println!(
                "  {}Spendable:{} {} ZCL",
                GREEN,
                RESET,
                format_zcl(balance.spendable)
            );
            println!(
                "  {}Notes:{}     {} total, {} spendable",
                DIM, RESET, balance.note_count, balance.spendable_note_count
            );
        }
        Err(e) => println!("{}[error]{} Balance query failed: {}", RED, RESET, e),
    }
}

// ============================================================================
// Send
// ============================================================================

fn cmd_send(args: &[&str], state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    if args.len() < 2 {
        println!("Usage: send <address> <amount> [memo]");
        println!("  Example: send zs1abc...xyz 1.5");
        println!("  Example: send zs1abc...xyz 0.001 \"Thanks!\"");
        return;
    }

    let to_address = args[0].to_string();
    let amount_str = args[1];
    let memo = if args.len() > 2 {
        Some(args[2..].join(" "))
    } else {
        None
    };

    // Validate address
    if !zipherx_crypto::address::validate_address(&to_address) {
        println!("{}[error]{} Invalid Zclassic address.", RED, RESET);
        return;
    }

    // Parse amount
    let amount_zatoshis = match parse_zcl(amount_str) {
        Some(a) if a > 0 => a,
        _ => {
            println!("{}[error]{} Invalid amount: {}", RED, RESET, amount_str);
            return;
        }
    };

    let fee = zipherx_core::send::DEFAULT_FEE;

    // Confirmation
    println!();
    println!("{}{}=== SEND CONFIRMATION ==={}", BOLD, YELLOW, RESET);
    println!("  To:     {}", to_address);
    println!("  Amount: {} ZCL", format_zcl(amount_zatoshis));
    println!("  Fee:    {} ZCL", format_zcl(fee));
    println!("  Total:  {} ZCL", format_zcl(amount_zatoshis + fee));
    if let Some(ref m) = memo {
        println!("  Memo:   {}", m);
    }
    println!();
    print!("{}Confirm? (yes/no): {}", YELLOW, RESET);
    let _ = io::stdout().flush();

    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm).unwrap_or(0);
    if confirm.trim().to_lowercase() != "yes" {
        println!("{}Send cancelled.{}", DIM, RESET);
        return;
    }

    let request = SendRequest {
        to_address,
        amount_zatoshis,
        fee_zatoshis: fee,
        memo,
    };

    let sk = state.sk_bytes.as_ref().unwrap();
    let wallet = state.wallet.as_ref().unwrap();

    let progress_fn: Option<zipherx_core::async_send::SendProgressFn> = Some(Arc::new(|phase| {
        use zipherx_core::async_send::SendPhase;
        let msg = match phase {
            SendPhase::Validating => "Validating request...".to_string(),
            SendPhase::NoteSelection { count, total_value } => {
                format!("Selected {} notes ({} zatoshis)", count, total_value)
            }
            SendPhase::WitnessValidation { note_index, total } => {
                format!("Validating witness {}/{}...", note_index, total)
            }
            SendPhase::Building {
                spend_index,
                total_spends,
            } => {
                format!("Building proof {}/{}...", spend_index, total_spends)
            }
            SendPhase::Broadcasting => "Broadcasting to network...".to_string(),
            SendPhase::PeerResponse { accepted, rejected, total } => {
                if *rejected > 0 {
                    format!("Peers: {}/{} accepted, {} REJECTED", accepted, total, rejected)
                } else {
                    format!("Peers: {}/{} accepted", accepted, total)
                }
            }
            SendPhase::Recording => "Recording in database...".to_string(),
            SendPhase::Complete { ref txid } => {
                format!("Complete! txid={}", txid)
            }
            SendPhase::Error { ref message } => {
                eprintln!("\r{}[send]{} Failed: {}", RED, RESET, message);
                return;
            }
        };
        print!("\r{}[send]{} {}                    ", DIM, RESET, msg);
        let _ = io::stdout().flush();
    }));

    println!();
    match state
        .runtime
        .block_on(wallet.send(request, sk, progress_fn))
    {
        Ok(result) => {
            println!();
            println!("{}Transaction sent!{}", BRIGHT_GREEN, RESET);
            println!("  TXID:   {}{}{}", DIM, result.txid, RESET);
            println!("  Amount: {} ZCL", format_zcl(result.amount));
            println!("  Fee:    {} ZCL", format_zcl(result.fee));
            println!("  Change: {} ZCL", format_zcl(result.change_value));
            println!("  Notes:  {} used", result.notes_used);
        }
        Err(e) => {
            println!();
            println!("{}[error]{} Send failed: {}", RED, RESET, e);
        }
    }
}

// ============================================================================
// Sync
// ============================================================================

fn cmd_sync(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    let wallet = state.wallet.as_ref().unwrap();
    let sk = state.sk_bytes.as_ref().unwrap();

    // First try to reconnect if needed
    let peer_count = wallet.get_connected_peer_count();
    if peer_count == 0 {
        print!("{}[sync]{} No peers — reconnecting...", DIM, RESET);
        let _ = io::stdout().flush();
        match state.runtime.block_on(wallet.connect_network()) {
            Ok(()) => {
                println!(
                    "\r{}[sync]{} Reconnected ({} peers)          ",
                    GREEN,
                    RESET,
                    wallet.get_connected_peer_count()
                );
            }
            Err(e) => {
                println!("\r{}[sync]{} Network error: {}          ", RED, RESET, e);
                return;
            }
        }
    }

    let peer_count_ref = wallet.connected_peer_count.clone();
    let progress_fn: Option<zipherx_core::async_sync::SyncProgressFn> =
        Some(Arc::new(move |status| {
            let msg = match &status {
                SyncStatus::Idle => return,
                SyncStatus::BoostDownload {
                    downloaded_bytes,
                    total_bytes,
                } => {
                    let pct = if *total_bytes > 0 {
                        downloaded_bytes * 100 / total_bytes
                    } else {
                        0
                    };
                    format!(
                        "Boost download: {:.1} MB / {:.1} MB ({}%)",
                        *downloaded_bytes as f64 / 1_048_576.0,
                        *total_bytes as f64 / 1_048_576.0,
                        pct
                    )
                }
                SyncStatus::BoostLoad { loaded, total } => {
                    let pct = if *total > 0 { loaded * 100 / total } else { 0 };
                    format!("Loading boost: {} / {} ({}%)", loaded, total, pct)
                }
                SyncStatus::HeaderSync {
                    current_height,
                    target_height,
                } => {
                    let pct = if *target_height > 0 {
                        current_height * 100 / target_height
                    } else {
                        0
                    };
                    format!("Headers: {} / {} ({}%)", current_height, target_height, pct)
                }
                SyncStatus::DeltaSync {
                    current_height,
                    target_height,
                } => {
                    let pct = if *target_height > 0 {
                        current_height * 100 / target_height
                    } else {
                        0
                    };
                    format!(
                        "Delta sync: {} / {} ({}%)",
                        current_height, target_height, pct
                    )
                }
                SyncStatus::BoostScan { outputs_total } => {
                    format!(
                        "Scanning boost outputs: {} outputs (CPU-intensive)",
                        outputs_total
                    )
                }
                SyncStatus::BlockScan {
                    current_height,
                    target_height,
                    notes_found,
                } => {
                    let pct = if *target_height > 0 {
                        current_height * 100 / target_height
                    } else {
                        0
                    };
                    format!(
                        "Block scan: {} / {} ({}%) — {} notes found",
                        current_height, target_height, pct, notes_found
                    )
                }
                SyncStatus::GapFill { gaps_remaining } => {
                    format!("Filling gaps: {} remaining", gaps_remaining)
                }
                SyncStatus::WitnessUpdate {
                    notes_updated,
                    total_notes,
                } => {
                    format!("Witnesses: {} / {}", notes_updated, total_notes)
                }
                SyncStatus::Complete { height } => {
                    format!("Sync complete at height {}", height)
                }
                SyncStatus::Failed(ref reason) => {
                    format!("Sync failed: {}", reason)
                }
            };
            let peers = peer_count_ref.load(Ordering::Relaxed);
            print!(
                "\r{}[sync]{} {} {}({} peers){}          ",
                GREEN, RESET, msg, DIM, peers, RESET
            );
            let _ = io::stdout().flush();
        }));

    println!("{}[sync]{} Starting sync...", GREEN, RESET);
    match state.runtime.block_on(wallet.sync(sk, progress_fn)) {
        Ok(height) => {
            println!();
            println!(
                "{}[sync]{} Synced to height {}{}{}",
                GREEN, RESET, BRIGHT_GREEN, height, RESET
            );
        }
        Err(e) => {
            println!();
            println!("{}[sync]{} Sync error: {}", RED, RESET, e);
        }
    }
}

// ============================================================================
// Transaction history
// ============================================================================

fn cmd_history(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    let wallet = state.wallet.as_ref().unwrap();
    match state
        .runtime
        .block_on(wallet.get_transaction_history(20, 0))
    {
        Ok(txs) => {
            if txs.is_empty() {
                println!(
                    "{}No transactions yet.{} Run 'sync' to scan the blockchain.",
                    DIM, RESET
                );
                return;
            }

            println!("{}Recent transactions:{}", BOLD, RESET);
            println!(
                "  {}{:<10} {:<14} {:<12} {:<8} {}{}",
                DIM, "Type", "Amount (ZCL)", "Height", "Confs", "TXID", RESET
            );
            println!("  {}{}{}", DIM, "-".repeat(70), RESET);

            for tx in &txs {
                let type_color = match tx.tx_type.as_str() {
                    "received" => GREEN,
                    "sent" => YELLOW,
                    _ => DIM,
                };
                let sign = if tx.tx_type == "sent" { "-" } else { "+" };
                println!(
                    "  {}{:<10}{} {}{}{:<14}{} {:<12} {:<8} {}{}{}",
                    type_color,
                    tx.tx_type,
                    RESET,
                    type_color,
                    sign,
                    format_zcl(tx.amount),
                    RESET,
                    tx.height,
                    tx.confirmations,
                    DIM,
                    &tx.txid[..16],
                    RESET,
                );
                if let Some(ref memo) = tx.memo {
                    if !memo.is_empty() {
                        println!("    {}memo: {}{}", DIM, memo, RESET);
                    }
                }
            }

            // Show totals
            match state.runtime.block_on(wallet.get_transaction_counts()) {
                Ok((in_count, out_count)) => {
                    println!();
                    println!(
                        "  {}Total: {} received, {} sent{}",
                        DIM, in_count, out_count, RESET
                    );
                }
                Err(_) => {}
            }
        }
        Err(e) => println!("{}[error]{} History query failed: {}", RED, RESET, e),
    }
}

// ============================================================================
// Peers
// ============================================================================

fn cmd_peers(state: &WalletState) {
    if let Some(ref wallet) = state.wallet {
        let count = wallet.get_connected_peer_count();
        println!("{}Connected peers:{} {}", BOLD, RESET, count);
    } else {
        println!(
            "{}Wallet not initialized — no peer connections.{}",
            DIM, RESET
        );
    }
}

// ============================================================================
// Tor
// ============================================================================

fn cmd_tor() {
    let state = zipherx_tor::client::get_state();
    let (state_str, color) = match state {
        zipherx_tor::TorState::Disconnected => ("Disconnected", DIM),
        zipherx_tor::TorState::Connecting => ("Connecting", YELLOW),
        zipherx_tor::TorState::Bootstrapping => ("Bootstrapping", YELLOW),
        zipherx_tor::TorState::Connected => ("Connected", GREEN),
        zipherx_tor::TorState::Error => ("Error", RED),
    };
    println!(
        "{}Tor status:{} {}{}{}",
        BOLD, RESET, color, state_str, RESET
    );

    let port = zipherx_tor::client::get_socks_port();
    if port > 0 {
        println!("  SOCKS5 port: {}", port);
    }

    let progress = zipherx_tor::client::get_bootstrap_progress();
    println!("  Bootstrap: {}%", progress);

    if let Some(onion) = zipherx_tor::hidden_service::get_onion_address() {
        println!("  Onion: {}", onion);
    }
}

// ============================================================================
// Export
// ============================================================================

fn cmd_export(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    println!(
        "{}WARNING: Exporting your spending key gives FULL access to your funds.{}",
        RED, RESET
    );
    println!("{}Never share it with anyone.{}", YELLOW, RESET);
    println!();

    let password = prompt_password("Re-enter your password to confirm: ");
    if password.is_empty() {
        println!("{}Export cancelled.{}", DIM, RESET);
        return;
    }

    // Verify password by trying to derive the same session key and decrypt
    // We create a temporary storage to test the password
    let test_storage = platform::create_secure_storage(&state.data_dir());
    test_storage.set_password(&password);
    match test_storage.load_key("spending_key") {
        Ok(sk) => {
            let sk_hex = hex::encode(&sk);
            println!();
            println!("{}Spending key (hex):{}", BOLD, RESET);
            println!("  {}{}{}", BRIGHT_GREEN, sk_hex, RESET);
            println!();
            println!(
                "{}This key has been displayed ONCE. Copy it securely now.{}",
                YELLOW, RESET
            );
        }
        Err(_) => {
            println!("{}[error]{} Wrong password.", RED, RESET);
        }
    }
}

// ============================================================================
// Delete
// ============================================================================

fn cmd_delete(state: &mut WalletState) {
    println!(
        "{}{}WARNING: This will PERMANENTLY delete your wallet data!{}",
        BOLD, RED, RESET
    );
    println!(
        "{}Make sure you have backed up your recovery phrase or spending key.{}",
        YELLOW, RESET
    );
    println!();
    print!("{}Type 'DELETE' to confirm: {}", RED, RESET);
    let _ = io::stdout().flush();

    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm).unwrap_or(0);
    if confirm.trim() != "DELETE" {
        println!("{}Delete cancelled.{}", DIM, RESET);
        return;
    }

    // Stop background sync
    if let Some(ref wallet) = state.wallet {
        wallet.stop_background_sync();
    }

    let data_dir = state.data_dir();

    // Delete key files
    let _ = state.storage.delete_key("spending_key");
    let _ = state.storage.delete_key("db_encryption_key");

    // Delete database files
    let _ = std::fs::remove_file(data_dir.join("wallet.db"));
    let _ = std::fs::remove_file(data_dir.join("headers.db"));
    let _ = std::fs::remove_dir_all(data_dir.join("delta"));
    let _ = std::fs::remove_dir_all(data_dir.join("keys"));

    state.wallet = None;
    state.sk_bytes = None;
    state.db_encryption_key = None;

    println!("{}[delete]{} Wallet data deleted.", GREEN, RESET);
    println!(
        "{}Use 'create' or 'restore' to set up a new wallet.{}",
        DIM, RESET
    );
}

// ============================================================================
// Repair
// ============================================================================

fn cmd_repair(state: &WalletState) {
    if !state.require_wallet() {
        return;
    }

    println!("{}[repair]{} Running database repair...", YELLOW, RESET);
    let wallet = state.wallet.as_ref().unwrap();
    match state.runtime.block_on(wallet.repair_database()) {
        Ok(()) => {
            println!("{}[repair]{} Database repair complete.", GREEN, RESET);
            println!("{}Run 'sync' to rescan the blockchain.{}", DIM, RESET);
        }
        Err(e) => println!("{}[repair]{} Repair failed: {}", RED, RESET, e),
    }
}

// ============================================================================
// Validate
// ============================================================================

fn cmd_validate(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: validate <zclassic_address>");
        return;
    }
    let addr = args[0];
    if zipherx_crypto::address::validate_address(addr) {
        println!("{}Valid{} Zclassic shielded address.", GREEN, RESET);
    } else {
        println!("{}INVALID{} address.", RED, RESET);
    }
}

// ============================================================================
// Password prompts
// ============================================================================

fn prompt_password(prompt: &str) -> String {
    match rpassword::prompt_password(format!("{}{}{}", GREEN, prompt, RESET)) {
        Ok(p) => p,
        Err(_) => {
            // Fallback for environments where rpassword doesn't work
            print!("{}{}{}", GREEN, prompt, RESET);
            let _ = io::stdout().flush();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap_or(0);
            input.trim().to_string()
        }
    }
}

fn prompt_new_password() -> String {
    let p1 = prompt_password("Set wallet password: ");
    if p1.is_empty() {
        return String::new();
    }
    if p1.len() < 8 {
        println!("{}Password must be at least 8 characters.{}", RED, RESET);
        return String::new();
    }
    let p2 = prompt_password("Confirm password: ");
    if p1 != p2 {
        println!("{}Passwords do not match.{}", RED, RESET);
        return String::new();
    }
    p1
}

// ============================================================================
// Fallback basic REPL (no readline)
// ============================================================================

fn run_basic_repl(state: &mut WalletState) {
    loop {
        let prompt = if state.is_wallet_loaded() {
            format!("{}zipherx{} > ", BRIGHT_GREEN, RESET)
        } else {
            format!("{}zipherx{} > ", DIM, RESET)
        };
        print!("{}", prompt);
        let _ = io::stdout().flush();
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break,
            Ok(_) => {
                let line = input.trim();
                if line.is_empty() {
                    continue;
                }
                if !handle_command(line, state) {
                    break;
                }
            }
            Err(e) => {
                eprintln!("{}Input error: {}{}", RED, e, RESET);
                break;
            }
        }
    }
    if let Some(ref wallet) = state.wallet {
        wallet.stop_background_sync();
    }
    println!("{}Goodbye.{}", DIM, RESET);
}
