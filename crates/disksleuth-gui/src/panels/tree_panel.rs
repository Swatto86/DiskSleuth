/// Main TreeView results panel.
use crate::state::AppState;
use crate::widgets;
use egui::Ui;

/// Draw the tree panel (centre content area).
pub fn tree_panel(ui: &mut Ui, state: &mut AppState) {
    // Column headers.
    ui.horizontal(|ui| {
        let header_height = 20.0;
        let rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), header_height),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(0x22, 0x22, 0x34));

        ui.allocate_exact_size(
            egui::vec2(ui.available_width(), header_height),
            egui::Sense::hover(),
        );

        let muted = egui::Color32::from_rgb(0x6c, 0x70, 0x86);
        painter.text(
            egui::pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Name",
            egui::FontId::proportional(12.0),
            muted,
        );

        let right_start = rect.right() - 300.0;
        painter.text(
            egui::pos2(right_start, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Size",
            egui::FontId::proportional(12.0),
            muted,
        );

        painter.text(
            egui::pos2(right_start + 80.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "%",
            egui::FontId::proportional(12.0),
            muted,
        );

        painter.text(
            egui::pos2(right_start + 130.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Bar",
            egui::FontId::proportional(12.0),
            muted,
        );

        painter.text(
            egui::pos2(right_start + 240.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Files",
            egui::FontId::proportional(12.0),
            muted,
        );
    });

    ui.separator();

    // Tree view.
    widgets::tree_view::tree_view(ui, state);
}
