/// Floating window listing stale files — not modified within a selectable
/// age threshold — sorted by size so the biggest reclaim wins come first.
use crate::state::AppState;
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::NodeIndex;

/// Threshold presets: (days, label).
const THRESHOLDS: [(u64, &str); 5] = [
    (30, "1 month"),
    (90, "3 months"),
    (180, "6 months"),
    (365, "1 year"),
    (730, "2 years"),
];

fn threshold_label(days: u64) -> String {
    THRESHOLDS
        .iter()
        .find(|(d, _)| *d == days)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| format!("{days} days"))
}

pub fn old_files_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_old_files_window {
        return;
    }
    // Compute the cache lazily on first open (and after invalidation).
    if state.old_files.is_none() && state.tree.is_some() {
        state.refresh_old_files();
    }

    let mut open = true;
    let mut select: Option<NodeIndex> = None;
    let mut new_threshold: Option<u64> = None;

    egui::Window::new("📅 Old Files")
        .open(&mut open)
        .default_size([640.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            if state.tree.is_none() {
                ui.label("Run a scan first.");
                return;
            }

            let muted = ui.visuals().weak_text_color();
            ui.horizontal(|ui| {
                ui.label("Not modified in:");
                egui::ComboBox::from_id_salt("old_files_threshold")
                    .selected_text(threshold_label(state.old_files_min_age_days))
                    .show_ui(ui, |ui| {
                        for (days, label) in THRESHOLDS {
                            if ui
                                .selectable_label(state.old_files_min_age_days == days, label)
                                .clicked()
                            {
                                new_threshold = Some(days);
                            }
                        }
                    });

                if let Some(files) = state.old_files.as_deref() {
                    let total: u64 = files.iter().map(|f| f.size).sum();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} files · {} listed",
                                files.len(),
                                format_size(total)
                            ))
                            .size(11.0)
                            .color(muted),
                        );
                    });
                }
            });
            ui.separator();

            let Some(files) = state.old_files.as_deref() else {
                return;
            };
            if files.is_empty() {
                ui.label(
                    egui::RichText::new("No files older than the selected threshold. 🎉")
                        .color(muted),
                );
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("old_files_grid")
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Age").size(11.0).color(muted));
                            ui.label(egui::RichText::new("Size").size(11.0).color(muted));
                            ui.label(egui::RichText::new("Path").size(11.0).color(muted));
                            ui.end_row();

                            for file in files {
                                let age = if file.age_days >= 365 {
                                    format!("{:.1} y", file.age_days as f64 / 365.0)
                                } else {
                                    format!("{} d", file.age_days)
                                };
                                ui.label(egui::RichText::new(age).size(11.0).color(muted));
                                ui.label(
                                    egui::RichText::new(format_size(file.size))
                                        .strong()
                                        .size(12.0),
                                );
                                if ui.link(&file.path).clicked() {
                                    select = Some(file.index);
                                }
                                ui.end_row();
                            }
                        });
                });
        });

    if let Some(days) = new_threshold {
        state.old_files_min_age_days = days;
        state.refresh_old_files();
    }
    if let Some(idx) = select {
        state.selected_node = Some(idx);
        state.reveal_node_in_tree(idx);
    }
    state.show_old_files_window = open;
}
