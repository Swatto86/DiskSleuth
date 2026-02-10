/// Chart panel — visual breakdowns of disk usage.
///
/// Phase 2: will contain pie/donut chart of file type breakdown
/// and optional treemap/sunburst visualisation.

use crate::state::AppState;
use crate::theme::DiskSleuthTheme;
use disksleuth_core::analysis::{analyse_file_types, FileCategory};
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::FileTree;
use egui::{Rect, Ui, Vec2};

/// Draw the chart panel showing file type breakdown.
pub fn chart_panel(ui: &mut Ui, state: &AppState, theme: &DiskSleuthTheme) {
    // Obtain tree reference — final tree, then live tree.
    let live_guard;
    let tree: &FileTree;

    if let Some(ref t) = state.tree {
        tree = t;
    } else if let Some(ref lt) = state.live_tree {
        live_guard = lt.read();
        if live_guard.len() == 0 {
            return;
        }
        tree = &*live_guard;
    } else {
        return;
    };

    ui.heading("File Types");
    ui.add_space(4.0);

    let stats = analyse_file_types(tree);

    for stat in &stats {
        let cat = stat.category.unwrap_or(FileCategory::Other);
        let pct = if tree.total_size > 0 {
            (stat.total_size as f64 / tree.total_size as f64 * 100.0) as f32
        } else {
            0.0
        };

        ui.horizontal(|ui| {
            // Category colour dot.
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
            let color = category_color(cat, theme);
            ui.painter_at(dot_rect).circle_filled(dot_rect.center(), 4.0, color);

            // Label.
            ui.label(
                egui::RichText::new(cat.label())
                    .color(theme.text_primary)
                    .size(12.0),
            );

            // Size.
            ui.label(
                egui::RichText::new(format_size(stat.total_size))
                    .color(theme.text_secondary)
                    .size(12.0),
            );

            // Percentage.
            ui.label(
                egui::RichText::new(format!("({:.1}%)", pct))
                    .color(theme.text_muted)
                    .size(11.0),
            );
        });

        // Mini bar.
        let bar_width = ui.available_width() - 16.0;
        let bar_height = 4.0;
        let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());
        let painter = ui.painter_at(bar_rect);
        painter.rect_filled(bar_rect, 2.0, theme.background);

        let fill_w = bar_width * (pct / 100.0).clamp(0.0, 1.0);
        if fill_w > 0.5 {
            let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_height));
            painter.rect_filled(fill_rect, 2.0, category_color(cat, theme));
        }

        ui.add_space(2.0);
    }
}

/// Map a file category to a display colour.
///
/// Pastel shades for dark mode, deeper/darker shades for light mode
/// so that every colour remains clearly visible against its background.
fn category_color(cat: FileCategory, theme: &DiskSleuthTheme) -> egui::Color32 {
    let is_light = theme.background.r() > 128;
    match cat {
        FileCategory::Documents => {
            if is_light { egui::Color32::from_rgb(0x3a, 0x6f, 0xd8) }
            else        { egui::Color32::from_rgb(0x89, 0xb4, 0xfa) }
        }
        FileCategory::Images => {
            if is_light { egui::Color32::from_rgb(0xb0, 0x82, 0x10) }
            else        { egui::Color32::from_rgb(0xf9, 0xe2, 0xaf) }
        }
        FileCategory::Video => {
            if is_light { egui::Color32::from_rgb(0xc0, 0x3e, 0x52) }
            else        { egui::Color32::from_rgb(0xf3, 0x8b, 0xa8) }
        }
        FileCategory::Audio => {
            if is_light { egui::Color32::from_rgb(0x7b, 0x46, 0xc7) }
            else        { egui::Color32::from_rgb(0xcb, 0xa6, 0xf7) }
        }
        FileCategory::Archives => {
            if is_light { egui::Color32::from_rgb(0xc0, 0x66, 0x20) }
            else        { egui::Color32::from_rgb(0xfa, 0xb3, 0x87) }
        }
        FileCategory::Code => {
            if is_light { egui::Color32::from_rgb(0x28, 0x88, 0x28) }
            else        { egui::Color32::from_rgb(0xa6, 0xe3, 0xa1) }
        }
        FileCategory::Executables => {
            if is_light { egui::Color32::from_rgb(0xc0, 0x3e, 0x52) }
            else        { egui::Color32::from_rgb(0xf3, 0x8b, 0xa8) }
        }
        FileCategory::System => {
            if is_light { egui::Color32::from_rgb(0x1a, 0x8a, 0x80) }
            else        { egui::Color32::from_rgb(0x94, 0xe2, 0xd5) }
        }
        FileCategory::Other => theme.text_muted,
    }
}
