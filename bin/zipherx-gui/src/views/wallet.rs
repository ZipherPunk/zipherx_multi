//! Wallet view — balance display, receive, sync progress, celebrations.

use crate::app::{fmt_zcl, ZipherXApp};
use crate::theme;
use crate::widgets::qr;

/// Show the main wallet dashboard.
pub fn show(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical_centered(|ui| {
        ui.add_space(15.0);

        // -- Celebrations (clearing / settlement / receive) --
        show_celebrations(app, ui, ctx);

        // -- Mempool incoming TX notification --
        if let Some((ref _txid, amount)) = app.mempool_tx_notification {
            ui.add_space(5.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(0, 25, 30))
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "INCOMING: +{} ZCL detected in mempool",
                                fmt_zcl(amount)
                            ))
                            .font(theme::mono(12.0))
                            .color(theme::CYAN),
                        );
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("[X]")
                                    .font(theme::mono(11.0))
                                    .color(theme::MUTED),
                            ))
                            .clicked()
                        {
                            app.mempool_tx_notification = None;
                            app.mempool_notification_time = None;
                        }
                    });
                });
            ui.add_space(5.0);
        }

        // -- Pending incoming TX banner (mempool dismissed, waiting for block) --
        // Only show after user dismisses the mempool notification (or it auto-expires)
        if app.pending_incoming_txid.is_some()
            && app.receive_celebration.is_none()
            && app.mempool_tx_notification.is_none()
        {
            ui.add_space(5.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(0, 20, 25))
                .inner_margin(12.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("[<] INCOMING TX PENDING")
                            .font(theme::mono(12.0))
                            .color(theme::CYAN),
                    );
                    if let Some(amount) = app.pending_incoming_amount {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("[ +{} ZCL ]", fmt_zcl(amount)))
                                .font(theme::mono(12.0))
                                .color(theme::CYAN),
                        );
                    }
                    if let Some(ref msg) = app.pending_incoming_message {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(msg.as_str())
                                .font(theme::mono(10.0))
                                .color(egui::Color32::from_rgba_unmultiplied(0, 188, 212, 200)),
                        );
                    }
                    if let Some(ref txid) = app.pending_incoming_txid {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("tx: {}...", &txid[..txid.len().min(16)]))
                                .font(theme::mono(9.0))
                                .color(theme::MUTED),
                        );
                    }
                });
            ui.add_space(5.0);
        }

        // -- Pending settlement banner (after clearing dismissed, waiting for block) --
        if app.pending_confirmation_txid.is_some() && app.clearing_celebration.is_none() {
            ui.add_space(5.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(30, 25, 0))
                .inner_margin(12.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("[~] AWAITING SETTLEMENT")
                            .font(theme::mono(12.0))
                            .color(theme::YELLOW),
                    );
                    if let Some(ref msg) = app.pending_settlement_message {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(msg.as_str())
                                .font(theme::mono(10.0))
                                .color(egui::Color32::from_rgba_unmultiplied(255, 215, 0, 200)),
                        );
                    }
                    if let Some(ref txid) = app.pending_confirmation_txid {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(format!("tx: {}...", &txid[..txid.len().min(16)]))
                                .font(theme::mono(9.0))
                                .color(theme::MUTED),
                        );
                    }
                });
            ui.add_space(5.0);
        }

        // -- Balance --
        ui.label(
            egui::RichText::new("BALANCE")
                .font(theme::mono(11.0))
                .color(theme::MUTED),
        );
        ui.label(
            egui::RichText::new(format!("{} ZCL", fmt_zcl(app.balance.spendable)))
                .font(theme::mono(28.0))
                .color(theme::GREEN),
        );
        if app.balance.total != app.balance.spendable {
            ui.label(
                egui::RichText::new(format!(
                    "({} ZCL total, {} spendable notes)",
                    fmt_zcl(app.balance.total),
                    app.balance.spendable_note_count
                ))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
            );
        } else if app.balance.spendable_note_count > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{} spendable note{}",
                    app.balance.spendable_note_count,
                    if app.balance.spendable_note_count == 1 { "" } else { "s" }
                ))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
            );
        }

        ui.add_space(15.0);

        // -- Action buttons --
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ RECEIVE ]")
                        .font(theme::mono(14.0))
                        .color(theme::CYAN),
                ))
                .clicked()
            {
                app.show_receive = !app.show_receive;
                if app.show_receive {
                    if let Some(ref addr) = app.address {
                        app.qr_texture = qr::generate_qr_texture(ctx, addr);
                    }
                }
            }
            ui.add_space(10.0);

            // Disable send if pending confirmation
            let send_enabled = app.pending_confirmation_txid.is_none() && !app.send_in_progress;
            if ui
                .add_enabled(
                    send_enabled,
                    egui::Button::new(
                        egui::RichText::new("[ SEND ]")
                            .font(theme::mono(14.0))
                            .color(if send_enabled { theme::GREEN } else { theme::MUTED }),
                    ),
                )
                .clicked()
            {
                app.tab = crate::app::Tab::Send;
            }
        });

        // -- Receive modal --
        if app.show_receive {
            show_receive(app, ui, ctx);
        }

        ui.add_space(15.0);

        // -- Boost download failure dialog --
        if let Some((ref reason, attempts)) = app.boost_failed.clone() {
            show_boost_failed_dialog(app, ui, &reason, attempts);
        }

        // -- Sync progress (only during initial sync, not background resyncs) --
        if app.is_syncing && !app.initial_sync_done {
            show_sync_progress(app, ui);
        }

        // -- Recent transactions --
        if !app.transactions.is_empty() {
            show_recent_transactions(app, ui, ctx);
        }

        // -- Error display --
        if let Some(err) = app.error.clone() {
            ui.add_space(10.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(40, 10, 10))
                .inner_margin(8.0)
                .rounding(4.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&err)
                                .font(theme::mono(11.0))
                                .color(theme::RED),
                        );
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("[X]")
                                    .font(theme::mono(11.0))
                                    .color(theme::MUTED),
                            ))
                            .clicked()
                        {
                            app.error = None;
                        }
                    });
                });
        }
    });
}

fn show_receive(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.add_space(10.0);
    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(15.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("YOUR SHIELDED ADDRESS")
                    .font(theme::mono(12.0))
                    .color(theme::CYAN),
            );
            ui.add_space(8.0);

            // QR code
            if let Some(ref texture) = app.qr_texture {
                let size = egui::Vec2::splat(160.0);
                ui.image(egui::load::SizedTexture::new(texture.id(), size));
            }

            ui.add_space(8.0);

            // Address text (selectable + copy button)
            if let Some(ref addr) = app.address {
                let short = if addr.len() > 20 {
                    format!("{}...{}", &addr[..10], &addr[addr.len() - 10..])
                } else {
                    addr.clone()
                };
                ui.label(
                    egui::RichText::new(&short)
                        .font(theme::mono(11.0))
                        .color(theme::GREEN),
                );
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ COPY ADDRESS ]")
                            .font(theme::mono(11.0))
                            .color(theme::CYAN),
                    ))
                    .clicked()
                {
                    ctx.copy_text(addr.clone());
                    app.clipboard_clear_at = Some(std::time::Instant::now());
                    // GUI-H3: ensure repaint fires for clipboard auto-clear
                    ctx.request_repaint_after(std::time::Duration::from_secs(31));
                }
            }

            ui.add_space(5.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ CLOSE ]")
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                ))
                .clicked()
            {
                app.show_receive = false;
            }
        });
}

fn show_sync_progress(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let progress = app.overall_progress.clamp(0.0, 1.0);

    // Friendly phase label
    let phase_label = match app.sync_phase.as_str() {
        "boost_download" => "Downloading boost",
        "boost_failed" => "Boost download failed",
        "boost_load" => "Loading headers",
        "header_sync" => "Syncing headers",
        "delta_sync" => "Downloading outputs",
        "boost_scan" => "Scanning outputs",
        "block_scan" => "Scanning blocks",
        "witness_update" => "Updating witnesses",
        "gap_fill" => "Filling gaps",
        other if other.starts_with("Synced") => "Finishing up",
        _ => "Syncing",
    };

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {:.0}%", phase_label, progress * 100.0))
                .font(theme::mono(10.0))
                .color(theme::CYAN),
        );
    });

    // Compact progress bar
    let bar_width = ui.available_width() - 20.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::Vec2::new(bar_width, 6.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(30, 30, 30));
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::Vec2::new(rect.width() * progress, rect.height()),
    );
    ui.painter().rect_filled(filled, 2.0, theme::GREEN);
}

fn show_recent_transactions(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.label(
        egui::RichText::new("RECENT ACTIVITY")
            .font(theme::mono(11.0))
            .color(theme::MUTED),
    );
    ui.add_space(5.0);

    let recent: Vec<_> = app.transactions.iter().take(5).cloned().collect();
    for (i, tx) in recent.iter().enumerate() {
        let (icon, color, sign) = match tx.tx_type.as_str() {
            "received" => ("[+]", theme::GREEN, "+"),
            "sent" => ("[-]", theme::RED, "-"),
            "self" => ("[S]", theme::YELLOW, "~"),
            _ => ("[?]", theme::MUTED, ""),
        };

        let frame_resp = egui::Frame::none()
            .fill(if i % 2 == 0 {
                egui::Color32::from_rgb(15, 15, 15)
            } else {
                theme::PANEL_BG
            })
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icon)
                            .font(theme::mono(11.0))
                            .color(color),
                    );
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}{} ZCL", sign, fmt_zcl(tx.amount)))
                                    .font(theme::mono(12.0))
                                    .color(color),
                            );
                        });
                        // Date/time + confirmations
                        ui.horizontal(|ui| {
                            let time_str = if tx.timestamp > 0 {
                                format_timestamp(tx.timestamp)
                            } else {
                                "pending".to_string()
                            };
                            ui.label(
                                egui::RichText::new(&time_str)
                                    .font(theme::mono(9.0))
                                    .color(theme::MUTED),
                            );
                            ui.label(
                                egui::RichText::new("\u{2022}")
                                    .font(theme::mono(9.0))
                                    .color(theme::MUTED),
                            );
                            let conf_label = if tx.confirmations == 0 {
                                "unconfirmed".to_string()
                            } else {
                                format!("{} conf", tx.confirmations)
                            };
                            ui.label(
                                egui::RichText::new(&conf_label)
                                    .font(theme::mono(9.0))
                                    .color(theme::MUTED),
                            );
                        });
                    });
                });

                // Expanded details
                if app.history_expanded == Some(i) {
                    ui.label(
                        egui::RichText::new(format!("TXID: {}", &tx.txid))
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[COPY TXID]")
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(tx.txid.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        // GUI-H3: ensure repaint fires for clipboard auto-clear
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                }
            });

        // Click to expand/collapse — use frame rect, not ui.min_rect()
        let last = ui.interact(
            frame_resp.response.rect,
            egui::Id::new(format!("tx_{}", i)),
            egui::Sense::click(),
        );
        if last.clicked() {
            app.history_expanded = if app.history_expanded == Some(i) {
                None
            } else {
                Some(i)
            };
        }
    }
}

/// Format a Unix timestamp to a human-readable local date/time string.
/// Uses platform-specific APIs: POSIX localtime_r on Unix, manual UTC on Windows.
fn format_timestamp(ts: u64) -> String {
    #[cfg(unix)]
    {
        let time_t = ts as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::localtime_r(&time_t, &mut tm) };
        if result.is_null() {
            return "unknown".to_string();
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
        )
    }
    #[cfg(not(unix))]
    {
        // Pure-Rust UTC fallback for Windows (no libc::localtime_r)
        let secs = ts;
        let days = secs / 86400;
        let day_secs = secs % 86400;
        let h = day_secs / 3600;
        let mi = (day_secs % 3600) / 60;
        let mut y = 1970i64;
        let mut remaining = days as i64;
        loop {
            let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
            if remaining < dy { break; }
            remaining -= dy;
            y += 1;
        }
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let mdays = if leap {
            [31,29,31,30,31,30,31,31,30,31,30,31]
        } else {
            [31,28,31,30,31,30,31,31,30,31,30,31]
        };
        let mut m = 0usize;
        for md in &mdays {
            if remaining < *md as i64 { break; }
            remaining -= *md as i64;
            m += 1;
        }
        format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m + 1, remaining + 1, h, mi)
    }
}

fn show_celebrations(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Clearing celebration (mempool accepted)
    if let Some(ref msg) = app.clearing_celebration.clone() {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(0, 30, 15))
            .inner_margin(12.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("CLEARING")
                        .font(theme::mono(16.0))
                        .color(theme::GREEN),
                );
                ui.label(
                    egui::RichText::new(msg)
                        .font(theme::mono(11.0))
                        .color(theme::GREEN),
                );
                if let Some(ref dur) = app.clearing_duration {
                    ui.label(
                        egui::RichText::new(format!("Time: {}", dur))
                            .font(theme::mono(10.0))
                            .color(theme::MUTED),
                    );
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ DISMISS ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.clearing_celebration = None;
                    app.clearing_duration = None;
                }
            });
        ui.add_space(5.0);
    }

    // Receive celebration (incoming TX confirmed)
    if let Some(ref msg) = app.receive_celebration.clone() {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(0, 25, 30))
            .inner_margin(12.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("RECEIVED")
                        .font(theme::mono(16.0))
                        .color(theme::CYAN),
                );
                if let Some(amount) = app.receive_amount {
                    ui.label(
                        egui::RichText::new(format!("+{} ZCL", fmt_zcl(amount)))
                            .font(theme::mono(14.0))
                            .color(theme::CYAN),
                    );
                }
                ui.label(
                    egui::RichText::new(msg)
                        .font(theme::mono(11.0))
                        .color(theme::CYAN),
                );
                if let Some(ref txid) = app.receive_txid {
                    ui.label(
                        egui::RichText::new(format!("TXID: {}", txid))
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[COPY TXID]")
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(txid.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        // GUI-H3: ensure repaint fires for clipboard auto-clear
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ DISMISS ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.receive_celebration = None;
                    app.receive_amount = None;
                    app.receive_txid = None;
                }
            });
        ui.add_space(5.0);
    }

    // Settlement celebration (block confirmed)
    if let Some(ref msg) = app.settlement_celebration.clone() {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(30, 25, 0))
            .inner_margin(12.0)
            .rounding(6.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("SETTLEMENT")
                        .font(theme::mono(16.0))
                        .color(theme::GOLD),
                );
                ui.label(
                    egui::RichText::new(msg)
                        .font(theme::mono(11.0))
                        .color(theme::GOLD),
                );
                if let Some(ref dur) = app.settlement_duration {
                    ui.label(
                        egui::RichText::new(format!("Time: {}", dur))
                            .font(theme::mono(10.0))
                            .color(theme::MUTED),
                    );
                }
                if let Some(ref txid) = app.settlement_txid {
                    ui.label(
                        egui::RichText::new(format!("TXID: {}", txid))
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[COPY TXID]")
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(txid.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        // GUI-H3: ensure repaint fires for clipboard auto-clear
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ DISMISS ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.settlement_celebration = None;
                    app.settlement_duration = None;
                    app.settlement_txid = None;
                }
            });
        ui.add_space(5.0);
    }
}

fn show_boost_failed_dialog(app: &mut ZipherXApp, ui: &mut egui::Ui, reason: &str, attempts: u32) {
    ui.add_space(10.0);
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(40, 10, 10))
        .stroke(egui::Stroke::new(1.0, theme::RED))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("BOOST DOWNLOAD FAILED")
                    .font(theme::mono(13.0))
                    .color(theme::RED),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "Failed after {} attempts: {}",
                    attempts,
                    if reason.len() > 80 { &reason[..80] } else { reason }
                ))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "The fast sync (boost) file could not be downloaded.\n\
                     You can continue with P2P header sync (much slower, may\n\
                     take hours), or quit and try again later."
                )
                .font(theme::mono(10.0))
                .color(egui::Color32::from_rgb(200, 200, 200)),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CONTINUE WITH P2P SYNC ]")
                            .font(theme::mono(11.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                {
                    // User chose to continue
                    if let Some(ref state) = app.shared_state {
                        if let Ok(mut s) = state.lock() {
                            s.boost_failed_continue = Some(true);
                        }
                    }
                    app.boost_failed = None;
                }
                ui.add_space(15.0);
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ QUIT ]")
                            .font(theme::mono(11.0))
                            .color(theme::RED),
                    ))
                    .clicked()
                {
                    std::process::exit(0);
                }
            });
        });
    ui.add_space(10.0);
}
