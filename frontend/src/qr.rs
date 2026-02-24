//! QR code generation and texture caching for WHIP/WHEP URLs.

use egui::{ColorImage, TextureHandle};
use std::collections::HashMap;

/// Pixels per QR module (each black/white square).
const PIXELS_PER_MODULE: usize = 4;

/// Quiet zone border in modules (QR spec recommends 4, we use 2 for compactness).
const QUIET_ZONE: usize = 2;

/// Cache of generated QR code textures, keyed by URL.
pub struct QrCache {
    textures: HashMap<String, TextureHandle>,
}

impl QrCache {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    /// Get or create a QR code texture for the given URL.
    pub fn get_or_create(&mut self, ctx: &egui::Context, url: &str) -> Option<&TextureHandle> {
        if !self.textures.contains_key(url) {
            let image = generate_qr_image(url)?;
            let texture =
                ctx.load_texture(format!("qr:{}", url), image, egui::TextureOptions::NEAREST);
            self.textures.insert(url.to_string(), texture);
        }
        self.textures.get(url)
    }
}

/// Generate a QR code as a ColorImage from a URL string.
fn generate_qr_image(url: &str) -> Option<ColorImage> {
    let qr = qrcodegen::QrCode::encode_text(url, qrcodegen::QrCodeEcc::Medium).ok()?;

    let size = qr.size() as usize;
    let img_size = (size + 2 * QUIET_ZONE) * PIXELS_PER_MODULE;

    // Build RGBA bytes: white background, black modules
    let mut rgba = vec![255u8; img_size * img_size * 4];

    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x as i32, y as i32) {
                let px_x = (x + QUIET_ZONE) * PIXELS_PER_MODULE;
                let px_y = (y + QUIET_ZONE) * PIXELS_PER_MODULE;
                for dy in 0..PIXELS_PER_MODULE {
                    for dx in 0..PIXELS_PER_MODULE {
                        let idx = ((px_y + dy) * img_size + (px_x + dx)) * 4;
                        rgba[idx] = 0; // R
                        rgba[idx + 1] = 0; // G
                        rgba[idx + 2] = 0; // B
                                           // A stays 255
                    }
                }
            }
        }
    }

    Some(ColorImage::from_rgba_unmultiplied(
        [img_size, img_size],
        &rgba,
    ))
}
