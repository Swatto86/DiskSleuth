/// Proportional size bar widget -- shows relative size with colour coding.
use egui::{Color32, Rect, Ui, Vec2};

/// Draw a horizontal size bar with a filled portion.
pub fn size_bar(ui: &mut Ui, percent: f32, width: f32, height: f32) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background.
    painter.rect_filled(rect, 2.0, Color32::from_rgb(0x2a, 0x2a, 0x3c));

    // Fill.
    let fill_w = width * (percent / 100.0).clamp(0.0, 1.0);
    if fill_w > 0.5 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, height));
        painter.rect_filled(fill_rect, 2.0, bar_color(percent));
    }
}

/// Interpolate between green (small) and pink (large) based on percentage.
fn bar_color(percent: f32) -> Color32 {
    let t = (percent / 100.0).clamp(0.0, 1.0);
    let a = Color32::from_rgb(0xa6, 0xe3, 0xa1); // green
    let b = Color32::from_rgb(0xf3, 0x8b, 0xa8); // pink
    Color32::from_rgb(
        (a.r() as f32 * (1.0 - t) + b.r() as f32 * t) as u8,
        (a.g() as f32 * (1.0 - t) + b.g() as f32 * t) as u8,
        (a.b() as f32 * (1.0 - t) + b.b() as f32 * t) as u8,
    )
}
