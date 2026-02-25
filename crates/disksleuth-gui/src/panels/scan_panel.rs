/// Scan panel — drive selection and scan controls in the left sidebar.
use crate::state::{AppPhase, AppState};
use crate::widgets;

use egui::Ui;

/// Draw the scan panel (left sidebar content).
pub fn scan_panel(ui: &mut Ui, state: &mut AppState) {
    widgets::drive_picker::drive_picker(ui, state);

    // Scanning progress indicator.
    if state.phase == AppPhase::Scanning {
        ui.add_space(8.0);
        ui.spinner();
        ui.label(
            egui::RichText::new(format!(
                "{} files found",
                disksleuth_core::model::size::format_count(state.scan_files_found)
            ))
            .color(egui::Color32::from_rgb(0xb8, 0xb8, 0xc4)),
        );
    }

    ui.add_space(16.0);
    ui.separator();
    ui.add_space(8.0);

    // Analysis shortcuts (only when results available).
    if state.tree.is_some() {
        ui.heading("Analysis");
        ui.add_space(4.0);

        if ui
            .selectable_label(false, "📊 Top 10 Largest Files")
            .clicked()
        {
            // Scroll to / highlight top files — for now, select the first largest file.
            if let Some(ref tree) = state.tree {
                if let Some(&idx) = tree.largest_files.first() {
                    state.selected_node = Some(idx);
                }
            }
        }

        if ui
            .selectable_label(false, "📁 File Type Breakdown")
            .clicked()
        {
            // Phase 2: open file type analysis panel.
        }

        if ui.selectable_label(false, "📅 Old Files").clicked() {
            // Phase 2: open age analysis panel.
        }

        if ui.selectable_label(false, "🔁 Duplicates").clicked() {
            // Phase 2: open duplicate detection panel.
        }
    }
}
