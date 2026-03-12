//! ZipherX cypherpunk dark theme.
//!
//! Terminal-green-on-black aesthetic. All UI elements draw from this palette.

use egui::{Color32, FontFamily, FontId, Visuals};

/// Primary accent — terminal green.
pub const GREEN: Color32 = Color32::from_rgb(0, 255, 64);

/// Secondary accent — information highlights.
pub const CYAN: Color32 = Color32::from_rgb(0, 204, 255);

/// Error / danger.
pub const RED: Color32 = Color32::from_rgb(255, 68, 68);

/// Reward / special event.
pub const GOLD: Color32 = Color32::from_rgb(255, 215, 0);

/// Warning / amber.
pub const YELLOW: Color32 = Color32::from_rgb(255, 200, 0);

/// De-emphasized text.
pub const MUTED: Color32 = Color32::from_rgb(136, 136, 136);

/// Root background — near-black.
pub const BG: Color32 = Color32::from_rgb(5, 5, 5);

/// Panel / card background — slightly lighter than BG.
pub const PANEL_BG: Color32 = Color32::from_rgb(18, 18, 18);

/// Convenience: monospace `FontId` at the given size.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Apply the cypherpunk dark visuals to the egui context.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(GREEN);
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL_BG;
    visuals.widgets.noninteractive.bg_fill = PANEL_BG;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 30);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 40, 40);
    visuals.widgets.active.bg_fill = Color32::from_rgb(50, 50, 50);
    visuals.selection.bg_fill = CYAN.gamma_multiply(0.3);
    ctx.set_visuals(visuals);
}
