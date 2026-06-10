/// Top action bar -- scan controls, theme toggle, monitor toggle, and branding.
use crate::state::{AppPhase, AppState, ExportFormat};
use egui::Ui;

/// Draw the toolbar.
pub fn toolbar(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // App title -- uses the egui accent/hyperlink colour so it adapts to
        // dark and light mode automatically.
        ui.label(
            egui::RichText::new("🔍 DiskSleuth")
                .size(18.0)
                .strong()
                .color(ui.visuals().hyperlink_color),
        );

        ui.separator();

        // Scan button.
        let can_scan = state.phase != AppPhase::Scanning && state.selected_drive_index.is_some();
        let scan_btn = ui.add_enabled(
            can_scan,
            egui::Button::new("▶ Scan").min_size(egui::vec2(70.0, 28.0)),
        );
        if scan_btn.clicked() {
            // `.get()` rather than indexing: the drive list may have shrunk
            // since the selection was made (drive unplugged + refreshed).
            if let Some(path) = state
                .selected_drive_index
                .and_then(|idx| state.drives.get(idx))
                .map(|d| d.path.clone())
            {
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

        // Scan a custom folder instead of a whole drive.
        let can_pick = state.phase != AppPhase::Scanning;
        if ui
            .add_enabled(can_pick, egui::Button::new("📂 Scan Folder…"))
            .on_hover_text("Pick a specific folder to scan instead of a whole drive")
            .clicked()
        {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title("Choose a folder to scan")
                .pick_folder()
            {
                state.start_scan(folder);
            }
        }

        // Refresh drives — disabled during a scan to prevent a jarring
        // state reset while results are being accumulated.
        let can_refresh = state.phase != AppPhase::Scanning;
        if ui
            .add_enabled(can_refresh, egui::Button::new("🔄 Refresh"))
            .on_hover_text(if can_refresh {
                "Re-enumerate drives"
            } else {
                "Cannot refresh drives while a scan is running"
            })
            .clicked()
        {
            state.refresh_drives();
        }

        ui.separator();

        // Export menu (only when results available).
        let can_export = state.tree.is_some();
        let mut export_format: Option<ExportFormat> = None;
        ui.add_enabled_ui(can_export, |ui| {
            ui.menu_button("📤 Export", |ui| {
                if ui
                    .button("CSV — open in a spreadsheet")
                    .on_hover_text("Write all rows to a CSV file in your Documents folder")
                    .clicked()
                {
                    export_format = Some(ExportFormat::Csv);
                    ui.close_menu();
                }
                if ui
                    .button("JSON — structured data")
                    .on_hover_text("Write all rows to a JSON file in your Documents folder")
                    .clicked()
                {
                    export_format = Some(ExportFormat::Json);
                    ui.close_menu();
                }
            })
            .response
            .on_hover_text(if can_export {
                "Export results to your Documents folder"
            } else {
                "Run a scan first to enable export"
            });
        });
        if let Some(format) = export_format {
            state.export(format);
        }

        // Right-aligned controls.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // About button.
            if ui.button("ℹ").on_hover_text("About DiskSleuth").clicked() {
                state.show_about = true;
            }

            // ── Theme toggle (☀ light / 🌙 dark) ──────────────────
            let theme_label = if state.dark_mode { "☀" } else { "🌙" };
            let theme_tip = if state.dark_mode {
                "Switch to light mode"
            } else {
                "Switch to dark mode"
            };
            if ui.button(theme_label).on_hover_text(theme_tip).clicked() {
                state.dark_mode = !state.dark_mode;
            }

            ui.separator();

            // ── Live write monitor toggle ─────────────────────────
            let monitor_label = if state.monitor_active {
                egui::RichText::new("👁 Monitoring").color(
                    egui::Color32::from_rgb(0xa6, 0xe3, 0xa1), // green active indicator
                )
            } else {
                egui::RichText::new("👁 Monitor")
            };
            let monitor_tip = if state.show_monitor_panel {
                "Hide write monitor panel"
            } else {
                "Show live file write monitor"
            };
            if ui
                .button(monitor_label)
                .on_hover_text(monitor_tip)
                .clicked()
            {
                state.show_monitor_panel = !state.show_monitor_panel;
            }

            ui.separator();

            // Elevation indicator — read from cached AppState field (computed once
            // at startup) rather than calling is_elevated() on every frame.
            if state.is_elevated {
                ui.label(
                    egui::RichText::new("🛡 Admin")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(0xa6, 0xe3, 0xa1)),
                );
            }
        });
    });
}
