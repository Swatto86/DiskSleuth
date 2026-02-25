/// Top action bar -- scan controls and app branding.
use crate::state::{AppPhase, AppState};
use egui::Ui;

/// Draw the toolbar.
pub fn toolbar(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // App title.
        ui.label(
            egui::RichText::new("🔍 DiskSleuth")
                .size(18.0)
                .strong()
                .color(egui::Color32::from_rgb(0x89, 0xb4, 0xfa)),
        );

        ui.separator();

        // Scan button.
        let can_scan = state.phase != AppPhase::Scanning && state.selected_drive_index.is_some();
        let scan_btn = ui.add_enabled(
            can_scan,
            egui::Button::new("▶ Scan").min_size(egui::vec2(70.0, 28.0)),
        );
        if scan_btn.clicked() {
            if let Some(idx) = state.selected_drive_index {
                let path = state.drives[idx].path.clone();
                state.start_scan(path);
            }
        }

        // Stop button (only during scan).
        let can_stop = state.phase == AppPhase::Scanning;
        let stop_btn = ui.add_enabled(
            can_stop,
            egui::Button::new("⏹ Stop").min_size(egui::vec2(70.0, 28.0)),
        );
        if stop_btn.clicked() {
            state.cancel_scan();
        }

        // Refresh drives.
        if ui
            .button("🔄 Refresh")
            .on_hover_text("Re-enumerate drives")
            .clicked()
        {
            state.drives = disksleuth_core::platform::enumerate_drives();
        }

        ui.separator();

        // Export button (only when results available).
        let can_export = state.tree.is_some();
        if ui
            .add_enabled(can_export, egui::Button::new("📤 Export"))
            .on_hover_text("Export to CSV (Phase 2)")
            .clicked()
        {
            // Phase 2: implement CSV/JSON export.
        }

        // Spacer.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // About button.
            if ui.button("ℹ").on_hover_text("About DiskSleuth").clicked() {
                state.show_about = true;
            }

            // Elevation indicator.
            let elevated = disksleuth_core::platform::is_elevated();
            if elevated {
                ui.label(
                    egui::RichText::new("🛡 Admin")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(0xa6, 0xe3, 0xa1)),
                );
            }
        });
    });
}
