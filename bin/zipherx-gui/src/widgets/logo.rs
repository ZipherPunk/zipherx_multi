//! ZipherX logo widget — loads the official Zipherpunk Z-shield logo.

use crate::app::ZipherXApp;

const LOGO_BYTES: &[u8] = include_bytes!("../../../../assets/zipherpunk_logo.png");

/// Load the logo texture if not already cached.
fn ensure_logo(app: &mut ZipherXApp, ctx: &egui::Context) {
    if app.logo_texture.is_some() {
        return;
    }
    let img = match image::load_from_memory(LOGO_BYTES) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return,
    };
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    app.logo_texture = Some(ctx.load_texture("zipherx-logo", color_image, egui::TextureOptions::LINEAR));
}

/// Display the logo at the given size.
pub fn show_logo(app: &mut ZipherXApp, ui: &mut egui::Ui, ctx: &egui::Context, size: f32) {
    ensure_logo(app, ctx);
    if let Some(ref texture) = app.logo_texture {
        let img_size = egui::Vec2::splat(size);
        ui.image(egui::load::SizedTexture::new(texture.id(), img_size));
    }
}
