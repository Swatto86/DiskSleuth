/// Floating window for duplicate file detection.
///
/// Hashing runs on a background thread (see `AppState::start_duplicate_scan`)
/// with live progress; results are grouped by content with the most wasteful
/// groups first. Clicking a path locates the file in the tree/treemap.
use crate::state::AppState;
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::NodeIndex;
use std::sync::atomic::Ordering;

/// Minimum-size presets: (bytes, label).
const MIN_SIZES: [(u64, &str); 4] = [
    (64 * 1024, "64 KB"),
    (1024 * 1024, "1 MB"),
    (10 * 1024 * 1024, "10 MB"),
    (100 * 1024 * 1024, "100 MB"),
];

/// Maximum duplicate groups rendered.
const SHOW_TOP_GROUPS: usize = 100;

fn min_size_label(bytes: u64) -> String {
    MIN_SIZES
        .iter()
        .find(|(b, _)| *b == bytes)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| format_size(bytes))
}

pub fn duplicates_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_duplicates_window {
        return;
    }

    let mut open = true;
    let mut select: Option<NodeIndex> = None;
    let mut start_search = false;
    let mut cancel_search = false;
    let mut new_min_size: Option<u64> = None;

    egui::Window::new("🔁 Duplicate Files")
        .open(&mut open)
        .default_size([680.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            if state.tree.is_none() {
                ui.label("Run a scan first.");
                return;
            }
            let muted = ui.visuals().weak_text_color();
            let searching = state.duplicate_scan.is_some();

            ui.horizontal(|ui| {
                ui.label("Minimum size:");
                ui.add_enabled_ui(!searching, |ui| {
                    egui::ComboBox::from_id_salt("dup_min_size")
                        .selected_text(min_size_label(state.duplicate_min_size))
                        .show_ui(ui, |ui| {
                            for (bytes, label) in MIN_SIZES {
                                if ui
                                    .selectable_label(state.duplicate_min_size == bytes, label)
                                    .clicked()
                                {
                                    new_min_size = Some(bytes);
                                }
                            }
                        });
                });

                if searching {
                    if ui.button("⏹ Cancel").clicked() {
                        cancel_search = true;
                    }
                } else if ui.button("🔍 Find Duplicates").clicked() {
                    start_search = true;
                }
            });

            // Progress bar while the background hash runs.
            if let Some(ref scan) = state.duplicate_scan {
                let done = scan.files_done.load(Ordering::Relaxed);
                let fraction = if scan.files_total > 0 {
                    done as f32 / scan.files_total as f32
                } else {
                    1.0
                };
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .text(format!("Hashing {done} / {} candidates…", scan.files_total)),
                );
                return;
            }

            ui.separator();

            let Some(groups) = state.duplicates.as_deref() else {
                ui.label(
                    egui::RichText::new(
                        "Files are compared by size, then a 4 KB prefix hash, then a full \
                         content hash — only true byte-for-byte duplicates are reported.",
                    )
                    .size(11.0)
                    .color(muted),
                );
                return;
            };

            if groups.is_empty() {
                ui.label(
                    egui::RichText::new("No duplicates found at this size threshold. 🎉")
                        .color(muted),
                );
                return;
            }

            let wasted: u64 = groups.iter().map(|g| g.wasted_bytes()).sum();
            ui.label(
                egui::RichText::new(format!(
                    "{} duplicate groups — {} reclaimable by keeping one copy of each",
                    groups.len(),
                    format_size(wasted)
                ))
                .strong(),
            );
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (gi, group) in groups.iter().take(SHOW_TOP_GROUPS).enumerate() {
                        let title = format!(
                            "{} copies × {} — {} wasted",
                            group.files.len(),
                            format_size(group.size),
                            format_size(group.wasted_bytes()),
                        );
                        egui::CollapsingHeader::new(title)
                            .id_salt(("dup_group", gi))
                            .default_open(gi < 5)
                            .show(ui, |ui| {
                                for file in &group.files {
                                    if ui.link(&file.path).clicked() {
                                        select = Some(file.index);
                                    }
                                }
                            });
                    }
                    if groups.len() > SHOW_TOP_GROUPS {
                        ui.label(
                            egui::RichText::new(format!(
                                "… and {} more groups (showing the {SHOW_TOP_GROUPS} most wasteful)",
                                groups.len() - SHOW_TOP_GROUPS
                            ))
                            .size(11.0)
                            .color(muted),
                        );
                    }
                });
        });

    if let Some(bytes) = new_min_size {
        state.duplicate_min_size = bytes;
    }
    if cancel_search {
        state.cancel_duplicate_scan();
    }
    if start_search {
        state.start_duplicate_scan();
    }
    if let Some(idx) = select {
        state.selected_node = Some(idx);
        state.reveal_node_in_tree(idx);
    }
    state.show_duplicates_window = open;
}
