//! Settings view — sync, network, Tor, peers, maintenance, security, danger zone.

use zeroize::Zeroize;

use crate::app::ZipherXApp;
use crate::sync::SyncCommand;
use crate::theme;
use zipherx_platform::SecureStorage;

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
        let color = if app.is_syncing {
            theme::RED
        } else {
            theme::GREEN
        };
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
            } else if let (Some(ref state), Some(ref sk)) = (&app.shared_state, &app.sk_bytes) {
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
                    if p2p_selected {
                        theme::GREEN.linear_multiply(0.5)
                    } else {
                        theme::MUTED.linear_multiply(0.3)
                    },
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
                    if node_selected {
                        theme::CYAN.linear_multiply(0.5)
                    } else {
                        theme::MUTED.linear_multiply(0.3)
                    },
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
        let is_connected = matches!(
            app.node_daemon_status,
            crate::fullnode::manager::DaemonStatus::Running
        );
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
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(ref info) = app.node_chain_info {
                                ui.label(
                                    egui::RichText::new(format!("Block {}", info.blocks))
                                        .font(theme::mono(10.0))
                                        .color(theme::MUTED),
                                );
                            }
                        });
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
                    let (rect, _) = ui
                        .allocate_exact_size(egui::Vec2::new(bar_width, 4.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 2.0, egui::Color32::from_rgb(30, 30, 30));
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
    let toggle_icon = if app.peer_section_expanded {
        "[-]"
    } else {
        "[+]"
    };
    let header_text = format!(
        "PEER DETAILS  {} connected  {}",
        app.peer_infos.len(),
        toggle_icon,
    );
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new(&header_text)
                    .font(theme::mono(11.0))
                    .color(theme::GREEN),
            )
            .frame(false),
        )
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
                    app.peer_action_result = Some("Custom peer support coming soon.".to_string());
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
        let options = [(60, "1 min"), (300, "5 min"), (900, "15 min"), (0, "Never")];
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

    // Unified Backup / Export button
    ui.add_space(5.0);
    if !app.show_export && !app.show_mnemonic_export && !app.show_seed_export {
        if ui
            .add(
                egui::Button::new(
                    egui::RichText::new("[ BACKUP / EXPORT ]")
                        .font(theme::mono(12.0))
                        .color(egui::Color32::WHITE),
                )
                .fill(theme::GREEN.linear_multiply(0.2))
                .stroke(egui::Stroke::new(1.0, theme::GREEN.linear_multiply(0.5)))
                .rounding(4.0),
            )
            .clicked()
        {
            app.show_export_confirm = true;
            app.show_export_step2 = false;
        }
        ui.label(
            egui::RichText::new("Export recovery phrase, private keys, or seed.")
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
    }

    // Export confirmation dialog (password re-auth, then stepped flow)
    if app.show_export_confirm {
        show_export_confirm(app, ui, ctx);
    }

    // Export key display — step 2: individual keys (auto-dismisses after 60s)
    if app.show_export {
        show_export_display(app, ui, ctx);
    }

    // Mnemonic display (auto-dismisses after 60s)
    if app.show_mnemonic_export {
        show_mnemonic_export_display(app, ui, ctx);
    }

    // Seed display
    if app.show_seed_export {
        show_seed_export_display(app, ui, ctx);
    }

    // WIF Import button
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(5.0);
    if !app.show_wif_import {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ IMPORT WIF KEYS ]")
                    .font(theme::mono(12.0))
                    .color(theme::CYAN),
            ))
            .clicked()
        {
            app.show_wif_import = true;
        }
        ui.label(
            egui::RichText::new("Import transparent private keys (WIF format).")
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
    }

    if app.show_wif_import {
        show_wif_import(app, ui, ctx);
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
                        app.maintenance_status = Some("Starting full rescan...".to_string());
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
                            drop(app.sk_bytes.take()); // Ensure zeroed bytes are dropped

                            // Delete all data from encrypted storage
                            app.storage.delete_all_data();

                            // Delete wallet DB, headers, delta store, boost cache
                            let data_dir = app.storage.data_dir().clone();
                            for name in &[
                                "wallet.db", "wallet.db-wal", "wallet.db-shm",
                                "zipherx_wallet.db", "zipherx_wallet.db-wal", "zipherx_wallet.db-shm",
                                "headers.db", "headers.db-wal", "headers.db-shm",
                                "zipherx_headers.db", "zipherx_headers.db-wal", "zipherx_headers.db-shm",
                            ] {
                                let _ = std::fs::remove_file(data_dir.join(name));
                            }
                            for dir_name in &["delta", "BoostCache"] {
                                let _ = std::fs::remove_dir_all(data_dir.join(dir_name));
                            }
                            // Delete delta files
                            for pattern in &[
                                "delta_manifest.json", "delta_nullifiers.bin",
                                "delta_sapling_roots.bin", "shielded_outputs_delta.bin",
                            ] {
                                let _ = std::fs::remove_file(data_dir.join(pattern));
                            }

                            eprintln!("[ZipherX] All data deleted. Exiting for clean restart.");

                            // Exit the process — wallet thread holds stale state
                            std::process::exit(0);
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
            // If password not yet verified, show password prompt
            if !app.show_export_step2 && !app.show_export {
                ui.label(
                    egui::RichText::new("ENTER PASSWORD TO ACCESS BACKUP")
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
                let enter =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
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
                            // Password verified — move to Step 1 (backup options)
                            app.show_export = true;
                            app.show_export_step2 = false;
                            app.password_error = None;
                            // Pre-load shielded key
                            if let Some(ref sk) = app.sk_bytes {
                                if let Ok(encoded) = zipherx_crypto::keys::encode_spending_key(sk) {
                                    app.export_key_display = encoded;
                                }
                            }
                            // Pre-load primary transparent WIF
                            if let Ok(seed) = app.storage.load_key("wallet_seed") {
                                if let Ok(wif) =
                                    zipherx_crypto::transparent::export_transparent_wif(
                                        &seed, 0, 0, false,
                                    )
                                {
                                    app.export_t_key_display = (*wif).clone();
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
            } else if !app.show_export_step2 {
                // Step 1: BACKUP YOUR WALLET — password was verified
                ui.label(
                    egui::RichText::new("BACKUP YOUR WALLET")
                        .font(theme::mono(13.0))
                        .color(theme::GREEN),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Your recovery phrase backs up your entire wallet:\n\
                         shielded keys, transparent keys, and all derived addresses.\n\
                         Export it first as your primary backup.",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );
                ui.add_space(10.0);

                // Primary action: Export Recovery Phrase
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("[ EXPORT RECOVERY PHRASE ]")
                                .font(theme::mono(12.0))
                                .color(egui::Color32::WHITE),
                        )
                        .fill(theme::GREEN.linear_multiply(0.3))
                        .stroke(egui::Stroke::new(1.0, theme::GREEN.linear_multiply(0.6)))
                        .rounding(4.0),
                    )
                    .clicked()
                {
                    // Load mnemonic from secure storage
                    match app.storage.load_key("wallet_mnemonic") {
                        Ok(mnemonic_bytes) => {
                            if let Ok(phrase) = String::from_utf8(mnemonic_bytes) {
                                app.export_mnemonic_display = phrase;
                                app.show_mnemonic_export = true;
                                app.show_export_confirm = false;
                                app.mnemonic_export_auto_dismiss =
                                    Some(std::time::Instant::now());
                            } else {
                                app.password_error =
                                    Some("Failed to decode mnemonic".into());
                            }
                        }
                        Err(_) => {
                            app.password_error = Some(
                                "No recovery phrase stored (wallet imported from key/seed)"
                                    .into(),
                            );
                        }
                    }
                }

                ui.add_space(6.0);

                // Secondary: Show Individual Keys
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ SHOW INDIVIDUAL KEYS ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    // Load funded transparent keys for export display.
                    // For seed-derived keys: derive WIF from seed + child_index.
                    // For imported keys: show address + message (user must keep backup).
                    app.export_funded_keys.clear();

                    // Read funded transparent addresses from shared state
                    let funded_keys: Vec<(String, u64, bool, u32, bool)> =
                        if let Some(ref st) = app.shared_state {
                            if let Ok(s) = st.lock() {
                                s.funded_transparent_keys.clone()
                            } else {
                                Vec::new()
                            }
                        } else {
                            Vec::new()
                        };

                    // Try to load seed for deriving WIFs of seed-derived keys
                    let seed_opt = app.storage.load_key("wallet_seed").ok();

                    eprintln!("[ZipherX] Export: {} funded transparent addresses found", funded_keys.len());
                    for (addr, balance, is_change, child_index, is_imported) in &funded_keys {
                        eprintln!("[ZipherX]   addr={} balance={} is_change={} child_index={} is_imported={}",
                            addr, balance, is_change, child_index, is_imported);
                        if *is_imported {
                            // Imported key — WIF not derivable from seed
                            app.export_funded_keys.push((
                                addr.clone(),
                                "IMPORTED \u{2014} keep your original WIF backup".to_string(),
                                *balance,
                                *is_change,
                                true,
                            ));
                        } else if let Some(ref seed) = seed_opt {
                            // Seed-derived key — derive WIF
                            match zipherx_crypto::transparent::export_transparent_wif(
                                seed,
                                0,
                                *child_index,
                                *is_change,
                            ) {
                                Ok(wif) => {
                                    app.export_funded_keys.push((
                                        addr.clone(),
                                        (*wif).clone(),
                                        *balance,
                                        *is_change,
                                        false,
                                    ));
                                }
                                Err(e) => {
                                    eprintln!("[ZipherX]   WIF derivation FAILED for {}: {}", addr, e);
                                    // Still show the address without WIF
                                    app.export_funded_keys.push((
                                        addr.clone(),
                                        format!("WIF derivation failed: {}", e),
                                        *balance,
                                        *is_change,
                                        false,
                                    ));
                                }
                            }
                        }
                    }

                    // Fallback: if no funded keys from shared state, use primary t-addr WIF
                    if app.export_funded_keys.is_empty() && !app.export_t_key_display.is_empty() {
                        if let Some(ref t_addr) = app.transparent_address {
                            app.export_funded_keys.push((
                                t_addr.clone(),
                                app.export_t_key_display.clone(),
                                app.transparent_balance,
                                false,
                                false,
                            ));
                        }
                    }

                    app.show_export_step2 = true;
                    app.show_export = true;
                    app.show_export_confirm = false;
                    app.export_auto_dismiss = Some(std::time::Instant::now());
                }

                ui.add_space(6.0);

                // Tertiary: Export Seed (Hex)
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ EXPORT SEED (HEX) ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    match app.storage.load_key("wallet_seed") {
                        Ok(seed) => {
                            app.export_seed_display = hex::encode(&seed);
                            app.show_seed_export = true;
                            app.show_export_confirm = false;
                            app.seed_export_auto_dismiss = Some(std::time::Instant::now());
                        }
                        Err(_) => {
                            app.password_error =
                                Some("No seed stored (wallet imported from key only)".into());
                        }
                    }
                }

                ui.add_space(8.0);
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.show_export_confirm = false;
                    app.export_key_display.zeroize();
                    app.export_t_key_display.zeroize();
                }

                if let Some(ref err) = app.password_error {
                    if err != "CONFIRM_DELETE" {
                        ui.label(
                            egui::RichText::new(err)
                                .font(theme::mono(10.0))
                                .color(theme::RED),
                        );
                    }
                }
            }
        });
}

/// Mnemonic export confirmation — now triggered from the unified backup flow.
#[allow(dead_code)]
fn show_mnemonic_export_confirm(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(25, 20, 10))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("ENTER PASSWORD TO EXPORT RECOVERY PHRASE")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(5.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut app.mnemonic_export_password)
                    .password(true)
                    .hint_text("Password")
                    .font(theme::mono(12.0))
                    .desired_width((ui.available_width() - 20.0).min(350.0)),
            );

            ui.add_space(5.0);
            let enter =
                response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
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
                    if app.storage.verify_password(&app.mnemonic_export_password) {
                        match app.storage.load_key("wallet_mnemonic") {
                            Ok(mnemonic_bytes) => {
                                if let Ok(phrase) = String::from_utf8(mnemonic_bytes) {
                                    app.export_mnemonic_display = phrase;
                                    app.show_mnemonic_export = true;
                                    app.show_mnemonic_export_confirm = false;
                                    app.mnemonic_export_auto_dismiss =
                                        Some(std::time::Instant::now());
                                } else {
                                    app.password_error =
                                        Some("Failed to decode mnemonic".into());
                                }
                            }
                            Err(_) => {
                                app.password_error = Some(
                                    "No recovery phrase stored (wallet imported from key/seed)"
                                        .into(),
                                );
                            }
                        }
                    } else {
                        app.password_error = Some("Wrong password".into());
                    }
                    app.mnemonic_export_password.zeroize();
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(12.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.mnemonic_export_password.zeroize();
                    app.show_mnemonic_export_confirm = false;
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
            app.export_t_key_display.zeroize();
            for (_, ref mut wif, _, _, _) in app.export_funded_keys.iter_mut() {
                wif.zeroize();
            }
            app.export_funded_keys.clear();
            app.show_export = false;
            app.show_export_step2 = false;
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
                egui::RichText::new("PRIVATE KEYS \u{2014} KEEP SECRET")
                    .font(theme::mono(12.0))
                    .color(theme::RED),
            );
            ui.add_space(5.0);

            // Shielded private key
            if !app.export_key_display.is_empty() {
                ui.label(
                    egui::RichText::new("SHIELDED (z-address)")
                        .font(theme::mono(10.0))
                        .color(theme::GREEN),
                );
                ui.label(
                    egui::RichText::new(&app.export_key_display)
                        .font(theme::mono(9.0))
                        .color(theme::YELLOW),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ COPY SHIELDED KEY ]")
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(app.export_key_display.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                });
            }

            // Funded transparent keys (primary + change + imported)
            if !app.export_funded_keys.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "TRANSPARENT KEYS ({})",
                        app.export_funded_keys.len()
                    ))
                    .font(theme::mono(10.0))
                    .color(theme::YELLOW),
                );
                ui.add_space(4.0);

                let keys_snapshot: Vec<_> = app.export_funded_keys.clone();
                for (i, (addr, wif, balance, is_change, is_imported)) in
                    keys_snapshot.iter().enumerate()
                {
                    let label = if *is_imported {
                        "IMPORTED"
                    } else if *is_change {
                        "CHANGE"
                    } else {
                        "PRIMARY"
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "[{}] {} — {} ZCL",
                            label,
                            &addr[..addr.len().min(20)],
                            crate::app::fmt_zcl(*balance)
                        ))
                        .font(theme::mono(9.0))
                        .color(theme::MUTED),
                    );
                    ui.label(
                        egui::RichText::new(wif)
                            .font(theme::mono(9.0))
                            .color(theme::YELLOW),
                    );
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(format!("[ COPY KEY {} ]", i + 1))
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(wif.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                    ui.add_space(4.0);
                }
            } else if !app.export_t_key_display.is_empty() {
                // Fallback: single transparent key (no funded keys loaded)
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("TRANSPARENT (t-address)")
                        .font(theme::mono(10.0))
                        .color(theme::YELLOW),
                );
                ui.label(
                    egui::RichText::new(&app.export_t_key_display)
                        .font(theme::mono(9.0))
                        .color(theme::YELLOW),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ COPY TRANSPARENT KEY ]")
                                .font(theme::mono(9.0))
                                .color(theme::CYAN),
                        ))
                        .clicked()
                    {
                        ctx.copy_text(app.export_t_key_display.clone());
                        app.clipboard_clear_at = Some(std::time::Instant::now());
                        ctx.request_repaint_after(std::time::Duration::from_secs(31));
                    }
                });
            }

            // Copy All Keys button
            if !app.export_key_display.is_empty()
                || !app.export_funded_keys.is_empty()
                || !app.export_t_key_display.is_empty()
            {
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("[ COPY ALL KEYS ]")
                                .font(theme::mono(10.0))
                                .color(theme::CYAN),
                        )
                        .stroke(egui::Stroke::new(1.0, theme::CYAN.linear_multiply(0.3))),
                    )
                    .clicked()
                {
                    let mut all = String::new();
                    if !app.export_key_display.is_empty() {
                        all.push_str("SHIELDED KEY:\n");
                        all.push_str(&app.export_key_display);
                        all.push('\n');
                    }
                    if !app.export_funded_keys.is_empty() {
                        for (addr, wif, balance, is_change, is_imported) in
                            &app.export_funded_keys
                        {
                            let label = if *is_imported {
                                "IMPORTED"
                            } else if *is_change {
                                "CHANGE"
                            } else {
                                "PRIMARY"
                            };
                            all.push_str(&format!(
                                "\nTRANSPARENT [{}] {} ({} ZCL):\n{}\n",
                                label,
                                addr,
                                crate::app::fmt_zcl(*balance),
                                wif
                            ));
                        }
                    } else if !app.export_t_key_display.is_empty() {
                        all.push_str("\nTRANSPARENT KEY:\n");
                        all.push_str(&app.export_t_key_display);
                        all.push('\n');
                    }
                    ctx.copy_text(all);
                    app.clipboard_clear_at = Some(std::time::Instant::now());
                    ctx.request_repaint_after(std::time::Duration::from_secs(31));
                }
            }

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Your recovery phrase backs up all keys above.")
                    .font(theme::mono(9.0))
                    .color(theme::MUTED),
            );

            ui.add_space(5.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ DISMISS ]")
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                ))
                .clicked()
            {
                app.export_key_display.zeroize();
                app.export_t_key_display.zeroize();
                for (_, ref mut wif, _, _, _) in app.export_funded_keys.iter_mut() {
                    wif.zeroize();
                }
                app.export_funded_keys.clear();
                app.show_export = false;
                app.show_export_step2 = false;
                app.export_auto_dismiss = None;
            }
        });
}

// ---------------------------------------------------------------------------
// MNEMONIC EXPORT — display (auto-dismiss 60s)
// ---------------------------------------------------------------------------

fn show_mnemonic_export_display(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Auto-dismiss after 60s
    if let Some(start) = app.mnemonic_export_auto_dismiss {
        if start.elapsed().as_secs() >= 60 {
            app.export_mnemonic_display.zeroize();
            app.show_mnemonic_export = false;
            app.mnemonic_export_auto_dismiss = None;
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
        .fill(egui::Color32::from_rgb(30, 20, 0))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("RECOVERY PHRASE (24 WORDS) \u{2014} KEEP SECRET")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            );
            ui.label(
                egui::RichText::new("WRITE THESE DOWN AND KEEP THEM SAFE!")
                    .font(theme::mono(10.0))
                    .color(theme::RED),
            );
            ui.add_space(8.0);

            // Display words in a 4x6 grid
            let words: Vec<&str> = app.export_mnemonic_display.split_whitespace().collect();
            egui::Grid::new("mnemonic_export_grid")
                .num_columns(4)
                .spacing([20.0, 6.0])
                .show(ui, |ui| {
                    for (i, word) in words.iter().enumerate() {
                        ui.label(
                            egui::RichText::new(format!("{:>2}. {}", i + 1, word))
                                .font(theme::mono(12.0))
                                .color(theme::GREEN),
                        );
                        if (i + 1) % 4 == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ COPY PHRASE ]")
                            .font(theme::mono(9.0))
                            .color(theme::CYAN),
                    ))
                    .clicked()
                {
                    ctx.copy_text(app.export_mnemonic_display.clone());
                    app.clipboard_clear_at = Some(std::time::Instant::now());
                    ctx.request_repaint_after(std::time::Duration::from_secs(6));
                }
            });

            ui.add_space(5.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ DISMISS ]")
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                ))
                .clicked()
            {
                app.export_mnemonic_display.zeroize();
                app.show_mnemonic_export = false;
                app.mnemonic_export_auto_dismiss = None;
            }
        });
}

#[allow(dead_code)]
fn show_seed_export_confirm(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(30, 25, 0))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("ENTER PASSWORD TO EXPORT SEED")
                    .font(theme::mono(12.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(5.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.seed_export_password)
                    .password(true)
                    .hint_text("wallet password")
                    .font(theme::mono(12.0))
                    .desired_width(250.0),
            );
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                let enter = response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter
                    || ui
                        .add(egui::Button::new(
                            egui::RichText::new("[ CONFIRM ]")
                                .font(theme::mono(11.0))
                                .color(theme::GREEN),
                        ))
                        .clicked()
                {
                    if app.storage.verify_password(&app.seed_export_password) {
                        match app.storage.load_key("wallet_seed") {
                            Ok(seed) => {
                                app.export_seed_display = hex::encode(&seed);
                                app.show_seed_export = true;
                                app.show_seed_export_confirm = false;
                                app.seed_export_auto_dismiss =
                                    Some(std::time::Instant::now());
                            }
                            Err(_) => {
                                app.send_error =
                                    Some("No seed stored (wallet imported from key only)".into());
                                app.show_seed_export_confirm = false;
                            }
                        }
                    } else {
                        app.send_error = Some("Wrong password".into());
                    }
                    app.seed_export_password.zeroize();
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.show_seed_export_confirm = false;
                    app.seed_export_password.zeroize();
                }
            });
        });
}

fn show_seed_export_display(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // Auto-dismiss after 60s
    if let Some(t) = app.seed_export_auto_dismiss {
        if t.elapsed().as_secs() >= 60 {
            app.export_seed_display.zeroize();
            app.show_seed_export = false;
            app.seed_export_auto_dismiss = None;
            return;
        }
    }

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(30, 10, 10))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("WALLET SEED \u{2014} KEEP SECRET")
                    .font(theme::mono(12.0))
                    .color(theme::RED),
            );
            ui.add_space(3.0);
            ui.label(
                egui::RichText::new("This 64-byte seed derives ALL keys (shielded + transparent).")
                    .font(theme::mono(9.0))
                    .color(theme::YELLOW),
            );
            ui.add_space(5.0);

            // Seed hex display
            ui.label(
                egui::RichText::new("SEED (128 hex)")
                    .font(theme::mono(10.0))
                    .color(theme::GREEN),
            );
            ui.label(
                egui::RichText::new(&app.export_seed_display)
                    .font(theme::mono(9.0))
                    .color(theme::YELLOW),
            );
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ COPY SEED ]")
                            .font(theme::mono(9.0))
                            .color(theme::CYAN),
                    ))
                    .clicked()
                {
                    ctx.copy_text(app.export_seed_display.clone());
                    app.clipboard_clear_at = Some(std::time::Instant::now());
                    ctx.request_repaint_after(std::time::Duration::from_secs(31));
                }
            });

            ui.add_space(5.0);
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ DISMISS ]")
                        .font(theme::mono(11.0))
                        .color(theme::MUTED),
                ))
                .clicked()
            {
                app.export_seed_display.zeroize();
                app.show_seed_export = false;
                app.seed_export_auto_dismiss = None;
            }
        });
}

// ---------------------------------------------------------------------------
// WIF IMPORT
// ---------------------------------------------------------------------------

fn show_wif_import(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(10, 20, 25))
        .inner_margin(12.0)
        .rounding(4.0)
        .stroke(egui::Stroke::new(1.0, theme::CYAN.linear_multiply(0.3)))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("IMPORT TRANSPARENT KEYS")
                    .font(theme::mono(13.0))
                    .color(theme::CYAN),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Paste one or more WIF private keys (one per line).\n\
                     These keys will be encrypted and stored locally.",
                )
                .font(theme::mono(10.0))
                .color(theme::MUTED),
            );
            ui.add_space(6.0);

            // Multi-line text input
            ui.add(
                egui::TextEdit::multiline(&mut app.wif_import_text)
                    .font(theme::mono(10.0))
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("5K... or L... or K... (one per line)"),
            );

            ui.add_space(6.0);

            // Validate button
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ VALIDATE ]")
                            .font(theme::mono(11.0))
                            .color(theme::CYAN),
                    ))
                    .clicked()
                {
                    let lines: Vec<&str> = app
                        .wif_import_text
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty())
                        .collect();
                    let mut results = Vec::new();
                    for line in &lines {
                        match zipherx_crypto::transparent::decode_wif(line) {
                            Ok((_sk, addr)) => {
                                let prefix = if line.len() > 8 {
                                    format!("{}...", &line[..8])
                                } else {
                                    line.to_string()
                                };
                                results.push((true, addr, prefix));
                            }
                            Err(e) => {
                                let prefix = if line.len() > 8 {
                                    format!("{}...", &line[..8])
                                } else {
                                    line.to_string()
                                };
                                results.push((false, e.to_string(), prefix));
                            }
                        }
                    }
                    app.wif_import_results = Some(results);
                }

                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ CANCEL ]")
                            .font(theme::mono(11.0))
                            .color(theme::MUTED),
                    ))
                    .clicked()
                {
                    app.wif_import_text.zeroize();
                    app.wif_import_results = None;
                    app.show_wif_import = false;
                }
            });

            // Validation results
            if let Some(ref results) = app.wif_import_results {
                ui.add_space(6.0);
                let valid_count = results.iter().filter(|(v, _, _)| *v).count();
                let invalid_count = results.len() - valid_count;

                for (valid, addr_or_err, prefix) in results {
                    let (icon, color) = if *valid {
                        ("[OK]", theme::GREEN)
                    } else {
                        ("[X]", theme::RED)
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(icon)
                                .font(theme::mono(10.0))
                                .color(color),
                        );
                        ui.label(
                            egui::RichText::new(prefix)
                                .font(theme::mono(9.0))
                                .color(theme::MUTED),
                        );
                        ui.label(
                            egui::RichText::new(if *valid {
                                format!("-> {}", addr_or_err)
                            } else {
                                addr_or_err.clone()
                            })
                            .font(theme::mono(9.0))
                            .color(color),
                        );
                    });
                }

                if invalid_count > 0 {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} invalid key(s) will be skipped.",
                            invalid_count
                        ))
                        .font(theme::mono(9.0))
                        .color(theme::RED),
                    );
                }

                // Warning about recovery phrase
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "WARNING: Imported keys are NOT covered by your recovery phrase.\n\
                         Back up these WIF keys separately.",
                    )
                    .font(theme::mono(9.0))
                    .color(theme::YELLOW),
                );

                // Import button (only if there are valid keys)
                if valid_count > 0 {
                    ui.add_space(6.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "[ IMPORT {} KEY(S) ]",
                                    valid_count
                                ))
                                .font(theme::mono(12.0))
                                .color(egui::Color32::WHITE),
                            )
                            .fill(theme::CYAN.linear_multiply(0.2))
                            .stroke(egui::Stroke::new(
                                1.0,
                                theme::CYAN.linear_multiply(0.5),
                            ))
                            .rounding(4.0),
                        )
                        .clicked()
                    {
                        // Encrypt and store each valid WIF key
                        let lines: Vec<&str> = app
                            .wif_import_text
                            .lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .collect();
                        let mut imported = 0u32;
                        for line in &lines {
                            if let Ok((sk_bytes, address)) =
                                zipherx_crypto::transparent::decode_wif(line)
                            {
                                // Queue raw key + address for the wallet thread to encrypt & store
                                if let Some(ref state) = app.shared_state {
                                    if let Ok(mut s) = state.lock() {
                                        s.pending_wif_imports.push((sk_bytes.to_vec(), address));
                                    }
                                }
                                imported += 1;
                            }
                        }
                        if imported > 0 {
                            app.imported_key_count += imported;
                        }
                        app.wif_import_text.zeroize();
                        app.wif_import_results = None;
                        app.show_wif_import = false;
                    }
                }
            }
        });
}
