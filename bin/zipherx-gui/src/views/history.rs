//! Transaction history view with filtering.

use crate::app::{fmt_zcl, HistoryFilter, ZipherXApp};
use crate::theme;

pub fn show(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.vertical_centered(|ui| {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("TRANSACTION HISTORY")
                .font(theme::mono(16.0))
                .color(theme::GREEN),
        );
        ui.add_space(10.0);
    });

    // Filter buttons
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let filters = [
            (HistoryFilter::All, "ALL"),
            (HistoryFilter::Received, "RECEIVED"),
            (HistoryFilter::Sent, "SENT"),
        ];
        for (filter, label) in &filters {
            let selected = app.history_filter == *filter;
            let color = if selected { theme::GREEN } else { theme::MUTED };
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(format!("[{}]", label))
                        .font(theme::mono(11.0))
                        .color(color),
                ))
                .clicked()
            {
                app.history_filter = *filter;
                app.history_expanded = None;
            }
            ui.add_space(5.0);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} transactions", app.transactions.len()))
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
            );
        });
    });

    ui.add_space(5.0);
    ui.separator();

    // Filtered transactions
    let filtered: Vec<_> = app
        .transactions
        .iter()
        .filter(|tx| match app.history_filter {
            HistoryFilter::All => true,
            HistoryFilter::Received => tx.tx_type == "received",
            HistoryFilter::Sent => {
                tx.tx_type == "sent"
                    || tx.tx_type == "self"
                    || tx.tx_type == "self_z2t"
                    || tx.tx_type == "self_t2z"
            }
        })
        .cloned()
        .collect();

    if filtered.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            ui.label(
                egui::RichText::new("No transactions yet")
                    .font(theme::mono(13.0))
                    .color(theme::MUTED),
            );
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for (i, tx) in filtered.iter().enumerate() {
                let (icon, color, sign) = match tx.tx_type.as_str() {
                    "received" => ("[+]", theme::GREEN, "+"),
                    "sent" => ("[-]", theme::RED, "-"),
                    "self" => ("[S]", theme::YELLOW, "~"),
                    "self_z2t" => ("[z>t]", theme::YELLOW, "~"),
                    "self_t2z" => ("[t>z]", theme::YELLOW, "~"),
                    _ => ("[?]", theme::MUTED, ""),
                };

                let bg = if i % 2 == 0 {
                    egui::Color32::from_rgb(12, 12, 12)
                } else {
                    theme::PANEL_BG
                };

                let is_expanded = app.history_expanded == Some(i);

                let frame_resp = egui::Frame::none()
                    .fill(bg)
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        // Main row
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(icon)
                                    .font(theme::mono(12.0))
                                    .color(color),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("{}{} ZCL", sign, fmt_zcl(tx.amount)))
                                    .font(theme::mono(13.0))
                                    .color(color),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Confirmations
                                    let conf = if tx.confirmations == 0 {
                                        "unconfirmed".to_string()
                                    } else {
                                        format!("{} conf", tx.confirmations)
                                    };
                                    ui.label(
                                        egui::RichText::new(&conf)
                                            .font(theme::mono(10.0))
                                            .color(theme::MUTED),
                                    );

                                    ui.add_space(6.0);

                                    // Height
                                    if tx.height > 0 {
                                        ui.label(
                                            egui::RichText::new(format!("block {}", tx.height))
                                                .font(theme::mono(10.0))
                                                .color(theme::MUTED),
                                        );
                                        ui.add_space(6.0);
                                    }

                                    // Timestamp
                                    if tx.timestamp > 0 {
                                        ui.label(
                                            egui::RichText::new(format_timestamp(tx.timestamp))
                                                .font(theme::mono(10.0))
                                                .color(theme::MUTED),
                                        );
                                    }
                                },
                            );
                        });

                        let mut btn_clicked = false;

                        // Expanded details
                        if is_expanded {
                            ui.add_space(5.0);
                            ui.separator();
                            detail_row(ui, "TXID", &tx.txid);
                            let copy_btn = ui.add(egui::Button::new(
                                egui::RichText::new("[COPY TXID]")
                                    .font(theme::mono(9.0))
                                    .color(theme::CYAN),
                            ));
                            if copy_btn.clicked() {
                                ctx.copy_text(tx.txid.clone());
                                app.clipboard_clear_at = Some(std::time::Instant::now());
                                // GUI-H3: ensure repaint fires for clipboard auto-clear
                                ctx.request_repaint_after(std::time::Duration::from_secs(31));
                                btn_clicked = true;
                            }

                            detail_row(ui, "Type", &tx.tx_type);
                            detail_row(ui, "Amount", &format!("{} ZCL", fmt_zcl(tx.amount)));
                            if tx.fee > 0 {
                                detail_row(ui, "Fee", &format!("{} ZCL", fmt_zcl(tx.fee)));
                            }
                            if let Some(ref addr) = tx.address {
                                let is_transparent =
                                    addr.starts_with("t1") || addr.starts_with("t3");
                                if is_transparent {
                                    // Transparent addresses are publicly visible on-chain
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new("Address")
                                                .font(theme::mono(10.0))
                                                .color(theme::MUTED),
                                        );
                                        ui.label(
                                            egui::RichText::new("[ON-CHAIN]")
                                                .font(theme::mono(9.0))
                                                .color(theme::YELLOW),
                                        );
                                        ui.label(
                                            egui::RichText::new(addr)
                                                .font(theme::mono(10.0))
                                                .color(theme::GREEN),
                                        );
                                    });
                                } else {
                                    detail_row(ui, "Address", addr);
                                }
                            }
                            if let Some(ref memo) = tx.memo {
                                if !memo.is_empty() {
                                    detail_row(ui, "Memo", memo);
                                }
                            }
                            if tx.height > 0 {
                                detail_row(ui, "Height", &tx.height.to_string());
                            }
                            detail_row(ui, "Confirmations", &tx.confirmations.to_string());
                        }

                        btn_clicked
                    });

                let btn_was_clicked = frame_resp.inner;

                // Click on FULL frame rect (including margins) to expand/collapse.
                // Use raw pointer input instead of ui.interact() to avoid creating
                // a competing click widget that steals clicks from buttons inside.
                let click_rect = frame_resp.response.rect;
                let clicked_row = ctx.input(|i| {
                    i.pointer.primary_released()
                        && i.pointer
                            .latest_pos()
                            .is_some_and(|pos| click_rect.contains(pos))
                });
                if clicked_row && !btn_was_clicked {
                    app.history_expanded = if app.history_expanded == Some(i) {
                        None
                    } else {
                        Some(i)
                    };
                }
                // Hover effect
                if ui.rect_contains_pointer(click_rect) {
                    ui.painter()
                        .rect_filled(click_rect, 0.0, egui::Color32::from_white_alpha(5));
                }
            }
        });
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}: ", label))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
        ui.label(
            egui::RichText::new(value)
                .font(theme::mono(10.0))
                .color(theme::GREEN),
        );
    });
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let elapsed = now.saturating_sub(ts);
    if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
    }
}
