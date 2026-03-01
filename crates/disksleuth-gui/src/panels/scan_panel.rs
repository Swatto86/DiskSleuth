/// Scan panel — drive selection and scan controls in the left sidebar.
use crate::state::AppState;
use crate::widgets;

use egui::Ui;

/// Draw the scan panel (left sidebar content).
pub fn scan_panel(ui: &mut Ui, state: &mut AppState) {
    widgets::drive_picker::drive_picker(ui, state);

    // Note: scanning progress (spinner + file count) is shown in the tree
    // view and the status bar — no need to duplicate it here.

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Analysis shortcuts (only when results available).
    if state.tree.is_some() {
        ui.heading("Analysis");
        ui.add_space(4.0);

        // "Top Largest Files" is live — selects the first result node.
        if ui
            .selectable_label(false, "\u{1f4ca} Top 10 Largest Files")
            .on_hover_text("Select the largest file found in the scan")
            .clicked()
        {
            if let Some(ref tree) = state.tree {
                if let Some(&idx) = tree.largest_files.first() {
                    state.selected_node = Some(idx);
                }
            }
        }

        ui.add_space(2.0);

        // Stub shortcuts — disabled until implemented; tooltip explains why.
        let coming_soon = "Coming in a future release";
        ui.add_enabled(
            false,
            egui::SelectableLabel::new(false, "\u{1f4c1} File Type Breakdown"),
        )
        .on_disabled_hover_text(coming_soon);

        ui.add_space(2.0);
        ui.add_enabled(
            false,
            egui::SelectableLabel::new(false, "\u{1f4c5} Old Files"),
        )
        .on_disabled_hover_text(coming_soon);

        ui.add_space(2.0);
        ui.add_enabled(
            false,
            egui::SelectableLabel::new(false, "\u{1f501} Duplicates"),
        )
        .on_disabled_hover_text(coming_soon);
    }
}
