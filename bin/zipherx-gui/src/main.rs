//! ZipherX Desktop — Privacy-first Zclassic wallet.
//!
//! Native egui/eframe GUI. Single binary, no JVM, no FFI bridge.
//! Calls the Rust core (AsyncWallet) directly.

mod app;
mod effects;
mod fullnode;
mod platform;
mod sync;
mod theme;
mod views;
mod widgets;

use app::{Phase, Tab, ZipherXApp};
use zeroize::Zeroize;

fn load_window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/zipherpunk_logo.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt::init();

    let icon = load_window_icon();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([480.0, 780.0])
        .with_min_inner_size([400.0, 600.0])
        .with_title("ZipherX");

    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(icon_data);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "ZipherX",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(ZipherXApp::default()))
        }),
    )
}

impl eframe::App for ZipherXApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // -- Clipboard auto-clear after 30s --
        if let Some(clear_at) = self.clipboard_clear_at {
            if clear_at.elapsed().as_secs() >= 30 {
                ctx.copy_text(String::new());
                self.clipboard_clear_at = None;
            } else {
                // Ensure repaint fires even when the window is unfocused
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }

        // -- Auto-lock: detect user interaction --
        // Note: interaction detection runs BEFORE auto-lock check, so any
        // activity in the current frame resets the timer before it's checked.
        let had_interaction = ctx.input(|i| {
            i.pointer.any_click()
                || i.key_pressed(egui::Key::Enter)
                || i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))
        });
        if had_interaction {
            self.last_interaction = std::time::Instant::now();
        }

        // -- Auto-lock: idle timeout (suppressed during sync/download) --
        if self.auto_lock_secs > 0
            && self.last_interaction.elapsed().as_secs() > self.auto_lock_secs
            && matches!(self.phase, Phase::Ready)
            && !self.is_syncing
            && !self.send_in_progress
        {
            // Lock the wallet
            if let Some(ref mut sk) = self.sk_bytes {
                for b in sk.iter_mut() {
                    unsafe { std::ptr::write_volatile(b, 0) };
                }
            }
            self.sk_bytes = None;
            self.password_input.zeroize();
            self.export_key_display.zeroize();
            self.export_t_key_display.zeroize();
            self.export_password.zeroize();
            self.reauth_password.zeroize();
            self.send_address.zeroize();
            self.send_amount.zeroize();
            self.send_memo.zeroize();
            self.node_rpc_password.zeroize();
            self.node_rpc_user.zeroize();
            self.show_export = false;
            self.show_export_confirm = false;
            self.show_send_confirm = false;
            self.show_send_reauth = false;
            self.storage.lock();
            self.phase = Phase::Locked;
        }

        // -- Read shared state from wallet thread --
        // Poll even when locked so sync/download continues updating status bar.
        if self.shared_state.is_some() {
            poll_shared_state(self, ctx);
        }

        // Schedule repaints even when locked (status bar shows sync progress).
        if self.is_syncing || self.send_in_progress {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // -- Poll full node daemon (every 5s) --
        if self.fullnode_enabled && self.node_poll_interval.elapsed().as_secs() >= 5 {
            views::node::poll_node_info(self);
            self.node_poll_interval = std::time::Instant::now();
        }

        // -- Bottom status bar (always visible) --
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("ZipherX v{}", self.version))
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
                ui.separator();

                // Sync status
                let (sync_dot, sync_label, sync_color) = if self.sync_error.is_some() {
                    (theme::RED, "sync error", theme::RED)
                } else if self.is_syncing {
                    (theme::CYAN, "syncing", theme::CYAN)
                } else if self.initial_sync_done {
                    (theme::GREEN, "synced", theme::GREEN)
                } else {
                    (theme::YELLOW, "ready", theme::YELLOW)
                };
                ui.colored_label(sync_dot, "\u{2022}");
                ui.label(
                    egui::RichText::new(sync_label)
                        .font(theme::mono(10.0))
                        .color(sync_color),
                );

                ui.separator();

                // Peers — color-coded: green (>=3), yellow/amber (1-2), red (0)
                let peer_color = if self.peer_count >= 3 {
                    theme::GREEN
                } else if self.peer_count >= 1 {
                    theme::YELLOW
                } else {
                    theme::RED
                };
                ui.label(
                    egui::RichText::new(format!("{} peers", self.peer_count))
                        .font(theme::mono(10.0))
                        .color(peer_color),
                );

                if self.block_height > 0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("height {}", self.block_height))
                            .font(theme::mono(10.0))
                            .color(theme::MUTED),
                    );
                }

                // Tor indicator
                if self.tor_enabled {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("TOR")
                            .font(theme::mono(10.0))
                            .color(theme::CYAN),
                    );
                }
            });
        });

        // -- Phase routing --
        match self.phase {
            Phase::Disclaimer => {
                views::disclaimer::show(self, ctx);
            }
            Phase::Locked => {
                views::unlock::show(self, ctx);
            }
            Phase::Setup => {
                views::unlock::show(self, ctx);
            }
            Phase::Ready => {
                show_ready(self, ctx);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ready screen — tab bar + content
// ---------------------------------------------------------------------------

fn show_ready(app: &mut ZipherXApp, ctx: &egui::Context) {
    // -- Tab bar --
    egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let tabs = [
                (Tab::Wallet, "Wallet"),
                (Tab::Send, "Send"),
                (Tab::History, "History"),
                (Tab::Node, "Node"),
                (Tab::Settings, "Settings"),
            ];
            for (tab, label) in &tabs {
                let selected = app.tab == *tab;
                let color = if selected { theme::GREEN } else { theme::MUTED };
                let btn = egui::Button::new(
                    egui::RichText::new(*label)
                        .font(theme::mono(13.0))
                        .color(color),
                )
                .frame(false)
                .min_size(egui::Vec2::new(50.0, 24.0));
                let resp = ui.add(btn);
                if selected {
                    let rect = resp.rect;
                    ui.painter().line_segment(
                        [
                            egui::Pos2::new(rect.left() + 4.0, rect.bottom()),
                            egui::Pos2::new(rect.right() - 4.0, rect.bottom()),
                        ],
                        egui::Stroke::new(2.0, theme::GREEN),
                    );
                }
                if resp.clicked() {
                    app.tab = *tab;
                }
                ui.add_space(6.0);
            }
        });
    });

    // -- Central content --
    egui::CentralPanel::default().show(ctx, |ui| match app.tab {
        Tab::Wallet => views::wallet::show(app, ui, ctx),
        Tab::Send => views::send::show(app, ui, ctx),
        Tab::History => views::history::show(app, ui, ctx),
        Tab::Node => views::node::show(app, ui, ctx),
        Tab::Settings => views::settings::show(app, ui, ctx),
    });

    // -- Particle overlay --
    if !app.confetti_particles.is_empty() || !app.firework_particles.is_empty() {
        let dt = ctx.input(|i| i.predicted_dt);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("particles"),
        ));
        effects::confetti::update_and_draw(&mut app.confetti_particles, &painter, dt);
        effects::confetti::update_and_draw(&mut app.firework_particles, &painter, dt);
        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Poll shared state from the wallet thread
// ---------------------------------------------------------------------------

fn poll_shared_state(app: &mut ZipherXApp, ctx: &egui::Context) {
    let state = match &app.shared_state {
        Some(s) => s.clone(),
        None => return,
    };

    // --- Hold the mutex ONLY while reading/writing shared state ---
    {
        let Ok(mut s) = state.lock() else { return };

        // Sync progress
        if !s.sync_phase.is_empty() && s.sync_phase != "Idle" {
            app.sync_phase = s.sync_phase.clone();
            app.sync_progress = s.sync_progress;
            app.sync_current = s.sync_current;
            app.sync_target = s.sync_target;
            app.is_syncing = !s.sync_complete;

            // Update sync tasks
            let phase = s.sync_phase.clone();
            let current = s.sync_current;
            let target = s.sync_target;
            update_sync_tasks(app, &phase, current, target);
        }

        if s.sync_complete {
            app.is_syncing = false;
            app.block_height = s.sync_height;
            app.initial_sync_done = true;
            s.sync_complete = false;
            // Reset sync state to prevent stale display
            s.sync_phase = "Idle".to_string();
            app.sync_tasks.clear();
            app.overall_progress = 0.0;
            app.sync_start_time = None;
        }

        if let Some(ref err) = s.sync_error {
            app.sync_error = Some(err.clone());
            app.error = Some(err.clone());
            app.is_syncing = false;
            s.sync_error = None;
            s.sync_phase = "Idle".to_string();
        }

        // Boost download failure — transfer to app for dialog display
        if let Some(ref bf) = s.boost_failed {
            app.boost_failed = Some(bf.clone());
            s.boost_failed = None;
        }
        // User responded to boost failure dialog
        if let Some(continue_sync) = app.boost_failed.as_ref().and_then(|_| {
            // Check if user has responded via the shared state
            s.boost_failed_continue.take()
        }) {
            if !continue_sync {
                // User chose to quit
                app.is_syncing = false;
                app.boost_failed = None;
                s.sync_phase = "Idle".to_string();
            } else {
                // User chose to continue — clear the dialog
                app.boost_failed = None;
            }
        }

        // Balance — suppress updates while TX pending confirmation to avoid
        // showing incorrect intermediate state (sent notes marked but change
        // not yet mined). The wallet view shows a "pending" indicator.
        if app.pending_confirmation_txid.is_none() {
            app.balance.total = s.total_balance;
            app.balance.spendable = s.spendable_balance;
            app.balance.note_count = s.note_count;
            app.balance.spendable_note_count = s.spendable_note_count;
        }
        app.transparent_balance = s.transparent_balance;
        app.imported_key_count = s.imported_key_count;

        // Network
        app.peer_count = s.peer_count;
        if s.block_height > app.block_height {
            app.block_height = s.block_height;
        }

        // Transactions
        if !s.transactions.is_empty() {
            app.transactions = s.transactions.clone();
        }

        // Send result
        if let Some(result) = s.send_result.take() {
            app.send_in_progress = false;
            match result {
                Ok(info) => {
                    app.pending_confirmation_txid = Some(info.txid.clone());
                    s.pending_confirmation = true;
                    let sent_types = ["sent", "self", "self_z2t", "self_t2z"];
                    app.confirmed_sent_count_at_send = app
                        .transactions
                        .iter()
                        .filter(|t| {
                            t.confirmations > 0 && sent_types.contains(&t.tx_type.as_str())
                        })
                        .count();

                    // Clearing celebration
                    let elapsed = app.send_timestamp.map(|t| t.elapsed().as_secs());
                    app.clearing_celebration = Some(app::random_clearing_message().to_string());
                    app.clearing_duration = elapsed.map(|e| format!("{}s", e));
                    app.mempool_accepted = true;

                    // Pre-pick the pending settlement message for after dismiss
                    app.pending_settlement_message =
                        Some(app::random_pending_settlement_message().to_string());

                    // Spawn confetti
                    let screen = ctx.screen_rect();
                    app.confetti_particles =
                        effects::confetti::spawn_confetti(screen.center().x, screen.center().y);

                    // Clear send form
                    app.send_address.zeroize();
                    app.send_amount.zeroize();
                    app.send_memo.zeroize();
                    app.send_error = None;
                    app.tab = Tab::Wallet;
                }
                Err(e) => {
                    app.send_error = Some(e);
                    app.mempool_accepted = false;
                    app.mempool_peer_status = None;
                }
            }
        }

        // Send progress
        if app.send_in_progress {
            app.send_phase = s.send_phase.clone();
            app.send_phase_current = s.send_current;
            app.send_phase_total = s.send_total;
            if s.mempool_accepted {
                app.mempool_accepted = true;
                app.mempool_peer_status = s.mempool_peer_status.clone();
            }
        }

        // Mempool TX notification (incoming)
        // Skip if we've already seen/confirmed this txid, are already tracking it,
        // or it's likely our own sent TX's change output coming back to us.
        if let Some(info) = s.mempool_tx.take() {
            let is_likely_change = app.send_in_progress
                || app
                    .send_timestamp
                    .map(|t| t.elapsed().as_secs() < 120)
                    .unwrap_or(false);
            let dominated = app.known_received_txids.contains(&info.txid)
                || app.pending_incoming_txid.as_deref() == Some(&info.txid)
                || app.pending_confirmation_txid.as_deref() == Some(&info.txid)
                || is_likely_change;
            if !dominated {
                app.mempool_tx_notification = Some((info.txid.clone(), info.amount));
                app.mempool_notification_time = Some(std::time::Instant::now());
                // Track for confirmation polling (like sent TXs)
                app.pending_incoming_txid = Some(info.txid);
                app.pending_incoming_amount = Some(info.amount);
                app.pending_incoming_message =
                    Some(app::random_pending_incoming_message().to_string());
                app.pending_incoming_resync_timer = None;
                app.pending_incoming_resync_count = 0;
            }
        }

        // Peer info
        if !s.peer_infos.is_empty() {
            app.peer_infos = s.peer_infos.clone();
        }

        // Maintenance result
        if let Some(result) = s.maintenance_result.take() {
            app.maintenance_in_progress = false;
            match result {
                Ok(msg) => app.maintenance_status = Some(msg),
                Err(msg) => {
                    app.maintenance_status = Some(msg.clone());
                    app.error = Some(msg);
                }
            }
        }
    } // --- MutexGuard dropped here — safe to re-lock below ---

    // Detect new received transactions → trigger receive celebration
    detect_new_received(app, ctx);

    // Auto-dismiss mempool notification after 60s
    if let Some(t) = app.mempool_notification_time {
        if t.elapsed().as_secs() >= 60 {
            app.mempool_tx_notification = None;
            app.mempool_notification_time = None;
        }
    }

    // Check pending confirmation (sent TX)
    check_pending_confirmation(app, ctx);

    // Safety timeout: if TX hasn't been confirmed after 10 minutes, clear pending state
    // gracefully. The wallet thread's autonomous sync will still keep syncing, so
    // balance and history will update as soon as the TX appears on-chain.
    if app.pending_confirmation_txid.is_some() {
        if let Some(ts) = app.send_timestamp {
            if ts.elapsed().as_secs() >= 600 {
                eprintln!("[ZipherX] Safety timeout: clearing pending TX after 10 minutes");
                app.pending_confirmation_txid = None;
                app.pending_settlement_message = None;
                app.mempool_accepted = false;
                app.mempool_peer_status = None;
                if let Some(ref state) = app.shared_state {
                    if let Ok(mut s) = state.lock() {
                        s.pending_confirmation = false;
                    }
                }
            }
        }
    }

    // Check pending incoming TX confirmation
    check_incoming_confirmation(app, ctx);

    // Auto re-sync while awaiting incoming TX confirmation (every 10s, up to 18 retries)
    if app.pending_incoming_txid.is_some()
        && !app.is_syncing
        && !app.send_in_progress
        && app.pending_confirmation_txid.is_none()
    {
        let should_resync = match app.pending_incoming_resync_timer {
            None => {
                app.pending_incoming_resync_timer = Some(std::time::Instant::now());
                false
            }
            Some(timer)
                if timer.elapsed().as_secs() >= 10 && app.pending_incoming_resync_count < 18 =>
            {
                true
            }
            _ => false,
        };
        if should_resync {
            if let Ok(mut s) = state.lock() {
                s.command = Some(sync::SyncCommand::StartSync {
                    sk_bytes: app.sk_bytes.as_ref().cloned().unwrap_or_default(),
                });
            }
            app.is_syncing = true;
            app.pending_incoming_resync_timer = Some(std::time::Instant::now());
            app.pending_incoming_resync_count += 1;
        }
        ctx.request_repaint();
    }

    // Reset incoming resync state when confirmation detected
    if app.pending_incoming_txid.is_none() && app.pending_incoming_resync_timer.is_some() {
        app.pending_incoming_resync_timer = None;
        app.pending_incoming_resync_count = 0;
    }

    // Auto re-sync while awaiting block confirmation (every 10s, up to 18 retries)
    if app.pending_confirmation_txid.is_some() && !app.is_syncing && !app.send_in_progress {
        let should_resync = match app.pending_resync_timer {
            None => {
                // First time — start timer
                app.pending_resync_timer = Some(std::time::Instant::now());
                false
            }
            Some(timer) if timer.elapsed().as_secs() >= 10 && app.pending_resync_count < 18 => true,
            _ => false,
        };
        if should_resync {
            if let Ok(mut s) = state.lock() {
                s.command = Some(sync::SyncCommand::StartSync {
                    sk_bytes: app.sk_bytes.as_ref().cloned().unwrap_or_default(),
                });
            }
            app.is_syncing = true;
            app.pending_resync_timer = Some(std::time::Instant::now());
            app.pending_resync_count += 1;
        }
        ctx.request_repaint();
    }

    // Reset resync state when confirmation detected
    if app.pending_confirmation_txid.is_none() && app.pending_resync_timer.is_some() {
        app.pending_resync_timer = None;
        app.pending_resync_count = 0;
    }

    // Sync pending_confirmation flag to wallet thread so it uses faster polling (15s).
    if let Some(ref state) = app.shared_state {
        if let Ok(mut s) = state.lock() {
            s.pending_confirmation = app.pending_confirmation_txid.is_some()
                || app.pending_incoming_txid.is_some();
        }
    }

    // Background sync (inv MSG_BLOCK + periodic 15s/90s) is handled autonomously
    // by the wallet thread — works even when the UI is locked (sk_bytes zeroed).
    // The wallet thread caches its own sk_bytes from the first StartSync.

    // Request repaint: throttle to ~10fps during sync/send (progress updates
    // don't need 60fps), otherwise periodic 2s for peer count / notifications.
    if app.is_syncing || app.send_in_progress {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    } else if app.pending_confirmation_txid.is_some() || app.pending_incoming_txid.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}

fn update_sync_tasks(app: &mut ZipherXApp, phase: &str, current: u64, target: u64) {
    let task_defs = [
        ("boost_download", "Downloading boost file"),
        ("boost_load", "Loading boost headers"),
        ("header_sync", "Syncing block headers"),
        ("delta_sync", "Downloading shielded outputs"),
        ("boost_scan", "Scanning boost outputs"),
        ("block_scan", "Scanning for transactions"),
        ("witness_update", "Verifying witnesses"),
    ];

    // Initialize tasks if empty
    if app.sync_tasks.is_empty() {
        app.sync_tasks = task_defs
            .iter()
            .map(|(id, title)| app::SyncTask {
                id: id.to_string(),
                title: title.to_string(),
                status: app::SyncTaskStatus::Pending,
                detail: String::new(),
                progress: 0.0,
                start_time: None,
                end_time: None,
            })
            .collect();
        app.sync_start_time = Some(std::time::Instant::now());
    }

    // Find current phase index
    let phase_idx = app.sync_tasks.iter().position(|t| t.id == phase);

    // Update tasks
    let now = std::time::Instant::now();
    for (i, task) in app.sync_tasks.iter_mut().enumerate() {
        if let Some(idx) = phase_idx {
            if i < idx && task.status != app::SyncTaskStatus::Completed {
                task.status = app::SyncTaskStatus::Completed;
                task.progress = 1.0;
                task.end_time = Some(now);
            } else if i == idx {
                task.status = app::SyncTaskStatus::InProgress;
                task.progress = if target > 0 {
                    current as f32 / target as f32
                } else {
                    0.0
                };
                if task.start_time.is_none() {
                    task.start_time = Some(now);
                }
                // Detail string
                task.detail = match phase {
                    "boost_download" => {
                        let mb = current / (1024 * 1024);
                        let total_mb = if target > 0 {
                            target / (1024 * 1024)
                        } else {
                            0
                        };
                        if total_mb > 0 {
                            format!("{}MB / {}MB", mb, total_mb)
                        } else {
                            format!("{}MB", mb)
                        }
                    }
                    "boost_load" => format!("{} / {} headers", current, target),
                    "header_sync" => format!("Height {} / {}", current, target),
                    "delta_sync" => format!("Height {} / {}", current, target),
                    "boost_scan" => format!("{} outputs (CPU-intensive)", target),
                    "block_scan" => format!("Block {} / {}", current, target),
                    "witness_update" => format!("{} / {} notes", current, target),
                    _ => {
                        if target > 0 {
                            format!("{} / {}", current, target)
                        } else {
                            String::new()
                        }
                    }
                };
            }
        }
    }

    // Recalculate overall progress
    let total = app.sync_tasks.len() as f32;
    let mut weighted = 0.0f32;
    for task in &app.sync_tasks {
        weighted += match task.status {
            app::SyncTaskStatus::Completed => 1.0,
            app::SyncTaskStatus::InProgress => task.progress,
            _ => 0.0,
        };
    }
    app.overall_progress = weighted / total;
}

fn check_pending_confirmation(app: &mut ZipherXApp, ctx: &egui::Context) {
    let Some(pending_txid) = app.pending_confirmation_txid.clone() else {
        return;
    };

    // Only use exact txid match — Sapling transactions are not malleable,
    // so the txid never changes after broadcast. Strategy 2 (count-based)
    // was removed because it caused false-positive settlements when
    // back-to-back sends are in flight.
    let confirmed = app
        .transactions
        .iter()
        .any(|t| t.txid == pending_txid && t.confirmations > 0);

    if confirmed {
        let elapsed = app.send_timestamp.map(|t| t.elapsed().as_secs());
        let confirmed_tx = app
            .transactions
            .iter()
            .find(|t| t.txid == pending_txid && t.confirmations > 0);

        app.settlement_txid = confirmed_tx.map(|t| t.txid.clone()).or(Some(pending_txid));
        app.settlement_celebration = Some(app::random_settlement_message().to_string());
        app.settlement_duration = elapsed.map(|e| format!("{}s", e));
        app.pending_confirmation_txid = None;
        app.pending_settlement_message = None;
        app.mempool_accepted = false;
        app.mempool_peer_status = None;

        // Spawn fireworks
        let screen = ctx.screen_rect();
        app.firework_particles =
            effects::confetti::spawn_fireworks(screen.center().x, screen.bottom());
    }
}

/// Check if a pending incoming mempool TX has been confirmed in a block.
fn check_incoming_confirmation(app: &mut ZipherXApp, ctx: &egui::Context) {
    let pending_txid = match &app.pending_incoming_txid {
        Some(txid) => txid.clone(),
        None => return,
    };

    let confirmed = app
        .transactions
        .iter()
        .any(|t| t.txid == pending_txid && t.confirmations > 0);

    if confirmed {
        let amount = app.pending_incoming_amount.unwrap_or(0);
        app.receive_celebration = Some(app::random_receive_message().to_string());
        app.receive_amount = Some(amount);
        app.receive_txid = Some(pending_txid.clone());

        // Clear pending incoming state
        app.pending_incoming_txid = None;
        app.pending_incoming_amount = None;
        app.pending_incoming_message = None;

        // Clear mempool notification if it matches
        if let Some((ref mp_txid, _)) = app.mempool_tx_notification {
            if *mp_txid == pending_txid {
                app.mempool_tx_notification = None;
                app.mempool_notification_time = None;
            }
        }

        // Add to known set so detect_new_received doesn't double-fire
        app.known_received_txids.insert(pending_txid);

        // Spawn cyan confetti
        let screen = ctx.screen_rect();
        app.confetti_particles =
            effects::confetti::spawn_receive_confetti(screen.center().x, screen.center().y);
    }
}

/// Detect new received transactions and trigger receive celebration.
fn detect_new_received(app: &mut ZipherXApp, ctx: &egui::Context) {
    // Skip if we haven't done initial sync yet or if a send celebration is active
    if !app.initial_sync_done
        || app.clearing_celebration.is_some()
        || app.settlement_celebration.is_some()
    {
        return;
    }

    // First call after initial sync: seed the known set with all existing TXs.
    // This prevents false "new receive" celebrations on every startup.
    if !app.receive_txids_seeded {
        for tx in &app.transactions {
            if tx.tx_type == "received" {
                app.known_received_txids.insert(tx.txid.clone());
            }
        }
        app.receive_txids_seeded = true;
        return;
    }

    // Find received TXs with confirmations > 0 that we haven't seen yet.
    // Skip any TX that's tracked by pending_incoming_txid — let
    // check_incoming_confirmation handle it so the banner lifecycle completes.
    let mut new_received: Option<(String, u64)> = None;
    for tx in &app.transactions {
        if tx.tx_type == "received" && tx.confirmations > 0 {
            if !app.known_received_txids.contains(&tx.txid) {
                if app.pending_incoming_txid.as_deref() == Some(tx.txid.as_str()) {
                    continue; // defer to check_incoming_confirmation
                }
                app.known_received_txids.insert(tx.txid.clone());
                // Track the latest new receive for celebration
                new_received = Some((tx.txid.clone(), tx.amount));
            }
        }
    }

    // Also seed known set with unconfirmed received TXs (so they don't trigger twice)
    for tx in &app.transactions {
        if tx.tx_type == "received" {
            app.known_received_txids.insert(tx.txid.clone());
        }
    }

    // Trigger celebration for the newest received TX
    if let Some((txid, amount)) = new_received {
        app.receive_celebration = Some(app::random_receive_message().to_string());
        app.receive_amount = Some(amount);
        app.receive_txid = Some(txid.clone());

        // Clear mempool notification if it matches this TX
        if let Some((ref mp_txid, _)) = app.mempool_tx_notification {
            if *mp_txid == txid {
                app.mempool_tx_notification = None;
                app.mempool_notification_time = None;
            }
        }

        // Spawn cyan confetti
        let screen = ctx.screen_rect();
        app.confetti_particles =
            effects::confetti::spawn_receive_confetti(screen.center().x, screen.center().y);

        // Switch to wallet tab to show celebration
        app.tab = Tab::Wallet;
    }
}
