/// Floating window listing the paths that could not be read during the scan.
///
/// The status bar only shows the aggregate count; this window explains which
/// items were skipped and why (usually access-denied on a system directory).
use crate::state::AppState;
use disksleuth_core::model::size::format_count;

pub fn errors_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_errors {
        return;
    }
    let mut open = true;

    egui::Window::new("\u{26a0} Skipped Items")
        .open(&mut open)
        .default_size([700.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            let muted = ui.visuals().weak_text_color();
            if state.scan_errors.is_empty() {
                ui.label(egui::RichText::new("Nothing was skipped in the last scan.").color(muted));
                return;
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} items could not be read, so their size is missing from the totals. \
                     Running DiskSleuth as administrator usually reduces this.",
                    format_count(state.scan_error_count)
                ))
                .size(11.0)
                .color(muted),
            );
            if state.scan_error_count as usize > state.scan_errors.len() {
                ui.label(
                    egui::RichText::new(format!(
                        "Listing the first {} of them.",
                        format_count(state.scan_errors.len() as u64)
                    ))
                    .size(11.0)
                    .color(muted),
                );
            }
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("scan_errors_grid")
                        .num_columns(2)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Path").size(11.0).color(muted));
                            ui.label(egui::RichText::new("Reason").size(11.0).color(muted));
                            ui.end_row();

                            for (path, message) in &state.scan_errors {
                                ui.label(egui::RichText::new(path).size(12.0));
                                ui.label(egui::RichText::new(message).size(11.0).color(muted));
                                ui.end_row();
                            }
                        });
                });
        });

    state.show_errors = open;
}
