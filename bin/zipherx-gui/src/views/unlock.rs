//! Unlock screen — password entry, create/restore/import wallet.

use zeroize::Zeroize;
use zipherx_platform::SecureStorage;

use crate::app::{Phase, SetupMode, ZipherXApp};
use crate::sync;
use crate::theme;

/// Show the unlock / setup screen.
pub fn show(app: &mut ZipherXApp, ctx: &egui::Context) {
    let has_wallet = app.storage.has_key("spending_key");

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);

            // Logo
            crate::widgets::logo::show_logo(app, ui, ctx, 80.0);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("ZIPHERX")
                    .font(theme::mono(28.0))
                    .color(theme::GREEN),
            );
            ui.label(
                egui::RichText::new("Privacy-first Zclassic wallet")
                    .font(theme::mono(12.0))
                    .color(theme::MUTED),
            );
            ui.add_space(30.0);

            if has_wallet && app.setup_mode.is_none() {
                show_unlock(app, ui);
            } else if let Some(mode) = app.setup_mode {
                show_setup_flow(app, ui, mode);
            } else {
                show_password_create(app, ui);
            }
        });
    });
}

/// Password entry to unlock an existing wallet.
fn show_unlock(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Enter password to unlock")
            .font(theme::mono(14.0))
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    let response = ui.add(
        egui::TextEdit::singleline(&mut app.password_input)
            .password(true)
            .hint_text("Password")
            .font(theme::mono(14.0))
            .desired_width((ui.available_width() - 20.0).min(350.0)),
    );

    if let Some(ref err) = app.password_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }

    ui.add_space(15.0);
    let enter_pressed = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let unlock_clicked = ui
        .add(egui::Button::new(
            egui::RichText::new("[ UNLOCK ]")
                .font(theme::mono(14.0))
                .color(theme::GREEN),
        ))
        .clicked();

    if enter_pressed || unlock_clicked {
        handle_unlock(app);
    }
}

/// Password creation screen (first time, no wallet yet).
fn show_password_create(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Create a password to secure your wallet")
            .font(theme::mono(13.0))
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    let field_width = (ui.available_width() - 20.0).min(350.0);
    ui.add(
        egui::TextEdit::singleline(&mut app.password_input)
            .password(true)
            .hint_text("Password")
            .font(theme::mono(14.0))
            .desired_width(field_width),
    );
    ui.add_space(5.0);
    ui.add(
        egui::TextEdit::singleline(&mut app.password_confirm)
            .password(true)
            .hint_text("Confirm password")
            .font(theme::mono(14.0))
            .desired_width(field_width),
    );

    if let Some(ref err) = app.password_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }

    ui.add_space(20.0);

    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ CREATE NEW WALLET ]")
                    .font(theme::mono(13.0))
                    .color(theme::GREEN),
            ))
            .clicked()
        {
            handle_password_then_setup(app, SetupMode::Create);
        }
        ui.add_space(10.0);
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ RESTORE ]")
                    .font(theme::mono(13.0))
                    .color(theme::CYAN),
            ))
            .clicked()
        {
            handle_password_then_setup(app, SetupMode::Restore);
        }
        ui.add_space(10.0);
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ IMPORT KEY ]")
                    .font(theme::mono(13.0))
                    .color(theme::CYAN),
            ))
            .clicked()
        {
            handle_password_then_setup(app, SetupMode::Import);
        }
    });
}

/// Setup flow after password is created.
fn show_setup_flow(app: &mut ZipherXApp, ui: &mut egui::Ui, mode: SetupMode) {
    match mode {
        SetupMode::Create => show_create_result(app, ui),
        SetupMode::Restore => show_restore_flow(app, ui),
        SetupMode::Import => show_import_flow(app, ui),
    }
}

/// Show mnemonic words after wallet creation.
fn show_create_result(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    if app.mnemonic_words.is_empty() {
        ui.label(
            egui::RichText::new("Creating wallet...")
                .font(theme::mono(14.0))
                .color(theme::MUTED),
        );
        return;
    }

    ui.label(
        egui::RichText::new("WRITE DOWN YOUR SEED PHRASE")
            .font(theme::mono(16.0))
            .color(theme::YELLOW),
    );
    ui.label(
        egui::RichText::new("Store this securely. It is the ONLY way to recover your wallet.")
            .font(theme::mono(11.0))
            .color(theme::RED),
    );
    ui.add_space(15.0);

    // Display words in a 4x6 grid
    egui::Grid::new("mnemonic_grid")
        .num_columns(4)
        .spacing([20.0, 8.0])
        .show(ui, |ui| {
            for (i, word) in app.mnemonic_words.iter().enumerate() {
                ui.label(
                    egui::RichText::new(format!("{:>2}. {}", i + 1, word))
                        .font(theme::mono(13.0))
                        .color(theme::GREEN),
                );
                if (i + 1) % 4 == 0 {
                    ui.end_row();
                }
            }
        });

    ui.add_space(20.0);
    if ui
        .add(egui::Button::new(
            egui::RichText::new("[ I HAVE SAVED MY SEED PHRASE ]")
                .font(theme::mono(14.0))
                .color(theme::GREEN),
        ))
        .clicked()
    {
        // Zeroize words from display
        for word in app.mnemonic_words.iter_mut() {
            word.zeroize();
        }
        app.mnemonic_words.clear();
        finalize_wallet(app);
    }

    if let Some(ref err) = app.setup_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }
}

/// Restore from mnemonic seed phrase.
fn show_restore_flow(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Enter your 24-word seed phrase")
            .font(theme::mono(14.0))
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    ui.add(
        egui::TextEdit::multiline(&mut app.mnemonic_input)
            .hint_text("word1 word2 word3 ... word24")
            .font(theme::mono(12.0))
            .desired_width((ui.available_width() - 20.0).min(450.0))
            .desired_rows(3),
    );

    if let Some(ref err) = app.setup_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ RESTORE ]")
                    .font(theme::mono(14.0))
                    .color(theme::GREEN),
            ))
            .clicked()
        {
            handle_restore(app);
        }
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ BACK ]")
                    .font(theme::mono(14.0))
                    .color(theme::MUTED),
            ))
            .clicked()
        {
            app.setup_mode = None;
            app.mnemonic_input.zeroize();
            app.setup_error = None;
        }
    });
}

/// Import from private key (hex or encoded).
fn show_import_flow(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("Enter your private key")
            .font(theme::mono(14.0))
            .color(theme::MUTED),
    );
    ui.label(
        egui::RichText::new("(hex or secret-extended-key format)")
            .font(theme::mono(11.0))
            .color(theme::MUTED),
    );
    ui.add_space(10.0);

    ui.add(
        egui::TextEdit::singleline(&mut app.import_key_input)
            .password(true)
            .hint_text("Private key")
            .font(theme::mono(12.0))
            .desired_width((ui.available_width() - 20.0).min(450.0)),
    );

    if let Some(ref err) = app.setup_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ IMPORT ]")
                    .font(theme::mono(14.0))
                    .color(theme::GREEN),
            ))
            .clicked()
        {
            handle_import(app);
        }
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ BACK ]")
                    .font(theme::mono(14.0))
                    .color(theme::MUTED),
            ))
            .clicked()
        {
            app.setup_mode = None;
            app.import_key_input.zeroize();
            app.setup_error = None;
        }
    });
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn handle_unlock(app: &mut ZipherXApp) {
    if app.password_input.is_empty() {
        app.password_error = Some("Password required".into());
        return;
    }

    app.storage.set_password(&app.password_input);

    // Try to load spending key to verify password
    match app.storage.load_key("spending_key") {
        Ok(sk) => {
            // Derive address
            match zipherx_crypto::keys::derive_address(&sk, 0) {
                Ok((addr_bytes, _)) => {
                    if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                        app.address = Some(addr);
                    }
                }
                Err(_) => {}
            }
            app.sk_bytes = Some(sk);
            app.password_error = None;
            app.password_input.zeroize();
            app.password_confirm.zeroize();
            app.phase = Phase::Ready;

            // Reuse existing wallet thread if present (re-unlock after auto-lock),
            // otherwise spawn a new one (first unlock after app start).
            if app.shared_state.is_none() {
                // Set syncing state immediately so the UI shows progress
                app.is_syncing = true;
                app.sync_phase = "Initializing wallet...".to_string();
                app.sync_tasks.clear();
                app.overall_progress = 0.0;
                app.sync_start_time = Some(std::time::Instant::now());

                // Start background wallet thread
                let state = sync::start_wallet_thread(
                    app.data_dir.clone(),
                    app.storage.clone(),
                );
                app.shared_state = Some(state.clone());

                // Trigger initial sync
                if let Some(ref sk) = app.sk_bytes {
                    if let Ok(mut s) = state.lock() {
                        s.command = Some(sync::SyncCommand::StartSync {
                            sk_bytes: sk.clone(),
                        });
                    }
                }
            } else {
                // Wallet thread already running — trigger a sync to refresh
                if let Some(ref state) = app.shared_state {
                    if let Some(ref sk) = app.sk_bytes {
                        if let Ok(mut s) = state.lock() {
                            s.command = Some(sync::SyncCommand::StartSync {
                                sk_bytes: sk.clone(),
                            });
                        }
                    }
                }
                app.is_syncing = true;
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("wrong password") || msg.contains("decryption failed") {
                app.password_error = Some("Wrong password".into());
            } else if msg.contains("KeyNotFound") {
                // No wallet yet — password accepted, go to setup
                app.password_error = None;
                app.password_input.zeroize();
                // Keep password set in storage for later key encryption
                app.phase = Phase::Locked;
                // Show setup options
                app.setup_mode = None; // Will show create/restore/import buttons
                // Actually transition: since has_wallet is false, show_password_create
                // is shown. Once user clicks a button, handle_password_then_setup runs.
            } else {
                app.password_error = Some(format!("Unlock failed: {}", msg));
            }
            app.storage.lock();
        }
    }
}

fn handle_password_then_setup(app: &mut ZipherXApp, mode: SetupMode) {
    if app.password_input.is_empty() {
        app.password_error = Some("Password required".into());
        return;
    }
    if app.password_input != app.password_confirm {
        app.password_error = Some("Passwords do not match".into());
        return;
    }
    if app.password_input.len() < 8 {
        app.password_error = Some("Password must be at least 8 characters".into());
        return;
    }

    app.storage.set_password(&app.password_input);
    app.password_error = None;

    match mode {
        SetupMode::Create => {
            // Create wallet
            match zipherx_crypto::mnemonic::generate() {
                Ok(phrase) => {
                    match zipherx_crypto::mnemonic::to_seed(&phrase) {
                        Ok(seed) => {
                            match zipherx_crypto::keys::derive_spending_key(&seed, 0) {
                                Ok(sk) => {
                                    // Store spending key
                                    if let Err(e) = app.storage.store_key("spending_key", &sk) {
                                        app.setup_error = Some(format!("Failed to store key: {}", e));
                                        return;
                                    }
                                    app.sk_bytes = Some(sk.to_vec());
                                    app.mnemonic_words = phrase.split_whitespace().map(|w| w.to_string()).collect();
                                    app.setup_mode = Some(SetupMode::Create);
                                }
                                Err(e) => {
                                    app.setup_error = Some(format!("Key derivation failed: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            app.setup_error = Some(format!("Seed derivation failed: {}", e));
                        }
                    }
                }
                Err(e) => {
                    app.setup_error = Some(format!("Mnemonic generation failed: {}", e));
                }
            }
        }
        SetupMode::Restore => {
            app.setup_mode = Some(SetupMode::Restore);
        }
        SetupMode::Import => {
            app.setup_mode = Some(SetupMode::Import);
        }
    }
}

fn handle_restore(app: &mut ZipherXApp) {
    let words: Vec<String> = app
        .mnemonic_input
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect();

    if words.len() != 24 {
        app.setup_error = Some(format!("Expected 24 words, got {}", words.len()));
        return;
    }

    let phrase = words.join(" ");
    match zipherx_crypto::mnemonic::to_seed(&phrase) {
        Ok(seed) => {
            match zipherx_crypto::keys::derive_spending_key(&seed, 0) {
                Ok(sk) => {
                    if let Err(e) = app.storage.store_key("spending_key", &sk) {
                        app.setup_error = Some(format!("Failed to store key: {}", e));
                        return;
                    }
                    app.sk_bytes = Some(sk.to_vec());
                    app.mnemonic_input.zeroize();
                    finalize_wallet(app);
                }
                Err(e) => {
                    app.setup_error = Some(format!("Key derivation failed: {}", e));
                }
            }
        }
        Err(e) => {
            app.setup_error = Some(format!("Invalid mnemonic: {}", e));
        }
    }
}

fn handle_import(app: &mut ZipherXApp) {
    let key_str = app.import_key_input.trim().to_string();
    if key_str.is_empty() {
        app.setup_error = Some("Private key required".into());
        return;
    }

    let sk_result: Result<Vec<u8>, String> = if key_str.starts_with("secret-extended-key") {
        zipherx_crypto::keys::decode_spending_key(&key_str)
            .map_err(|e| format!("{}", e))
    } else {
        hex::decode(&key_str)
            .map_err(|e| format!("Invalid hex: {}", e))
    };

    match sk_result {
        Ok(sk) => {
            if let Err(e) = app.storage.store_key("spending_key", &sk) {
                app.setup_error = Some(format!("Failed to store key: {}", e));
                return;
            }
            app.sk_bytes = Some(sk);
            app.import_key_input.zeroize();
            finalize_wallet(app);
        }
        Err(e) => {
            app.setup_error = Some(format!("Invalid key: {}", e));
        }
    }
}

fn finalize_wallet(app: &mut ZipherXApp) {
    // Derive address
    if let Some(ref sk) = app.sk_bytes {
        match zipherx_crypto::keys::derive_address(sk, 0) {
            Ok((addr_bytes, _)) => {
                if let Ok(addr) = zipherx_crypto::address::encode_address(&addr_bytes) {
                    app.address = Some(addr);
                }
            }
            Err(_) => {}
        }
    }

    app.password_input.zeroize();
    app.password_confirm.zeroize();
    app.setup_error = None;
    app.setup_mode = None;
    app.phase = Phase::Ready;

    // Set syncing state immediately so the UI shows progress
    app.is_syncing = true;
    app.sync_phase = "Initializing wallet...".to_string();
    app.sync_tasks.clear();
    app.overall_progress = 0.0;
    app.sync_start_time = Some(std::time::Instant::now());

    // Start background wallet thread
    let state = sync::start_wallet_thread(
        app.data_dir.clone(),
        app.storage.clone(),
    );
    app.shared_state = Some(state.clone());

    // Trigger initial sync
    if let Some(ref sk) = app.sk_bytes {
        if let Ok(mut s) = state.lock() {
            s.command = Some(sync::SyncCommand::StartSync {
                sk_bytes: sk.clone(),
            });
        }
    }
}
