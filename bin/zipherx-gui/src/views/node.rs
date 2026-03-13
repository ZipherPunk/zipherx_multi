//! Node Management view — full node status, controls, bootstrap, and logs.
//!
//! Matches the official ZipherX macOS app's NodeManagementView structure:
//! Prerequisites, daemon control, node info, bootstrap, configuration, logs.

use crate::app::ZipherXApp;
use crate::fullnode::manager::DaemonStatus;
use crate::fullnode::rpc::RpcClient;
use crate::theme;

pub fn show(app: &mut ZipherXApp, ui: &mut egui::Ui, _ctx: &egui::Context) {
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("FULL NODE")
                    .font(theme::mono(16.0))
                    .color(theme::GREEN),
            );

            // -- MODE SELECTION --
            section_header(ui, "WALLET MODE");
            show_mode_selection(app, ui);

            if !app.fullnode_enabled {
                ui.add_space(20.0);
                egui::Frame::none()
                    .fill(theme::PANEL_BG)
                    .inner_margin(12.0)
                    .rounding(4.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(
                                "Full node mode manages a local zclassicd daemon.\n\
                                 Your wallet communicates directly with your own node\n\
                                 for maximum privacy and trustlessness.\n\n\
                                 Storage: ~15 GB  |  macOS/Linux only",
                            )
                            .font(theme::mono(10.0))
                            .color(theme::MUTED),
                        );
                    });
                return;
            }

            // -- PREREQUISITES --
            section_header(ui, "PREREQUISITES");
            show_prerequisites(app, ui);

            // -- DAEMON STATUS & CONTROL --
            section_header(ui, "DAEMON STATUS");
            show_daemon_status(app, ui);

            section_header(ui, "CONTROLS");
            show_daemon_controls(app, ui);

            // -- NODE INFO --
            if app.node_daemon_status == DaemonStatus::Running {
                section_header(ui, "BLOCKCHAIN");
                show_blockchain_info(app, ui);

                section_header(ui, "NETWORK");
                show_network_info(app, ui);

                section_header(ui, "MEMPOOL");
                show_mempool_info(app, ui);
            }

            // -- BOOTSTRAP --
            section_header(ui, "BOOTSTRAP");
            show_bootstrap(app, ui);

            // -- DAEMON BINARY --
            section_header(ui, "DAEMON BINARY");
            show_daemon_path(app, ui);

            // -- LOGS --
            section_header(ui, "DAEMON LOG");
            show_logs(app, ui);

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
// Mode selection
// ---------------------------------------------------------------------------

fn show_mode_selection(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let p2p_selected = !app.fullnode_enabled;
        let node_selected = app.fullnode_enabled;

        if ui
            .add(egui::Button::new(
                egui::RichText::new(if p2p_selected {
                    "[ P2P LIGHT ] *"
                } else {
                    "[ P2P LIGHT ]"
                })
                .font(theme::mono(12.0))
                .color(if p2p_selected {
                    theme::GREEN
                } else {
                    theme::MUTED
                }),
            ))
            .clicked()
        {
            app.fullnode_enabled = false;
        }

        ui.add_space(10.0);

        if ui
            .add(egui::Button::new(
                egui::RichText::new(if node_selected {
                    "[ FULL NODE ] *"
                } else {
                    "[ FULL NODE ]"
                })
                .font(theme::mono(12.0))
                .color(if node_selected {
                    theme::GREEN
                } else {
                    theme::MUTED
                }),
            ))
            .clicked()
        {
            app.fullnode_enabled = true;
        }
    });

    let mode_desc = if app.fullnode_enabled {
        "Full blockchain verification with local zclassicd daemon"
    } else {
        "P2P light: connects directly to peers, no local daemon"
    };
    ui.label(
        egui::RichText::new(mode_desc)
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );
}

// ---------------------------------------------------------------------------
// Prerequisites
// ---------------------------------------------------------------------------

fn show_prerequisites(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let daemon_found = app.node_daemon_path.is_some();
    let data_dir = app.node_data_dir.clone();
    let bootstrap = crate::fullnode::bootstrap::BootstrapManager::new(data_dir.clone());
    let has_chain = bootstrap.has_chain_data();

    // Check for Sapling params — search all known locations
    let has_params = {
        let check_dir = |dir: &std::path::Path| -> bool {
            dir.join("sapling-spend.params").exists() && dir.join("sapling-output.params").exists()
        };
        let home = dirs::home_dir();
        let data = dirs::data_dir(); // ~/Library/Application Support on macOS
        check_dir(&app.data_dir)
            || home
                .as_ref()
                .map(|h| check_dir(&h.join(".zcash-params")))
                .unwrap_or(false)
            || data
                .as_ref()
                .map(|d| check_dir(&d.join("ZipherX").join("sapling-params")))
                .unwrap_or(false)
            || data
                .as_ref()
                .map(|d| check_dir(&d.join("ZipherX")))
                .unwrap_or(false)
            || data
                .as_ref()
                .map(|d| check_dir(&d.join("ZcashParams")))
                .unwrap_or(false)
    };

    // Check for zstd (use `where` on Windows, `which` on Unix)
    let has_zstd = {
        #[cfg(target_os = "windows")]
        let cmd = std::process::Command::new("where").arg("zstd").output();
        #[cfg(not(target_os = "windows"))]
        let cmd = std::process::Command::new("which").arg("zstd").output();
        cmd.map(|o| o.status.success()).unwrap_or(false)
    };

    let all_met = daemon_found && has_chain && has_params && has_zstd;

    egui::Frame::none()
        .fill(if all_met {
            egui::Color32::from_rgb(0, 20, 10)
        } else {
            egui::Color32::from_rgb(30, 20, 0)
        })
        .inner_margin(10.0)
        .rounding(4.0)
        .show(ui, |ui| {
            let mut check = |met: bool, label: &str| {
                let (icon, color) = if met {
                    ("[OK]", theme::GREEN)
                } else {
                    ("[!!]", theme::RED)
                };
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icon)
                            .font(theme::mono(10.0))
                            .color(color),
                    );
                    ui.label(
                        egui::RichText::new(label)
                            .font(theme::mono(10.0))
                            .color(if met { theme::GREEN } else { theme::YELLOW }),
                    );
                });
            };

            check(daemon_found, "Zclassic daemon (zclassicd)");
            check(has_chain, "Blockchain data");
            check(has_params, "Sapling parameters");
            check(has_zstd, "zstd (for bootstrap extraction)");

            if !all_met {
                ui.add_space(5.0);
                if !daemon_found {
                    ui.label(
                        egui::RichText::new(
                            "Install zclassicd: build from source or use the DAEMON BINARY section below",
                        )
                        .font(theme::mono(9.0))
                        .color(theme::MUTED),
                    );
                }
                if !has_zstd {
                    ui.label(
                        egui::RichText::new("Install zstd: brew install zstd (macOS) or apt install zstd (Linux)")
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                }
                if !has_chain {
                    ui.label(
                        egui::RichText::new(
                            "Use BOOTSTRAP section below to download blockchain data",
                        )
                        .font(theme::mono(9.0))
                        .color(theme::MUTED),
                    );
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Daemon status
// ---------------------------------------------------------------------------

fn show_daemon_status(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let (dot_color, status_text) = match &app.node_daemon_status {
        DaemonStatus::Stopped => (theme::RED, "STOPPED".to_string()),
        DaemonStatus::Starting => (theme::YELLOW, "STARTING...".to_string()),
        DaemonStatus::Running => (theme::GREEN, "RUNNING".to_string()),
        DaemonStatus::Stopping => (theme::YELLOW, "STOPPING...".to_string()),
        DaemonStatus::Error(e) => (theme::RED, format!("ERROR: {}", e)),
    };

    ui.horizontal(|ui| {
        ui.colored_label(dot_color, "\u{2022}");
        ui.label(
            egui::RichText::new(&status_text)
                .font(theme::mono(12.0))
                .color(dot_color),
        );
        if let Some(pid) = app.node_daemon_pid {
            ui.label(
                egui::RichText::new(format!("(PID: {})", pid))
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
            );
        }
    });
}

// ---------------------------------------------------------------------------
// Daemon controls
// ---------------------------------------------------------------------------

fn show_daemon_controls(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let can_start = app.node_daemon_status == DaemonStatus::Stopped
            || matches!(app.node_daemon_status, DaemonStatus::Error(_));
        let can_stop = app.node_daemon_status == DaemonStatus::Running;

        if can_start {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ START DAEMON ]")
                        .font(theme::mono(12.0))
                        .color(theme::GREEN),
                ))
                .clicked()
            {
                start_daemon(app);
            }
        }

        if can_stop {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ STOP DAEMON ]")
                        .font(theme::mono(12.0))
                        .color(theme::RED),
                ))
                .clicked()
            {
                stop_daemon(app);
            }
        }

        // Detect external daemon
        if app.node_daemon_status == DaemonStatus::Stopped {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("[ DETECT RUNNING ]")
                        .font(theme::mono(11.0))
                        .color(theme::CYAN),
                ))
                .clicked()
            {
                detect_external_daemon(app);
            }
        }
    });

    if let Some(ref err) = app.node_error {
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new(err)
                .font(theme::mono(10.0))
                .color(theme::RED),
        );
    }
}

// ---------------------------------------------------------------------------
// Blockchain / Network / Mempool info
// ---------------------------------------------------------------------------

fn show_blockchain_info(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    if let Some(ref info) = app.node_chain_info {
        ui.label(
            egui::RichText::new(format!(
                "Chain: {}  |  Blocks: {} / {}",
                info.chain, info.blocks, info.headers
            ))
            .font(theme::mono(11.0))
            .color(theme::GREEN),
        );

        // Sync progress bar
        let progress = if info.headers > 0 {
            info.blocks as f32 / info.headers as f32
        } else {
            0.0
        };
        let bar = egui::ProgressBar::new(progress).text(format!("{:.1}%", progress * 100.0));
        ui.add(bar);

        // Size on disk
        let size_gb = info.size_on_disk as f64 / (1024.0 * 1024.0 * 1024.0);
        ui.label(
            egui::RichText::new(format!("Size on disk: {:.2} GB", size_gb))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );

        if info.pruned {
            ui.label(
                egui::RichText::new("Pruned node")
                    .font(theme::mono(10.0))
                    .color(theme::YELLOW),
            );
        }
    } else {
        ui.label(
            egui::RichText::new("Querying blockchain info...")
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
    }
}

fn show_network_info(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    if let Some(ref info) = app.node_network_info {
        ui.label(
            egui::RichText::new(format!("Version: {} {}", info.version, info.subversion))
                .font(theme::mono(10.0))
                .color(theme::MUTED),
        );
        ui.label(
            egui::RichText::new(format!(
                "Connections: {}  |  Protocol: {}",
                info.connections, info.protocol_version
            ))
            .font(theme::mono(11.0))
            .color(if info.connections > 0 {
                theme::GREEN
            } else {
                theme::RED
            }),
        );
    }
}

fn show_mempool_info(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    if let Some(ref info) = app.node_mempool_info {
        let tx_count = info.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let mem_usage = info.get("usage").and_then(|v| v.as_u64()).unwrap_or(0);
        let mem_kb = mem_usage / 1024;
        ui.label(
            egui::RichText::new(format!(
                "Transactions: {}  |  Memory: {} KB",
                tx_count, mem_kb
            ))
            .font(theme::mono(10.0))
            .color(theme::MUTED),
        );
    }
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

fn show_bootstrap(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let data_dir = app.node_data_dir.clone();
    let bootstrap = crate::fullnode::bootstrap::BootstrapManager::new(data_dir);

    egui::Frame::none()
        .fill(theme::PANEL_BG)
        .inner_margin(10.0)
        .rounding(4.0)
        .show(ui, |ui| {
            if bootstrap.has_chain_data() {
                let size_gb = bootstrap.chain_data_size() as f64 / (1024.0 * 1024.0 * 1024.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("[OK]")
                            .font(theme::mono(10.0))
                            .color(theme::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(format!("Blockchain data: {:.2} GB", size_gb))
                            .font(theme::mono(10.0))
                            .color(theme::GREEN),
                    );
                });

                // Show data directory
                ui.label(
                    egui::RichText::new(format!("Data dir: {}", app.node_data_dir.display()))
                        .font(theme::mono(9.0))
                        .color(theme::MUTED),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("[!!]")
                            .font(theme::mono(10.0))
                            .color(theme::YELLOW),
                    );
                    ui.label(
                        egui::RichText::new("No blockchain data found")
                            .font(theme::mono(10.0))
                            .color(theme::YELLOW),
                    );
                });

                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(
                        "Without blockchain data, the daemon will sync from genesis.\n\
                         This can take several hours.\n\n\
                         Bootstrap downloads a pre-synced blockchain from:\n\
                         github.com/ZipherPunk/zclassic-bootstrap",
                    )
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
                );

                ui.add_space(8.0);

                // Install bootstrap button
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("[ INSTALL FRESH BOOTSTRAP ]")
                            .font(theme::mono(12.0))
                            .color(theme::GREEN),
                    ))
                    .clicked()
                {
                    // TODO: Implement bootstrap download from GitHub
                    // ZipherPunk/zclassic-bootstrap (split-file tar.zst)
                    app.node_log_lines.push(
                        "[ZipherX] Bootstrap download not yet implemented in egui. \
                         Use the official ZipherX macOS app or download manually from \
                         github.com/ZipherPunk/zclassic-bootstrap"
                            .to_string(),
                    );
                    app.node_error = Some(
                        "Bootstrap download coming soon. See daemon log for manual instructions."
                            .to_string(),
                    );
                }
            }

            ui.add_space(5.0);

            // Manual bootstrap instructions
            if bootstrap.needs_bootstrap() {
                ui.label(
                    egui::RichText::new(
                        "Manual: download bootstrap from GitHub, extract to data dir with:\n\
                         zstd -d bootstrap.tar.zst && tar xf bootstrap.tar -C ~/.zclassic/",
                    )
                    .font(theme::mono(9.0))
                    .color(theme::MUTED),
                );
            }
        });
}

// ---------------------------------------------------------------------------
// Daemon binary path
// ---------------------------------------------------------------------------

fn show_daemon_path(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let path_display = app
        .node_daemon_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "Not found".to_string());

    ui.horizontal(|ui| {
        let (icon, color) = if app.node_daemon_path.is_some() {
            ("[OK]", theme::GREEN)
        } else {
            ("[!!]", theme::RED)
        };
        ui.label(
            egui::RichText::new(icon)
                .font(theme::mono(10.0))
                .color(color),
        );
        ui.label(
            egui::RichText::new(&path_display)
                .font(theme::mono(10.0))
                .color(if app.node_daemon_path.is_some() {
                    theme::GREEN
                } else {
                    theme::MUTED
                }),
        );
    });

    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ DETECT ]")
                    .font(theme::mono(10.0))
                    .color(theme::CYAN),
            ))
            .clicked()
        {
            app.node_daemon_path =
                crate::fullnode::manager::FullNodeManager::find_running_daemon_path()
                    .or_else(crate::fullnode::manager::FullNodeManager::find_daemon);
            if app.node_daemon_path.is_none() {
                app.node_error = Some("zclassicd not found on this system".to_string());
            } else {
                app.node_error = None;
            }
        }

        // Manual path input
        ui.add(
            egui::TextEdit::singleline(&mut app.node_daemon_path_input)
                .hint_text("/path/to/zclassicd")
                .font(theme::mono(10.0))
                .desired_width(200.0),
        );

        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ SET ]")
                    .font(theme::mono(10.0))
                    .color(theme::GREEN),
            ))
            .clicked()
        {
            if !app.node_daemon_path_input.is_empty() {
                let path = std::path::PathBuf::from(&app.node_daemon_path_input);
                if path.exists() {
                    app.node_daemon_path = Some(path);
                    app.node_error = None;
                } else {
                    app.node_error =
                        Some(format!("File not found: {}", app.node_daemon_path_input));
                }
            }
        }
    });

    // Data directory
    ui.add_space(5.0);
    ui.label(
        egui::RichText::new(format!("Data dir: {}", app.node_data_dir.display()))
            .font(theme::mono(10.0))
            .color(theme::MUTED),
    );

    // Build instructions
    ui.add_space(5.0);
    ui.label(
        egui::RichText::new(
            "Build from source: git clone https://github.com/nicedayzhu/zclassic\n\
             cd zclassic && ./zcutil/build.sh -j$(nproc)",
        )
        .font(theme::mono(9.0))
        .color(theme::MUTED),
    );
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

fn show_logs(app: &mut ZipherXApp, ui: &mut egui::Ui) {
    let frame = egui::Frame::none()
        .fill(egui::Color32::from_rgb(10, 10, 10))
        .inner_margin(8.0)
        .rounding(4.0);

    frame.show(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if app.node_log_lines.is_empty() {
                    ui.label(
                        egui::RichText::new("No daemon logs yet.")
                            .font(theme::mono(9.0))
                            .color(theme::MUTED),
                    );
                } else {
                    for line in &app.node_log_lines {
                        let color = if line.contains("Error") || line.contains("error") {
                            theme::RED
                        } else if line.contains("[ZipherX]") {
                            theme::CYAN
                        } else {
                            theme::MUTED
                        };
                        ui.label(
                            egui::RichText::new(line)
                                .font(theme::mono(9.0))
                                .color(color),
                        );
                    }
                }
            });
    });

    if !app.node_log_lines.is_empty() {
        if ui
            .add(egui::Button::new(
                egui::RichText::new("[ CLEAR LOGS ]")
                    .font(theme::mono(10.0))
                    .color(theme::MUTED),
            ))
            .clicked()
        {
            app.node_log_lines.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

fn start_daemon(app: &mut ZipherXApp) {
    use crate::fullnode::manager::{FullNodeConfig, FullNodeManager};

    let mut config = FullNodeConfig::default();
    config.daemon_path = app.node_daemon_path.clone();
    config.data_dir = Some(app.node_data_dir.clone());

    // Read existing credentials if available
    if let Some((user, pass, port)) = FullNodeManager::read_conf_credentials(&app.node_data_dir) {
        config.rpc_user = user;
        config.rpc_password = pass;
        config.rpc_port = port;
    }

    let mut manager = FullNodeManager::new(config.clone());
    match manager.start() {
        Ok(()) => {
            app.node_daemon_status = manager.status.clone();
            app.node_daemon_pid = manager.pid();
            app.node_rpc_port = config.rpc_port;
            app.node_rpc_user = config.rpc_user;
            app.node_rpc_password = config.rpc_password;
            app.node_error = None;
            for line in &manager.log_lines {
                app.node_log_lines.push(line.clone());
            }
            app.node_manager = Some(crate::fullnode::manager::new_shared(
                FullNodeConfig::default(),
            ));
        }
        Err(e) => {
            app.node_daemon_status = manager.status.clone();
            app.node_error = Some(e);
            for line in &manager.log_lines {
                app.node_log_lines.push(line.clone());
            }
        }
    }
}

fn stop_daemon(app: &mut ZipherXApp) {
    let rpc = RpcClient::new(
        &format!("http://127.0.0.1:{}", app.node_rpc_port),
        &app.node_rpc_user,
        &app.node_rpc_password,
    );
    match rpc.stop() {
        Ok(_) => {
            app.node_daemon_status = DaemonStatus::Stopping;
            app.node_log_lines
                .push("[ZipherX] Stop command sent to daemon.".to_string());
        }
        Err(e) => {
            app.node_error = Some(format!("Failed to stop daemon: {}", e));
            app.node_log_lines
                .push(format!("[ZipherX] Stop failed: {}", e));
        }
    }
}

fn detect_external_daemon(app: &mut ZipherXApp) {
    // Try to read credentials from conf
    if let Some((user, pass, port)) =
        crate::fullnode::manager::FullNodeManager::read_conf_credentials(&app.node_data_dir)
    {
        let rpc = RpcClient::new(&format!("http://127.0.0.1:{}", port), &user, &pass);
        if rpc.is_alive() {
            app.node_daemon_status = DaemonStatus::Running;
            app.node_rpc_port = port;
            app.node_rpc_user = user;
            app.node_rpc_password = pass;
            app.node_log_lines
                .push("[ZipherX] Detected running daemon via RPC.".to_string());
            app.node_error = None;

            // Find binary path (prefer running process path)
            if app.node_daemon_path.is_none() {
                app.node_daemon_path =
                    crate::fullnode::manager::FullNodeManager::find_running_daemon_path()
                        .or_else(crate::fullnode::manager::FullNodeManager::find_daemon);
            }

            // Fetch info
            if let Ok(info) = rpc.get_blockchain_info() {
                app.node_chain_info = Some(info);
            }
            if let Ok(info) = rpc.get_network_info() {
                app.node_network_info = Some(info);
            }
            if let Ok(info) = rpc.get_mempool_info() {
                app.node_mempool_info = Some(info);
            }
            return;
        }
    }

    // Try default credentials
    let rpc = RpcClient::new("http://127.0.0.1:8023", "zipherx", "");
    if rpc.is_alive() {
        app.node_daemon_status = DaemonStatus::Running;
        app.node_log_lines
            .push("[ZipherX] Detected running daemon (default port).".to_string());
        app.node_error = None;
    } else {
        app.node_log_lines
            .push("[ZipherX] No running daemon detected.".to_string());
        app.node_error = Some("No running zclassicd daemon found".to_string());
    }
}

/// Poll daemon for updated info. Called periodically from the main loop.
///
// Note (GUI-L2): RPC credentials are held in memory for the duration of full
// node mode. They are zeroized when the app locks or drops (see ZipherXApp::Drop).
pub fn poll_node_info(app: &mut ZipherXApp) {
    // Auto-detect: if status is Stopped, try to find a running daemon
    if app.node_daemon_status == DaemonStatus::Stopped {
        auto_detect_daemon(app);
        return;
    }

    let rpc = RpcClient::new(
        &format!("http://127.0.0.1:{}", app.node_rpc_port),
        &app.node_rpc_user,
        &app.node_rpc_password,
    );

    // Check if Stopping -> Stopped (daemon finished shutting down)
    if app.node_daemon_status == DaemonStatus::Stopping {
        if !rpc.is_alive() {
            app.node_daemon_status = DaemonStatus::Stopped;
            app.node_daemon_pid = None;
            app.node_chain_info = None;
            app.node_network_info = None;
            app.node_mempool_info = None;
            app.node_log_lines
                .push("[ZipherX] Daemon has stopped.".to_string());
        }
        return;
    }

    // Check if Starting -> Running
    if app.node_daemon_status == DaemonStatus::Starting {
        if rpc.is_alive() {
            app.node_daemon_status = DaemonStatus::Running;
            app.node_log_lines
                .push("[ZipherX] Daemon is now accepting RPC connections.".to_string());
        }
        return;
    }

    // Status is Running — fetch info
    match rpc.get_blockchain_info() {
        Ok(info) => {
            app.node_chain_info = Some(info);
        }
        Err(e) => {
            // Connection lost
            if e.contains("connection") || e.contains("Connection") || e.contains("refused") {
                app.node_daemon_status = DaemonStatus::Stopped;
                app.node_daemon_pid = None;
                app.node_log_lines
                    .push("[ZipherX] Lost connection to daemon.".to_string());
                return;
            }
        }
    }

    // Fetch network info
    if let Ok(info) = rpc.get_network_info() {
        app.node_network_info = Some(info);
    }

    // Fetch mempool info
    if let Ok(info) = rpc.get_mempool_info() {
        app.node_mempool_info = Some(info);
    }
}

/// Silently try to detect a running daemon (called on each poll when Stopped).
fn auto_detect_daemon(app: &mut ZipherXApp) {
    // Need credentials to check
    if let Some((user, pass, port)) =
        crate::fullnode::manager::FullNodeManager::read_conf_credentials(&app.node_data_dir)
    {
        let rpc = RpcClient::new(&format!("http://127.0.0.1:{}", port), &user, &pass);
        if rpc.is_alive() {
            app.node_daemon_status = DaemonStatus::Running;
            app.node_rpc_port = port;
            app.node_rpc_user = user;
            app.node_rpc_password = pass;
            app.node_log_lines
                .push("[ZipherX] Auto-detected running daemon.".to_string());
            app.node_error = None;

            // Find the daemon binary path (prefer running process path)
            if app.node_daemon_path.is_none() {
                app.node_daemon_path =
                    crate::fullnode::manager::FullNodeManager::find_running_daemon_path()
                        .or_else(crate::fullnode::manager::FullNodeManager::find_daemon);
            }

            // Fetch initial info
            if let Ok(info) = rpc.get_blockchain_info() {
                app.node_chain_info = Some(info);
            }
            if let Ok(info) = rpc.get_network_info() {
                app.node_network_info = Some(info);
            }
        }
    }
}
