/// Main TreeView results panel with sortable column headers.
use crate::state::AppState;
use crate::widgets;
use disksleuth_core::model::SortKey;
use egui::Ui;

/// Draw the tree panel (centre content area).
pub fn tree_panel(ui: &mut Ui, state: &mut AppState) {
    let mut clicked_sort: Option<SortKey> = None;

    // Column headers.
    ui.horizontal(|ui| {
        let header_height = 20.0;
        let rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), header_height),
        );
        let painter = ui.painter_at(rect);
        // Theme-aware header background — extreme_bg_color is the darkest
        // panel tint in dark mode and the lightest in light mode.
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        ui.allocate_exact_size(
            egui::vec2(ui.available_width(), header_height),
            egui::Sense::hover(),
        );

        let right_start = rect.right() - 300.0;

        // (x offset, click width, label, sort key) — None = not sortable.
        let columns: [(f32, f32, &str, Option<SortKey>); 5] = [
            (rect.left() + 8.0, 120.0, "Name", Some(SortKey::Name)),
            (right_start, 60.0, "Size", Some(SortKey::Size)),
            (right_start + 80.0, 40.0, "%", None),
            (right_start + 130.0, 50.0, "Usage", None),
            (right_start + 240.0, 50.0, "Files", Some(SortKey::Files)),
        ];

        let muted = ui.visuals().weak_text_color();
        let active = ui.visuals().hyperlink_color;

        for (i, &(x, w, label, key)) in columns.iter().enumerate() {
            let is_active = key.is_some() && key == Some(state.sort_key);
            let text = if is_active {
                format!("{label} {}", if state.sort_ascending { "▲" } else { "▼" })
            } else {
                label.to_string()
            };

            let mut color = if is_active { active } else { muted };

            if let Some(sort_key) = key {
                let click_rect = egui::Rect::from_min_size(
                    egui::pos2(x - 4.0, rect.top()),
                    egui::vec2(w, header_height),
                );
                let response = ui.interact(
                    click_rect,
                    ui.id().with(("tree_header", i)),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    color = active;
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    clicked_sort = Some(sort_key);
                }
                response.on_hover_text("Sort by this column");
            }

            painter.text(
                egui::pos2(x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                egui::FontId::proportional(12.0),
                color,
            );
        }
    });

    ui.separator();

    // Tree view.
    widgets::tree_view::tree_view(ui, state);

    if let Some(key) = clicked_sort {
        state.set_sort(key);
    }
}
