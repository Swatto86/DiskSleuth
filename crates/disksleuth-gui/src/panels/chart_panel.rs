/// Chart panel — visual breakdowns of disk usage.
///
/// Phase 2: will contain pie/donut chart of file type breakdown
/// and optional treemap/sunburst visualisation.
use crate::state::{AppPhase, AppState};
use disksleuth_core::analysis::FileCategory;
use disksleuth_core::model::size::format_size;
use egui::{Rect, Ui, Vec2};

/// Draw the chart panel showing file type breakdown.
///
/// Uses `AppState::file_type_stats` (pre-computed once after scan completion)
/// rather than calling `analyse_file_types` on every render frame.  This
/// avoids iterating over millions of nodes at 60 fps.
pub fn chart_panel(ui: &mut Ui, state: &AppState) {
    // Extract theme-adaptive colours once for correct rendering in both
    // dark and light mode.
    let color_normal = ui.visuals().text_color();
    let color_muted = ui.visuals().weak_text_color();
    let bar_track_bg = ui.visuals().extreme_bg_color;

    ui.heading("File Types");
    ui.add_space(4.0);

    // Use the pre-computed cache; during an active scan show a placeholder.
    let stats = match state.file_type_stats.as_deref() {
        Some(s) => s,
        None => {
            if state.phase == AppPhase::Scanning {
                ui.label(
                    egui::RichText::new("Available after scan completes.")
                        .color(color_muted)
                        .size(12.0),
                );
            }
            return;
        }
    };

    // Total size is only meaningful from the completed tree.
    let total_size = state.tree.as_ref().map_or(0u64, |t| t.total_size);

    for stat in stats {
        let cat = stat.category.unwrap_or(FileCategory::Other);
        let pct = if total_size > 0 {
            (stat.total_size as f64 / total_size as f64 * 100.0) as f32
        } else {
            0.0
        };

        ui.horizontal(|ui| {
            // Category colour dot.
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
            let color = category_color(cat);
            ui.painter_at(dot_rect)
                .circle_filled(dot_rect.center(), 4.0, color);

            // Label.
            ui.label(
                egui::RichText::new(cat.label())
                    .color(color_normal)
                    .size(12.0),
            );

            // Size.
            ui.label(
                egui::RichText::new(format_size(stat.total_size))
                    .color(color_normal)
                    .size(12.0),
            );

            // Percentage.
            ui.label(
                egui::RichText::new(format!("({:.1}%)", pct))
                    .color(color_muted)
                    .size(11.0),
            );
        });

        // Mini bar.
        let bar_width = ui.available_width() - 16.0;
        let bar_height = 4.0;
        let (bar_rect, _) =
            ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());
        let painter = ui.painter_at(bar_rect);
        painter.rect_filled(bar_rect, 2.0, bar_track_bg);

        let fill_w = bar_width * (pct / 100.0).clamp(0.0, 1.0);
        if fill_w > 0.5 {
            let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_height));
            painter.rect_filled(fill_rect, 2.0, category_color(cat));
        }

        ui.add_space(2.0);
    }
}

/// Map a file category to a display colour (dark mode).
fn category_color(cat: FileCategory) -> egui::Color32 {
    match cat {
        FileCategory::Documents => egui::Color32::from_rgb(0x89, 0xb4, 0xfa),
        FileCategory::Images => egui::Color32::from_rgb(0xf9, 0xe2, 0xaf),
        FileCategory::Video => egui::Color32::from_rgb(0xf3, 0x8b, 0xa8),
        FileCategory::Audio => egui::Color32::from_rgb(0xcb, 0xa6, 0xf7),
        FileCategory::Archives => egui::Color32::from_rgb(0xfa, 0xb3, 0x87),
        FileCategory::Code => egui::Color32::from_rgb(0xa6, 0xe3, 0xa1),
        FileCategory::Executables => egui::Color32::from_rgb(0xf3, 0x8b, 0xa8),
        FileCategory::System => egui::Color32::from_rgb(0x94, 0xe2, 0xd5),
        FileCategory::Other => egui::Color32::from_rgb(0x6c, 0x70, 0x86),
    }
}
