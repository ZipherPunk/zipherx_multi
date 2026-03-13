//! Send view — address, amount, memo, re-authentication, progress.

use zeroize::Zeroize;

use crate::app::{fmt_zcl, parse_zcl, ZipherXApp};
use crate::sync::SyncCommand;
use crate::theme;

pub fn show(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical_centered(|ui| {
        ui.add_space(15.0);
        ui.label(
            egui::RichText::new("SEND ZCL")
                .font(theme::mono(18.0))
                .color(theme::GREEN),
        );
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(format!("Spendable: {} ZCL", fmt_zcl(app.balance.spendable)))
                .font(theme::mono(11.0))
                .color(theme::MUTED),
        );
        ui.add_space(15.0);

        // -- Pending confirmation block --
        if app.pending_confirmation_txid.is_some() {
            ui.label(
                egui::RichText::new("Cannot send while a transaction is awaiting confirmation.")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            );
            return;
        }

        // -- Send in progress --
        if app.send_in_progress {
            show_send_progress(app, ui);
            return;
        }

        // -- Re-authentication dialog --
        if app.show_send_reauth {
            show_reauth(app, ui);
            return;
        }

        // -- Confirmation dialog --
        if app.show_send_confirm {
            show_confirm(app, ui, ctx);
            return;
        }

        // -- Send form --
        show_send_form(app, ui);
    });
}

fn show_send_form(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    // To address
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("TO:")
                .font(theme::mono(12.0))
                .color(theme::MUTED),
        );
        // Address validation badge
        if !app.send_address.is_empty() {
            let valid = zipherx_crypto::address::validate_address(&app.send_address);
            let (badge, color) = if valid {
                ("[OK]", theme::GREEN)
            } else {
                ("[!!]", theme::RED)
            };
            ui.label(
                egui::RichText::new(badge)
                    .font(theme::mono(10.0))
                    .color(color),
            );
        }
    });
    ui.add(
        egui::TextEdit::singleline(&mut app.send_address)
            .hint_text("zs1...")
            .font(theme::mono(11.0))
            .desired_width(ui.available_width() - 20.0),
    );
    ui.add_space(8.0);

    // Amount
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("AMOUNT (ZCL):")
                .font(theme::mono(12.0))
                .color(theme::MUTED),
        );
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[MAX]")
                    .font(theme::mono(10.0))
                    .color(theme::CYAN),
            ))
            .clicked()
        {
            let max = app.balance.spendable.saturating_sub(app.send_fee);
            app.send_amount = fmt_zcl(max);
        }
    });
    ui.add(
        egui::TextEdit::singleline(&mut app.send_amount)
            .hint_text("0.0")
            .font(theme::mono(14.0))
            .desired_width((ui.available_width() - 20.0).min(300.0)),
    );
    ui.add_space(8.0);

    // Memo
    ui.label(
        egui::RichText::new("MEMO (optional):")
            .font(theme::mono(12.0))
            .color(theme::MUTED),
    );
    ui.add(
        egui::TextEdit::multiline(&mut app.send_memo)
            .hint_text("Encrypted memo (max 512 bytes)")
            .font(theme::mono(11.0))
            .desired_width(ui.available_width() - 20.0)
            .desired_rows(2),
    );
    let memo_bytes = app.send_memo.as_bytes().len();
    let memo_color = if memo_bytes > 512 {
        theme::RED
    } else {
        theme::MUTED
    };
    ui.label(
        egui::RichText::new(format!("{}/512 bytes", memo_bytes))
            .font(theme::mono(9.0))
            .color(memo_color),
    );
    ui.add_space(8.0);

    // Fee
    ui.label(
        egui::RichText::new(format!("Fee: {} ZCL", fmt_zcl(app.send_fee)))
            .font(theme::mono(11.0))
            .color(theme::MUTED),
    );

    // Error
    if let Some(ref err) = app.send_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(11.0))
                .color(theme::RED),
        );
    }

    ui.add_space(15.0);
    if ui
        .add(egui::Button::new(
            egui::RichText::new("[ REVIEW TRANSACTION ]")
                .font(theme::mono(14.0))
                .color(theme::GREEN),
        ))
        .clicked()
    {
        if let Err(e) = validate_send(app) {
            app.send_error = Some(e);
        } else {
            app.send_error = None;
            app.show_send_confirm = true;
        }
    }
}

fn show_confirm(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    let amount = parse_zcl(&app.send_amount).unwrap_or(0);

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(20, 20, 20))
        .inner_margin(15.0)
        .rounding(6.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("CONFIRM TRANSACTION")
                    .font(theme::mono(16.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(10.0);

            // Color-coded address
            let addr = &app.send_address;
            if addr.len() > 20 {
                let prefix = &addr[..8];
                let middle = &addr[8..addr.len() - 8];
                let suffix = &addr[addr.len() - 8..];
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("To: ")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    );
                    ui.label(
                        egui::RichText::new(prefix)
                            .font(theme::mono(11.0))
                            .color(theme::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(middle)
                            .font(theme::mono(11.0))
                            .color(theme::YELLOW),
                    );
                    ui.label(
                        egui::RichText::new(suffix)
                            .font(theme::mono(11.0))
                            .color(theme::GREEN),
                    );
                });
            } else {
                ui.label(
                    egui::RichText::new(format!("To: {}", addr))
                        .font(theme::mono(11.0))
                        .color(theme::GREEN),
                );
            }

            ui.label(
                egui::RichText::new(format!("Amount: {} ZCL", fmt_zcl(amount)))
                    .font(theme::mono(13.0))
                    .color(theme::GREEN),
            );
            ui.label(
                egui::RichText::new(format!("Fee: {} ZCL", fmt_zcl(app.send_fee)))
                    .font(theme::mono(11.0))
                    .color(theme::MUTED),
            );
            ui.label(
                egui::RichText::new(format!("Total: {} ZCL", fmt_zcl(amount + app.send_fee)))
                    .font(theme::mono(11.0))
                    .color(theme::YELLOW),
            );

            if !app.send_memo.is_empty() {
                ui.label(
                    egui::RichText::new(format!("Memo: {}", app.send_memo))
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
            }

            ui.add_space(15.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CONFIRM & SEND ]")
                            .font(theme::mono(14.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                {
                    app.show_send_confirm = false;
                    app.show_send_reauth = true;
                }
                ui.add_space(10.0);
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(14.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.show_send_confirm = false;
                }
            });
        });
}

fn show_reauth(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(20, 15, 15))
        .inner_margin(15.0)
        .rounding(6.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("RE-AUTHENTICATE TO SEND")
                    .font(theme::mono(14.0))
                    .color(theme::YELLOW),
            );
            ui.label(
                egui::RichText::new("Enter your password to authorize this transaction.")
                    .font(theme::mono(11.0))
                    .color(theme::MUTED),
            );
            ui.add_space(10.0);

            // GUI-H4: Rate limiting on re-authentication attempts
            if let Some(lockout) = app.reauth_lockout_until {
                let elapsed = lockout.elapsed().as_secs();
                if elapsed < 60 {
                    ui.label(
                        egui::RichText::new(format!(
                            "Too many failed attempts. Try again in {}s",
                            60 - elapsed
                        ))
                        .font(theme::mono(11.0))
                        .color(theme::RED),
                    );
                    ui.add_space(10.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CANCEL ]")
                                .font(theme::mono(14.0))
                                .color(theme::MUTED),
                        ))
                        .clicked()
                    {
                        app.reauth_password.zeroize();
                        app.show_send_reauth = false;
                        app.send_error = None;
                    }
                    return;
                } else {
                    app.reauth_lockout_until = None;
                    app.reauth_failed_attempts = 0;
                }
            }

            let response = ui.add(
                egui::TextEdit::singleline(&mut app.reauth_password)
                    .password(true)
                    .hint_text("Password")
                    .font(theme::mono(14.0))
                    .desired_width((ui.available_width() - 20.0).min(350.0)),
            );

            if let Some(ref err) = app.send_error {
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(err)
                        .font(theme::mono(11.0))
                        .color(theme::RED),
                );
            }

            ui.add_space(10.0);
            let enter_pressed =
                response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ AUTHORIZE ]")
                            .font(theme::mono(14.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                    || enter_pressed
                {
                    if app.storage.verify_password(&app.reauth_password) {
                        app.reauth_password.zeroize();
                        app.show_send_reauth = false;
                        app.send_error = None;
                        // Reset rate limiter on success
                        app.reauth_failed_attempts = 0;
                        app.reauth_lockout_until = None;
                        execute_send(app);
                    } else {
                        app.send_error = Some("Wrong password".into());
                        app.reauth_failed_attempts += 1;
                        if app.reauth_failed_attempts >= 5 {
                            app.reauth_lockout_until = Some(std::time::Instant::now());
                        }
                    }
                }
                ui.add_space(10.0);
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(14.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.reauth_password.zeroize();
                    app.show_send_reauth = false;
                    app.send_error = None;
                }
            });
        });
}

fn show_send_progress(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(15.0)
        .rounding(6.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("SENDING...")
                    .font(theme::mono(16.0))
                    .color(theme::CYAN),
            );
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(&app.send_phase)
                    .font(theme::mono(12.0))
                    .color(theme::GREEN),
            );
            if app.send_phase_total > 0 {
                let progress = app.send_phase_current as f32 / app.send_phase_total as f32;
                let bar_width = (ui.available_width() - 40.0).max(100.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::Vec2::new(bar_width, 10.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, egui::Color32::from_rgb(30, 30, 30));
                let filled = egui::Rect::from_min_size(
                    rect.min,
                    egui::Vec2::new(rect.width() * progress, rect.height()),
                );
                ui.painter().rect_filled(filled, 2.0, theme::CYAN);
            }
            if app.mempool_accepted {
                ui.add_space(5.0);
                if let Some(ref status) = app.mempool_peer_status {
                    ui.label(
                        egui::RichText::new(format!("Mempool accepted: {}", status))
                            .font(theme::mono(11.0))
                            .color(theme::GREEN),
                    );
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Validation & execution
// ---------------------------------------------------------------------------

/// Validate send parameters and snapshot them into `app.validated_send` to
/// prevent TOCTOU race between validation and execution (GUI-H2).
fn validate_send(app: &mut ZipherXApp) -> Result<(), String> {
    if app.send_address.is_empty() {
        return Err("Address is required".into());
    }
    if !zipherx_crypto::address::validate_address(&app.send_address) {
        return Err("Invalid Zclassic address".into());
    }
    let amount = parse_zcl(&app.send_amount).ok_or("Invalid amount format")?;
    if amount == 0 {
        return Err("Amount must be greater than 0".into());
    }
    if amount + app.send_fee > app.balance.spendable {
        return Err(format!(
            "Insufficient balance: {} ZCL available, {} ZCL needed",
            fmt_zcl(app.balance.spendable),
            fmt_zcl(amount + app.send_fee)
        ));
    }
    // Rust strings are always valid UTF-8, so only byte length needs checking.
    if app.send_memo.as_bytes().len() > 512 {
        return Err("Memo exceeds 512 byte limit".into());
    }
    // Snapshot validated parameters so execute_send uses the same values
    app.validated_send = Some(crate::app::ValidatedSend {
        address: app.send_address.clone(),
        amount,
        fee: app.send_fee,
        memo: if app.send_memo.is_empty() {
            None
        } else {
            Some(app.send_memo.clone())
        },
    });
    Ok(())
}

fn execute_send(app: &mut ZipherXApp) {
    let sk = match &app.sk_bytes {
        Some(sk) => sk.clone(),
        None => {
            app.send_error = Some("Wallet is locked".into());
            return;
        }
    };

    // Use the validated snapshot (GUI-H2: prevents TOCTOU race with text fields)
    let validated = match app.validated_send.take() {
        Some(v) => v,
        None => {
            app.send_error = Some("No validated send parameters".into());
            return;
        }
    };

    app.send_in_progress = true;
    app.send_timestamp = Some(std::time::Instant::now());

    // Send command to wallet thread using snapshotted parameters
    if let Some(ref state) = app.shared_state {
        if let Ok(mut s) = state.lock() {
            s.command = Some(SyncCommand::Send {
                to_address: validated.address,
                amount: validated.amount,
                fee: validated.fee,
                memo: validated.memo,
                sk_bytes: sk,
            });
        }
    }
}
