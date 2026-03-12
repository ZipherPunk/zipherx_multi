//! Settings view — sync, network, Tor, peers, maintenance, security, danger zone.

use zeroize::Zeroize;

use crate::app::{Phase, ZipherXApp};
use crate::sync::SyncCommand;
use crate::theme;

pub fn show(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("SETTINGS")
                    .font(theme::mono(16.0))
                    .color(theme::GREEN),
            );

            // -- SYNC --
            section_header(ui, "SYNC");
            show_sync_section(app, ui);

            // -- NETWORK --
            section_header(ui, "NETWORK");
            show_network_section(app, ui);

            // -- WALLET MODE --
            section_header(ui, "WALLET MODE");
            show_wallet_mode(app, ui);

            // -- PEER MANAGEMENT --
            section_header(ui, "PEER MANAGEMENT");
            show_peer_management(app, ui);

            // -- SECURITY --
            section_header(ui, "SECURITY");
            show_security_section(app, ui, ctx);

            // -- MAINTENANCE --
            section_header(ui, "MAINTENANCE");
            show_maintenance_section(app, ui);

            // -- ABOUT --
            section_header(ui, "ABOUT");
            show_about_section(app, ui);

            // -- DANGER ZONE --
            section_header(ui, "DANGER ZONE");
            show_danger_zone(app, ui);

            ui.add_space(20.0);
        });
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(15.0);
    ui.label(
        egui::RichText::new(title)
            .font(theme::mono(13.0))
            .color(theme::CYAN),
    );
    ui.separator();
}

// ---------------------------------------------------------------------------
// SYNC
// ---------------------------------------------------------------------------

fn show_sync_section(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let label = if app.is_syncing {
            "[ STOP SYNC ]"
        } else {
            "[ START SYNC ]"
        };
        let color = if app.is_syncing { theme::RED } else { theme::GREEN };
        if ui
            .add(egui::Button::new(
                egui::RichText::new(label)
                    .font(theme::mono(12.0))
                    .color(color),
            ))
            .clicked()
        {
            if app.is_syncing {
                app.is_syncing = false;
            } else if let (Some(ref state), Some(ref sk)) =
                (&app.shared_state, &app.sk_bytes)
            {
                if let Ok(mut s) = state.lock() {
                    s.command = Some(SyncCommand::StartSync {
                        sk_bytes: sk.clone(),
                    });
                }
                app.is_syncing = true;
            }
        }
    });
    if app.block_height > 0 {
        ui.label(
            egui::RichText::new(format!("Block height: {}", app.block_height))
                .font(theme::mono(11.0))
                .color(theme::MUTED),
        );
    }
    ui.label(
        egui::RichText::new("First sync: 10-30 min. Subsequent: <1 min.")
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );
}

// ---------------------------------------------------------------------------
// NETWORK
// ---------------------------------------------------------------------------

fn show_network_section(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let peer_color = if app.peer_count > 0 {
        theme::GREEN
    } else {
        theme::RED
    };
    ui.label(
        egui::RichText::new(format!("Connected peers: {}", app.peer_count))
            .font(theme::mono(11.0))
            .color(peer_color),
    );

    // Tor toggle
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Tor:")
                .font(theme::mono(11.0))
                .color(theme::MUTED),
        );
        let tor_label = if app.tor_enabled {
            "[ENABLED]"
        } else {
            "[DISABLED]"
        };
        let tor_color = if app.tor_enabled {
            theme::GREEN
        } else {
            theme::MUTED
        };
        if ui
            .add(egui::Button::new(
                egui::RichText::new(tor_label)
                    .font(theme::mono(11.0))
                    .color(tor_color),
            ))
            .clicked()
        {
            app.tor_enabled = !app.tor_enabled;
            if let Some(ref state) = app.shared_state {
                if let Ok(mut s) = state.lock() {
                    s.command = Some(SyncCommand::SetTorEnabled(app.tor_enabled));
                }
            }
        }
    });
    if app.tor_enabled {
        ui.label(
            egui::RichText::new("All P2P traffic routed through Tor. Takes effect on next sync.")
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
    }
    ui.label(
        egui::RichText::new(format!("Status: {}", app.tor_state))
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );
    if let Some(ref onion) = app.onion_address {
        ui.label(
            egui::RichText::new(format!("Onion: {}", onion))
                .font(theme::mono(10.0))
                .color(theme::CYAN),
        );
    }
}

// ---------------------------------------------------------------------------
// WALLET MODE
// ---------------------------------------------------------------------------

fn show_wallet_mode(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    // -- Current mode display (bordered panel with icon, name, description, badge) --
    let (mode_icon, mode_name, mode_desc, mode_color) = if app.fullnode_enabled {
        (
            "[N]",
            "ZipherX Full Node",
            "Full blockchain verification with local zclassicd daemon",
            theme::CYAN,
        )
    } else {
        (
            "[P]",
            "ZipherX P2P Light",
            "Fast P2P network, no local daemon required",
            theme::GREEN,
        )
    };

    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(12.0)
        .rounding(4.0)
        .stroke(egui::Stroke::new(1.0, mode_color.linear_multiply(0.5)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(mode_icon)
                        .font(theme::mono(16.0))
                        .color(mode_color),
                );
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(mode_name)
                            .font(theme::mono(12.0))
                            .color(mode_color),
                    );
                    ui.label(
                        egui::RichText::new(mode_desc)
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::none()
                        .fill(theme::GREEN.linear_multiply(0.2))
                        .inner_margin(egui::Vec2::new(8.0, 3.0))
                        .rounding(3.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("Active")
                                    .font(theme::mono(9.0))
                                    .color(theme::GREEN),
                            );
                        });
                });
            });
        });

    ui.add_space(8.0);

    // -- Wallet Source picker (two styled buttons in bordered container) --
    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(10.0)
        .rounding(4.0)
        .stroke(egui::Stroke::new(1.0, theme::MUTED.linear_multiply(0.3)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Wallet Source")
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
            });
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                let p2p_selected = !app.fullnode_enabled;
                let node_selected = app.fullnode_enabled;

                // P2P button
                let p2p_fill = if p2p_selected {
                    theme::GREEN.linear_multiply(0.25)
                } else {
                    egui::Color32::from_rgb(25, 25, 25)
                };
                let p2p_text_color = if p2p_selected {
                    egui::Color32::WHITE
                } else {
                    theme::MUTED
                };
                let p2p_btn = egui::Button::new(
                    egui::RichText::new("[P2P]  P2P Light")
                        .font(theme::mono(11.0))
                        .color(p2p_text_color),
                )
                .fill(p2p_fill)
                .stroke(egui::Stroke::new(
                    1.0,
                    if p2p_selected { theme::GREEN.linear_multiply(0.5) } else { theme::MUTED.linear_multiply(0.3) },
                ))
                .rounding(4.0);

                if ui.add(p2p_btn).clicked() && !p2p_selected {
                    app.fullnode_enabled = false;
                    app.show_fullnode_confirm = false;
                }

                ui.add_space(8.0);

                // Full Node button
                let fn_fill = if node_selected {
                    theme::CYAN.linear_multiply(0.25)
                } else {
                    egui::Color32::from_rgb(25, 25, 25)
                };
                let fn_text_color = if node_selected {
                    egui::Color32::WHITE
                } else {
                    theme::MUTED
                };
                let fn_btn = egui::Button::new(
                    egui::RichText::new("[N]  Full Node")
                        .font(theme::mono(11.0))
                        .color(fn_text_color),
                )
                .fill(fn_fill)
                .stroke(egui::Stroke::new(
                    1.0,
                    if node_selected { theme::CYAN.linear_multiply(0.5) } else { theme::MUTED.linear_multiply(0.3) },
                ))
                .rounding(4.0);

                if ui.add(fn_btn).clicked() && !node_selected {
                    app.show_fullnode_confirm = true;
                }
            });

            ui.add_space(4.0);
            let source_desc = if app.fullnode_enabled {
                "Full blockchain verification with local daemon"
            } else {
                "Secure wallet with P2P network"
            };
            ui.label(
                egui::RichText::new(source_desc)
                    .font(theme::mono(9.0))
                    .color(theme::MUTED),
            );
        });

    // -- Mode change confirmation (security notice) --
    if app.show_fullnode_confirm {
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(25, 20, 10))
            .inner_margin(14.0)
            .rounding(4.0)
            .stroke(egui::Stroke::new(1.0, theme::YELLOW.linear_multiply(0.5)))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("SWITCH TO FULL NODE MODE?")
                        .font(theme::mono(13.0))
                        .color(theme::YELLOW),
                );
                ui.add_space(8.0);

                ui.label(
                    egui::RichText::new("SECURITY NOTICE")
                        .font(theme::mono(11.0))
                        .color(theme::CYAN),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Full Node mode uses the local zclassicd daemon.\n\
                         Your wallet connects to your own node via local RPC\n\
                         for maximum privacy and trustlessness.\n\
                         You verify every block independently, trusting no one.",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("REQUIREMENTS")
                        .font(theme::mono(11.0))
                        .color(theme::CYAN),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "\u{2022} ~15 GB disk space for blockchain data\n\
                         \u{2022} ~10 minutes with bootstrap (or 2-4 hours without)\n\
                         \u{2022} Stable internet connection during sync\n\
                         \u{2022} zclassicd + zclassic-cli must be installed",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("BOOTSTRAP")
                        .font(theme::mono(11.0))
                        .color(theme::CYAN),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "ZipherX downloads the latest blockchain bootstrap for fast sync:\n\
                         github.com/ZipherPunk/zclassic-bootstrap",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "\"By running a full node, you become part of the network's\n\
                         backbone. You verify every transaction independently,\n\
                         trusting no one. This is the cypherpunk way.\"",
                    )
                    .font(theme::mono(9.0))
                    .color(theme::GREEN),
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("[ I UNDERSTAND, CONTINUE ]")
                                    .font(theme::mono(12.0))
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(theme::GREEN.linear_multiply(0.3))
                            .stroke(egui::Stroke::new(1.0, theme::GREEN.linear_multiply(0.6)))
                            .rounding(4.0),
                        )
                        .clicked()
                    {
                        app.fullnode_enabled = true;
                        app.show_fullnode_confirm = false;
                        app.tab = crate::app::Tab::Node;
                    }
                    ui.add_space(8.0);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CANCEL ]")
                                .font(theme::mono(12.0))
                                .color(theme::MUTED),
                        ))
                        .clicked()
                    {
                        app.show_fullnode_confirm = false;
                    }
                });
            });
    }

    // -- Full Node status (when active) --
    if app.fullnode_enabled && !app.show_fullnode_confirm {
        ui.add_space(8.0);

        // Daemon connection status (bordered, color-coded)
        let is_connected =
            matches!(app.node_daemon_status, crate::fullnode::manager::DaemonStatus::Running);
        let border_color = if is_connected {
            theme::GREEN.linear_multiply(0.5)
        } else {
            theme::RED.linear_multiply(0.5)
        };

        egui::Frame::none()
            .fill(theme::PANEL_BG)
            .inner_margin(10.0)
            .rounding(4.0)
            .stroke(egui::Stroke::new(1.0, border_color))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (dot, status_text) = match &app.node_daemon_status {
                        crate::fullnode::manager::DaemonStatus::Running => {
                            (theme::GREEN, "Daemon Connected")
                        }
                        crate::fullnode::manager::DaemonStatus::Starting => {
                            (theme::YELLOW, "Daemon Starting...")
                        }
                        crate::fullnode::manager::DaemonStatus::Stopping => {
                            (theme::YELLOW, "Daemon Stopping...")
                        }
                        crate::fullnode::manager::DaemonStatus::Error(e) => {
                            let _ = e;
                            (theme::RED, "Daemon Error")
                        }
                        _ => (theme::RED, "Daemon Offline"),
                    };
                    ui.colored_label(dot, "\u{2022}");
                    ui.label(
                        egui::RichText::new(status_text)
                            .font(theme::mono(11.0))
                            .color(dot),
                    );

                    // Block height on the right
                    if is_connected {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if let Some(ref info) = app.node_chain_info {
                                    ui.label(
                                        egui::RichText::new(format!("Block {}", info.blocks))
                                            .font(theme::mono(10.0))
                                            .color(theme::MUTED),
                                    );
                                }
                            },
                        );
                    }
                });
            });

        // Node sync progress (if syncing)
        if is_connected {
            if let Some(ref info) = app.node_chain_info {
                if info.blocks < info.headers && info.headers > 0 {
                    let progress = info.blocks as f32 / info.headers as f32;
                    ui.add_space(4.0);
                    let bar_width = ui.available_width() - 20.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::Vec2::new(bar_width, 4.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(
                        rect,
                        2.0,
                        egui::Color32::from_rgb(30, 30, 30),
                    );
                    let filled = egui::Rect::from_min_size(
                        rect.min,
                        egui::Vec2::new(rect.width() * progress, rect.height()),
                    );
                    ui.painter().rect_filled(filled, 2.0, theme::GREEN);
                    ui.label(
                        egui::RichText::new(format!(
                            "Syncing: {} / {} ({:.1}%)",
                            info.blocks,
                            info.headers,
                            progress * 100.0
                        ))
                        .font(theme::mono(9.0))
                        .color(theme::MUTED),
                    );
                }
            }
        }

        // Network info
        if let Some(ref net) = app.node_network_info {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "Connections: {}  |  {}  |  Protocol {}",
                    net.connections, net.subversion, net.protocol_version
                ))
                .font(theme::mono(9.0))
                .color(theme::MUTED),
            );
        }

        // Node Management button (styled like official app)
        ui.add_space(8.0);
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("[ NODE MANAGEMENT ]  >")
                        .font(theme::mono(12.0))
                        .color(egui::Color32::WHITE),
                )
                .fill(theme::CYAN.linear_multiply(0.2))
                .stroke(egui::Stroke::new(1.0, theme::CYAN.linear_multiply(0.4)))
                .rounding(4.0),
            )
            .clicked()
        {
            app.tab = crate::app::Tab::Node;
        }
    }
}

// ---------------------------------------------------------------------------
// PEER MANAGEMENT (collapsible)
// ---------------------------------------------------------------------------

fn show_peer_management(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    // Collapsible header
    let toggle_icon = if app.peer_section_expanded { "[-]" } else { "[+]" };
    let header_text = format!(
        "PEER DETAILS  {} connected  {}",
        app.peer_infos.len(),
        toggle_icon,
    );
    if ui
        .add(egui::Button::new(
            egui::RichText::new(&header_text)
                .font(theme::mono(11.0))
                .color(theme::GREEN),
        ).frame(false))
        .clicked()
    {
        app.peer_section_expanded = !app.peer_section_expanded;
        if app.peer_section_expanded {
            // Request peer info from wallet thread
            if let Some(ref state) = app.shared_state {
                if let Ok(mut s) = state.lock() {
                    s.command = Some(SyncCommand::RefreshPeerInfo);
                }
            }
        }
    }

    if !app.peer_section_expanded {
        return;
    }

    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(10.0)
        .rounding(4.0)
        .show(ui, |ui| {
            // Connected peers list
            if app.peer_infos.is_empty() {
                ui.label(
                    egui::RichText::new("No peers connected.")
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
            } else {
                for peer in &app.peer_infos {
                    ui.horizontal(|ui| {
                        ui.colored_label(theme::GREEN, "\u{2022}");
                        ui.label(
                            egui::RichText::new(&peer.address)
                                .font(theme::mono(10.0))
                                .color(theme::GREEN),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "v{} {} h:{}",
                                peer.protocol_version, peer.user_agent, peer.start_height
                            ))
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                        );
                    });
                }
            }

            ui.add_space(8.0);

            // Refresh button
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ REFRESH ]")
                        .font(theme::mono(10.0))
                        .color(theme::CYAN),
                ))
                .clicked()
            {
                if let Some(ref state) = app.shared_state {
                    if let Ok(mut s) = state.lock() {
                        s.command = Some(SyncCommand::RefreshPeerInfo);
                    }
                }
            }

            ui.add_space(8.0);

            // Add custom peer
            ui.label(
                egui::RichText::new("ADD CUSTOM PEER")
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.custom_peer_host)
                        .hint_text("IP address")
                        .font(theme::mono(10.0))
                        .desired_width(140.0),
                );
                ui.label(
                    egui::RichText::new(":")
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut app.custom_peer_port)
                        .hint_text("8033")
                        .font(theme::mono(10.0))
                        .desired_width(50.0),
                );
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ ADD ]")
                            .font(theme::mono(10.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                {
                    // TODO: wire to SyncCommand::AddPeer
                    app.peer_action_result =
                        Some("Custom peer support coming soon.".to_string());
                }
            });

            if let Some(ref result) = app.peer_action_result {
                ui.label(
                    egui::RichText::new(result)
                        .font(theme::mono(9.0))
                        .color(theme::YELLOW),
                );
            }
        });
}

// ---------------------------------------------------------------------------
// SECURITY
// ---------------------------------------------------------------------------

fn show_security_section(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Auto-lock timeout
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Auto-lock:")
                .font(theme::mono(11.0))
                .color(theme::MUTED),
        );
        let options = [
            (60, "1 min"),
            (300, "5 min"),
            (900, "15 min"),
            (0, "Never"),
        ];
        for (secs, label) in &options {
            let selected = app.auto_lock_secs == *secs;
            let color = if selected { theme::GREEN } else { theme::MUTED };
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(format!("[{}]", label))
                        .font(theme::mono(10.0))
                        .color(color),
                ))
                .clicked()
            {
                app.auto_lock_secs = *secs;
            }
        }
    });

    // Export private key
    ui.add_space(5.0);
    if !app.show_export {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ EXPORT PRIVATE KEY ]")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            ))
            .clicked()
        {
            app.show_export_confirm = true;
        }
    }

    // Export confirmation dialog
    if app.show_export_confirm {
        show_export_confirm(app, ui, ctx);
    }

    // Export key display (auto-dismisses after 60s)
    if app.show_export {
        show_export_display(app, ui, ctx);
    }
}

// ---------------------------------------------------------------------------
// MAINTENANCE
// ---------------------------------------------------------------------------

fn show_maintenance_section(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let can_act = !app.maintenance_in_progress && !app.is_syncing;

    // Repair Database
    if !app.show_repair_confirm {
        if ui
            .add_enabled(
                can_act,
                egui::Button::new(
                    egui::RichText::new("[ REPAIR DATABASE ]")
                        .font(theme::mono(12.0))
                        .color(if can_act { theme::YELLOW } else { theme::MUTED }),
                ),
            )
            .clicked()
        {
            app.show_repair_confirm = true;
        }
    }

    ui.label(
        egui::RichText::new("Clears tree state, preserves notes and history.")
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );

    // Repair confirmation
    if app.show_repair_confirm {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(30, 25, 0))
            .inner_margin(10.0)
            .rounding(4.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Repair database? This will clear the commitment tree.")
                        .font(theme::mono(11.0))
                        .color(theme::YELLOW),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ REPAIR ]")
                                .font(theme::mono(11.0))
                                .color(theme::RED),
                        ))
                        .clicked()
                    {
                        if let Some(ref state) = app.shared_state {
                            if let Ok(mut s) = state.lock() {
                                s.command = Some(SyncCommand::RepairDatabase);
                            }
                        }
                        app.maintenance_in_progress = true;
                        app.maintenance_status = Some("Repairing database...".to_string());
                        app.show_repair_confirm = false;
                    }
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CANCEL ]")
                                .font(theme::mono(11.0))
                                .color(theme::MUTED),
                        ))
                        .clicked()
                    {
                        app.show_repair_confirm = false;
                    }
                });
            });
    }

    ui.add_space(8.0);

    // Full Rescan
    if !app.show_rescan_confirm {
        if ui
            .add_enabled(
                can_act,
                egui::Button::new(
                    egui::RichText::new("[ FULL RESCAN ]")
                        .font(theme::mono(12.0))
                        .color(if can_act { theme::YELLOW } else { theme::MUTED }),
                ),
            )
            .clicked()
        {
            app.show_rescan_confirm = true;
        }
    }

    ui.label(
        egui::RichText::new("Re-downloads everything from scratch.")
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );

    // Rescan confirmation
    if app.show_rescan_confirm {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(30, 25, 0))
            .inner_margin(10.0)
            .rounding(4.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Full rescan? Clears ALL sync state and re-downloads everything.",
                    )
                    .font(theme::mono(11.0))
                    .color(theme::YELLOW),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ FULL RESCAN ]")
                                .font(theme::mono(11.0))
                                .color(theme::RED),
                        ))
                        .clicked()
                    {
                        if let Some(ref state) = app.shared_state {
                            if let Ok(mut s) = state.lock() {
                                s.command = Some(SyncCommand::FullRescan);
                            }
                        }
                        app.maintenance_in_progress = true;
                        app.maintenance_status =
                            Some("Starting full rescan...".to_string());
                        app.show_rescan_confirm = false;
                    }
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CANCEL ]")
                                .font(theme::mono(11.0))
                                .color(theme::MUTED),
                        ))
                        .clicked()
                    {
                        app.show_rescan_confirm = false;
                    }
                });
            });
    }

    // Status display
    if app.maintenance_in_progress {
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.spinner();
            if let Some(ref status) = app.maintenance_status {
                ui.label(
                    egui::RichText::new(status)
                        .font(theme::mono(10.0))
                        .color(theme::YELLOW),
                );
            }
        });
    } else if let Some(ref status) = app.maintenance_status {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(status)
                .font(theme::mono(10.0))
                .color(theme::GREEN),
        );
    }
}

// ---------------------------------------------------------------------------
// ABOUT
// ---------------------------------------------------------------------------

fn show_about_section(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(format!("ZipherX v{}", app.version))
            .font(theme::mono(11.0))
            .color(theme::MUTED),
    );
    ui.label(
        egui::RichText::new(format!(
            "Platform: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
        .font(theme::mono(10.0))
        .color(theme::MUTED),
    );
    ui.label(
        egui::RichText::new("Runtime: native egui (no JVM)")
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );
    if app.block_height > 0 {
        ui.label(
            egui::RichText::new(format!("Synced to: {}", app.block_height))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
    }
}

// ---------------------------------------------------------------------------
// DANGER ZONE
// ---------------------------------------------------------------------------

fn show_danger_zone(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.add_space(5.0);
    if ui
        .add(egui::Button::new(
            egui::RichText::new("[ DELETE ALL DATA ]")
                .font(theme::mono(13.0))
                .color(theme::RED),
        ))
        .clicked()
    {
        // Show inline confirmation
        app.password_error = Some("CONFIRM_DELETE".into());
    }

    // Delete confirmation
    if app.password_error.as_deref() == Some("CONFIRM_DELETE") {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(40, 10, 10))
            .inner_margin(12.0)
            .rounding(4.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("ARE YOU SURE? This cannot be undone.")
                        .font(theme::mono(12.0))
                        .color(theme::RED),
                );
                ui.label(
                    egui::RichText::new(
                        "All wallet data, keys, and databases will be permanently deleted.",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );
                ui.add_space(5.0);

                ui.add(
                    egui::TextEdit::singleline(&mut app.reauth_password)
                        .password(true)
                        .hint_text("Enter password to confirm")
                        .font(theme::mono(12.0))
                        .desired_width((ui.available_width() - 20.0).min(350.0)),
                );

                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ DELETE EVERYTHING ]")
                                .font(theme::mono(12.0))
                                .color(theme::RED),
                        ))
                        .clicked()
                    {
                        if app.storage.verify_password(&app.reauth_password) {
                            // Stop sync
                            if let Some(ref state) = app.shared_state {
                                if let Ok(mut s) = state.lock() {
                                    s.command = Some(SyncCommand::Stop);
                                }
                            }

                            // Zeroize secrets
                            if let Some(ref mut sk) = app.sk_bytes {
                                for b in sk.iter_mut() {
                                    unsafe { std::ptr::write_volatile(b, 0) };
                                }
                            }
                            app.sk_bytes = None;

                            // Delete all data
                            app.storage.delete_all_data();

                            // Reset state
                            app.address = None;
                            app.balance = Default::default();
                            app.transactions.clear();
                            app.shared_state = None;
                            app.reauth_password.zeroize();
                            app.password_error = None;
                            app.phase = Phase::Locked;
                        } else {
                            app.password_error = Some("Wrong password".into());
                        }
                    }
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CANCEL ]")
                                .font(theme::mono(12.0))
                                .color(theme::MUTED),
                        ))
                        .clicked()
                    {
                        app.reauth_password.zeroize();
                        app.password_error = None;
                    }
                });
            });
    }
}

// ---------------------------------------------------------------------------
// Export key helpers
// ---------------------------------------------------------------------------

fn show_export_confirm(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(25, 20, 10))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("ENTER PASSWORD TO EXPORT KEY")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(5.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut app.export_password)
                    .password(true)
                    .hint_text("Password")
                    .font(theme::mono(12.0))
                    .desired_width((ui.available_width() - 20.0).min(350.0)),
            );

            ui.add_space(5.0);
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CONFIRM ]")
                            .font(theme::mono(12.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                    || enter
                {
                    if app.storage.verify_password(&app.export_password) {
                        // Export key
                        if let Some(ref sk) = app.sk_bytes {
                            match zipherx_crypto::keys::encode_spending_key(sk) {
                                Ok(encoded) => {
                                    app.export_key_display = encoded;
                                    app.show_export = true;
                                    app.show_export_confirm = false;
                                    app.export_auto_dismiss =
                                        Some(std::time::Instant::now());
                                }
                                Err(e) => {
                                    app.password_error =
                                        Some(format!("Export failed: {}", e));
                                }
                            }
                        }
                    } else {
                        app.password_error = Some("Wrong password".into());
                    }
                    app.export_password.zeroize();
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(12.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.export_password.zeroize();
                    app.show_export_confirm = false;
                }
            });

            if let Some(ref err) = app.password_error {
                if err != "CONFIRM_DELETE" {
                    ui.label(
                        egui::RichText::new(err)
                            .font(theme::mono(10.0))
                            .color(theme::RED),
                    );
                }
            }
        });
}

fn show_export_display(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Auto-dismiss after 60s
    if let Some(start) = app.export_auto_dismiss {
        if start.elapsed().as_secs() >= 60 {
            app.export_key_display.zeroize();
            app.show_export = false;
            app.export_auto_dismiss = None;
            return;
        }
        let remaining = 60 - start.elapsed().as_secs();
        ui.label(
            egui::RichText::new(format!("Auto-dismiss in {}s", remaining))
                .font(theme::mono(9.0))
                .color(theme::MUTED),
        );
    }

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(30, 10, 10))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("PRIVATE KEY \u{2014} KEEP SECRET")
                    .font(theme::mono(12.0))
                    .color(theme::RED),
            );
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(&app.export_key_display)
                    .font(theme::mono(9.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ COPY ]")
                            .font(theme::mono(11.0))
                            .color(theme::CYAN),
                    ))
                    .clicked()
                {
                    ctx.copy_text(app.export_key_display.clone());
                    app.clipboard_clear_at = Some(std::time::Instant::now());
                    // GUI-H3: ensure repaint fires for clipboard auto-clear
                    ctx.request_repaint_after(std::time::Duration::from_secs(31));
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ DISMISS ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.export_key_display.zeroize();
                    app.show_export = false;
                    app.export_auto_dismiss = None;
                }
            });
        });
}
