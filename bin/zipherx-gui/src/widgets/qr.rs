//! QR code texture generation for egui.

use egui::{ColorImage, Context, TextureHandle};

const QR_SCALE: usize = 4;

/// Generate a QR code texture from a data string.
pub fn generate_qr_texture(ctx: &Context, data: &str) -> Option<TextureHandle> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    let image = code.render::<qrcode::render::unicode::Dense1x2>().build();
    let _ = image; // We need the raw module data instead

    let modules = code.to_colors();
    let width = code.width() as usize;

    let pixel_width = width * QR_SCALE;
    let mut pixels = vec![egui::Color32::WHITE; pixel_width * pixel_width];

    for (i, color) in modules.iter().enumerate() {
        let row = i / width;
        let col = i % width;
        let c = match color {
            qrcode::Color::Dark => egui::Color32::BLACK,
            qrcode::Color::Light => egui::Color32::WHITE,
        };
        for dy in 0..QR_SCALE {
            for dx in 0..QR_SCALE {
                let px = col * QR_SCALE + dx;
                let py = row * QR_SCALE + dy;
                pixels[py * pixel_width + px] = c;
            }
        }
    }

    let image = ColorImage {
        size: [pixel_width, pixel_width],
        pixels,
    };

    Some(ctx.load_texture("qr_code", image, egui::TextureOptions::NEAREST))
}
