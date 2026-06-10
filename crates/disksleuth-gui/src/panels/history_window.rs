/// Floating window for scan history and snapshot comparison.
///
/// Every completed (non-cancelled) scan records a compact snapshot. Pick a
/// baseline (A) and a comparison (B) snapshot — by default the two most
/// recent scans of the same root — to see which directories grew or shrank.
use crate::state::AppState;
use disksleuth_core::model::size::format_size;

/// Maximum directory deltas rendered in the comparison table.
const SHOW_TOP_DELTAS: usize = 50;

/// Format a signed byte delta, e.g. "+1.2 GB" / "-340.0 MB".
fn format_delta(delta: i128) -> String {
    let sign = if delta >= 0 { "+" } else { "-" };
    format!(
        "{sign}{}",
        format_size(delta.unsigned_abs().min(u64::MAX as u128) as u64)
    )
}

pub fn history_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_history_window {
        return;
    }
    // Refresh the cached diff for the active pair before rendering.
    state.refresh_history_diff();

    let mut open = true;
    let mut set_baseline: Option<usize> = None;
    let mut set_compare: Option<usize> = None;

    egui::Window::new("🕒 Scan History")
        .open(&mut open)
        .default_size([720.0, 480.0])
        .resizable(true)
        .show(ctx, |ui| {
            if state.scan_history.is_empty() {
                ui.label("No completed scans yet — finish a scan to record one.");
                return;
            }

            let muted = ui.visuals().weak_text_color();
            let accent = ui.visuals().hyperlink_color;
            let active_pair = state.history_pair();

            ui.label(
                egui::RichText::new(
                    "Mark a baseline (A) and a comparison (B) scan to see what changed. \
                     Scans of the same path compare meaningfully.",
                )
                .size(11.0)
                .color(muted),
            );
            ui.add_space(4.0);

            egui::Grid::new("history_grid")
                .num_columns(7)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for header in ["A", "B", "When", "Path", "Total", "Files", "Duration"] {
                        ui.label(egui::RichText::new(header).size(11.0).color(muted));
                    }
                    ui.end_row();

                    // Newest first.
                    for (i, snap) in state.scan_history.iter().enumerate().rev() {
                        let is_a = active_pair.map(|(a, _)| a) == Some(i);
                        let is_b = active_pair.map(|(_, b)| b) == Some(i);
                        if ui.radio(is_a, "").clicked() {
                            set_baseline = Some(i);
                        }
                        if ui.radio(is_b, "").clicked() {
                            set_compare = Some(i);
                        }
                        ui.label(
                            egui::RichText::new(snap.taken.format("%H:%M:%S").to_string())
                                .size(12.0),
                        );
                        ui.label(egui::RichText::new(&snap.root_path).size(12.0));
                        ui.label(
                            egui::RichText::new(format_size(snap.total_size))
                                .size(12.0)
                                .color(accent),
                        );
                        ui.label(
                            egui::RichText::new(disksleuth_core::model::size::format_count(
                                snap.file_count,
                            ))
                            .size(12.0),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:.1}s", snap.duration.as_secs_f64()))
                                .size(11.0)
                                .color(muted),
                        );
                        ui.end_row();
                    }
                });

            ui.separator();

            // ── Comparison ────────────────────────────────────────────────
            let Some(((a, b), _)) = state.history_diff.as_ref().map(|(k, d)| (*k, d.len())) else {
                ui.label(
                    egui::RichText::new("Run two scans of the same path to compare them.")
                        .color(muted),
                );
                return;
            };
            let old = &state.scan_history[a];
            let new = &state.scan_history[b];

            if !old.root_path.eq_ignore_ascii_case(&new.root_path) {
                ui.label(
                    egui::RichText::new(format!(
                        "⚠ Comparing different roots ({} vs {}) — directory deltas may be \
                         meaningless.",
                        old.root_path, new.root_path
                    ))
                    .color(egui::Color32::from_rgb(0xfa, 0xb3, 0x87)),
                );
            }

            let total_delta = new.total_size as i128 - old.total_size as i128;
            ui.label(
                egui::RichText::new(format!(
                    "{} → {}  ({} overall, {})",
                    old.taken.format("%H:%M:%S"),
                    new.taken.format("%H:%M:%S"),
                    format_delta(total_delta),
                    format_size(new.total_size),
                ))
                .strong(),
            );
            ui.add_space(4.0);

            let diff = &state.history_diff.as_ref().expect("checked above").1;
            if diff.is_empty() {
                ui.label(egui::RichText::new("No directory changes detected.").color(muted));
                return;
            }

            let grow_color = egui::Color32::from_rgb(0xf3, 0x8b, 0xa8); // consumed space
            let shrink_color = egui::Color32::from_rgb(0xa6, 0xe3, 0xa1); // freed space

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("history_diff_grid")
                        .num_columns(4)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for header in ["Change", "Before", "After", "Directory"] {
                                ui.label(egui::RichText::new(header).size(11.0).color(muted));
                            }
                            ui.end_row();

                            for delta in diff.iter().take(SHOW_TOP_DELTAS) {
                                let d = delta.delta();
                                ui.label(
                                    egui::RichText::new(format_delta(d))
                                        .strong()
                                        .size(12.0)
                                        .color(if d >= 0 { grow_color } else { shrink_color }),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        delta
                                            .old_size
                                            .map(format_size)
                                            .unwrap_or_else(|| "(new)".into()),
                                    )
                                    .size(11.0)
                                    .color(muted),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        delta
                                            .new_size
                                            .map(format_size)
                                            .unwrap_or_else(|| "(removed)".into()),
                                    )
                                    .size(11.0)
                                    .color(muted),
                                );
                                ui.label(egui::RichText::new(&delta.path).size(12.0));
                                ui.end_row();
                            }
                        });
                });
        });

    if let Some(i) = set_baseline {
        state.history_baseline = Some(i);
    }
    if let Some(i) = set_compare {
        state.history_compare = Some(i);
    }
    state.show_history_window = open;
}
