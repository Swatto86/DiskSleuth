/// Chart panel — visual breakdowns of disk usage.
///
/// Phase 2: will contain pie/donut chart of file type breakdown
/// and optional treemap/sunburst visualisation.
use crate::state::AppState;
use disksleuth_core::analysis::{analyse_file_types, FileCategory};
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::FileTree;
use egui::{Rect, Ui, Vec2};

/// Draw the chart panel showing file type breakdown.
pub fn chart_panel(ui: &mut Ui, state: &AppState) {
    // Obtain tree reference — final tree, then live tree.
    let live_guard;
    let tree: &FileTree;

    if let Some(ref t) = state.tree {
        tree = t;
    } else if let Some(ref lt) = state.live_tree {
        live_guard = lt.read();
        if live_guard.is_empty() {
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
            let color = category_color(cat);
            ui.painter_at(dot_rect)
                .circle_filled(dot_rect.center(), 4.0, color);

            // Label.
            ui.label(
                egui::RichText::new(cat.label())
                    .color(egui::Color32::from_rgb(0xe4, 0xe4, 0xe8))
                    .size(12.0),
            );

            // Size.
            ui.label(
                egui::RichText::new(format_size(stat.total_size))
                    .color(egui::Color32::from_rgb(0xb8, 0xb8, 0xc4))
                    .size(12.0),
            );

            // Percentage.
            ui.label(
                egui::RichText::new(format!("({:.1}%)", pct))
                    .color(egui::Color32::from_rgb(0x6c, 0x70, 0x86))
                    .size(11.0),
            );
        });

        // Mini bar.
        let bar_width = ui.available_width() - 16.0;
        let bar_height = 4.0;
        let (bar_rect, _) =
            ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());
        let painter = ui.painter_at(bar_rect);
        painter.rect_filled(bar_rect, 2.0, egui::Color32::from_rgb(0x1e, 0x1e, 0x2e));

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
