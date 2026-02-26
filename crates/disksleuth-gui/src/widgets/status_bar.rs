/// Bottom status bar — scan progress and statistics.
use crate::state::{AppPhase, AppState};
use disksleuth_core::model::size::{format_count, format_size};
use egui::Ui;

/// Draw the status bar at the bottom of the window.
pub fn status_bar(ui: &mut Ui, state: &AppState) {
    // Extract theme-adaptive colours once for this frame.
    let color_accent = ui.visuals().hyperlink_color;
    let color_weak = ui.visuals().weak_text_color();
    let color_normal = ui.visuals().text_color();
    let color_warning = egui::Color32::from_rgb(0xfa, 0xb3, 0x87);
    let color_success = egui::Color32::from_rgb(0xa6, 0xe3, 0xa1);

    ui.horizontal(|ui| {
        match state.phase {
            AppPhase::Idle => {
                ui.label(egui::RichText::new("Ready").size(12.0).color(color_weak));
            }
            AppPhase::Scanning => {
                // Animated spinner.
                ui.spinner();

                // Scan tier badge.
                let tier_label = if state.scan_is_mft { "MFT" } else { "Walk" };
                let tier_color = if state.scan_is_mft {
                    color_accent
                } else {
                    color_weak
                };
                ui.label(egui::RichText::new(tier_label).size(11.0).color(tier_color));
                ui.separator();

                // Truncate current path for display.
                let display_path = truncate_path(&state.scan_current_path, 60);
                ui.label(
                    egui::RichText::new(format!("Scanning {}...", display_path))
                        .size(12.0)
                        .color(color_normal),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!("{} files", format_count(state.scan_files_found)))
                        .size(12.0)
                        .color(color_normal),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!("{} dirs", format_count(state.scan_dirs_found)))
                        .size(12.0)
                        .color(color_normal),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format_size(state.scan_total_size))
                        .size(12.0)
                        .color(color_accent),
                );

                if state.scan_error_count > 0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} errors",
                            format_count(state.scan_error_count)
                        ))
                        .size(12.0)
                        .color(color_warning),
                    );
                }
            }
            AppPhase::Results => {
                if let Some(ref tree) = state.tree {
                    let status_text = if state.scan_was_cancelled {
                        "\u{23f9} Scan stopped (partial results)"
                    } else {
                        "\u{2713} Scan complete"
                    };
                    let status_color = if state.scan_was_cancelled {
                        color_warning
                    } else {
                        color_success
                    };
                    ui.label(
                        egui::RichText::new(status_text)
                            .size(12.0)
                            .color(status_color),
                    );

                    // Scan tier badge.
                    ui.separator();
                    let tier_label = if state.scan_is_mft { "MFT" } else { "Walk" };
                    let tier_color = if state.scan_is_mft {
                        color_accent
                    } else {
                        color_weak
                    };
                    ui.label(egui::RichText::new(tier_label).size(11.0).color(tier_color));

                    ui.separator();

                    ui.label(
                        egui::RichText::new(format!(
                            "{} files",
                            format_count(tree.nodes.iter().filter(|n| !n.is_dir).count() as u64)
                        ))
                        .size(12.0)
                        .color(color_normal),
                    );

                    ui.separator();

                    ui.label(
                        egui::RichText::new(format_size(tree.total_size))
                            .size(12.0)
                            .color(color_accent),
                    );

                    if let Some(duration) = state.scan_duration {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("{:.1}s", duration.as_secs_f64()))
                                .size(12.0)
                                .color(color_weak),
                        );
                    }

                    if state.scan_error_count > 0 {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} skipped",
                                format_count(state.scan_error_count)
                            ))
                            .size(12.0)
                            .color(color_warning),
                        );
                    }
                }
            }
        }
    });
}

/// Truncate a path string to fit within `max_len` characters,
/// replacing the middle with "..." if needed.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    let half = (max_len - 3) / 2;
    format!("{}...{}", &path[..half], &path[path.len() - half..])
}
