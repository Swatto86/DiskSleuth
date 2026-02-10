/// Details panel — shows information about the currently selected node.

use crate::state::AppState;
use crate::theme::DiskSleuthTheme;
use disksleuth_core::model::size::{format_count, format_size};
use disksleuth_core::model::FileTree;
use egui::Ui;

/// Draw the details panel for the currently selected node.
pub fn details_panel(ui: &mut Ui, state: &AppState, theme: &DiskSleuthTheme) {
    let selected = match state.selected_node {
        Some(s) => s,
        None => {
            ui.label(
                egui::RichText::new("Select an item to see details")
                    .color(theme.text_muted)
                    .italics(),
            );
            return;
        }
    };

    // Obtain tree reference — final tree first, then live tree.
    let live_guard;
    let tree: &FileTree;

    if let Some(ref t) = state.tree {
        tree = t;
    } else if let Some(ref lt) = state.live_tree {
        live_guard = lt.read();
        tree = &*live_guard;
    } else {
        return;
    };

    // Guard against stale indices pointing beyond the current tree.
    if selected.0 as usize >= tree.len() {
        return;
    }

    let node = tree.node(selected);
    let full_path = tree.full_path(selected);

    ui.heading(
        egui::RichText::new(if node.is_error {
            "⚠"
        } else if node.is_dir {
            "📁"
        } else {
            "📄"
        })
            .size(16.0),
    );
    ui.add_space(2.0);

    // Error badge.
    if node.is_error {
        ui.label(
            egui::RichText::new("Access denied — contents could not be read")
                .size(11.0)
                .color(theme.warning)
                .italics(),
        );
        ui.add_space(2.0);
    }

    // Name.
    ui.label(
        egui::RichText::new(node.name.as_str())
            .size(14.0)
            .strong()
            .color(theme.text_primary),
    );

    ui.add_space(4.0);

    // Path.
    ui.label(
        egui::RichText::new(&full_path)
            .size(11.0)
            .color(theme.text_muted),
    );

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // Stats grid.
    egui::Grid::new("details_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Size:").color(theme.text_muted));
            ui.label(
                egui::RichText::new(format_size(node.size))
                    .color(theme.accent)
                    .strong(),
            );
            ui.end_row();

            if node.size != node.allocated_size {
                ui.label(egui::RichText::new("On disk:").color(theme.text_muted));
                ui.label(
                    egui::RichText::new(format_size(node.allocated_size))
                        .color(theme.text_secondary),
                );
                ui.end_row();
            }

            ui.label(egui::RichText::new("% of parent:").color(theme.text_muted));
            ui.label(
                egui::RichText::new(format!("{:.1}%", node.percent_of_parent))
                    .color(theme.text_secondary),
            );
            ui.end_row();

            if node.is_dir {
                ui.label(egui::RichText::new("Files:").color(theme.text_muted));
                ui.label(
                    egui::RichText::new(format_count(node.descendant_count))
                        .color(theme.text_secondary),
                );
                ui.end_row();
            }

            if let Some(modified) = node.modified {
                if let Ok(duration) = modified.elapsed() {
                    let days = duration.as_secs() / 86400;
                    let date_str = if days == 0 {
                        "Today".to_string()
                    } else if days == 1 {
                        "Yesterday".to_string()
                    } else if days < 365 {
                        format!("{days} days ago")
                    } else {
                        format!("{:.1} years ago", days as f64 / 365.0)
                    };
                    ui.label(egui::RichText::new("Modified:").color(theme.text_muted));
                    ui.label(
                        egui::RichText::new(date_str).color(theme.text_secondary),
                    );
                    ui.end_row();
                }
            }
        });

    ui.add_space(8.0);

    // Action buttons.
    ui.horizontal(|ui| {
        if ui.button("📂 Open in Explorer").clicked() {
            let target = if node.is_dir {
                full_path.clone()
            } else {
                format!("/select,{}", full_path)
            };
            let _ = std::process::Command::new("explorer.exe")
                .arg(&target)
                .spawn();
        }

        if ui.button("📋 Copy Path").clicked() {
            ui.ctx().copy_text(full_path);
        }
    });
}
