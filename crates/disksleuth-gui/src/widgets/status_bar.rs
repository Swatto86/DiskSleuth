/// Bottom status bar — scan progress and statistics.

use crate::state::{AppPhase, AppState};
use crate::theme::DiskSleuthTheme;
use disksleuth_core::model::size::{format_count, format_size};
use egui::Ui;

/// Draw the status bar at the bottom of the window.
pub fn status_bar(ui: &mut Ui, state: &AppState, theme: &DiskSleuthTheme) {
    ui.horizontal(|ui| {
        match state.phase {
            AppPhase::Idle => {
                ui.label(
                    egui::RichText::new("Ready")
                        .size(12.0)
                        .color(theme.text_muted),
                );
            }
            AppPhase::Scanning => {
                // Animated spinner.
                ui.spinner();

                // Scan tier badge.
                let tier_label = if state.scan_is_mft { "MFT" } else { "Walk" };
                let tier_color = if state.scan_is_mft { theme.accent } else { theme.text_muted };
                ui.label(
                    egui::RichText::new(tier_label)
                        .size(11.0)
                        .color(tier_color),
                );
                ui.separator();

                // Truncate current path for display.
                let display_path = truncate_path(&state.scan_current_path, 60);
                ui.label(
                    egui::RichText::new(format!("Scanning {}...", display_path))
                        .size(12.0)
                        .color(theme.text_secondary),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!(
                        "{} files",
                        format_count(state.scan_files_found)
                    ))
                    .size(12.0)
                    .color(theme.text_primary),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!(
                        "{} dirs",
                        format_count(state.scan_dirs_found)
                    ))
                    .size(12.0)
                    .color(theme.text_primary),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format_size(state.scan_total_size))
                        .size(12.0)
                        .color(theme.accent),
                );

                if state.scan_error_count > 0 {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} errors",
                            format_count(state.scan_error_count)
                        ))
                        .size(12.0)
                        .color(theme.warning),
                    );
                }
            }
            AppPhase::Results => {
                if let Some(ref tree) = state.tree {
                    let status_text = if state.scan_was_cancelled {
                        "⏹ Scan stopped (partial results)"
                    } else {
                        "✓ Scan complete"
                    };
                    let status_color = if state.scan_was_cancelled {
                        theme.warning
                    } else {
                        theme.success
                    };
                    ui.label(
                        egui::RichText::new(status_text)
                            .size(12.0)
                            .color(status_color),
                    );

                    // Scan tier badge.
                    ui.separator();
                    let tier_label = if state.scan_is_mft { "MFT" } else { "Walk" };
                    let tier_color = if state.scan_is_mft { theme.accent } else { theme.text_muted };
                    ui.label(
                        egui::RichText::new(tier_label)
                            .size(11.0)
                            .color(tier_color),
                    );

                    ui.separator();

                    ui.label(
                        egui::RichText::new(format!(
                            "{} files",
                            format_count(
                                tree.nodes.iter().filter(|n| !n.is_dir).count() as u64
                            )
                        ))
                        .size(12.0)
                        .color(theme.text_primary),
                    );

                    ui.separator();

                    ui.label(
                        egui::RichText::new(format_size(tree.total_size))
                            .size(12.0)
                            .color(theme.accent),
                    );

                    if let Some(duration) = state.scan_duration {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("{:.1}s", duration.as_secs_f64()))
                                .size(12.0)
                                .color(theme.text_muted),
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
                            .color(theme.warning),
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
