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
            HistoryFilter::Sent => tx.tx_type == "sent" || tx.tx_type == "self",
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
                                egui::RichText::new(format!(
                                    "{}{} ZCL",
                                    sign,
                                    fmt_zcl(tx.amount)
                                ))
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
                                            egui::RichText::new(format!(
                                                "block {}",
                                                tx.height
                                            ))
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

                        // Expanded details
                        if is_expanded {
                            ui.add_space(5.0);
                            ui.separator();
                            detail_row(ui, "TXID", &tx.txid);
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
                            }

                            detail_row(ui, "Type", &tx.tx_type);
                            detail_row(ui, "Amount", &format!("{} ZCL", fmt_zcl(tx.amount)));
                            if tx.fee > 0 {
                                detail_row(ui, "Fee", &format!("{} ZCL", fmt_zcl(tx.fee)));
                            }
                            if let Some(ref addr) = tx.address {
                                detail_row(ui, "Address", addr);
                            }
                            if let Some(ref memo) = tx.memo {
                                if !memo.is_empty() {
                                    detail_row(ui, "Memo", memo);
                                }
                            }
                            if tx.height > 0 {
                                detail_row(ui, "Height", &tx.height.to_string());
                            }
                            detail_row(
                                ui,
                                "Confirmations",
                                &tx.confirmations.to_string(),
                            );
                        }
                    });

                // Click on FULL frame rect (including margins) to expand/collapse
                let click_rect = frame_resp.response.rect;
                let click_resp = ui.interact(click_rect, egui::Id::new(("tx_row", i)), egui::Sense::click());
                if click_resp.clicked() {
                    app.history_expanded = if app.history_expanded == Some(i) {
                        None
                    } else {
                        Some(i)
                    };
                }
                // Hover effect
                if click_resp.hovered() {
                    ui.painter().rect_filled(click_rect, 0.0, egui::Color32::from_white_alpha(5));
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
