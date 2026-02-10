/// Proportional size bar widget — shows relative size with colour coding.

use crate::theme::DiskSleuthTheme;
use egui::{Rect, Ui, Vec2};

/// Draw a horizontal size bar with a filled portion.
pub fn size_bar(ui: &mut Ui, theme: &DiskSleuthTheme, percent: f32, width: f32, height: f32) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background.
    painter.rect_filled(rect, 2.0, theme.surface);

    // Fill.
    let fill_w = width * (percent / 100.0).clamp(0.0, 1.0);
    if fill_w > 0.5 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, height));
        painter.rect_filled(fill_rect, 2.0, theme.bar_color(percent));
    }
}
