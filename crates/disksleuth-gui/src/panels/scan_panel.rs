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

    // Analysis shortcuts — each opens its floating window. Most need a
    // completed scan; history only needs at least one recorded snapshot.
    let have_tree = state.tree.is_some();
    if have_tree || !state.scan_history.is_empty() {
        ui.heading("Analysis");
        ui.add_space(4.0);
        let need_scan = "Run a scan first";

        if ui
            .add_enabled(
                have_tree,
                egui::SelectableLabel::new(state.show_largest_window, "\u{1f4ca} Largest Files"),
            )
            .on_hover_text("The biggest individual files found in the scan")
            .on_disabled_hover_text(need_scan)
            .clicked()
        {
            state.show_largest_window = !state.show_largest_window;
        }

        ui.add_space(2.0);
        if ui
            .add_enabled(
                have_tree,
                egui::SelectableLabel::new(state.show_old_files_window, "\u{1f4c5} Old Files"),
            )
            .on_hover_text("Large files that have not been modified in months or years")
            .on_disabled_hover_text(need_scan)
            .clicked()
        {
            state.show_old_files_window = !state.show_old_files_window;
        }

        ui.add_space(2.0);
        if ui
            .add_enabled(
                have_tree,
                egui::SelectableLabel::new(state.show_duplicates_window, "\u{1f501} Duplicates"),
            )
            .on_hover_text("Find files with identical content (hash-verified)")
            .on_disabled_hover_text(need_scan)
            .clicked()
        {
            state.show_duplicates_window = !state.show_duplicates_window;
        }

        ui.add_space(2.0);
        if ui
            .add_enabled(
                !state.scan_history.is_empty(),
                egui::SelectableLabel::new(state.show_history_window, "\u{1f552} Scan History"),
            )
            .on_hover_text("Compare completed scans to see which directories grew")
            .on_disabled_hover_text("Complete a scan to start recording history")
            .clicked()
        {
            state.show_history_window = !state.show_history_window;
        }
    }
}
